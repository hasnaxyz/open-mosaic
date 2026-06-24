use serde_json::Value;
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
    assert!(stdout.contains("observe"));
    assert!(stdout.contains("subscribe"));
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
