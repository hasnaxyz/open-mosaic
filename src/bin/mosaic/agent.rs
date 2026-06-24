use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

const AGENT_SCHEMA_VERSION: &str = "mosaic.agent.v1";
const TITLE_MARKER_CLAUDE: char = '\u{2733}';
const TITLE_MARKER_WORKING_DOT: char = '\u{2802}';
const TITLE_MARKER_WORKING_BLOCK: char = '\u{2838}';

pub(crate) fn enrich_panes_data(data: Value) -> Value {
    match data {
        Value::Array(entries) => Value::Array(entries.into_iter().map(enrich_pane_entry).collect()),
        other => other,
    }
}

fn enrich_pane_entry(entry: Value) -> Value {
    match entry {
        Value::Object(mut object) => {
            if !has_compatible_agent_metadata(&object) {
                let metadata = detect_agent_metadata(&object);
                object.insert("mosaic_agent".to_owned(), metadata);
            }
            Value::Object(object)
        },
        other => other,
    }
}

fn has_compatible_agent_metadata(entry: &Map<String, Value>) -> bool {
    entry
        .get("mosaic_agent")
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get("schema_version"))
        .and_then(Value::as_str)
        == Some(AGENT_SCHEMA_VERSION)
}

fn detect_agent_metadata(entry: &Map<String, Value>) -> Value {
    let command = first_string(entry, &["pane_command", "terminal_command", "plugin_url"]);
    let cwd = first_string(entry, &["pane_cwd"]);
    let title = first_string(entry, &["title"]);
    let is_plugin = entry
        .get("is_plugin")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let exited = entry
        .get("exited")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let held = entry
        .get("is_held")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let detection = detect_kind(command.as_deref(), title.as_deref(), is_plugin);
    let status = if exited {
        "exited"
    } else if held {
        "held"
    } else {
        "running"
    };

    json!({
        "schema_version": AGENT_SCHEMA_VERSION,
        "kind": detection.kind,
        "confidence": detection.confidence,
        "signals": detection.signals,
        "status": status,
        "composer_state": composer_state(title.as_deref(), &detection.kind),
        "submit_keys": submit_keys_for(&detection.kind),
        "cwd": cwd,
        "repo": cwd.as_deref().and_then(detect_repo),
        "command": command,
        "current_task": current_task_from_title(title.as_deref(), &detection.kind),
    })
}

#[derive(Debug, PartialEq)]
struct Detection {
    kind: String,
    confidence: f64,
    signals: Vec<String>,
}

fn detect_kind(command: Option<&str>, title: Option<&str>, is_plugin: bool) -> Detection {
    let command_lower = command.unwrap_or_default().to_ascii_lowercase();
    let title_lower = title.unwrap_or_default().to_ascii_lowercase();
    let command_basename = command.and_then(command_basename).unwrap_or_default();
    let mut signals = Vec::new();

    if is_plugin {
        signals.push("pane:is_plugin".to_owned());
        return Detection {
            kind: "plugin".to_owned(),
            confidence: 0.7,
            signals,
        };
    }

    if contains_any(&command_lower, &["codewith"]) || contains_any(&title_lower, &["codewith"]) {
        signals.push(signal_source("codewith", &command_lower));
        return Detection {
            kind: "codewith".to_owned(),
            confidence: if command_lower.contains("codewith") {
                0.95
            } else {
                0.75
            },
            signals,
        };
    }

    if contains_any(&command_lower, &["claude"]) || contains_any(&title_lower, &["claude code"]) {
        signals.push(signal_source("claude", &command_lower));
        return Detection {
            kind: "claude_code".to_owned(),
            confidence: if command_lower.contains("claude") {
                0.92
            } else {
                0.78
            },
            signals,
        };
    }

    if contains_any(&command_lower, &["opencode", "open-code"]) {
        signals.push("command:opencode".to_owned());
        return Detection {
            kind: "opencode".to_owned(),
            confidence: 0.92,
            signals,
        };
    }

    if contains_any(&command_lower, &["codex", "@openai/codex"]) {
        signals.push("command:codex".to_owned());
        return Detection {
            kind: "codex".to_owned(),
            confidence: 0.9,
            signals,
        };
    }

    if looks_like_log_command(&command_lower, &title_lower) {
        signals.push("command_or_title:logs".to_owned());
        return Detection {
            kind: "log".to_owned(),
            confidence: 0.82,
            signals,
        };
    }

    if looks_like_server_command(&command_lower) {
        signals.push("command:server".to_owned());
        return Detection {
            kind: "server".to_owned(),
            confidence: 0.8,
            signals,
        };
    }

    if matches!(
        command_basename.as_str(),
        "bash" | "zsh" | "fish" | "sh" | "dash" | "nu" | "pwsh" | "powershell" | "cmd.exe"
    ) {
        signals.push(format!("command:{command_basename}"));
        return Detection {
            kind: "shell".to_owned(),
            confidence: 0.8,
            signals,
        };
    }

    Detection {
        kind: "unknown".to_owned(),
        confidence: 0.0,
        signals,
    }
}

fn first_string(entry: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| entry.get(*key))
        .filter_map(Value::as_str)
        .find(|value| !value.trim().is_empty() && *value != "-")
        .map(ToOwned::to_owned)
}

fn command_basename(command: &str) -> Option<String> {
    let first = command.split_whitespace().next()?;
    Path::new(first)
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn signal_source(needle: &str, command_lower: &str) -> String {
    if command_lower.contains(needle) {
        format!("command:{needle}")
    } else {
        format!("title:{needle}")
    }
}

fn looks_like_log_command(command_lower: &str, title_lower: &str) -> bool {
    contains_any(
        command_lower,
        &[
            "tail -f",
            "tail -n",
            "journalctl",
            "docker logs",
            "kubectl logs",
            "pm2 logs",
        ],
    ) || title_lower.contains("log")
}

fn looks_like_server_command(command_lower: &str) -> bool {
    contains_any(
        command_lower,
        &[
            "npm run dev",
            "pnpm dev",
            "bun run dev",
            "yarn dev",
            "vite",
            "next dev",
            "cargo run",
            "uvicorn",
            "rails server",
            "python -m http.server",
        ],
    )
}

fn submit_keys_for(kind: &str) -> Vec<&'static str> {
    match kind {
        "codewith" => vec!["Tab", "Enter"],
        "claude_code" | "opencode" | "codex" | "shell" => vec!["Enter"],
        _ => Vec::new(),
    }
}

fn composer_state(title: Option<&str>, kind: &str) -> &'static str {
    let Some(title) = title else {
        return "unknown";
    };
    if matches!(kind, "codewith" | "claude_code" | "opencode" | "codex")
        && (title.starts_with(TITLE_MARKER_WORKING_DOT)
            || title.starts_with(TITLE_MARKER_WORKING_BLOCK))
    {
        "working"
    } else {
        "unknown"
    }
}

fn current_task_from_title(title: Option<&str>, kind: &str) -> Option<String> {
    if !matches!(kind, "codewith" | "claude_code" | "opencode" | "codex") {
        return None;
    }
    let title = title?.trim();
    let has_agent_marker = title.starts_with(TITLE_MARKER_CLAUDE)
        || title.starts_with(TITLE_MARKER_WORKING_DOT)
        || title.starts_with(TITLE_MARKER_WORKING_BLOCK);
    if !has_agent_marker {
        return None;
    }
    let normalized = title
        .trim_start_matches(TITLE_MARKER_CLAUDE)
        .trim_start_matches(TITLE_MARKER_WORKING_DOT)
        .trim_start_matches(TITLE_MARKER_WORKING_BLOCK)
        .trim();
    if normalized.is_empty()
        || normalized.eq_ignore_ascii_case("claude code")
        || normalized.eq_ignore_ascii_case("codewith")
    {
        None
    } else {
        Some(normalized.to_owned())
    }
}

fn detect_repo(cwd: &str) -> Option<Value> {
    let mut current = PathBuf::from(cwd);
    if !current.is_absolute() {
        return None;
    }
    loop {
        if current.join(".git").exists() {
            let name = current
                .file_name()
                .map(|name| name.to_string_lossy().to_string());
            return Some(json!({
                "path": current.to_string_lossy(),
                "name": name,
            }));
        }
        if !current.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn enriches_codewith_pane_with_repo_metadata() {
        let repo = tempdir().expect("repo dir");
        std::fs::create_dir(repo.path().join(".git")).expect(".git dir");
        let data = json!([{
            "id": 1,
            "is_plugin": false,
            "title": "\u{2838} Build Open Mosaic",
            "exited": false,
            "is_held": false,
            "pane_command": "node /home/user/.bun/bin/codewith --no-alt-screen",
            "pane_cwd": repo.path(),
        }]);

        let enriched = enrich_panes_data(data);
        let agent = &enriched[0]["mosaic_agent"];
        assert_eq!(agent["schema_version"], AGENT_SCHEMA_VERSION);
        assert_eq!(agent["kind"], "codewith");
        assert_eq!(agent["status"], "running");
        assert_eq!(agent["composer_state"], "working");
        assert_eq!(agent["current_task"], "Build Open Mosaic");
        assert_eq!(
            agent["repo"]["path"],
            repo.path().to_string_lossy().as_ref()
        );
        assert!(agent["submit_keys"]
            .as_array()
            .expect("submit keys")
            .iter()
            .any(|key| key == "Tab"));
    }

    #[test]
    fn detects_claude_code_from_command_or_title() {
        let detection = detect_kind(Some("claude"), Some("\u{2733} Claude Code"), false);
        assert_eq!(detection.kind, "claude_code");
        assert!(detection.confidence > 0.9);
    }

    #[test]
    fn detects_opencode_codex_server_log_and_shell_kinds() {
        assert_eq!(
            detect_kind(Some("opencode run"), None, false).kind,
            "opencode"
        );
        assert_eq!(
            detect_kind(Some("/usr/local/bin/codex"), None, false).kind,
            "codex"
        );
        assert_eq!(
            detect_kind(Some("bun run dev"), Some("web"), false).kind,
            "server"
        );
        assert_eq!(
            detect_kind(Some("tail -f app.log"), Some("logs"), false).kind,
            "log"
        );
        assert_eq!(
            detect_kind(Some("/bin/bash"), Some("shell"), false).kind,
            "shell"
        );
    }

    #[test]
    fn plugin_panes_are_not_reported_as_agents() {
        let enriched = enrich_panes_data(json!([{
            "id": 2,
            "is_plugin": true,
            "title": "status-bar",
            "plugin_url": "zellij:status-bar"
        }]));
        assert_eq!(enriched[0]["mosaic_agent"]["kind"], "plugin");
        assert!(enriched[0]["mosaic_agent"]["submit_keys"]
            .as_array()
            .expect("submit keys")
            .is_empty());
    }

    #[test]
    fn existing_mosaic_agent_metadata_is_preserved() {
        let enriched = enrich_panes_data(json!([{
            "id": 1,
            "is_plugin": false,
            "mosaic_agent": {"schema_version": "mosaic.agent.v1", "kind": "custom"}
        }]));
        assert_eq!(enriched[0]["mosaic_agent"]["kind"], "custom");
    }

    #[test]
    fn incompatible_mosaic_agent_metadata_is_replaced() {
        let enriched = enrich_panes_data(json!([{
            "id": 1,
            "is_plugin": false,
            "pane_command": "bash",
            "mosaic_agent": {"schema_version": "future.agent.v9", "kind": "custom"}
        }]));
        assert_eq!(
            enriched[0]["mosaic_agent"]["schema_version"],
            AGENT_SCHEMA_VERSION
        );
        assert_eq!(enriched[0]["mosaic_agent"]["kind"], "shell");
    }

    #[test]
    fn non_array_payloads_are_not_modified() {
        let payload = json!({"unexpected": true});
        assert_eq!(enrich_panes_data(payload.clone()), payload);
    }
}
