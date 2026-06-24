use serde_json::{json, Value};
use std::{fs, process::Command, thread};
use tempfile::tempdir;

#[test]
fn mosaic_help_exposes_agentic_control_surface() {
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .arg("--help")
        .output()
        .expect("mosaic --help should run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Open Mosaic agentic terminal workspace control CLI"));
    assert!(stdout.contains("prompt"));
    assert!(stdout.contains("queue"));
    assert!(stdout.contains("audit"));
    assert!(stdout.contains("adapters"));
    assert!(stdout.contains("machines"));
    assert!(stdout.contains("goals"));
    assert!(stdout.contains("web"));
    assert!(stdout.contains("observe"));
    assert!(stdout.contains("subscribe"));
    assert!(stdout.contains("dashboard"));
}

#[test]
fn real_agent_smoke_script_keeps_safe_defaults() {
    let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("mosaic-agent-workflow-smoke.sh");
    let script = fs::read_to_string(script_path).expect("real agent smoke script");

    assert!(script.contains("MOSAIC_AGENT_SMOKE_SUBMIT=enter"));
    assert!(script.contains("submit_mode=\"${MOSAIC_AGENT_SMOKE_SUBMIT:-none}\""));
    assert!(script.contains("submit_args=(--no-submit)"));
    assert!(script.contains("and agent.get(\"status\") == \"running\""));
    assert!(script.contains("agent.get(\"composer_state\") != \"working\""));
    assert!(script.contains("--dry-run prompt send"));
    assert!(script.contains("export MOSAIC_AUDIT_REDACT=1"));
    assert!(script.contains("sessions close \"$SESSION\" --delete"));
    assert!(!script.contains("/home/hasna"));
    assert!(!script.to_ascii_lowercase().contains("spark"));
}

#[test]
fn web_link_defaults_to_read_only_watch_url() {
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args(["web", "link", "--session", "work"])
        .output()
        .expect("mosaic web link should run");
    assert!(output.status.success());

    let envelope: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("web link");
    assert_eq!(envelope["schema_version"], "mosaic.control.v1");
    assert_eq!(envelope["event"], "web.link");
    assert_eq!(envelope["web_schema_version"], "mosaic.web.v1");
    assert_eq!(envelope["mode"], "watch");
    assert_eq!(envelope["session"], "work");
    assert_eq!(envelope["url"], "http://127.0.0.1:8082/work");
    assert_eq!(envelope["bookmarkable"], true);
    assert_eq!(envelope["read_only_required"], true);
    assert_eq!(envelope["watcher"], true);
    assert_eq!(envelope["control_allowed"], false);
    assert_eq!(envelope["input_forwarding"], false);
    assert_eq!(envelope["can_create_sessions"], false);
    assert_eq!(envelope["auth"]["link_contains_token"], false);
    assert_eq!(envelope["auth"]["recommended_token_type"], "read_only");
    assert_eq!(envelope["security"]["raw_tokens_in_urls"], "unsupported");
}

#[test]
fn web_link_control_mode_preserves_base_path() {
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args([
            "web",
            "link",
            "--session",
            "agent-work",
            "--base-url",
            "https://mosaic.example.test/base/",
            "--mode",
            "control",
            "--token-name",
            "operator",
        ])
        .output()
        .expect("mosaic web link should run");
    assert!(output.status.success());

    let envelope: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("web link");
    assert_eq!(envelope["mode"], "control");
    assert_eq!(
        envelope["url"],
        "https://mosaic.example.test/base/agent-work"
    );
    assert_eq!(envelope["routes"]["session"], "/base/agent-work");
    assert_eq!(envelope["routes"]["client_session"], "/base/session");
    assert_eq!(envelope["routes"]["control_websocket"], "/base/ws/control");
    assert_eq!(envelope["read_only_required"], false);
    assert_eq!(envelope["watcher"], false);
    assert_eq!(envelope["control_allowed"], true);
    assert_eq!(envelope["input_forwarding"], true);
    assert_eq!(envelope["can_create_sessions"], true);
    assert_eq!(envelope["auth"]["recommended_token_type"], "control");
    assert_eq!(envelope["auth"]["token_name"], "operator");
}

#[test]
fn web_link_rejects_unsafe_base_urls() {
    for base_url in [
        "file:///tmp/mosaic",
        "http://user:secret@example.test",
        "https://example.test/?token=secret",
        "https://example.test/#secret",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
            .args(["web", "link", "--session", "work", "--base-url", base_url])
            .output()
            .expect("mosaic web link should run");
        assert!(!output.status.success(), "{base_url} should be rejected");
        assert!(output.stdout.is_empty());
        let error: Value =
            serde_json::from_str(String::from_utf8_lossy(&output.stderr).trim()).expect("error");
        assert_eq!(error["code"], "invalid_web_base_url");
    }
}

#[test]
fn web_link_redaction_hides_session_url_and_token_label() {
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args([
            "web",
            "link",
            "--session",
            "private-work",
            "--base-url",
            "https://private.example.test/base/",
            "--token-name",
            "private-token",
            "--redact",
        ])
        .output()
        .expect("mosaic web link should run");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("private-work"));
    assert!(!stdout.contains("private.example.test"));
    assert!(!stdout.contains("private-token"));
    let envelope: Value = serde_json::from_str(stdout.trim()).expect("web link");
    assert_eq!(envelope["session"], "[redacted]");
    assert_eq!(envelope["base_url"], "[redacted]");
    assert_eq!(envelope["url"], "[redacted]");
    assert_eq!(envelope["routes"]["session"], "[redacted]");
    assert_eq!(envelope["routes"]["client_session"], "[redacted]");
    assert_eq!(envelope["routes"]["control_websocket"], "[redacted]");
    assert_eq!(envelope["auth"]["token_name"], "[redacted]");
    assert_eq!(envelope["auth"]["link_contains_token"], false);
}

#[test]
fn adapters_list_returns_portable_builtin_interfaces() {
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args(["adapters", "list", "--kind", "agent"])
        .output()
        .expect("mosaic adapters list should run");
    assert!(output.status.success());

    let envelope: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("adapter list");
    assert_eq!(envelope["schema_version"], "mosaic.control.v1");
    assert_eq!(envelope["event"], "adapters.list");
    assert_eq!(envelope["adapter_schema_version"], "mosaic.adapter.v1");
    let adapters = envelope["data"].as_array().expect("adapters");
    assert!(!adapters.is_empty());
    assert!(adapters
        .iter()
        .all(|adapter| adapter["kind"].as_str() == Some("agent")));
    let text = serde_json::to_string(&envelope).expect("adapter list json");
    assert!(!text.contains("/home/hasna"));
    assert!(!text.contains("spark"));
}

#[test]
fn adapters_list_accepts_schema_kind_names() {
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args(["adapters", "list", "--kind", "goal_system"])
        .output()
        .expect("mosaic adapters list should run");
    assert!(output.status.success());

    let envelope: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("adapter list");
    let adapters = envelope["data"].as_array().expect("adapters");
    assert!(!adapters.is_empty());
    assert!(adapters
        .iter()
        .all(|adapter| adapter["kind"].as_str() == Some("goal_system")));
}

#[test]
fn adapters_list_rejects_unknown_kinds_as_json_errors() {
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args(["adapters", "list", "--kind", "hasna_private"])
        .output()
        .expect("mosaic adapters list should run");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());

    let error: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stderr).trim()).expect("error");
    assert_eq!(error["code"], "invalid_adapter_kind");
}

#[test]
fn adapters_validate_accepts_a_manifest_without_executing_it() {
    let temp = tempdir().expect("manifest tempdir");
    let manifest_path = temp.path().join("adapter.json");
    fs::write(
        &manifest_path,
        json!({
            "schema_version": "mosaic.adapter.v1",
            "id": "example.agent",
            "kind": "agent",
            "name": "Example Agent",
            "version": "1.0.0",
            "capabilities": ["pane.detect", "prompt.send"],
            "command": ["example-agent", "--stdio"]
        })
        .to_string(),
    )
    .expect("write manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args([
            "adapters",
            "validate",
            "--file",
            manifest_path.to_str().expect("manifest path"),
        ])
        .output()
        .expect("mosaic adapters validate should run");
    assert!(output.status.success());

    let envelope: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("validation");
    assert_eq!(envelope["event"], "adapters.validate");
    assert_eq!(envelope["valid"], true);
    assert_eq!(envelope["adapter"]["id"], "example.agent");
}

#[test]
fn adapters_validate_rejects_invalid_manifests() {
    let temp = tempdir().expect("manifest tempdir");
    let manifest_path = temp.path().join("adapter.json");
    fs::write(
        &manifest_path,
        json!({
            "schema_version": "mosaic.adapter.v1",
            "id": "../bad",
            "kind": "hasna_private",
            "version": "1.0.0"
        })
        .to_string(),
    )
    .expect("write manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args([
            "adapters",
            "validate",
            "--file",
            manifest_path.to_str().expect("manifest path"),
        ])
        .output()
        .expect("mosaic adapters validate should run");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stderr).trim()).expect("error");
    assert_eq!(error["code"], "invalid_adapter_manifest");
}

fn goals_registry_json() -> Value {
    json!({
        "schema_version": "mosaic.goals.v1",
        "source": {
            "kind": "file",
            "adapter": "generic",
            "configured": true,
            "project_path": "/private/project"
        },
        "goals": [
            {
                "id": "goal-1",
                "title": "Secret launch goal",
                "status": "active",
                "description": "Ship privately"
            }
        ],
        "tasks": [
            {
                "id": "task-1",
                "goal_id": "goal-1",
                "title": "Secret task title",
                "description": "Sensitive task body",
                "status": "in_progress",
                "priority": "high",
                "agent": "cli",
                "blocked": false,
                "tags": ["goals"]
            },
            {
                "id": "task-2",
                "goal_id": "goal-1",
                "title": "Blocked task",
                "status": "blocked",
                "blocked": true
            }
        ]
    })
}

#[test]
fn goals_list_without_config_returns_empty_portable_registry() {
    let config_dir = tempdir().expect("config tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_CONFIG_HOME", config_dir.path())
        .args(["goals", "list"])
        .output()
        .expect("mosaic goals list should run");
    assert!(output.status.success());

    let envelope: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("goals list");
    assert_eq!(envelope["schema_version"], "mosaic.control.v1");
    assert_eq!(envelope["event"], "goals.list");
    assert_eq!(envelope["goal_schema_version"], "mosaic.goals.v1");
    assert_eq!(envelope["source"]["loaded"], false);
    assert_eq!(envelope["source"]["missing"], true);
    assert_eq!(envelope["summary"]["configured"], false);
    assert_eq!(envelope["summary"]["total_tasks"], 0);
}

#[test]
fn goals_validate_accepts_portable_registry() {
    let temp = tempdir().expect("goals tempdir");
    let registry_path = temp.path().join("goals.json");
    fs::write(&registry_path, goals_registry_json().to_string()).expect("write registry");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args([
            "goals",
            "validate",
            "--file",
            registry_path.to_str().expect("registry path"),
        ])
        .output()
        .expect("mosaic goals validate should run");
    assert!(output.status.success());

    let envelope: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("validation");
    assert_eq!(envelope["event"], "goals.validate");
    assert_eq!(envelope["valid"], true);
    assert_eq!(envelope["registry"]["schema_version"], "mosaic.goals.v1");
}

#[test]
fn goals_validate_rejects_duplicate_task_ids() {
    let temp = tempdir().expect("goals tempdir");
    let registry_path = temp.path().join("goals.json");
    let mut registry = goals_registry_json();
    registry["tasks"][1]["id"] = json!("task-1");
    fs::write(&registry_path, registry.to_string()).expect("write registry");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args([
            "goals",
            "validate",
            "--file",
            registry_path.to_str().expect("registry path"),
        ])
        .output()
        .expect("mosaic goals validate should run");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stderr).trim()).expect("error");
    assert_eq!(error["code"], "invalid_goals_registry");
}

#[test]
fn goals_list_redacts_titles_descriptions_and_paths() {
    let temp = tempdir().expect("goals tempdir");
    let registry_path = temp.path().join("goals.json");
    fs::write(&registry_path, goals_registry_json().to_string()).expect("write registry");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args([
            "goals",
            "list",
            "--file",
            registry_path.to_str().expect("registry path"),
            "--redact",
        ])
        .output()
        .expect("mosaic goals list should run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("Secret"));
    assert!(!stdout.contains("Sensitive"));
    assert!(!stdout.contains("/private/project"));
    assert!(!stdout.contains(registry_path.to_str().expect("registry path")));
    let envelope: Value = serde_json::from_str(stdout.trim()).expect("goals list");
    assert_eq!(envelope["data"]["tasks"][0]["title"], "[redacted]");
    assert_eq!(envelope["summary"]["active"][0]["title"], "[redacted]");
    assert_eq!(envelope["summary"]["blocked_tasks"], 1);
}

#[test]
fn goals_todos_plan_dry_run_redacts_project_path() {
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args([
            "--dry-run",
            "goals",
            "todos-plan",
            "--project",
            "/private/project",
            "--plan",
            "plan-1",
            "--todos-bin",
            "/private/bin/todos",
            "--redact",
        ])
        .output()
        .expect("mosaic goals todos dry-run should run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("/private/project"));
    assert!(!stdout.contains("/private/bin/todos"));
    let envelope: Value = serde_json::from_str(stdout.trim()).expect("todos dry-run");
    assert_eq!(envelope["event"], "goals.todos_plan");
    assert_eq!(envelope["status"], "dry_run");
    assert!(envelope["command"]["argv"]
        .as_array()
        .expect("argv")
        .iter()
        .any(|segment| segment.as_str() == Some("[redacted]")));
}

#[test]
fn goals_todos_plan_spawn_error_honors_redaction() {
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args([
            "goals",
            "todos-plan",
            "--project",
            "/private/project",
            "--plan",
            "plan-1",
            "--todos-bin",
            "/private/bin/missing-todos",
            "--redact",
        ])
        .output()
        .expect("mosaic goals todos spawn should run");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("/private/project"));
    assert!(!stderr.contains("/private/bin/missing-todos"));
    let error: Value = serde_json::from_str(stderr.trim()).expect("error");
    assert_eq!(error["code"], "goals_todos_failed");
}

#[cfg(unix)]
#[test]
fn goals_todos_plan_imports_external_todos_json() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().expect("todos tempdir");
    let todos_bin = temp.path().join("fake-todos");
    fs::write(
        &todos_bin,
        r#"#!/bin/sh
cat <<'JSON'
{"plan":{"id":"plan-1","name":"Portable plan","description":"Use adapter","status":"active"},"tasks":[{"id":"task-1","title":"Adapter task","description":"Run import","status":"in_progress","priority":"medium","assigned_to":"cli","tags":["adapter"]}]}
JSON
"#,
    )
    .expect("write fake todos");
    let mut permissions = fs::metadata(&todos_bin)
        .expect("fake todos metadata")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&todos_bin, permissions).expect("chmod fake todos");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args([
            "goals",
            "todos-plan",
            "--project",
            temp.path().to_str().expect("project path"),
            "--plan",
            "plan-1",
            "--todos-bin",
            todos_bin.to_str().expect("todos path"),
        ])
        .output()
        .expect("mosaic goals todos import should run");
    assert!(output.status.success());

    let envelope: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("todos import");
    assert_eq!(envelope["event"], "goals.todos_plan");
    assert_eq!(envelope["status"], "completed");
    assert_eq!(envelope["data"]["schema_version"], "mosaic.goals.v1");
    assert_eq!(envelope["data"]["source"]["adapter"], "todos");
    assert_eq!(envelope["data"]["goals"][0]["title"], "Portable plan");
    assert_eq!(envelope["summary"]["active_tasks"], 1);
}

fn machine_registry_json() -> Value {
    json!({
        "schema_version": "mosaic.machine.v1",
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
fn machines_local_returns_portable_descriptor() {
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args(["machines", "local"])
        .output()
        .expect("mosaic machines local should run");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: Value = serde_json::from_str(stdout.trim()).expect("local machine json");
    assert_eq!(envelope["schema_version"], "mosaic.control.v1");
    assert_eq!(envelope["event"], "machines.local");
    assert_eq!(envelope["machine_schema_version"], "mosaic.machine.v1");
    assert_eq!(envelope["data"]["id"], "local");
    assert_eq!(envelope["data"]["transport"]["kind"], "local");
    assert!(!stdout.contains("/home/hasna"));
    assert!(!stdout.contains("\"user\":\"hasna\""));
    assert!(!stdout.to_ascii_lowercase().contains("spark"));
}

#[test]
fn machines_validate_accepts_ssh_registry_without_connecting() {
    let temp = tempdir().expect("machine tempdir");
    let registry_path = temp.path().join("machines.json");
    fs::write(&registry_path, machine_registry_json().to_string()).expect("write registry");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args([
            "machines",
            "validate",
            "--file",
            registry_path.to_str().expect("registry path"),
        ])
        .output()
        .expect("mosaic machines validate should run");
    assert!(output.status.success());

    let envelope: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("validation");
    assert_eq!(envelope["event"], "machines.validate");
    assert_eq!(envelope["valid"], true);
    assert_eq!(envelope["registry"]["machines"][0]["id"], "dev-box");
}

#[test]
fn machines_list_includes_local_and_configured_machines() {
    let temp = tempdir().expect("machine tempdir");
    let registry_path = temp.path().join("machines.json");
    fs::write(&registry_path, machine_registry_json().to_string()).expect("write registry");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args([
            "machines",
            "list",
            "--file",
            registry_path.to_str().expect("registry path"),
        ])
        .output()
        .expect("mosaic machines list should run");
    assert!(output.status.success());

    let envelope: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("machine list");
    assert_eq!(envelope["event"], "machines.list");
    assert_eq!(envelope["registry"]["loaded"], true);
    let machines = envelope["data"].as_array().expect("machines");
    assert!(machines
        .iter()
        .any(|machine| machine["id"].as_str() == Some("local")));
    assert!(machines
        .iter()
        .any(|machine| machine["id"].as_str() == Some("dev-box")));
}

#[test]
fn machines_validate_rejects_unsafe_ssh_hosts() {
    let temp = tempdir().expect("machine tempdir");
    let registry_path = temp.path().join("machines.json");
    let mut registry = machine_registry_json();
    registry["machines"][0]["transport"]["host"] = json!("-oProxyCommand=bad");
    fs::write(&registry_path, registry.to_string()).expect("write registry");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args([
            "machines",
            "validate",
            "--file",
            registry_path.to_str().expect("registry path"),
        ])
        .output()
        .expect("mosaic machines validate should run");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());

    let error: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stderr).trim()).expect("error");
    assert_eq!(error["code"], "invalid_machine_registry");
    assert!(error["message"]
        .as_str()
        .expect("message")
        .contains("ssh host must not start"));
}

#[test]
fn machines_list_rejects_config_that_shadows_builtin_local_machine() {
    let temp = tempdir().expect("machine tempdir");
    let registry_path = temp.path().join("machines.json");
    let mut registry = machine_registry_json();
    registry["machines"][0]["id"] = json!("local");
    fs::write(&registry_path, registry.to_string()).expect("write registry");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args([
            "machines",
            "list",
            "--file",
            registry_path.to_str().expect("registry path"),
        ])
        .output()
        .expect("mosaic machines list should run");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());

    let error: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stderr).trim()).expect("error");
    assert_eq!(error["code"], "invalid_machine_registry");
    assert!(error["message"]
        .as_str()
        .expect("message")
        .contains("reserved"));
}

#[test]
fn machines_exec_dry_run_plans_ssh_without_leaking_redacted_prompt() {
    let temp = tempdir().expect("machine tempdir");
    let state_dir = temp.path().join("state");
    let registry_path = temp.path().join("machines.json");
    fs::write(&registry_path, machine_registry_json().to_string()).expect("write registry");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", &state_dir)
        .args([
            "--dry-run",
            "machines",
            "exec",
            "--file",
            registry_path.to_str().expect("registry path"),
            "--machine",
            "dev-box",
            "--redact-command",
            "--",
            "prompt",
            "send",
            "--text",
            "hello; rm -rf /",
        ])
        .output()
        .expect("mosaic machines exec dry-run should run");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let event: Value = serde_json::from_str(stdout.trim()).expect("machine exec event");
    assert_eq!(event["event"], "machines.exec");
    assert_eq!(event["status"], "dry_run");
    assert_eq!(event["ack"], "none");
    assert_eq!(event["machine"], "dev-box");
    assert_eq!(event["transport"], "ssh");
    assert_eq!(event["command"]["argv"][0], "ssh");
    assert_eq!(event["command"]["argv"][6], "[redacted]");
    assert_eq!(event["command"]["mosaic_command"][4], "[redacted]");
    assert_eq!(event["command"]["remote_shell_command"], "[redacted]");
    assert!(!stdout.contains("hello; rm -rf /"));

    let audit_path = state_dir.join("open-mosaic").join("audit.ndjson");
    let audit = fs::read_to_string(audit_path).expect("audit record");
    assert!(!audit.contains("hello; rm -rf /"));
    assert!(audit.contains("\"operation\":\"machines.exec\""));
}

#[test]
fn machines_exec_dry_run_shell_quotes_ssh_remote_command() {
    let temp = tempdir().expect("machine tempdir");
    let registry_path = temp.path().join("machines.json");
    fs::write(&registry_path, machine_registry_json().to_string()).expect("write registry");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args([
            "--dry-run",
            "machines",
            "exec",
            "--file",
            registry_path.to_str().expect("registry path"),
            "--machine",
            "dev-box",
            "--",
            "sessions",
            "list",
            "--filter",
            "name; whoami",
        ])
        .output()
        .expect("mosaic machines exec dry-run should run");
    assert!(output.status.success());

    let event: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("exec event");
    assert_eq!(
        event["command"]["remote_shell_command"],
        "/usr/local/bin/mosaic sessions list --filter 'name; whoami'"
    );
    assert_eq!(
        event["command"]["args"][5],
        "/usr/local/bin/mosaic sessions list --filter 'name; whoami'"
    );
}

#[test]
fn machines_exec_local_runs_mosaic_command_without_registry() {
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args([
            "machines",
            "exec",
            "--machine",
            "local",
            "--mosaic-bin",
            env!("CARGO_BIN_EXE_mosaic"),
            "--",
            "--version",
        ])
        .output()
        .expect("mosaic machines exec local should run");
    assert!(output.status.success());

    let event: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("exec event");
    assert_eq!(event["event"], "machines.exec");
    assert_eq!(event["status"], "completed");
    assert_eq!(event["ack"], "process_exited");
    assert_eq!(event["machine"], "local");
    assert_eq!(event["transport"], "local");
    assert_eq!(event["exit_code"], 0);
    assert!(event["stdout"].as_str().expect("stdout").contains("mosaic"));
}

#[test]
fn prompt_dry_run_emits_versioned_receipt_without_connecting() {
    let state_dir = tempdir().expect("state tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "test-session",
            "--dry-run",
            "prompt",
            "send",
            "--pane-id",
            "terminal_1",
            "--text",
            "hello",
        ])
        .output()
        .expect("mosaic prompt dry-run should run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let receipt: Value = serde_json::from_str(stdout.trim()).expect("receipt json");
    assert_eq!(receipt["schema_version"], "mosaic.control.v1");
    assert_eq!(receipt["operation"], "prompt.send");
    assert_eq!(receipt["status"], "dry_run");
    assert_eq!(receipt["pane_id"], "terminal_1");
}

#[test]
fn prompt_queue_writes_ndjson_queue_record() {
    let state_dir = tempdir().expect("state tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "queued-session",
            "prompt",
            "send",
            "--pane-id",
            "terminal_1",
            "--queue",
            "--text",
            "line one\nline two",
        ])
        .output()
        .expect("mosaic prompt queue should run");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let receipt: Value = serde_json::from_str(stdout.trim()).expect("receipt json");
    assert_eq!(receipt["status"], "queued");

    let queue_path = state_dir
        .path()
        .join("open-mosaic")
        .join("queues")
        .join("queued-session")
        .join("terminal_1.ndjson");
    let queue = fs::read_to_string(queue_path).expect("queue file");
    let record: Value = serde_json::from_str(queue.trim()).expect("queue json");
    assert_eq!(record["schema_version"], "mosaic.control.v1");
    assert_eq!(record["event"], "queued_prompt");
    assert_eq!(record["session"], "queued-session");
    assert_eq!(record["pane_id"], "terminal_1");
    assert!(record["timestamp_ms"].as_u64().is_some());
    assert_eq!(record["prompt"], "line one\nline two");
}

#[test]
fn queue_list_redacts_queued_prompt_records() {
    let state_dir = tempdir().expect("state tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "queued-session",
            "prompt",
            "send",
            "--pane-id",
            "terminal_1",
            "--queue",
            "--text",
            "secret prompt",
        ])
        .output()
        .expect("mosaic prompt queue should run");
    assert!(output.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "queued-session",
            "queue",
            "list",
            "--pane-id",
            "terminal_1",
            "--redact",
            "--limit",
            "1",
        ])
        .output()
        .expect("mosaic queue list should run");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: Value = serde_json::from_str(stdout.trim()).expect("queue list json");
    assert_eq!(envelope["schema_version"], "mosaic.control.v1");
    assert_eq!(envelope["event"], "queue.list");
    assert_eq!(envelope["data"][0]["event"], "queued_prompt");
    assert_eq!(envelope["data"][0]["prompt"], "[redacted]");
}

#[test]
fn queue_clear_removes_a_specific_receipt() {
    let state_dir = tempdir().expect("state tempdir");
    let first = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "queued-session",
            "prompt",
            "send",
            "--pane-id",
            "terminal_1",
            "--queue",
            "--text",
            "first",
        ])
        .output()
        .expect("first queue should run");
    assert!(first.status.success());
    let first_receipt: Value =
        serde_json::from_str(String::from_utf8_lossy(&first.stdout).trim()).expect("first receipt");

    let second = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "queued-session",
            "prompt",
            "send",
            "--pane-id",
            "terminal_1",
            "--queue",
            "--text",
            "second",
        ])
        .output()
        .expect("second queue should run");
    assert!(second.status.success());
    let second_receipt: Value =
        serde_json::from_str(String::from_utf8_lossy(&second.stdout).trim())
            .expect("second receipt");

    let clear = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "queued-session",
            "queue",
            "clear",
            "--pane-id",
            "terminal_1",
            "--receipt-id",
            first_receipt["id"].as_str().expect("first receipt id"),
        ])
        .output()
        .expect("queue clear should run");
    assert!(clear.status.success());
    let clear_receipt: Value =
        serde_json::from_str(String::from_utf8_lossy(&clear.stdout).trim()).expect("clear receipt");
    assert_eq!(clear_receipt["operation"], "queue.clear");
    assert_eq!(clear_receipt["status"], "accepted");
    assert_eq!(clear_receipt["ack"], "local_state_updated");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "queued-session",
            "queue",
            "list",
            "--pane-id",
            "terminal_1",
        ])
        .output()
        .expect("queue list should run");
    assert!(output.status.success());
    let envelope: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("queue list");
    let records = envelope["data"].as_array().expect("queue records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["prompt"], "second");
    assert_eq!(records[0]["receipt"]["id"], second_receipt["id"]);
}

#[test]
fn queue_clear_dry_run_does_not_mutate_queue() {
    let state_dir = tempdir().expect("state tempdir");
    let queued = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "queued-session",
            "prompt",
            "send",
            "--pane-id",
            "terminal_1",
            "--queue",
            "--text",
            "keep me",
        ])
        .output()
        .expect("queue should run");
    assert!(queued.status.success());

    let clear = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "queued-session",
            "--dry-run",
            "queue",
            "clear",
            "--pane-id",
            "terminal_1",
        ])
        .output()
        .expect("dry-run clear should run");
    assert!(clear.status.success());
    let receipt: Value =
        serde_json::from_str(String::from_utf8_lossy(&clear.stdout).trim()).expect("receipt");
    assert_eq!(receipt["operation"], "queue.clear");
    assert_eq!(receipt["status"], "dry_run");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "queued-session",
            "queue",
            "list",
            "--pane-id",
            "terminal_1",
        ])
        .output()
        .expect("queue list should run");
    assert!(output.status.success());
    let envelope: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("queue list");
    let records = envelope["data"].as_array().expect("queue records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["prompt"], "keep me");
}

#[test]
fn queue_clear_missing_receipt_fails_without_mutation() {
    let state_dir = tempdir().expect("state tempdir");
    let queued = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "queued-session",
            "prompt",
            "send",
            "--pane-id",
            "terminal_1",
            "--queue",
            "--text",
            "still queued",
        ])
        .output()
        .expect("queue should run");
    assert!(queued.status.success());

    let clear = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "queued-session",
            "queue",
            "clear",
            "--pane-id",
            "terminal_1",
            "--receipt-id",
            "missing-receipt",
        ])
        .output()
        .expect("missing receipt clear should run");
    assert!(!clear.status.success());
    assert!(clear.stdout.is_empty());
    let error: Value =
        serde_json::from_str(String::from_utf8_lossy(&clear.stderr).trim()).expect("error");
    assert_eq!(error["code"], "queue_record_not_found");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "queued-session",
            "queue",
            "list",
            "--pane-id",
            "terminal_1",
        ])
        .output()
        .expect("queue list should run");
    assert!(output.status.success());
    let envelope: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("queue list");
    let records = envelope["data"].as_array().expect("queue records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["prompt"], "still queued");
}

#[test]
fn queue_clear_rejects_session_path_traversal() {
    let state_dir = tempdir().expect("state tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "../../escape",
            "queue",
            "clear",
            "--pane-id",
            "terminal_1",
        ])
        .output()
        .expect("queue clear should run");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    let error: Value = serde_json::from_str(stderr.trim()).expect("error json");
    assert_eq!(error["code"], "invalid_path_component");
    assert!(!state_dir.path().join("escape").exists());
}

#[test]
fn queue_clear_preserves_concurrent_appends_for_other_receipts() {
    let state_dir = tempdir().expect("state tempdir");
    let first = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "queued-session",
            "prompt",
            "send",
            "--pane-id",
            "terminal_1",
            "--queue",
            "--text",
            "first",
        ])
        .output()
        .expect("first queue should run");
    assert!(first.status.success());
    let first_receipt: Value =
        serde_json::from_str(String::from_utf8_lossy(&first.stdout).trim()).expect("first receipt");
    let first_id = first_receipt["id"]
        .as_str()
        .expect("first receipt id")
        .to_owned();

    let state_path = state_dir.path().to_path_buf();
    let clear_state_path = state_path.clone();
    let clear_thread = thread::spawn(move || {
        Command::new(env!("CARGO_BIN_EXE_mosaic"))
            .env("XDG_STATE_HOME", clear_state_path)
            .args([
                "--session",
                "queued-session",
                "queue",
                "clear",
                "--pane-id",
                "terminal_1",
                "--receipt-id",
                &first_id,
            ])
            .output()
            .expect("queue clear should run")
    });

    let append_threads = (0..12)
        .map(|index| {
            let state_path = state_path.clone();
            thread::spawn(move || {
                Command::new(env!("CARGO_BIN_EXE_mosaic"))
                    .env("XDG_STATE_HOME", state_path)
                    .args([
                        "--session",
                        "queued-session",
                        "prompt",
                        "send",
                        "--pane-id",
                        "terminal_1",
                        "--queue",
                        "--text",
                        &format!("append-{index}"),
                    ])
                    .output()
                    .expect("append should run")
            })
        })
        .collect::<Vec<_>>();

    let clear = clear_thread.join().expect("clear thread");
    assert!(clear.status.success());
    for append_thread in append_threads {
        let output = append_thread.join().expect("append thread");
        assert!(output.status.success());
    }

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "queued-session",
            "queue",
            "list",
            "--pane-id",
            "terminal_1",
        ])
        .output()
        .expect("queue list should run");
    assert!(output.status.success());
    let envelope: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("queue list");
    let prompts = envelope["data"]
        .as_array()
        .expect("queue records")
        .iter()
        .map(|record| record["prompt"].as_str().expect("prompt").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(prompts.len(), 12);
    assert!(!prompts.iter().any(|prompt| prompt == "first"));
    for index in 0..12 {
        assert!(prompts
            .iter()
            .any(|prompt| prompt == &format!("append-{index}")));
    }
}

#[test]
fn audit_list_reads_recent_receipts() {
    let state_dir = tempdir().expect("state tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "audit-session",
            "--dry-run",
            "prompt",
            "send",
            "--pane-id",
            "terminal_1",
            "--text",
            "audit me",
        ])
        .output()
        .expect("mosaic prompt dry-run should run");
    assert!(output.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args(["audit", "list", "--limit", "1"])
        .output()
        .expect("mosaic audit list should run");
    assert!(output.status.success());
    let envelope: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("audit list");
    assert_eq!(envelope["event"], "audit.list");
    assert_eq!(envelope["data"][0]["event"], "receipt");
    assert_eq!(envelope["data"][0]["operation"], "prompt.send");
    assert_eq!(envelope["data"][0]["status"], "dry_run");
}

#[cfg(unix)]
#[test]
fn prompt_queue_uses_private_unix_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let state_dir = tempdir().expect("state tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "private-session",
            "prompt",
            "send",
            "--pane-id",
            "terminal_1",
            "--queue",
            "--text",
            "secret",
        ])
        .output()
        .expect("mosaic prompt queue should run");
    assert!(output.status.success());

    let queue_dir = state_dir
        .path()
        .join("open-mosaic")
        .join("queues")
        .join("private-session");
    let state_root = state_dir.path().join("open-mosaic");
    let queues_root = state_root.join("queues");
    let queue_file = queue_dir.join("terminal_1.ndjson");
    assert_eq!(
        fs::metadata(state_root)
            .expect("state root metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(queues_root)
            .expect("queues root metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(queue_dir)
            .expect("queue dir metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(queue_file)
            .expect("queue file metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn prompt_queue_rejects_session_path_traversal() {
    let state_dir = tempdir().expect("state tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "../../escape",
            "prompt",
            "send",
            "--pane-id",
            "terminal_1",
            "--queue",
            "--text",
            "do not write",
        ])
        .output()
        .expect("mosaic prompt queue should run");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    let error: Value = serde_json::from_str(stderr.trim()).expect("error json");
    assert_eq!(error["code"], "invalid_path_component");
    assert!(!state_dir.path().join("escape").exists());
}

#[test]
fn prompt_queue_rejects_invalid_pane_id_before_receipt() {
    let state_dir = tempdir().expect("state tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "--session",
            "queued-session",
            "prompt",
            "send",
            "--pane-id",
            "../pane",
            "--queue",
            "--text",
            "do not write",
        ])
        .output()
        .expect("mosaic prompt queue should run");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    let error: Value = serde_json::from_str(stderr.trim()).expect("error json");
    assert_eq!(error["code"], "invalid_pane_id");
}

#[test]
fn prompt_queue_does_not_emit_success_receipt_when_persistence_fails() {
    let state_dir = tempdir().expect("state tempdir");
    let blocking_file = state_dir.path().join("not-a-directory");
    fs::write(&blocking_file, "blocks state dir").expect("blocking file");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", &blocking_file)
        .args([
            "--session",
            "queued-session",
            "prompt",
            "send",
            "--pane-id",
            "terminal_1",
            "--queue",
            "--text",
            "do not report queued",
        ])
        .output()
        .expect("mosaic prompt queue should run");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    let error: Value = serde_json::from_str(stderr.trim()).expect("error json");
    assert_eq!(error["code"], "state_write_failed");
}

#[test]
fn runtime_errors_are_machine_readable_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .args(["--session", "missing-session", "panes", "list"])
        .output()
        .expect("mosaic panes list should run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let error: Value = serde_json::from_str(stderr.trim()).expect("error json");
    assert_eq!(error["schema_version"], "mosaic.control.v1");
    assert_eq!(error["event"], "error");
}

#[test]
fn dashboard_json_summarizes_local_queues_with_prompt_redaction_by_default() {
    let state_dir = tempdir().expect("state tempdir");
    let config_dir = tempdir().expect("config tempdir");
    let queued = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .env("XDG_CONFIG_HOME", config_dir.path())
        .args([
            "--session",
            "dashboard-session",
            "prompt",
            "send",
            "--pane-id",
            "terminal_1",
            "--queue",
            "--text",
            "secret dashboard prompt",
        ])
        .output()
        .expect("queue prompt");
    assert!(queued.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .env("XDG_CONFIG_HOME", config_dir.path())
        .args(["dashboard", "--limit", "5"])
        .output()
        .expect("dashboard");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let dashboard: Value = serde_json::from_str(stdout.trim()).expect("dashboard json");
    assert_eq!(dashboard["schema_version"], "mosaic.control.v1");
    assert_eq!(dashboard["event"], "dashboard.snapshot");
    assert_eq!(dashboard["state_scope"], "local_user");
    assert_eq!(dashboard["queues"]["total_pending"], 1);
    assert_eq!(dashboard["queues"]["prompt_bodies"], "redacted");
    assert_eq!(dashboard["queues"]["recent"][0]["prompt"], "[redacted]");
    assert_eq!(
        dashboard["queues"]["by_session"][0]["session"],
        "dashboard-session"
    );
    assert!(!stdout.contains("secret dashboard prompt"));
}

#[test]
fn dashboard_json_includes_optional_goals_registry_with_redaction() {
    let state_dir = tempdir().expect("state tempdir");
    let config_dir = tempdir().expect("config tempdir");
    let registry_path = config_dir.path().join("goals.json");
    fs::write(&registry_path, goals_registry_json().to_string()).expect("write registry");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "dashboard",
            "--goals-file",
            registry_path.to_str().expect("registry path"),
            "--redact",
        ])
        .output()
        .expect("dashboard");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("Secret"));
    assert!(!stdout.contains("/private/project"));
    let dashboard: Value = serde_json::from_str(stdout.trim()).expect("dashboard json");
    assert_eq!(dashboard["goals"]["status"], "loaded");
    assert_eq!(dashboard["goals"]["summary"]["total_goals"], 1);
    assert_eq!(dashboard["goals"]["summary"]["total_tasks"], 2);
    assert_eq!(dashboard["goals"]["summary"]["active_tasks"], 1);
    assert_eq!(
        dashboard["goals"]["summary"]["active"][0]["title"],
        "[redacted]"
    );
}

#[test]
fn dashboard_invalid_goals_registry_returns_partial_snapshot() {
    let state_dir = tempdir().expect("state tempdir");
    let config_dir = tempdir().expect("config tempdir");
    let registry_path = config_dir.path().join("goals.json");
    fs::write(&registry_path, "{\"schema_version\":\"bad\"}").expect("write registry");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .args([
            "dashboard",
            "--goals-file",
            registry_path.to_str().expect("registry path"),
        ])
        .output()
        .expect("dashboard");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let dashboard: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("dashboard");
    assert_eq!(dashboard["partial"], true);
    assert_eq!(dashboard["errors"][0]["section"], "goals");
    assert_eq!(dashboard["goals"]["status"], "error");
    assert_eq!(dashboard["queues"]["total_pending"], 0);
}

#[test]
fn dashboard_invalid_audit_state_returns_partial_snapshot() {
    let state_dir = tempdir().expect("state tempdir");
    let config_dir = tempdir().expect("config tempdir");
    let mosaic_state = state_dir.path().join("open-mosaic");
    fs::create_dir_all(&mosaic_state).expect("state dir");
    fs::write(mosaic_state.join("audit.ndjson"), "{not-json}\n").expect("audit");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .env("XDG_CONFIG_HOME", config_dir.path())
        .args(["dashboard"])
        .output()
        .expect("dashboard");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let dashboard: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("dashboard");
    assert_eq!(dashboard["partial"], true);
    assert_eq!(dashboard["errors"][0]["section"], "audit");
    assert_eq!(dashboard["audit"]["total_records"], 0);
    assert_eq!(dashboard["queues"]["total_pending"], 0);
}

#[test]
fn dashboard_invalid_queue_state_returns_partial_snapshot() {
    let state_dir = tempdir().expect("state tempdir");
    let config_dir = tempdir().expect("config tempdir");
    let queue_dir = state_dir
        .path()
        .join("open-mosaic")
        .join("queues")
        .join("bad-session");
    fs::create_dir_all(&queue_dir).expect("queue dir");
    fs::write(queue_dir.join("terminal_1.ndjson"), "{not-json}\n").expect("queue");

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .env("XDG_CONFIG_HOME", config_dir.path())
        .args(["dashboard"])
        .output()
        .expect("dashboard");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let dashboard: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).expect("dashboard");
    assert_eq!(dashboard["partial"], true);
    assert_eq!(dashboard["errors"][0]["section"], "queues");
    assert_eq!(dashboard["queues"]["total_pending"], 0);
    assert_eq!(dashboard["audit"]["total_records"], 0);
}

#[test]
fn dashboard_live_missing_session_returns_partial_snapshot() {
    let state_dir = tempdir().expect("state tempdir");
    let config_dir = tempdir().expect("config tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .env("XDG_CONFIG_HOME", config_dir.path())
        .args([
            "--session",
            "missing-dashboard-live-session",
            "dashboard",
            "--live",
        ])
        .output()
        .expect("dashboard live");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let dashboard: Value = serde_json::from_str(stdout.trim()).expect("dashboard json");
    assert_eq!(dashboard["event"], "dashboard.snapshot");
    assert_eq!(dashboard["partial"], true);
    assert_eq!(dashboard["errors"][0]["section"], "live");
    assert_eq!(dashboard["errors"][0]["code"], "session_not_found");
    assert_eq!(dashboard["live"]["status"], "error");
    assert_eq!(dashboard["queues"]["total_pending"], 0);
}

#[test]
fn dashboard_prompt_bodies_require_explicit_opt_in_and_redact_wins() {
    let state_dir = tempdir().expect("state tempdir");
    let config_dir = tempdir().expect("config tempdir");
    let queued = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .env("XDG_CONFIG_HOME", config_dir.path())
        .args([
            "--session",
            "dashboard-session",
            "prompt",
            "send",
            "--pane-id",
            "terminal_1",
            "--queue",
            "--text",
            "show only when asked",
        ])
        .output()
        .expect("queue prompt");
    assert!(queued.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .env("XDG_CONFIG_HOME", config_dir.path())
        .args(["dashboard", "--show-prompts"])
        .output()
        .expect("dashboard with prompts");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("show only when asked"));

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .env("XDG_CONFIG_HOME", config_dir.path())
        .args(["dashboard", "--show-prompts", "--redact"])
        .output()
        .expect("redacted dashboard");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("show only when asked"));
    let dashboard: Value = serde_json::from_str(stdout.trim()).expect("dashboard json");
    assert_eq!(dashboard["queues"]["prompt_bodies"], "redacted");
}

#[test]
fn dashboard_text_is_compact_and_redacted() {
    let state_dir = tempdir().expect("state tempdir");
    let config_dir = tempdir().expect("config tempdir");
    let queued = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .env("XDG_CONFIG_HOME", config_dir.path())
        .args([
            "--session",
            "dashboard-session",
            "prompt",
            "send",
            "--pane-id",
            "terminal_1",
            "--queue",
            "--text",
            "text dashboard secret",
        ])
        .output()
        .expect("queue prompt");
    assert!(queued.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .env("XDG_CONFIG_HOME", config_dir.path())
        .args(["dashboard", "--format", "text"])
        .output()
        .expect("dashboard text");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Open Mosaic Dashboard"));
    assert!(stdout.contains("Queues: 1 pending (redacted)"));
    assert!(stdout.contains("Goals: 0 goals, 0 tasks (not_configured)"));
    assert!(stdout.contains("Live: not_requested"));
    assert!(stdout.contains("Agent Metadata: 0 panes"));
    assert!(!stdout.contains("text dashboard secret"));
}

#[test]
fn dashboard_text_sanitizes_control_sequences_from_state() {
    let state_dir = tempdir().expect("state tempdir");
    let config_dir = tempdir().expect("config tempdir");
    let unsafe_session = "bad\n\x1b]0;pwned\x07";
    let queued = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .env("XDG_CONFIG_HOME", config_dir.path())
        .args([
            "--session",
            unsafe_session,
            "prompt",
            "send",
            "--pane-id",
            "terminal_1",
            "--queue",
            "--text",
            "safe text output",
        ])
        .output()
        .expect("queue prompt");
    assert!(queued.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_mosaic"))
        .env("XDG_STATE_HOME", state_dir.path())
        .env("XDG_CONFIG_HOME", config_dir.path())
        .args(["dashboard", "--format", "text"])
        .output()
        .expect("dashboard text");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains('\x1b'));
    assert!(!stdout.contains('\x07'));
    assert!(!stdout.contains("bad\n"));
    assert!(stdout.contains("bad??]0;pwned?"));
}
