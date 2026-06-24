use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
};

pub const GOALS_SCHEMA_VERSION: &str = "mosaic.goals.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodosCommandPlan {
    pub program: String,
    pub args: Vec<String>,
}

impl TodosCommandPlan {
    pub fn argv(&self) -> Vec<String> {
        let mut argv = Vec::with_capacity(1 + self.args.len());
        argv.push(self.program.clone());
        argv.extend(self.args.clone());
        argv
    }

    pub fn to_json(&self) -> Value {
        json!({
            "program": self.program,
            "args": self.args,
            "argv": self.argv(),
        })
    }
}

pub fn default_config_path() -> PathBuf {
    if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(config_home)
            .join("open-mosaic")
            .join("goals.json");
    }
    env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".config")
        .join("open-mosaic")
        .join("goals.json")
}

pub fn empty_registry() -> Value {
    json!({
        "schema_version": GOALS_SCHEMA_VERSION,
        "source": {
            "kind": "none",
            "adapter": null,
            "configured": false,
        },
        "goals": [],
        "tasks": [],
    })
}

pub fn normalize_registry_input(value: Value) -> Result<Value, String> {
    if value.get("schema_version").and_then(Value::as_str) == Some(GOALS_SCHEMA_VERSION) {
        validate_registry(&value)?;
        return Ok(value);
    }
    if value.get("schema_version").and_then(Value::as_str) == Some("mosaic.control.v1") {
        let Some(data) = value.get("data") else {
            return Err("Mosaic goals envelope must include data".to_owned());
        };
        let data = data.clone();
        validate_registry(&data)?;
        return Ok(data);
    }
    Err(format!(
        "registry schema_version must be {GOALS_SCHEMA_VERSION:?}"
    ))
}

pub fn validate_registry(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "goals registry must be a JSON object".to_owned())?;
    let schema_version = require_string(object.get("schema_version"), "schema_version")?;
    if schema_version != GOALS_SCHEMA_VERSION {
        return Err(format!(
            "schema_version must be {GOALS_SCHEMA_VERSION:?}, got {schema_version:?}"
        ));
    }
    if let Some(source) = object.get("source") {
        validate_source(source)?;
    }
    let goals = object
        .get("goals")
        .ok_or_else(|| "goals is required".to_owned())?
        .as_array()
        .ok_or_else(|| "goals must be an array".to_owned())?;
    let tasks = object
        .get("tasks")
        .ok_or_else(|| "tasks is required".to_owned())?
        .as_array()
        .ok_or_else(|| "tasks must be an array".to_owned())?;
    let mut seen_goal_ids = Vec::new();
    for goal in goals {
        validate_goal(goal)?;
        let id = goal.get("id").and_then(Value::as_str).unwrap_or_default();
        if seen_goal_ids.iter().any(|seen| seen == id) {
            return Err(format!("duplicate goal id {id:?}"));
        }
        seen_goal_ids.push(id.to_owned());
    }
    let mut seen_task_ids = Vec::new();
    for task in tasks {
        validate_task(task)?;
        let id = task.get("id").and_then(Value::as_str).unwrap_or_default();
        if seen_task_ids.iter().any(|seen| seen == id) {
            return Err(format!("duplicate task id {id:?}"));
        }
        seen_task_ids.push(id.to_owned());
    }
    Ok(())
}

pub fn registry_from_todos_plan(value: &Value, project: &Path) -> Result<Value, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "todos output must be a JSON object".to_owned())?;
    let plan = object
        .get("plan")
        .and_then(Value::as_object)
        .ok_or_else(|| "todos output must include a plan object".to_owned())?;
    let tasks = object
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| "todos output must include a tasks array".to_owned())?;
    let goal_id = require_string_map(plan, "id")?;
    let goal = json!({
        "id": goal_id,
        "title": string_map(plan, "name").unwrap_or_else(|| goal_id.to_owned()),
        "description": string_map(plan, "description"),
        "status": string_map(plan, "status").unwrap_or_else(|| "unknown".to_owned()),
        "priority": null,
        "source": {
            "adapter": "todos",
            "kind": "plan",
            "project_path": project.to_string_lossy(),
        },
        "metadata": compact_metadata(plan, &[
            "project_id",
            "task_list_id",
            "agent_id",
            "created_at",
            "updated_at",
            "synced_at",
        ]),
    });
    let normalized_tasks = tasks
        .iter()
        .map(|task| normalize_todos_task(task, goal_id, project))
        .collect::<Result<Vec<_>, _>>()?;
    let registry = json!({
        "schema_version": GOALS_SCHEMA_VERSION,
        "source": {
            "kind": "external_adapter",
            "adapter": "todos",
            "configured": true,
            "project_path": project.to_string_lossy(),
            "plan_id": goal_id,
        },
        "goals": [goal],
        "tasks": normalized_tasks,
    });
    validate_registry(&registry)?;
    Ok(registry)
}

pub fn build_todos_command_plan(
    todos_bin: &str,
    project: &Path,
    plan_id: &str,
) -> Result<TodosCommandPlan, String> {
    validate_command_segment(todos_bin, "todos binary")?;
    validate_text_token(plan_id, "plan id")?;
    let project = project.to_string_lossy().to_string();
    validate_path_arg(&project, "project path")?;
    Ok(TodosCommandPlan {
        program: todos_bin.to_owned(),
        args: vec![
            "--project".to_owned(),
            project,
            "--json".to_owned(),
            "plans".to_owned(),
            "--show".to_owned(),
            plan_id.to_owned(),
        ],
    })
}

pub fn redact_todos_command_plan(plan: &TodosCommandPlan) -> Value {
    let program = if is_path_like(&plan.program) {
        "[redacted]".to_owned()
    } else {
        plan.program.clone()
    };
    let args = plan
        .args
        .iter()
        .enumerate()
        .map(|(index, arg)| {
            if index > 0 && plan.args.get(index - 1).map(String::as_str) == Some("--project") {
                "[redacted]".to_owned()
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>();
    let redacted = TodosCommandPlan { program, args };
    redacted.to_json()
}

pub fn summarize_registry(registry: &Value, limit: usize, redact: bool) -> Value {
    let goals = registry
        .get("goals")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let tasks = registry
        .get("tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let by_goal_status = summarize_statuses(&goals);
    let by_task_status = summarize_statuses(&tasks);
    let blocked_tasks = tasks.iter().filter(|task| task_is_blocked(task)).count();
    let active_tasks = tasks.iter().filter(|task| task_is_active(task)).count();
    let mut recent_tasks = last_n(tasks, limit);
    let mut active = recent_tasks
        .iter()
        .filter(|task| task_is_active(task))
        .cloned()
        .collect::<Vec<_>>();
    if active.len() > limit {
        active = active.split_off(active.len() - limit);
    }
    if redact {
        redact_records(&mut recent_tasks);
        redact_records(&mut active);
    }
    let mut source = registry.get("source").cloned().unwrap_or(Value::Null);
    if redact {
        redact_value(&mut source);
    }
    json!({
        "schema_version": GOALS_SCHEMA_VERSION,
        "configured": source_configured(registry),
        "source": source,
        "total_goals": goals.len(),
        "total_tasks": registry
            .get("tasks")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0),
        "active_tasks": active_tasks,
        "blocked_tasks": blocked_tasks,
        "by_goal_status": by_goal_status,
        "by_task_status": by_task_status,
        "recent_tasks": recent_tasks,
        "active": active,
    })
}

pub fn redact_registry(registry: &mut Value) {
    redact_value(registry);
}

fn normalize_todos_task(task: &Value, goal_id: &str, project: &Path) -> Result<Value, String> {
    let object = task
        .as_object()
        .ok_or_else(|| "todos task must be a JSON object".to_owned())?;
    let id = require_string_map(object, "id")?;
    let status = string_map(object, "status").unwrap_or_else(|| "unknown".to_owned());
    let blocked =
        status == "blocked" || object.get("blocked").and_then(Value::as_bool) == Some(true);
    Ok(json!({
        "id": id,
        "goal_id": goal_id,
        "title": string_map(object, "title").unwrap_or_else(|| id.to_owned()),
        "description": string_map(object, "description"),
        "status": status,
        "priority": string_map(object, "priority"),
        "agent": string_map(object, "assigned_to")
            .or_else(|| string_map(object, "agent_id")),
        "blocked": blocked,
        "tags": object
            .get("tags")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        "source": {
            "adapter": "todos",
            "kind": "task",
            "project_path": project.to_string_lossy(),
        },
        "metadata": compact_metadata(object, &[
            "project_id",
            "parent_id",
            "plan_id",
            "task_list_id",
            "created_at",
            "updated_at",
            "completed_at",
            "locked_by",
            "locked_at",
            "started_at",
            "due_at",
        ]),
    }))
}

fn compact_metadata(object: &Map<String, Value>, keys: &[&str]) -> Value {
    let mut metadata = Map::new();
    for key in keys {
        if let Some(value) = object.get(*key) {
            metadata.insert((*key).to_owned(), value.clone());
        }
    }
    Value::Object(metadata)
}

fn validate_source(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "source must be an object".to_owned())?;
    optional_string(object.get("kind"), "source.kind")?;
    optional_string(object.get("adapter"), "source.adapter")?;
    optional_string(object.get("project_path"), "source.project_path")?;
    optional_string(object.get("plan_id"), "source.plan_id")?;
    if let Some(configured) = object.get("configured") {
        configured
            .as_bool()
            .ok_or_else(|| "source.configured must be a boolean".to_owned())?;
    }
    Ok(())
}

fn validate_goal(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "goal must be a JSON object".to_owned())?;
    validate_text_token(require_string(object.get("id"), "goal.id")?, "goal.id")?;
    require_string(object.get("title"), "goal.title")?;
    require_string(object.get("status"), "goal.status")?;
    optional_string(object.get("description"), "goal.description")?;
    optional_string(object.get("priority"), "goal.priority")?;
    if let Some(source) = object.get("source") {
        validate_source(source)?;
    }
    if let Some(metadata) = object.get("metadata") {
        if !metadata.is_object() {
            return Err("goal.metadata must be an object".to_owned());
        }
    }
    Ok(())
}

fn validate_task(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "task must be a JSON object".to_owned())?;
    validate_text_token(require_string(object.get("id"), "task.id")?, "task.id")?;
    if let Some(goal_id) = object.get("goal_id") {
        validate_text_token(
            require_string(Some(goal_id), "task.goal_id")?,
            "task.goal_id",
        )?;
    }
    require_string(object.get("title"), "task.title")?;
    require_string(object.get("status"), "task.status")?;
    optional_string(object.get("description"), "task.description")?;
    optional_string(object.get("priority"), "task.priority")?;
    optional_string(object.get("agent"), "task.agent")?;
    if let Some(blocked) = object.get("blocked") {
        blocked
            .as_bool()
            .ok_or_else(|| "task.blocked must be a boolean".to_owned())?;
    }
    validate_tags(object.get("tags"))?;
    validate_references(object.get("references"))?;
    if let Some(source) = object.get("source") {
        validate_source(source)?;
    }
    if let Some(metadata) = object.get("metadata") {
        if !metadata.is_object() {
            return Err("task.metadata must be an object".to_owned());
        }
    }
    Ok(())
}

fn require_string<'a>(value: Option<&'a Value>, field: &str) -> Result<&'a str, String> {
    let value = value.ok_or_else(|| format!("{field} is required"))?;
    let value = value
        .as_str()
        .ok_or_else(|| format!("{field} must be a string"))?;
    validate_string(value, field)?;
    Ok(value)
}

fn require_string_map<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a str, String> {
    require_string(object.get(field), field)
}

fn string_map(object: &Map<String, Value>, field: &str) -> Option<String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn optional_string(value: Option<&Value>, field: &str) -> Result<(), String> {
    if let Some(value) = value {
        if value.is_null() {
            return Ok(());
        }
        require_string(Some(value), field)?;
    }
    Ok(())
}

fn validate_string(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if value.chars().any(|character| character == '\0') {
        return Err(format!("{field} must not contain NUL bytes"));
    }
    Ok(())
}

fn validate_text_token(value: &str, field: &str) -> Result<(), String> {
    validate_string(value, field)?;
    if value.chars().any(|character| character.is_control()) {
        return Err(format!("{field} must not contain control characters"));
    }
    Ok(())
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

fn validate_references(value: Option<&Value>) -> Result<(), String> {
    let Some(value) = value else {
        return Ok(());
    };
    let references = value
        .as_array()
        .ok_or_else(|| "references must be an array".to_owned())?;
    for reference in references {
        match reference {
            Value::String(text) => {
                validate_string(text, "reference")?;
            },
            Value::Object(_) => {},
            _ => return Err("references entries must be strings or objects".to_owned()),
        }
    }
    Ok(())
}

fn validate_command_segment(segment: &str, field: &str) -> Result<(), String> {
    validate_string(segment, field)?;
    if segment.chars().any(|character| character.is_control()) {
        return Err(format!("{field} must not contain control characters"));
    }
    Ok(())
}

fn validate_path_arg(value: &str, field: &str) -> Result<(), String> {
    validate_string(value, field)?;
    if value.chars().any(|character| character == '\0') {
        return Err(format!("{field} must not contain NUL bytes"));
    }
    Ok(())
}

fn summarize_statuses(records: &[Value]) -> Vec<Value> {
    let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
    for record in records {
        let status = record
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        *by_status.entry(status).or_default() += 1;
    }
    by_status
        .into_iter()
        .map(|(status, count)| json!({ "status": status, "count": count }))
        .collect()
}

fn task_is_blocked(task: &Value) -> bool {
    task.get("blocked").and_then(Value::as_bool) == Some(true)
        || task
            .get("status")
            .and_then(Value::as_str)
            .map(|status| status == "blocked")
            .unwrap_or(false)
}

fn task_is_active(task: &Value) -> bool {
    task.get("status")
        .and_then(Value::as_str)
        .map(|status| {
            matches!(
                status,
                "active" | "claimed" | "in_progress" | "started" | "working"
            )
        })
        .unwrap_or(false)
}

fn last_n(mut values: Vec<Value>, limit: usize) -> Vec<Value> {
    if values.len() > limit {
        values = values.split_off(values.len() - limit);
    }
    values
}

fn source_configured(registry: &Value) -> bool {
    registry
        .get("source")
        .and_then(|source| source.get("configured"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn redact_records(records: &mut [Value]) {
    for record in records {
        redact_value(record);
    }
}

fn redact_value(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for key in [
                "title",
                "description",
                "project_path",
                "url",
                "link",
                "path",
                "current_task",
                "references",
                "metadata",
            ] {
                if object.contains_key(key) {
                    object.insert(key.to_owned(), json!("[redacted]"));
                }
            }
            for value in object.values_mut() {
                redact_value(value);
            }
        },
        Value::Array(values) => {
            for value in values {
                redact_value(value);
            }
        },
        _ => {},
    }
}

fn is_path_like(value: &str) -> bool {
    value.contains('/') || value.contains('\\')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Value {
        json!({
            "schema_version": GOALS_SCHEMA_VERSION,
            "source": {"kind": "file", "configured": true},
            "goals": [
                {"id": "goal-1", "title": "Ship Mosaic", "status": "active"}
            ],
            "tasks": [
                {
                    "id": "task-1",
                    "goal_id": "goal-1",
                    "title": "Add goal context",
                    "status": "in_progress",
                    "priority": "high",
                    "blocked": false,
                    "tags": ["goals"]
                }
            ]
        })
    }

    #[test]
    fn validates_portable_registry() {
        validate_registry(&registry()).expect("valid registry");
    }

    #[test]
    fn rejects_duplicate_task_ids() {
        let mut registry = registry();
        registry["tasks"] = json!([
            {"id": "task-1", "title": "one", "status": "pending"},
            {"id": "task-1", "title": "two", "status": "pending"}
        ]);
        let error = validate_registry(&registry).expect_err("duplicate task id");
        assert!(error.contains("duplicate task id"));
    }

    #[test]
    fn normalizes_todos_plan_json() {
        let todos = json!({
            "plan": {
                "id": "plan-1",
                "name": "Plan One",
                "description": "Build it",
                "status": "active"
            },
            "tasks": [
                {
                    "id": "task-1",
                    "title": "Task One",
                    "description": "Do it",
                    "status": "in_progress",
                    "priority": "high",
                    "assigned_to": "cli",
                    "tags": ["x"]
                }
            ]
        });
        let registry = registry_from_todos_plan(&todos, Path::new("/work/project"))
            .expect("normalized registry");
        assert_eq!(registry["schema_version"], GOALS_SCHEMA_VERSION);
        assert_eq!(registry["source"]["adapter"], "todos");
        assert_eq!(registry["goals"][0]["title"], "Plan One");
        assert_eq!(registry["tasks"][0]["goal_id"], "plan-1");
        assert_eq!(registry["tasks"][0]["agent"], "cli");
    }

    #[test]
    fn redacts_task_text_and_paths() {
        let mut registry = registry_from_todos_plan(
            &json!({
                "plan": {"id": "plan-1", "name": "Secret plan", "status": "active"},
                "tasks": [{"id": "task-1", "title": "Secret task", "status": "pending"}]
            }),
            Path::new("/private/repo"),
        )
        .expect("registry");
        redact_registry(&mut registry);
        let text = serde_json::to_string(&registry).expect("json");
        assert!(!text.contains("Secret"));
        assert!(!text.contains("/private/repo"));
        assert!(text.contains("[redacted]"));
    }

    #[test]
    fn builds_todos_command_without_shell_joining() {
        let plan = build_todos_command_plan("todos", Path::new("/tmp/a project"), "plan-1")
            .expect("command plan");
        assert_eq!(
            plan.argv(),
            vec![
                "todos",
                "--project",
                "/tmp/a project",
                "--json",
                "plans",
                "--show",
                "plan-1"
            ]
        );
    }

    #[test]
    fn redacts_path_like_todos_command_segments() {
        let plan =
            build_todos_command_plan("/private/bin/todos", Path::new("/tmp/a project"), "plan-1")
                .expect("command plan");
        let redacted = redact_todos_command_plan(&plan);
        let text = serde_json::to_string(&redacted).expect("json");
        assert!(!text.contains("/private/bin/todos"));
        assert!(!text.contains("/tmp/a project"));
        assert!(text.contains("[redacted]"));
    }
}
