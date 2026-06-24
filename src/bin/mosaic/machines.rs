use serde_json::{json, Map, Value};
use std::{env, path::PathBuf};

pub const MACHINE_SCHEMA_VERSION: &str = "mosaic.machine.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineCommandPlan {
    pub machine_id: String,
    pub transport_kind: String,
    pub program: String,
    pub args: Vec<String>,
    pub mosaic_command: Vec<String>,
    pub remote_shell_command: Option<String>,
}

impl MachineCommandPlan {
    pub fn argv(&self) -> Vec<String> {
        let mut argv = Vec::with_capacity(1 + self.args.len());
        argv.push(self.program.clone());
        argv.extend(self.args.clone());
        argv
    }

    pub fn to_json(&self) -> Value {
        json!({
            "machine": self.machine_id,
            "transport": self.transport_kind,
            "program": self.program,
            "args": self.args,
            "argv": self.argv(),
            "mosaic_command": self.mosaic_command,
            "remote_shell_command": self.remote_shell_command,
        })
    }
}

pub fn default_config_path() -> PathBuf {
    if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(config_home)
            .join("open-mosaic")
            .join("machines.json");
    }
    env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".config")
        .join("open-mosaic")
        .join("machines.json")
}

pub fn local_machine() -> Value {
    let hostname = env::var("HOSTNAME")
        .ok()
        .filter(|hostname| !hostname.trim().is_empty())
        .unwrap_or_else(|| "localhost".to_owned());
    json!({
        "schema_version": MACHINE_SCHEMA_VERSION,
        "id": "local",
        "name": hostname,
        "source": "builtin",
        "role": "local",
        "capabilities": ["machine.local", "transport.local_process"],
        "transport": {
            "kind": "local",
            "mosaic_bin": "mosaic",
        },
    })
}

pub fn validate_registry(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "machine registry must be a JSON object".to_owned())?;
    let schema_version = require_string(object.get("schema_version"), "schema_version")?;
    if schema_version != MACHINE_SCHEMA_VERSION {
        return Err(format!(
            "schema_version must be {MACHINE_SCHEMA_VERSION:?}, got {schema_version:?}"
        ));
    }
    let machines = object
        .get("machines")
        .ok_or_else(|| "machines is required".to_owned())?
        .as_array()
        .ok_or_else(|| "machines must be an array".to_owned())?;
    let mut seen_ids = Vec::new();
    for machine in machines {
        validate_machine(machine)?;
        let id = machine
            .get("id")
            .and_then(Value::as_str)
            .expect("validate_machine checked id")
            .to_owned();
        if seen_ids.iter().any(|seen| seen == &id) {
            return Err(format!("duplicate machine id {id:?}"));
        }
        seen_ids.push(id);
    }
    Ok(())
}

pub fn machines_from_registry(value: &Value) -> Result<Vec<Value>, String> {
    validate_registry(value)?;
    Ok(value
        .get("machines")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(mark_config_machine)
        .collect())
}

pub fn find_machine<'a>(machines: &'a [Value], id: &str) -> Option<&'a Value> {
    machines
        .iter()
        .find(|machine| machine.get("id").and_then(Value::as_str) == Some(id))
}

pub fn build_command_plan(
    machine: &Value,
    command: &[String],
    mosaic_bin_override: Option<&str>,
) -> Result<MachineCommandPlan, String> {
    if command.is_empty() {
        return Err("remote Mosaic command must not be empty".to_owned());
    }
    let id = machine
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "machine id is required".to_owned())?;
    validate_id(id, "id")?;
    let transport = machine
        .get("transport")
        .and_then(Value::as_object)
        .ok_or_else(|| "transport is required".to_owned())?;
    let kind = require_string_map(transport, "kind")?;
    let mosaic_bin = if let Some(mosaic_bin) = mosaic_bin_override {
        validate_command_segment(mosaic_bin)?
    } else if let Some(mosaic_bin) = transport.get("mosaic_bin").and_then(Value::as_str) {
        validate_command_segment(mosaic_bin)?
    } else {
        "mosaic"
    };
    let mut mosaic_command = Vec::with_capacity(1 + command.len());
    mosaic_command.push(mosaic_bin.to_owned());
    for segment in command {
        validate_command_segment(segment)?;
        mosaic_command.push(segment.clone());
    }

    match kind {
        "local" => Ok(MachineCommandPlan {
            machine_id: id.to_owned(),
            transport_kind: kind.to_owned(),
            program: mosaic_bin.to_owned(),
            args: command.to_vec(),
            mosaic_command,
            remote_shell_command: None,
        }),
        "ssh" => {
            let host = require_string_map(transport, "host")?;
            validate_ssh_host(host)?;
            let mut args = vec!["-o".to_owned(), "BatchMode=yes".to_owned()];
            if let Some(port) = transport.get("port") {
                let port = validate_port(port)?;
                args.push("-p".to_owned());
                args.push(port.to_string());
            }
            let target = if let Some(user) = transport.get("user").and_then(Value::as_str) {
                validate_ssh_user(user)?;
                format!("{user}@{host}")
            } else {
                host.to_owned()
            };
            args.push(target);
            let remote_shell_command = mosaic_command
                .iter()
                .map(|segment| shell_quote(segment))
                .collect::<Vec<_>>()
                .join(" ");
            args.push(remote_shell_command.clone());
            Ok(MachineCommandPlan {
                machine_id: id.to_owned(),
                transport_kind: kind.to_owned(),
                program: "ssh".to_owned(),
                args,
                mosaic_command,
                remote_shell_command: Some(remote_shell_command),
            })
        },
        _ => Err(format!("unsupported transport kind {kind:?}")),
    }
}

pub fn redact_mosaic_command(command: &[String]) -> Vec<String> {
    let mut redacted = Vec::with_capacity(command.len());
    let mut redact_next = false;
    for segment in command {
        if redact_next {
            redacted.push("[redacted]".to_owned());
            redact_next = false;
            continue;
        }
        if matches!(segment.as_str(), "--text" | "--file") {
            redacted.push(segment.clone());
            redact_next = true;
            continue;
        }
        if segment.starts_with("--text=") {
            redacted.push("--text=[redacted]".to_owned());
            continue;
        }
        if segment.starts_with("--file=") {
            redacted.push("--file=[redacted]".to_owned());
            continue;
        }
        redacted.push(segment.clone());
    }
    redacted
}

fn mark_config_machine(mut machine: Value) -> Value {
    if let Value::Object(object) = &mut machine {
        object
            .entry("schema_version".to_owned())
            .or_insert_with(|| json!(MACHINE_SCHEMA_VERSION));
        object
            .entry("source".to_owned())
            .or_insert_with(|| json!("config"));
    }
    machine
}

fn validate_machine(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "machine must be a JSON object".to_owned())?;
    let id = require_string(object.get("id"), "id")?;
    validate_id(id, "id")?;
    optional_string(object.get("name"), "name")?;
    optional_string(object.get("description"), "description")?;
    validate_tags(object.get("tags"))?;
    if let Some(metadata) = object.get("metadata") {
        if !metadata.is_object() {
            return Err("metadata must be an object".to_owned());
        }
    }
    let transport = object
        .get("transport")
        .and_then(Value::as_object)
        .ok_or_else(|| "transport must be an object".to_owned())?;
    validate_transport(transport)
}

fn validate_transport(transport: &Map<String, Value>) -> Result<(), String> {
    let kind = require_string_map(transport, "kind")?;
    match kind {
        "local" => {
            optional_command_segment(transport.get("mosaic_bin"), "mosaic_bin")?;
            Ok(())
        },
        "ssh" => {
            let host = require_string_map(transport, "host")?;
            validate_ssh_host(host)?;
            if let Some(user) = transport.get("user") {
                validate_ssh_user(require_string(Some(user), "user")?)?;
            }
            if let Some(port) = transport.get("port") {
                validate_port(port)?;
            }
            optional_command_segment(transport.get("mosaic_bin"), "mosaic_bin")?;
            Ok(())
        },
        _ => Err("transport.kind must be local or ssh".to_owned()),
    }
}

fn require_string<'a>(value: Option<&'a Value>, field: &str) -> Result<&'a str, String> {
    let value = value.ok_or_else(|| format!("{field} is required"))?;
    let value = value
        .as_str()
        .ok_or_else(|| format!("{field} must be a string"))?;
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if value.chars().any(|character| character == '\0') {
        return Err(format!("{field} must not contain NUL bytes"));
    }
    Ok(value)
}

fn require_string_map<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a str, String> {
    require_string(object.get(field), field)
}

fn optional_string(value: Option<&Value>, field: &str) -> Result<(), String> {
    if let Some(value) = value {
        require_string(Some(value), field)?;
    }
    Ok(())
}

fn validate_id(id: &str, field: &str) -> Result<(), String> {
    if id == "." || id == ".." {
        return Err(format!("{field} must not be a path marker"));
    }
    if id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
    {
        Ok(())
    } else {
        Err(format!(
            "{field} may contain only ASCII letters, numbers, dots, underscores, and hyphens"
        ))
    }
}

fn validate_tags(value: Option<&Value>) -> Result<(), String> {
    let Some(value) = value else {
        return Ok(());
    };
    let tags = value
        .as_array()
        .ok_or_else(|| "tags must be an array of strings".to_owned())?;
    for tag in tags {
        require_string(Some(tag), "tag")?;
    }
    Ok(())
}

fn optional_command_segment(value: Option<&Value>, field: &str) -> Result<(), String> {
    if let Some(value) = value {
        validate_command_segment(require_string(Some(value), field)?)?;
    }
    Ok(())
}

fn validate_command_segment(segment: &str) -> Result<&str, String> {
    if segment.trim().is_empty() {
        return Err("command segment must not be empty".to_owned());
    }
    if segment
        .chars()
        .any(|character| character == '\0' || character.is_control())
    {
        return Err("command segment must not contain control characters".to_owned());
    }
    Ok(segment)
}

fn validate_ssh_host(host: &str) -> Result<(), String> {
    if host.starts_with('-') {
        return Err("ssh host must not start with '-'".to_owned());
    }
    if host.contains('@') {
        return Err("ssh host must not contain '@'; use transport.user instead".to_owned());
    }
    validate_ssh_token(host, "ssh host")
}

fn validate_ssh_user(user: &str) -> Result<(), String> {
    if user.starts_with('-') {
        return Err("ssh user must not start with '-'".to_owned());
    }
    if user.contains('@') {
        return Err("ssh user must not contain '@'".to_owned());
    }
    validate_ssh_token(user, "ssh user")
}

fn validate_ssh_token(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if value
        .chars()
        .any(|character| character == '\0' || character.is_control() || character.is_whitespace())
    {
        return Err(format!(
            "{field} must not contain whitespace or control characters"
        ));
    }
    Ok(())
}

fn validate_port(value: &Value) -> Result<u16, String> {
    let port = value
        .as_u64()
        .ok_or_else(|| "port must be an integer".to_owned())?;
    if (1..=65535).contains(&port) {
        Ok(port as u16)
    } else {
        Err("port must be between 1 and 65535".to_owned())
    }
}

fn shell_quote(segment: &str) -> String {
    if segment.is_empty() {
        return "''".to_owned();
    }
    if segment.chars().all(is_shell_safe_char) {
        return segment.to_owned();
    }
    format!("'{}'", segment.replace('\'', "'\\''"))
}

fn is_shell_safe_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '/' | ':' | '=')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_registry() -> Value {
        json!({
            "schema_version": MACHINE_SCHEMA_VERSION,
            "machines": [
                {
                    "id": "dev-box",
                    "name": "Development box",
                    "transport": {
                        "kind": "ssh",
                        "host": "dev.example.org",
                        "user": "alice",
                        "port": 2222,
                        "mosaic_bin": "/usr/local/bin/mosaic"
                    }
                }
            ]
        })
    }

    #[test]
    fn validates_portable_ssh_registry() {
        validate_registry(&valid_registry()).expect("valid registry");
    }

    #[test]
    fn rejects_unsafe_ssh_hosts() {
        let mut registry = valid_registry();
        registry["machines"][0]["transport"]["host"] = json!("-oProxyCommand=bad");
        let error = validate_registry(&registry).expect_err("invalid host");
        assert!(error.contains("ssh host must not start"));

        registry["machines"][0]["transport"]["host"] = json!("alice@dev.example.org");
        let error = validate_registry(&registry).expect_err("invalid host user mix");
        assert!(error.contains("use transport.user"));
    }

    #[test]
    fn builds_ssh_command_with_shell_quoted_remote_segments() {
        let machines = machines_from_registry(&valid_registry()).expect("machines");
        let machine = find_machine(&machines, "dev-box").expect("machine");
        let plan = build_command_plan(
            machine,
            &[
                "prompt".to_owned(),
                "send".to_owned(),
                "--text".to_owned(),
                "hello; rm -rf /".to_owned(),
            ],
            None,
        )
        .expect("command plan");

        assert_eq!(plan.program, "ssh");
        assert_eq!(plan.args[0], "-o");
        assert_eq!(plan.args[1], "BatchMode=yes");
        assert_eq!(plan.args[2], "-p");
        assert_eq!(plan.args[3], "2222");
        assert_eq!(plan.args[4], "alice@dev.example.org");
        assert_eq!(
            plan.remote_shell_command.as_deref(),
            Some("/usr/local/bin/mosaic prompt send --text 'hello; rm -rf /'")
        );
    }

    #[test]
    fn redacts_prompt_sources_from_audit_command() {
        let command = redact_mosaic_command(&[
            "mosaic".to_owned(),
            "prompt".to_owned(),
            "send".to_owned(),
            "--text".to_owned(),
            "secret".to_owned(),
            "--file=prompt.txt".to_owned(),
        ]);
        assert_eq!(
            command,
            vec![
                "mosaic",
                "prompt",
                "send",
                "--text",
                "[redacted]",
                "--file=[redacted]"
            ]
        );
    }
}
