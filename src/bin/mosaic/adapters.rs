use serde_json::{json, Value};

pub const ADAPTER_SCHEMA_VERSION: &str = "mosaic.adapter.v1";

const KNOWN_KINDS: &[&str] = &[
    "agent",
    "project_registry",
    "task_system",
    "identity",
    "machine_registry",
    "transport",
];

pub fn built_in_adapters() -> Vec<Value> {
    vec![
        adapter(
            "mosaic.agent.generic-terminal",
            "agent",
            "Generic terminal agent panes",
            &[
                "pane.detect",
                "pane.metadata",
                "prompt.send",
                "prompt.queue",
                "observe.pane",
            ],
        ),
        adapter(
            "mosaic.agent.codewith",
            "agent",
            "Codewith-compatible agent panes",
            &[
                "pane.detect",
                "pane.metadata",
                "prompt.submit.enter",
                "prompt.submit.tab",
                "observe.pane",
            ],
        ),
        adapter(
            "mosaic.agent.claude-code",
            "agent",
            "Claude Code-compatible agent panes",
            &[
                "pane.detect",
                "pane.metadata",
                "prompt.submit.enter",
                "observe.pane",
            ],
        ),
        adapter(
            "mosaic.agent.opencode",
            "agent",
            "opencode-compatible agent panes",
            &[
                "pane.detect",
                "pane.metadata",
                "prompt.submit.enter",
                "observe.pane",
            ],
        ),
        adapter(
            "mosaic.agent.codex",
            "agent",
            "Codex-family agent panes",
            &[
                "pane.detect",
                "pane.metadata",
                "prompt.submit.enter",
                "observe.pane",
            ],
        ),
        adapter(
            "mosaic.project.git",
            "project_registry",
            "Local Git project metadata",
            &["project.detect", "repo.metadata"],
        ),
        adapter(
            "mosaic.task.local",
            "task_system",
            "Local task-system manifest contract",
            &["task.reference", "task.link_ref", "task.status"],
        ),
        adapter(
            "mosaic.identity.local-user",
            "identity",
            "Local OS user identity",
            &["identity.local_user", "audit.actor"],
        ),
        adapter(
            "mosaic.machine.local",
            "machine_registry",
            "Local machine metadata",
            &["machine.local", "machine.context"],
        ),
        adapter(
            "mosaic.transport.local",
            "transport",
            "Local process transport",
            &["transport.local_process", "transport.local_socket"],
        ),
        adapter(
            "mosaic.transport.ssh",
            "transport",
            "Portable SSH transport contract",
            &["transport.ssh", "machine.remote"],
        ),
    ]
}

pub fn known_kinds() -> &'static [&'static str] {
    KNOWN_KINDS
}

pub fn validate_adapter_manifest(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "adapter manifest must be a JSON object".to_owned())?;
    require_string(object.get("schema_version"), "schema_version").and_then(|schema_version| {
        if schema_version == ADAPTER_SCHEMA_VERSION {
            Ok(())
        } else {
            Err(format!(
                "schema_version must be {ADAPTER_SCHEMA_VERSION:?}, got {schema_version:?}"
            ))
        }
    })?;
    let id = require_string(object.get("id"), "id")?;
    validate_id(id)?;
    let kind = require_string(object.get("kind"), "kind")?;
    if !KNOWN_KINDS.contains(&kind) {
        return Err(format!(
            "kind must be one of {}, got {kind:?}",
            KNOWN_KINDS.join(", ")
        ));
    }
    require_string(object.get("version"), "version")?;
    if let Some(name) = object.get("name") {
        require_string(Some(name), "name")?;
    }
    if let Some(description) = object.get("description") {
        require_string(Some(description), "description")?;
    }
    validate_capabilities(object.get("capabilities"))?;
    if let Some(command) = object.get("command") {
        validate_command(command)?;
    }
    Ok(())
}

fn adapter(id: &str, kind: &str, name: &str, capabilities: &[&str]) -> Value {
    json!({
        "schema_version": ADAPTER_SCHEMA_VERSION,
        "id": id,
        "kind": kind,
        "name": name,
        "version": "0.1.0",
        "source": "builtin",
        "mode": "interface",
        "enabled": true,
        "description": "Built-in portable Open Mosaic adapter interface descriptor; no external services required.",
        "capabilities": capabilities,
        "command": null,
    })
}

fn require_string<'a>(value: Option<&'a Value>, field: &str) -> Result<&'a str, String> {
    let value = value.ok_or_else(|| format!("{field} is required"))?;
    let value = value
        .as_str()
        .ok_or_else(|| format!("{field} must be a string"))?;
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    Ok(value)
}

fn validate_id(id: &str) -> Result<(), String> {
    if id == "." || id == ".." {
        return Err("id must not be a path marker".to_owned());
    }
    if id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
    {
        Ok(())
    } else {
        Err("id may contain only ASCII letters, numbers, dots, underscores, and hyphens".to_owned())
    }
}

fn validate_capabilities(value: Option<&Value>) -> Result<(), String> {
    let Some(value) = value else {
        return Ok(());
    };
    let capabilities = value
        .as_array()
        .ok_or_else(|| "capabilities must be an array of strings".to_owned())?;
    for capability in capabilities {
        require_string(Some(capability), "capability")?;
    }
    Ok(())
}

fn validate_command(value: &Value) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    if value.as_str().is_some() {
        require_string(Some(value), "command")?;
        return Ok(());
    }
    let Some(command) = value.as_array() else {
        return Err("command must be null, a string, or an array of strings".to_owned());
    };
    if command.is_empty() {
        return Err("command array must not be empty".to_owned());
    }
    for segment in command {
        require_string(Some(segment), "command segment")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_minimal_manifest() {
        let manifest = json!({
            "schema_version": ADAPTER_SCHEMA_VERSION,
            "id": "example.agent",
            "kind": "agent",
            "version": "1.0.0",
            "capabilities": ["pane.detect"],
            "command": ["example-agent", "--json"],
        });

        validate_adapter_manifest(&manifest).expect("valid manifest");
    }

    #[test]
    fn rejects_private_or_path_like_ids() {
        let manifest = json!({
            "schema_version": ADAPTER_SCHEMA_VERSION,
            "id": "../hasna/private",
            "kind": "transport",
            "version": "1.0.0",
        });

        let error = validate_adapter_manifest(&manifest).expect_err("invalid manifest");
        assert!(error.contains("id may contain only ASCII"));
    }

    #[test]
    fn rejects_path_marker_ids() {
        let manifest = json!({
            "schema_version": ADAPTER_SCHEMA_VERSION,
            "id": "..",
            "kind": "transport",
            "version": "1.0.0",
        });

        let error = validate_adapter_manifest(&manifest).expect_err("invalid manifest");
        assert!(error.contains("path marker"));
    }

    #[test]
    fn rejects_empty_string_commands() {
        let manifest = json!({
            "schema_version": ADAPTER_SCHEMA_VERSION,
            "id": "example.agent",
            "kind": "agent",
            "version": "1.0.0",
            "command": "   ",
        });

        let error = validate_adapter_manifest(&manifest).expect_err("invalid manifest");
        assert!(error.contains("command must not be empty"));
    }

    #[test]
    fn built_ins_do_not_reference_private_hasna_services() {
        let text = serde_json::to_string(&built_in_adapters()).expect("built-ins json");
        assert!(!text.contains("/home/hasna"));
        assert!(!text.contains("spark"));
        assert!(!text.contains("private"));
    }
}
