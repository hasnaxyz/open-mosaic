use clap::{ArgEnum, Parser, Subcommand};
use serde_json::{json, Value};
use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    str::FromStr,
    sync::mpsc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zellij_client::os_input_output::{get_cli_client_os_input, ClientOsApi};
use zellij_utils::{
    cli::CliAction,
    consts::{
        create_config_and_cache_folders, ipc_connect, session_info_folder_for_session,
        ZELLIJ_SOCK_DIR,
    },
    data::{ClientId, PaneId},
    input::actions::Action,
    ipc::{ClientToServerMsg, ExitReason, IpcSenderWithContext, ServerToClientMsg},
    sessions::{get_active_session, get_sessions, ActiveSession},
};

#[path = "mosaic/adapters.rs"]
mod mosaic_adapters;
#[path = "mosaic/agent.rs"]
mod mosaic_agent;

const SCHEMA_VERSION: &str = "mosaic.control.v1";

#[derive(Parser, Debug)]
#[clap(
    name = "mosaic",
    version,
    about = "Open Mosaic agentic terminal workspace control CLI"
)]
struct MosaicCli {
    /// Target session name. Defaults to the only active session or $ZELLIJ_SESSION_NAME.
    #[clap(short, long)]
    session: Option<String>,

    /// Emit a receipt without mutating session state.
    #[clap(long)]
    dry_run: bool,

    #[clap(subcommand)]
    command: MosaicCommand,
}

#[derive(Subcommand, Debug)]
enum MosaicCommand {
    /// Create, list, attach, or close sessions.
    Sessions {
        #[clap(subcommand)]
        command: SessionCommand,
    },
    /// List panes in a session.
    Panes {
        #[clap(subcommand)]
        command: PanesCommand,
    },
    /// List tabs in a session.
    Tabs {
        #[clap(subcommand)]
        command: TabsCommand,
    },
    /// Create a new pane.
    Pane {
        #[clap(subcommand)]
        command: PaneCommand,
    },
    /// Create a new tab.
    Tab {
        #[clap(subcommand)]
        command: TabCommand,
    },
    /// Deliver or queue prompts to panes.
    Prompt {
        #[clap(subcommand)]
        command: PromptCommand,
    },
    /// Inspect and clear queued prompts.
    Queue {
        #[clap(subcommand)]
        command: QueueCommand,
    },
    /// Inspect local audit records.
    Audit {
        #[clap(subcommand)]
        command: AuditCommand,
    },
    /// List and validate portable adapter manifests.
    Adapters {
        #[clap(subcommand)]
        command: AdapterCommand,
    },
    /// Capture structured pane observations.
    Observe {
        #[clap(subcommand)]
        command: ObserveCommand,
    },
    /// Capture pane output.
    Capture(CaptureArgs),
    /// Subscribe to pane output.
    Subscribe(SubscribeArgs),
}

#[derive(Subcommand, Debug)]
enum SessionCommand {
    /// List active sessions.
    List,
    /// Create a session, optionally in the background.
    Create {
        name: String,
        #[clap(long)]
        background: bool,
    },
    /// Attach to a session.
    Attach { name: String },
    /// Close a running session.
    Close {
        name: String,
        /// Also delete resurrectable session state.
        #[clap(long)]
        delete: bool,
    },
}

#[derive(Subcommand, Debug)]
enum PanesCommand {
    /// List panes as a versioned Mosaic JSON envelope.
    List {
        #[clap(long)]
        all: bool,
    },
}

#[derive(Subcommand, Debug)]
enum TabsCommand {
    /// List tabs as a versioned Mosaic JSON envelope.
    List {
        #[clap(long)]
        all: bool,
    },
}

#[derive(Subcommand, Debug)]
enum PaneCommand {
    /// Run a command in a new pane.
    Create {
        #[clap(long)]
        name: Option<String>,
        #[clap(long)]
        cwd: Option<PathBuf>,
        #[clap(last = true, required = true)]
        command: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
enum TabCommand {
    /// Create a new tab.
    Create {
        #[clap(long)]
        name: Option<String>,
        #[clap(long)]
        cwd: Option<PathBuf>,
        #[clap(last = true)]
        command: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
enum PromptCommand {
    /// Send a prompt now, or enqueue it for later delivery.
    Send(PromptSendArgs),
}

#[derive(Subcommand, Debug)]
enum QueueCommand {
    /// List queued prompts from local Mosaic state.
    List(QueueListArgs),
    /// Clear queued prompts from local Mosaic state.
    Clear(QueueClearArgs),
}

#[derive(Parser, Debug)]
struct QueueListArgs {
    /// Optional pane ID filter.
    #[clap(long)]
    pane_id: Option<String>,
    /// Maximum records to return, newest records kept.
    #[clap(long)]
    limit: Option<usize>,
    /// Redact prompt bodies in returned queue records.
    #[clap(long)]
    redact: bool,
}

#[derive(Parser, Debug)]
struct QueueClearArgs {
    /// Target pane ID.
    #[clap(long)]
    pane_id: String,
    /// Clear only one queued prompt receipt ID. Omit to clear the pane queue.
    #[clap(long)]
    receipt_id: Option<String>,
}

#[derive(Subcommand, Debug)]
enum AuditCommand {
    /// List local audit records.
    List(AuditListArgs),
}

#[derive(Subcommand, Debug)]
enum AdapterCommand {
    /// List built-in portable adapter interface descriptors.
    List(AdapterListArgs),
    /// Validate a Mosaic adapter manifest without executing it.
    Validate(AdapterValidateArgs),
}

#[derive(Subcommand, Debug)]
enum ObserveCommand {
    /// Capture a structured snapshot of one pane.
    Pane(ObservePaneArgs),
}

#[derive(Parser, Debug)]
struct AuditListArgs {
    /// Maximum records to return, newest records kept.
    #[clap(long)]
    limit: Option<usize>,
    /// Redact prompt bodies if present in future audit records.
    #[clap(long)]
    redact: bool,
}

#[derive(Parser, Debug)]
struct AdapterListArgs {
    /// Optional adapter kind filter.
    #[clap(long)]
    kind: Option<String>,
}

#[derive(Parser, Debug)]
struct AdapterValidateArgs {
    /// Path to a JSON adapter manifest.
    #[clap(long)]
    file: PathBuf,
}

#[derive(Parser, Debug)]
struct ObservePaneArgs {
    /// Target pane ID.
    #[clap(short, long)]
    pane_id: String,
    /// Return only the last N captured lines; 0 means all captured lines.
    #[clap(long)]
    last_lines: Option<usize>,
    /// Include full scrollback before applying --last-lines.
    #[clap(long)]
    scrollback: bool,
    /// Preserve ANSI styling.
    #[clap(long)]
    ansi: bool,
    /// Redact returned terminal lines. Audit records never include raw lines.
    #[clap(long)]
    redact: bool,
}

#[derive(Parser, Debug)]
struct PromptSendArgs {
    /// Target pane ID, for example terminal_1, plugin_2, or 1.
    #[clap(short, long)]
    pane_id: String,
    /// Prompt text.
    #[clap(long)]
    text: Option<String>,
    /// Read prompt text from a file.
    #[clap(long)]
    file: Option<PathBuf>,
    /// Queue the prompt without sending it to the terminal.
    #[clap(long)]
    queue: bool,
    /// Paste/write without submitting.
    #[clap(long)]
    no_submit: bool,
    /// Submit key to send after the prompt.
    #[clap(long, arg_enum, default_value = "enter")]
    submit: SubmitKey,
    /// Use raw character write instead of bracketed paste.
    #[clap(long)]
    raw_write: bool,
}

#[derive(Parser, Debug)]
struct CaptureArgs {
    /// Target pane ID.
    #[clap(short, long)]
    pane_id: String,
    /// Include full scrollback.
    #[clap(long)]
    scrollback: bool,
    /// Preserve ANSI styling.
    #[clap(long)]
    ansi: bool,
}

#[derive(Parser, Debug)]
struct SubscribeArgs {
    /// Target pane ID.
    #[clap(short, long)]
    pane_id: String,
    /// Include last N scrollback lines in the initial event; 0 means all.
    #[clap(long)]
    scrollback: Option<usize>,
    /// Output raw text or NDJSON.
    #[clap(long, arg_enum, default_value = "ndjson")]
    format: StreamFormat,
    /// Preserve ANSI styling.
    #[clap(long)]
    ansi: bool,
}

#[derive(Clone, Debug, ArgEnum)]
enum SubmitKey {
    Enter,
    Tab,
    None,
}

#[derive(Clone, Debug, ArgEnum)]
enum StreamFormat {
    Raw,
    Ndjson,
}

#[derive(Debug)]
struct MosaicError {
    code: &'static str,
    message: String,
    exit_code: u8,
}

impl MosaicError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            exit_code: 2,
        }
    }
}

fn main() -> ExitCode {
    create_config_and_cache_folders();
    let cli = MosaicCli::parse();
    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            let _ = writeln!(io::stderr(), "{}", error_event(&error));
            ExitCode::from(error.exit_code)
        },
    }
}

fn run(cli: MosaicCli) -> Result<u8, MosaicError> {
    match cli.command {
        MosaicCommand::Sessions { command } => run_sessions(command, cli.dry_run),
        MosaicCommand::Panes { command } => {
            let session = resolve_session(cli.session)?;
            match command {
                PanesCommand::List { all } => {
                    let output = dispatch_cli_action_capture(
                        &session,
                        CliAction::ListPanes {
                            tab: all,
                            command: all,
                            state: all,
                            geometry: all,
                            all,
                            json: true,
                        },
                    )?;
                    print_envelope(
                        "panes.list",
                        &session,
                        mosaic_agent::enrich_panes_data(parse_server_json(output.lines)?),
                    )?;
                    Ok(output.exit_code)
                },
            }
        },
        MosaicCommand::Tabs { command } => {
            let session = resolve_session(cli.session)?;
            match command {
                TabsCommand::List { all } => {
                    let output = dispatch_cli_action_capture(
                        &session,
                        CliAction::ListTabs {
                            state: all,
                            dimensions: all,
                            panes: all,
                            layout: all,
                            all,
                            json: true,
                        },
                    )?;
                    print_envelope("tabs.list", &session, parse_server_json(output.lines)?)?;
                    Ok(output.exit_code)
                },
            }
        },
        MosaicCommand::Pane { command } => {
            let session = resolve_session(cli.session)?;
            match command {
                PaneCommand::Create { name, cwd, command } => {
                    let output = dispatch_cli_action_capture(
                        &session,
                        CliAction::NewPane {
                            command,
                            plugin: None,
                            direction: None,
                            cwd,
                            floating: false,
                            in_place: false,
                            close_replaced_pane: false,
                            name,
                            close_on_exit: false,
                            start_suspended: false,
                            configuration: None,
                            skip_plugin_cache: false,
                            x: None,
                            y: None,
                            width: None,
                            height: None,
                            pinned: None,
                            stacked: false,
                            blocking: false,
                            block_until_exit_success: false,
                            block_until_exit_failure: false,
                            block_until_exit: false,
                            unblock_condition: None,
                            near_current_pane: false,
                            borderless: None,
                            tab_id: None,
                        },
                    )?;
                    print_receipt("pane.create", Some(&session), None, "accepted", None)?;
                    for line in output.lines {
                        println!("{line}");
                    }
                    Ok(output.exit_code)
                },
            }
        },
        MosaicCommand::Tab { command } => {
            let session = resolve_session(cli.session)?;
            match command {
                TabCommand::Create { name, cwd, command } => {
                    let output = dispatch_cli_action_capture(
                        &session,
                        CliAction::NewTab {
                            layout: None,
                            layout_string: None,
                            layout_dir: None,
                            name,
                            cwd,
                            initial_command: command,
                            initial_plugin: None,
                            close_on_exit: false,
                            start_suspended: false,
                            block_until_exit_success: false,
                            block_until_exit_failure: false,
                            block_until_exit: false,
                        },
                    )?;
                    print_receipt("tab.create", Some(&session), None, "accepted", None)?;
                    for line in output.lines {
                        println!("{line}");
                    }
                    Ok(output.exit_code)
                },
            }
        },
        MosaicCommand::Prompt { command } => {
            let session = resolve_session(cli.session)?;
            match command {
                PromptCommand::Send(args) => run_prompt_send(&session, args, cli.dry_run),
            }
        },
        MosaicCommand::Queue { command } => run_queue(command, cli.session, cli.dry_run),
        MosaicCommand::Audit { command } => run_audit(command),
        MosaicCommand::Adapters { command } => run_adapters(command),
        MosaicCommand::Observe { command } => {
            let session = resolve_session(cli.session)?;
            run_observe(&session, command)
        },
        MosaicCommand::Capture(args) => {
            let session = resolve_session(cli.session)?;
            let output = dispatch_cli_action_capture(
                &session,
                CliAction::DumpScreen {
                    path: None,
                    full: args.scrollback,
                    pane_id: Some(args.pane_id),
                    ansi: args.ansi,
                },
            )?;
            for line in output.lines {
                println!("{line}");
            }
            Ok(output.exit_code)
        },
        MosaicCommand::Subscribe(args) => {
            let session = resolve_session(cli.session)?;
            run_subscribe(&session, args)
        },
    }
}

fn run_adapters(command: AdapterCommand) -> Result<u8, MosaicError> {
    match command {
        AdapterCommand::List(args) => {
            let mut adapters = mosaic_adapters::built_in_adapters();
            if let Some(kind) = args.kind {
                let kind = normalize_adapter_kind(&kind)?;
                adapters
                    .retain(|adapter| adapter.get("kind").and_then(Value::as_str) == Some(&kind));
            }
            print_value(json!({
                "schema_version": SCHEMA_VERSION,
                "event": "adapters.list",
                "adapter_schema_version": mosaic_adapters::ADAPTER_SCHEMA_VERSION,
                "timestamp_ms": now_millis(),
                "known_kinds": mosaic_adapters::known_kinds(),
                "data": adapters,
            }))?;
            Ok(0)
        },
        AdapterCommand::Validate(args) => {
            let raw = fs::read_to_string(&args.file).map_err(|e| {
                MosaicError::new(
                    "adapter_manifest_read_failed",
                    format!("failed to read {}: {e}", args.file.display()),
                )
            })?;
            let manifest = serde_json::from_str::<Value>(&raw).map_err(|e| {
                MosaicError::new(
                    "invalid_adapter_manifest_json",
                    format!("{}: {e}", args.file.display()),
                )
            })?;
            mosaic_adapters::validate_adapter_manifest(&manifest).map_err(|e| {
                MosaicError::new(
                    "invalid_adapter_manifest",
                    format!("{}: {e}", args.file.display()),
                )
            })?;
            print_value(json!({
                "schema_version": SCHEMA_VERSION,
                "event": "adapters.validate",
                "adapter_schema_version": mosaic_adapters::ADAPTER_SCHEMA_VERSION,
                "timestamp_ms": now_millis(),
                "valid": true,
                "adapter": manifest,
            }))?;
            Ok(0)
        },
    }
}

fn normalize_adapter_kind(kind: &str) -> Result<String, MosaicError> {
    let normalized = kind.trim().replace('-', "_");
    if mosaic_adapters::known_kinds().contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(MosaicError::new(
            "invalid_adapter_kind",
            format!(
                "adapter kind must be one of {}, got {kind:?}",
                mosaic_adapters::known_kinds().join(", ")
            ),
        ))
    }
}

fn run_observe(session: &str, command: ObserveCommand) -> Result<u8, MosaicError> {
    match command {
        ObserveCommand::Pane(args) => {
            validate_pane_id(&args.pane_id)?;
            let output = dispatch_cli_action_capture(
                session,
                CliAction::DumpScreen {
                    path: None,
                    full: args.scrollback,
                    pane_id: Some(args.pane_id.clone()),
                    ansi: args.ansi,
                },
            )?;
            let redacted = args.redact || env_flag_enabled("MOSAIC_OBSERVE_REDACT");
            let observation = build_pane_observation(
                session,
                &args.pane_id,
                output.lines,
                ObservationOptions {
                    last_lines: args.last_lines,
                    scrollback: args.scrollback,
                    ansi: args.ansi,
                    redacted,
                    exit_code: output.exit_code,
                },
            );
            audit(&observation.audit_record);
            print_value(observation.event)?;
            Ok(0)
        },
    }
}

fn run_queue(
    command: QueueCommand,
    session: Option<String>,
    dry_run: bool,
) -> Result<u8, MosaicError> {
    match command {
        QueueCommand::List(args) => {
            if let Some(pane_id) = args.pane_id.as_deref() {
                validate_pane_id(pane_id)?;
            }
            let mut records = read_queue_records(session.as_deref(), args.pane_id.as_deref())?;
            sort_values_by_timestamp(&mut records);
            if args.redact {
                redact_prompts(&mut records);
            }
            records = last_n_values(records, args.limit);
            print_value(json!({
                "schema_version": SCHEMA_VERSION,
                "event": "queue.list",
                "session": session,
                "pane_id": args.pane_id,
                "timestamp_ms": now_millis(),
                "data": records,
            }))?;
            Ok(0)
        },
        QueueCommand::Clear(args) => {
            validate_pane_id(&args.pane_id)?;
            let session = session.ok_or_else(|| {
                MosaicError::new(
                    "queue_session_required",
                    "pass --session when clearing a queue",
                )
            })?;
            if dry_run {
                print_receipt(
                    "queue.clear",
                    Some(&session),
                    Some(&args.pane_id),
                    "dry_run",
                    None,
                )?;
                return Ok(0);
            }
            clear_queue_records(&session, &args.pane_id, args.receipt_id.as_deref())?;
            print_local_state_receipt(
                "queue.clear",
                Some(&session),
                Some(&args.pane_id),
                "accepted",
                None,
            )?;
            Ok(0)
        },
    }
}

fn run_audit(command: AuditCommand) -> Result<u8, MosaicError> {
    match command {
        AuditCommand::List(args) => {
            let mut records = read_ndjson_file(&audit_path())?;
            sort_values_by_timestamp(&mut records);
            if args.redact {
                redact_prompts(&mut records);
            }
            records = last_n_values(records, args.limit);
            print_value(json!({
                "schema_version": SCHEMA_VERSION,
                "event": "audit.list",
                "timestamp_ms": now_millis(),
                "data": records,
            }))?;
            Ok(0)
        },
    }
}

fn run_sessions(command: SessionCommand, dry_run: bool) -> Result<u8, MosaicError> {
    match command {
        SessionCommand::List => {
            let sessions = get_sessions()
                .map_err(|e| MosaicError::new("sessions_list_failed", format!("{e:?}")))?
                .into_iter()
                .map(|(name, age)| {
                    json!({
                        "name": name,
                        "age_seconds": age.as_secs(),
                        "status": "running"
                    })
                })
                .collect::<Vec<_>>();
            print_value(json!({
                "schema_version": SCHEMA_VERSION,
                "event": "sessions.list",
                "timestamp_ms": now_millis(),
                "sessions": sessions,
            }))?;
            Ok(0)
        },
        SessionCommand::Create { name, background } => {
            if dry_run {
                print_receipt("session.create", Some(&name), None, "dry_run", None)?;
                return Ok(0);
            }
            let zellij_bin = env::var("MOSAIC_ZELLIJ_BIN").unwrap_or_else(|_| "zellij".to_owned());
            let mut command = Command::new(zellij_bin);
            command
                .arg("--session")
                .arg(&name)
                .arg("attach")
                .arg("--create");
            if background {
                command.arg("--create-background");
            }
            command.arg(&name);
            let status = command.status().map_err(|e| {
                MosaicError::new(
                    "session_create_failed",
                    format!("failed to spawn compatibility binary: {e}"),
                )
            })?;
            let receipt_status = if status.success() {
                "accepted"
            } else {
                "error"
            };
            print_receipt(
                "session.create",
                Some(&name),
                None,
                receipt_status,
                status.code().map(|code| code.to_string()),
            )?;
            Ok(status.code().unwrap_or(1) as u8)
        },
        SessionCommand::Attach { name } => {
            let zellij_bin = env::var("MOSAIC_ZELLIJ_BIN").unwrap_or_else(|_| "zellij".to_owned());
            let status = Command::new(zellij_bin)
                .arg("attach")
                .arg(&name)
                .status()
                .map_err(|e| {
                    MosaicError::new(
                        "session_attach_failed",
                        format!("failed to spawn compatibility binary: {e}"),
                    )
                })?;
            Ok(status.code().unwrap_or(1) as u8)
        },
        SessionCommand::Close { name, delete } => {
            if dry_run {
                print_receipt("session.close", Some(&name), None, "dry_run", None)?;
                return Ok(0);
            }
            close_session(&name)?;
            if delete {
                delete_session_state(&name)?;
            }
            print_receipt("session.close", Some(&name), None, "accepted", None)?;
            Ok(0)
        },
    }
}

fn run_prompt_send(session: &str, args: PromptSendArgs, dry_run: bool) -> Result<u8, MosaicError> {
    validate_pane_id(&args.pane_id)?;
    let prompt = read_prompt(args.text, args.file)?;
    let operation = if args.queue {
        "prompt.queue"
    } else {
        "prompt.send"
    };
    if dry_run || args.queue {
        let status = if dry_run { "dry_run" } else { "queued" };
        let receipt = receipt(operation, Some(session), Some(&args.pane_id), status, None);
        if args.queue && !dry_run {
            enqueue_prompt(session, &args.pane_id, &prompt, &receipt)?;
        }
        print_value(receipt.clone())?;
        audit(&receipt);
        return Ok(0);
    }

    let write_action = if args.raw_write {
        CliAction::WriteChars {
            chars: prompt,
            pane_id: Some(args.pane_id.clone()),
        }
    } else {
        CliAction::Paste {
            chars: prompt,
            pane_id: Some(args.pane_id.clone()),
        }
    };
    dispatch_cli_action_capture(session, write_action)?;
    if !args.no_submit {
        match args.submit {
            SubmitKey::Enter => {
                dispatch_cli_action_capture(
                    session,
                    CliAction::SendKeys {
                        keys: vec!["Enter".to_owned()],
                        pane_id: Some(args.pane_id.clone()),
                    },
                )?;
            },
            SubmitKey::Tab => {
                dispatch_cli_action_capture(
                    session,
                    CliAction::SendKeys {
                        keys: vec!["Tab".to_owned()],
                        pane_id: Some(args.pane_id.clone()),
                    },
                )?;
            },
            SubmitKey::None => {},
        }
    }
    print_receipt(
        operation,
        Some(session),
        Some(&args.pane_id),
        "accepted",
        None,
    )?;
    Ok(0)
}

struct CapturedOutput {
    lines: Vec<String>,
    exit_code: u8,
}

struct ObservationOptions {
    last_lines: Option<usize>,
    scrollback: bool,
    ansi: bool,
    redacted: bool,
    exit_code: u8,
}

struct PaneObservation {
    event: Value,
    audit_record: Value,
}

fn build_pane_observation(
    session: &str,
    pane_id: &str,
    captured_lines: Vec<String>,
    options: ObservationOptions,
) -> PaneObservation {
    let timestamp_ms = now_millis();
    let id = format!("mosaic-observe-{}-{timestamp_ms}", std::process::id());
    let captured_lines = normalize_captured_lines(captured_lines);
    let total_line_count = captured_lines.len();
    let mut lines = select_last_lines(captured_lines, options.last_lines);
    let truncated_head = lines.len() < total_line_count;
    let mut activity = summarize_lines(&lines, total_line_count, truncated_head, options.exit_code);
    if options.redacted {
        let has_last_line = !activity["last_non_empty_line"].is_null();
        redact_output_lines(&mut lines);
        if has_last_line {
            activity["last_non_empty_line"] = json!("[redacted]");
        }
    }
    let event = json!({
        "schema_version": SCHEMA_VERSION,
        "event": "observe.pane",
        "id": id,
        "session": session,
        "pane_id": pane_id,
        "timestamp_ms": timestamp_ms,
        "source": "dump_screen",
        "scrollback": options.scrollback,
        "ansi": options.ansi,
        "redacted": options.redacted,
        "activity": activity,
        "lines": lines,
    });
    let audit_record = json!({
        "schema_version": SCHEMA_VERSION,
        "event": "observation",
        "id": id,
        "operation": "observe.pane",
        "session": session,
        "pane_id": pane_id,
        "timestamp_ms": timestamp_ms,
        "status": "captured",
        "source": "dump_screen",
        "scrollback": options.scrollback,
        "ansi": options.ansi,
        "redacted": options.redacted,
        "activity": audit_safe_activity(&event["activity"]),
    });
    PaneObservation {
        event,
        audit_record,
    }
}

fn audit_safe_activity(activity: &Value) -> Value {
    let mut activity = activity.clone();
    if let Value::Object(object) = &mut activity {
        if object.remove("last_non_empty_line").is_some() {
            object.insert("last_non_empty_line_omitted".to_owned(), json!(true));
        }
    }
    activity
}

fn normalize_captured_lines(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .flat_map(|line| {
            line.split('\n')
                .map(|segment| segment.trim_end_matches('\r').to_owned())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn select_last_lines(lines: Vec<String>, last_lines: Option<usize>) -> Vec<String> {
    match last_lines {
        Some(0) | None => lines,
        Some(limit) if lines.len() > limit => lines[lines.len() - limit..].to_vec(),
        Some(_) => lines,
    }
}

fn summarize_lines(
    lines: &[String],
    total_line_count: usize,
    truncated_head: bool,
    exit_code: u8,
) -> Value {
    let non_empty_line_count = lines.iter().filter(|line| !line.trim().is_empty()).count();
    let last_non_empty_line = lines
        .iter()
        .rev()
        .find(|line| !line.trim().is_empty())
        .cloned();
    let char_count = lines.iter().map(|line| line.chars().count()).sum::<usize>();
    json!({
        "state": if non_empty_line_count == 0 { "empty" } else { "active" },
        "line_count_total": total_line_count,
        "line_count_returned": lines.len(),
        "non_empty_line_count": non_empty_line_count,
        "char_count_returned": char_count,
        "truncated_head": truncated_head,
        "last_non_empty_line": last_non_empty_line,
        "exit_code": exit_code,
    })
}

fn redact_output_lines(lines: &mut [String]) {
    for line in lines {
        if !line.is_empty() {
            *line = "[redacted]".to_owned();
        }
    }
}

fn env_flag_enabled(name: &str) -> bool {
    env::var(name)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod observation_tests {
    use super::*;

    #[test]
    fn pane_observation_trims_lines_and_summarizes_activity() {
        let observation = build_pane_observation(
            "work",
            "terminal_1",
            vec!["first\n\nlast".to_owned()],
            ObservationOptions {
                last_lines: Some(2),
                scrollback: true,
                ansi: false,
                redacted: false,
                exit_code: 0,
            },
        );

        assert_eq!(observation.event["schema_version"], SCHEMA_VERSION);
        assert_eq!(observation.event["event"], "observe.pane");
        assert_eq!(observation.event["session"], "work");
        assert_eq!(observation.event["pane_id"], "terminal_1");
        assert_eq!(observation.event["scrollback"], true);
        assert_eq!(observation.event["lines"][0], "");
        assert_eq!(observation.event["lines"][1], "last");
        assert_eq!(observation.event["activity"]["line_count_total"], 3);
        assert_eq!(observation.event["activity"]["line_count_returned"], 2);
        assert_eq!(observation.event["activity"]["non_empty_line_count"], 1);
        assert_eq!(observation.event["activity"]["truncated_head"], true);
        assert_eq!(observation.event["activity"]["last_non_empty_line"], "last");
        assert_eq!(observation.audit_record["event"], "observation");
        assert_eq!(observation.audit_record["operation"], "observe.pane");
        assert_eq!(observation.audit_record["lines"], Value::Null);
        assert_eq!(
            observation.audit_record["activity"]["last_non_empty_line"],
            Value::Null
        );
        assert_eq!(
            observation.audit_record["activity"]["last_non_empty_line_omitted"],
            true
        );
        assert_eq!(observation.audit_record["id"], observation.event["id"]);
    }

    #[test]
    fn pane_observation_redacts_returned_lines_and_last_line() {
        let observation = build_pane_observation(
            "work",
            "terminal_1",
            vec!["secret".to_owned(), "".to_owned()],
            ObservationOptions {
                last_lines: None,
                scrollback: false,
                ansi: false,
                redacted: true,
                exit_code: 0,
            },
        );

        assert_eq!(observation.event["redacted"], true);
        assert_eq!(observation.event["lines"][0], "[redacted]");
        assert_eq!(observation.event["lines"][1], "");
        assert_eq!(
            observation.event["activity"]["last_non_empty_line"],
            "[redacted]"
        );
        assert_eq!(observation.audit_record["lines"], Value::Null);
        let audit_json = serde_json::to_string(&observation.audit_record).expect("audit json");
        assert!(!audit_json.contains("secret"));
    }
}

fn dispatch_cli_action_capture(
    session: &str,
    cli_action: CliAction,
) -> Result<CapturedOutput, MosaicError> {
    let get_current_dir = || env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let actions = Action::actions_from_cli(cli_action, Box::new(get_current_dir), None)
        .map_err(|e| MosaicError::new("invalid_action", e))?;
    let mut all_lines = Vec::new();
    let mut exit_code = 0;

    for action in actions {
        let os_input = connect_to_session(session)?;
        let terminal_id = env::var("ZELLIJ_PANE_ID")
            .ok()
            .and_then(|value| value.trim().parse().ok());
        os_input.send_to_server(ClientToServerMsg::Action {
            action,
            terminal_id,
            client_id: None,
            is_cli_client: true,
        });

        loop {
            match os_input.recv_from_server() {
                Some((ServerToClientMsg::UnblockInputThread, _)) => break,
                Some((ServerToClientMsg::Log { lines }, _)) => {
                    all_lines.extend(lines);
                    break;
                },
                Some((ServerToClientMsg::LogError { lines }, _)) => {
                    os_input.send_to_server(ClientToServerMsg::ClientExited);
                    return Err(MosaicError::new("server_log_error", lines.join("\n")));
                },
                Some((ServerToClientMsg::Exit { exit_reason }, _)) => match exit_reason {
                    ExitReason::Error(error) => {
                        os_input.send_to_server(ClientToServerMsg::ClientExited);
                        return Err(MosaicError::new("server_exit_error", error));
                    },
                    ExitReason::CustomExitStatus(status) => {
                        exit_code = status as u8;
                        break;
                    },
                    _ => break,
                },
                Some(_) => {},
                None => {
                    return Err(MosaicError::new(
                        "server_disconnect",
                        "server disconnected before acknowledging action",
                    ));
                },
            }
        }
        os_input.send_to_server(ClientToServerMsg::ClientExited);
    }

    Ok(CapturedOutput {
        lines: all_lines,
        exit_code,
    })
}

fn run_subscribe(session: &str, args: SubscribeArgs) -> Result<u8, MosaicError> {
    let pane_id = PaneId::from_str(&args.pane_id).map_err(|e| {
        MosaicError::new(
            "invalid_pane_id",
            format!("invalid pane id {}: {e}", args.pane_id),
        )
    })?;
    let os_input = connect_to_session(session)?;
    os_input.send_to_server(ClientToServerMsg::SubscribeToPaneRenders {
        pane_ids: vec![pane_id],
        scrollback: args.scrollback,
        ansi: args.ansi,
    });

    let mut sequence: u64 = 0;
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    loop {
        match os_input.recv_from_server() {
            Some((
                ServerToClientMsg::PaneRenderUpdate {
                    pane_id,
                    viewport,
                    scrollback,
                    is_initial,
                },
                _,
            )) => {
                sequence += 1;
                let write_result = match args.format {
                    StreamFormat::Raw => write_raw(&mut stdout, &viewport, scrollback.as_ref()),
                    StreamFormat::Ndjson => write_json_line(
                        &mut stdout,
                        json!({
                            "schema_version": SCHEMA_VERSION,
                            "event": "pane_update",
                            "session": session,
                            "pane_id": pane_id.to_string(),
                            "sequence": sequence,
                            "timestamp_ms": now_millis(),
                            "is_initial": is_initial,
                            "viewport": viewport,
                            "scrollback": scrollback,
                        }),
                    ),
                };
                if let Err(error) = write_result {
                    os_input.send_to_server(ClientToServerMsg::ClientExited);
                    if error.kind() == io::ErrorKind::BrokenPipe {
                        return Ok(0);
                    }
                    return Err(MosaicError::new("stdout_write_failed", error.to_string()));
                }
            },
            Some((ServerToClientMsg::SubscribedPaneClosed { pane_id }, _)) => {
                sequence += 1;
                if let StreamFormat::Ndjson = args.format {
                    if let Err(error) = write_json_line(
                        &mut stdout,
                        json!({
                            "schema_version": SCHEMA_VERSION,
                            "event": "pane_closed",
                            "session": session,
                            "pane_id": pane_id.to_string(),
                            "sequence": sequence,
                            "timestamp_ms": now_millis(),
                        }),
                    ) {
                        os_input.send_to_server(ClientToServerMsg::ClientExited);
                        if error.kind() == io::ErrorKind::BrokenPipe {
                            return Ok(0);
                        }
                        return Err(MosaicError::new("stdout_write_failed", error.to_string()));
                    }
                }
            },
            Some((ServerToClientMsg::LogError { lines }, _)) => {
                os_input.send_to_server(ClientToServerMsg::ClientExited);
                return Err(MosaicError::new("server_log_error", lines.join("\n")));
            },
            Some((ServerToClientMsg::Exit { .. }, _)) => break,
            Some(_) => {},
            None => break,
        }
    }

    os_input.send_to_server(ClientToServerMsg::ClientExited);
    Ok(0)
}

fn connect_to_session(session: &str) -> Result<Box<dyn ClientOsApi>, MosaicError> {
    ensure_session_exists(session)?;
    let mut sock_path = ZELLIJ_SOCK_DIR.clone();
    fs::create_dir_all(&sock_path).map_err(|e| {
        MosaicError::new(
            "socket_dir_failed",
            format!(
                "failed to create socket directory {}: {e}",
                sock_path.display()
            ),
        )
    })?;
    zellij_utils::shared::set_permissions(&sock_path, 0o700).map_err(|e| {
        MosaicError::new(
            "socket_dir_failed",
            format!("failed to set socket directory permissions: {e}"),
        )
    })?;
    sock_path.push(session);

    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = get_cli_client_os_input()
            .map_err(|e| e.to_string())
            .map(|os_input| {
                os_input.connect_to_server(&sock_path);
                Box::new(os_input) as Box<dyn ClientOsApi>
            });
        let _ = sender.send(result);
    });

    match receiver.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(os_input)) => Ok(os_input),
        Ok(Err(error)) => Err(MosaicError::new("terminal_open_failed", error)),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(MosaicError::new(
            "session_connect_timeout",
            format!("timed out connecting to session {session:?}"),
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(MosaicError::new(
            "session_connect_failed",
            format!("failed to connect to session {session:?}"),
        )),
    }
}

fn ensure_session_exists(session: &str) -> Result<(), MosaicError> {
    let exists = get_sessions()
        .map_err(|e| MosaicError::new("sessions_list_failed", format!("{e:?}")))?
        .into_iter()
        .any(|(name, _)| name == session);
    if exists {
        Ok(())
    } else {
        Err(MosaicError::new(
            "session_not_found",
            format!("session {session:?} not found"),
        ))
    }
}

fn close_session(session: &str) -> Result<(), MosaicError> {
    ensure_session_exists(session)?;
    let path = ZELLIJ_SOCK_DIR.join(session);
    let stream = ipc_connect(&path).map_err(|e| {
        MosaicError::new(
            "session_close_failed",
            format!("failed to connect to session {session:?}: {e}"),
        )
    })?;
    #[cfg(windows)]
    {
        let reply = zellij_utils::consts::ipc_connect_reply(&path);
        IpcSenderWithContext::<ClientToServerMsg>::new(stream)
            .send_client_msg(ClientToServerMsg::KillSession)
            .map_err(|e| MosaicError::new("session_close_failed", e.to_string()))?;
        if let Ok(reply_stream) = reply {
            let mut receiver =
                zellij_utils::ipc::IpcReceiverWithContext::<ServerToClientMsg>::new(reply_stream);
            let _ = receiver.recv_server_msg();
        }
    }
    #[cfg(not(windows))]
    {
        IpcSenderWithContext::<ClientToServerMsg>::new(stream)
            .send_client_msg(ClientToServerMsg::KillSession)
            .map_err(|e| MosaicError::new("session_close_failed", e.to_string()))?;
    }
    Ok(())
}

fn delete_session_state(session: &str) -> Result<(), MosaicError> {
    let path = session_info_folder_for_session(session);
    match fs::remove_dir_all(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(MosaicError::new(
            "session_delete_state_failed",
            format!("failed to delete {}: {error}", path.display()),
        )),
    }
}

fn resolve_session(requested: Option<String>) -> Result<String, MosaicError> {
    if let Some(session) = requested {
        return Ok(session);
    }
    if let Ok(session) = env::var("ZELLIJ_SESSION_NAME") {
        if !session.is_empty() {
            return Ok(session);
        }
    }
    match get_active_session() {
        ActiveSession::One(session) => Ok(session),
        ActiveSession::None => Err(MosaicError::new(
            "no_active_session",
            "no active Mosaic/Zellij session found; pass --session",
        )),
        ActiveSession::Many => Err(MosaicError::new(
            "ambiguous_session",
            "multiple active sessions found; pass --session",
        )),
    }
}

fn read_prompt(text: Option<String>, file: Option<PathBuf>) -> Result<String, MosaicError> {
    match (text, file) {
        (Some(_), Some(_)) => Err(MosaicError::new(
            "invalid_prompt_source",
            "use either --text or --file, not both",
        )),
        (Some(text), None) => Ok(text),
        (None, Some(path)) => fs::read_to_string(&path).map_err(|e| {
            MosaicError::new(
                "prompt_file_read_failed",
                format!("failed to read prompt file {}: {e}", path.display()),
            )
        }),
        (None, None) => Err(MosaicError::new(
            "invalid_prompt_source",
            "prompt text required; pass --text or --file",
        )),
    }
}

fn validate_pane_id(pane_id: &str) -> Result<(), MosaicError> {
    PaneId::from_str(pane_id)
        .map(|_| ())
        .map_err(|e| MosaicError::new("invalid_pane_id", format!("invalid pane id {pane_id}: {e}")))
}

fn parse_server_json(lines: Vec<String>) -> Result<Value, MosaicError> {
    let raw = lines.join("\n");
    serde_json::from_str(&raw)
        .map_err(|e| MosaicError::new("invalid_server_json", format!("{e}: {raw}")))
}

fn print_envelope(event: &str, session: &str, data: Value) -> Result<(), MosaicError> {
    print_value(json!({
        "schema_version": SCHEMA_VERSION,
        "event": event,
        "session": session,
        "timestamp_ms": now_millis(),
        "data": data,
    }))
}

fn print_receipt(
    operation: &str,
    session: Option<&str>,
    pane_id: Option<&str>,
    status: &str,
    error: Option<String>,
) -> Result<(), MosaicError> {
    let receipt = receipt(operation, session, pane_id, status, error);
    print_value(receipt.clone())?;
    audit(&receipt);
    Ok(())
}

fn print_local_state_receipt(
    operation: &str,
    session: Option<&str>,
    pane_id: Option<&str>,
    status: &str,
    error: Option<String>,
) -> Result<(), MosaicError> {
    let mut receipt = receipt(operation, session, pane_id, status, error);
    if status == "accepted" {
        receipt["ack"] = json!("local_state_updated");
    }
    audit(&receipt);
    print_value(receipt)
}

fn receipt(
    operation: &str,
    session: Option<&str>,
    pane_id: Option<&str>,
    status: &str,
    error: Option<String>,
) -> Value {
    json!({
        "schema_version": SCHEMA_VERSION,
        "event": "receipt",
        "id": format!("mosaic-{}-{}", std::process::id(), now_millis()),
        "operation": operation,
        "session": session,
        "pane_id": pane_id,
        "status": status,
        "ack": if status == "accepted" { "server_accepted" } else { "none" },
        "timestamp_ms": now_millis(),
        "error": error,
    })
}

fn error_event(error: &MosaicError) -> Value {
    json!({
        "schema_version": SCHEMA_VERSION,
        "event": "error",
        "code": error.code,
        "message": error.message,
        "timestamp_ms": now_millis(),
    })
}

fn print_value(value: Value) -> Result<(), MosaicError> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    write_json_line(&mut stdout, value)
        .map_err(|e| MosaicError::new("stdout_write_failed", e.to_string()))
}

fn write_json_line(writer: &mut dyn Write, value: Value) -> io::Result<()> {
    writeln!(writer, "{value}")?;
    writer.flush()
}

fn write_raw(
    writer: &mut dyn Write,
    viewport: &[String],
    scrollback: Option<&Vec<String>>,
) -> io::Result<()> {
    if let Some(scrollback) = scrollback {
        for line in scrollback {
            writeln!(writer, "{line}")?;
        }
    }
    for line in viewport {
        writeln!(writer, "{line}")?;
    }
    writer.flush()
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn enqueue_prompt(
    session: &str,
    pane_id: &str,
    prompt: &str,
    receipt: &Value,
) -> Result<(), MosaicError> {
    let session_component = safe_path_component(session, "session")?;
    let dir = mosaic_state_dir().join("queues").join(session_component);
    create_private_dir(&dir)?;
    let path = dir.join(format!("{}.ndjson", sanitize_filename(pane_id)));
    let prompt_value = if env::var("MOSAIC_AUDIT_REDACT").is_ok() {
        json!("[redacted]")
    } else {
        json!(prompt)
    };
    with_state_file_lock(&queue_lock_path(&path), || {
        append_json_line(
            &path,
            &json!({
                "schema_version": SCHEMA_VERSION,
                "event": "queued_prompt",
                "session": session,
                "pane_id": pane_id,
                "timestamp_ms": now_millis(),
                "receipt": receipt,
                "prompt": prompt_value,
            }),
        )
    })
}

fn audit(record: &Value) {
    let path = audit_path();
    if let Some(parent) = path.parent() {
        let _ = create_private_dir(parent);
    }
    let _ = append_json_line(&path, record);
}

fn audit_path() -> PathBuf {
    mosaic_state_dir().join("audit.ndjson")
}

fn queue_file_path(session: &str, pane_id: &str) -> Result<PathBuf, MosaicError> {
    let session_component = safe_path_component(session, "session")?;
    Ok(mosaic_state_dir()
        .join("queues")
        .join(session_component)
        .join(format!("{}.ndjson", sanitize_filename(pane_id))))
}

fn read_queue_records(
    session: Option<&str>,
    pane_id: Option<&str>,
) -> Result<Vec<Value>, MosaicError> {
    match (session, pane_id) {
        (Some(session), Some(pane_id)) => read_ndjson_file(&queue_file_path(session, pane_id)?),
        (Some(session), None) => {
            let session_component = safe_path_component(session, "session")?;
            let dir = mosaic_state_dir().join("queues").join(session_component);
            read_ndjson_files_in_dir(&dir)
        },
        (None, Some(pane_id)) => {
            let root = mosaic_state_dir().join("queues");
            let mut records = Vec::new();
            for session_dir in read_child_dirs(&root)? {
                records.extend(read_ndjson_file(
                    &session_dir.join(format!("{}.ndjson", sanitize_filename(pane_id))),
                )?);
            }
            Ok(records)
        },
        (None, None) => {
            let root = mosaic_state_dir().join("queues");
            let mut records = Vec::new();
            for session_dir in read_child_dirs(&root)? {
                records.extend(read_ndjson_files_in_dir(&session_dir)?);
            }
            Ok(records)
        },
    }
}

fn clear_queue_records(
    session: &str,
    pane_id: &str,
    receipt_id: Option<&str>,
) -> Result<(), MosaicError> {
    let path = queue_file_path(session, pane_id)?;
    with_state_file_lock(&queue_lock_path(&path), || {
        clear_queue_records_locked(&path, receipt_id)
    })
}

fn clear_queue_records_locked(path: &Path, receipt_id: Option<&str>) -> Result<(), MosaicError> {
    if receipt_id.is_none() {
        match fs::remove_file(&path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(MosaicError::new(
                    "state_write_failed",
                    format!("failed to remove {}: {error}", path.display()),
                ));
            },
        }
    }

    let receipt_id = receipt_id.unwrap();
    let records = read_ndjson_file(&path)?;
    let original_len = records.len();
    let retained = records
        .into_iter()
        .filter(|record| {
            record
                .get("receipt")
                .and_then(|receipt| receipt.get("id"))
                .and_then(Value::as_str)
                != Some(receipt_id)
        })
        .collect::<Vec<_>>();
    if retained.len() == original_len {
        return Err(MosaicError::new(
            "queue_record_not_found",
            format!("queued prompt receipt {receipt_id:?} not found"),
        ));
    }
    if retained.is_empty() {
        fs::remove_file(&path).map_err(|e| {
            MosaicError::new(
                "state_write_failed",
                format!("failed to remove {}: {e}", path.display()),
            )
        })?;
        return Ok(());
    }
    write_ndjson_file(&path, &retained)
}

fn queue_lock_path(path: &Path) -> PathBuf {
    path.with_extension("ndjson.lock")
}

struct StateFileLock {
    path: PathBuf,
}

impl Drop for StateFileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn with_state_file_lock<T>(
    lock_path: &Path,
    operation: impl FnOnce() -> Result<T, MosaicError>,
) -> Result<T, MosaicError> {
    let _lock = acquire_state_file_lock(lock_path)?;
    operation()
}

fn acquire_state_file_lock(lock_path: &Path) -> Result<StateFileLock, MosaicError> {
    if let Some(parent) = lock_path.parent() {
        create_private_dir(parent)?;
    }
    for _ in 0..500 {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(lock_path) {
            Ok(_) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(lock_path, fs::Permissions::from_mode(0o600)).map_err(
                        |e| {
                            MosaicError::new(
                                "state_write_failed",
                                format!(
                                    "failed to set permissions on {}: {e}",
                                    lock_path.display()
                                ),
                            )
                        },
                    )?;
                }
                return Ok(StateFileLock {
                    path: lock_path.to_path_buf(),
                });
            },
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                thread::sleep(Duration::from_millis(10));
            },
            Err(error) => {
                return Err(MosaicError::new(
                    "state_write_failed",
                    format!("failed to lock {}: {error}", lock_path.display()),
                ));
            },
        }
    }
    Err(MosaicError::new(
        "state_lock_timeout",
        format!("timed out waiting for {}", lock_path.display()),
    ))
}

fn read_ndjson_files_in_dir(dir: &Path) -> Result<Vec<Value>, MosaicError> {
    let mut records = Vec::new();
    for path in read_child_files(dir)? {
        if path.extension().and_then(|ext| ext.to_str()) == Some("ndjson") {
            records.extend(read_ndjson_file(&path)?);
        }
    }
    Ok(records)
}

fn read_child_dirs(root: &Path) -> Result<Vec<PathBuf>, MosaicError> {
    read_dir_entries(root, true)
}

fn read_child_files(root: &Path) -> Result<Vec<PathBuf>, MosaicError> {
    read_dir_entries(root, false)
}

fn read_dir_entries(root: &Path, dirs: bool) -> Result<Vec<PathBuf>, MosaicError> {
    let mut entries = Vec::new();
    match fs::read_dir(root) {
        Ok(read_dir) => {
            for entry in read_dir {
                let entry = entry.map_err(|e| {
                    MosaicError::new(
                        "state_read_failed",
                        format!("failed to read {}: {e}", root.display()),
                    )
                })?;
                let file_type = entry.file_type().map_err(|e| {
                    MosaicError::new(
                        "state_read_failed",
                        format!("failed to stat {}: {e}", entry.path().display()),
                    )
                })?;
                if file_type.is_dir() == dirs {
                    entries.push(entry.path());
                }
            }
            entries.sort();
            Ok(entries)
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(entries),
        Err(error) => Err(MosaicError::new(
            "state_read_failed",
            format!("failed to read {}: {error}", root.display()),
        )),
    }
}

fn read_ndjson_file(path: &Path) -> Result<Vec<Value>, MosaicError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(MosaicError::new(
                "state_read_failed",
                format!("failed to open {}: {error}", path.display()),
            ));
        },
    };
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| {
            MosaicError::new(
                "state_read_failed",
                format!("failed to read {}: {e}", path.display()),
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let value = serde_json::from_str(&line).map_err(|e| {
            MosaicError::new(
                "invalid_state_json",
                format!("{}:{}: {e}", path.display(), index + 1),
            )
        })?;
        records.push(value);
    }
    Ok(records)
}

fn write_ndjson_file(path: &Path, records: &[Value]) -> Result<(), MosaicError> {
    if let Some(parent) = path.parent() {
        create_private_dir(parent)?;
    }
    let temp_path = ndjson_temp_path(path);
    {
        let mut options = OpenOptions::new();
        options.create(true).write(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp_path).map_err(|e| {
            MosaicError::new(
                "state_write_failed",
                format!("failed to open {}: {e}", temp_path.display()),
            )
        })?;
        for record in records {
            writeln!(file, "{record}").map_err(|e| {
                MosaicError::new(
                    "state_write_failed",
                    format!("failed to write {}: {e}", temp_path.display()),
                )
            })?;
        }
    }
    fs::rename(&temp_path, path).map_err(|e| {
        MosaicError::new(
            "state_write_failed",
            format!(
                "failed to replace {} with {}: {e}",
                path.display(),
                temp_path.display()
            ),
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|e| {
            MosaicError::new(
                "state_write_failed",
                format!("failed to set permissions on {}: {e}", path.display()),
            )
        })?;
    }
    Ok(())
}

fn ndjson_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.ndjson");
    path.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        now_millis()
    ))
}

fn last_n_values(mut records: Vec<Value>, limit: Option<usize>) -> Vec<Value> {
    if let Some(limit) = limit {
        if records.len() > limit {
            records = records.split_off(records.len() - limit);
        }
    }
    records
}

fn sort_values_by_timestamp(records: &mut [Value]) {
    records.sort_by_key(record_timestamp_ms);
}

fn record_timestamp_ms(record: &Value) -> u64 {
    record
        .get("timestamp_ms")
        .and_then(Value::as_u64)
        .or_else(|| {
            record
                .get("receipt")
                .and_then(|receipt| receipt.get("timestamp_ms"))
                .and_then(Value::as_u64)
        })
        .unwrap_or(0)
}

fn redact_prompts(records: &mut [Value]) {
    for record in records {
        redact_prompt_value(record);
    }
}

fn redact_prompt_value(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if object.contains_key("prompt") {
                object.insert("prompt".to_owned(), json!("[redacted]"));
            }
            for value in object.values_mut() {
                redact_prompt_value(value);
            }
        },
        Value::Array(values) => {
            for value in values {
                redact_prompt_value(value);
            }
        },
        _ => {},
    }
}

fn append_json_line(path: &Path, record: &Value) -> Result<(), MosaicError> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| {
        MosaicError::new(
            "state_write_failed",
            format!("failed to open {}: {e}", path.display()),
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|e| {
            MosaicError::new(
                "state_write_failed",
                format!("failed to set permissions on {}: {e}", path.display()),
            )
        })?;
    }
    writeln!(file, "{record}").map_err(|e| {
        MosaicError::new(
            "state_write_failed",
            format!("failed to write {}: {e}", path.display()),
        )
    })
}

fn create_private_dir(path: &Path) -> Result<(), MosaicError> {
    fs::create_dir_all(path).map_err(|e| {
        MosaicError::new(
            "state_write_failed",
            format!("failed to create {}: {e}", path.display()),
        )
    })?;
    #[cfg(unix)]
    {
        let state_root = mosaic_state_dir();
        if let Ok(relative) = path.strip_prefix(&state_root) {
            set_private_dir_permissions(&state_root)?;
            let mut current = state_root;
            for component in relative.components() {
                current.push(component.as_os_str());
                set_private_dir_permissions(&current)?;
            }
        } else {
            set_private_dir_permissions(path)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<(), MosaicError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| {
        MosaicError::new(
            "state_write_failed",
            format!("failed to set permissions on {}: {e}", path.display()),
        )
    })
}

fn safe_path_component(value: &str, field: &'static str) -> Result<String, MosaicError> {
    if value.trim().is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.contains('\0')
    {
        return Err(MosaicError::new(
            "invalid_path_component",
            format!("{field} is not safe for state storage"),
        ));
    }
    Ok(value.to_owned())
}

fn mosaic_state_dir() -> PathBuf {
    if let Ok(state_home) = env::var("XDG_STATE_HOME") {
        return PathBuf::from(state_home).join("open-mosaic");
    }
    env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".local")
        .join("state")
        .join("open-mosaic")
}

fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[allow(dead_code)]
fn _client_id_type_anchor(_: ClientId) {}
