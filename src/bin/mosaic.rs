use clap::{ArgEnum, Parser, Subcommand};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
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
#[path = "mosaic/goals.rs"]
mod mosaic_goals;
#[path = "mosaic/machines.rs"]
mod mosaic_machines;

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
    /// Inspect and use optional machine transports.
    Machines {
        #[clap(subcommand)]
        command: MachineCommand,
    },
    /// Inspect generic goals/tasks and optional task-system adapters.
    Goals {
        #[clap(subcommand)]
        command: GoalsCommand,
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
    /// Render an agent workspace dashboard snapshot.
    Dashboard(DashboardArgs),
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
enum MachineCommand {
    /// Show this machine's portable Mosaic descriptor.
    Local,
    /// List configured machines plus the local machine descriptor.
    List(MachineListArgs),
    /// Validate a machine registry file without connecting to any machine.
    Validate(MachineValidateArgs),
    /// Execute a Mosaic command through a configured machine transport.
    Exec(MachineExecArgs),
}

#[derive(Subcommand, Debug)]
enum GoalsCommand {
    /// List a portable goals/tasks registry.
    List(GoalsListArgs),
    /// Validate a portable goals/tasks registry.
    Validate(GoalsValidateArgs),
    /// Import one plan from the optional external todos CLI.
    TodosPlan(GoalsTodosPlanArgs),
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
struct MachineListArgs {
    /// Optional machine registry JSON file. Defaults to XDG config if present.
    #[clap(long)]
    file: Option<PathBuf>,
}

#[derive(Parser, Debug)]
struct MachineValidateArgs {
    /// Path to a Mosaic machine registry JSON file.
    #[clap(long)]
    file: PathBuf,
}

#[derive(Parser, Debug)]
struct MachineExecArgs {
    /// Optional machine registry JSON file. Defaults to XDG config if present.
    #[clap(long)]
    file: Option<PathBuf>,
    /// Machine ID to use. The built-in local machine is named "local".
    #[clap(long)]
    machine: String,
    /// Override the remote Mosaic binary path from the registry.
    #[clap(long)]
    mosaic_bin: Option<String>,
    /// Hide prompt bodies and prompt file paths from the returned command plan.
    #[clap(long)]
    redact_command: bool,
    /// Mosaic command to run on the target machine, for example: -- sessions list
    #[clap(last = true, required = true)]
    command: Vec<String>,
}

#[derive(Parser, Debug)]
struct GoalsListArgs {
    /// Optional goals registry JSON file. Defaults to XDG config if present.
    #[clap(long)]
    file: Option<PathBuf>,
    /// Maximum summary task records to include.
    #[clap(long, default_value = "10")]
    limit: usize,
    /// Redact task titles, descriptions, links, and local paths.
    #[clap(long)]
    redact: bool,
}

#[derive(Parser, Debug)]
struct GoalsValidateArgs {
    /// Path to a Mosaic goals registry JSON file.
    #[clap(long)]
    file: PathBuf,
}

#[derive(Parser, Debug)]
struct GoalsTodosPlanArgs {
    /// Project path passed to the external todos CLI.
    #[clap(long)]
    project: PathBuf,
    /// todos plan ID to import.
    #[clap(long)]
    plan: String,
    /// External todos binary to run when this adapter is explicitly invoked.
    #[clap(long, default_value = "todos")]
    todos_bin: String,
    /// Maximum summary task records to include.
    #[clap(long, default_value = "10")]
    limit: usize,
    /// Redact task titles, descriptions, links, and local paths in output.
    #[clap(long)]
    redact: bool,
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

#[derive(Parser, Debug)]
struct DashboardArgs {
    /// Output a stable JSON snapshot or compact terminal text.
    #[clap(long, arg_enum, default_value = "json")]
    format: DashboardFormat,
    /// Include live panes/tabs for the target session. Requires an active or explicit session.
    #[clap(long)]
    live: bool,
    /// Maximum recent queue and audit records to include.
    #[clap(long, default_value = "10")]
    limit: usize,
    /// Redact local paths, command details, and prompt bodies.
    #[clap(long)]
    redact: bool,
    /// Include queued prompt bodies. Prompts are redacted by default.
    #[clap(long)]
    show_prompts: bool,
    /// Optional goals registry JSON file. Defaults to XDG config if present.
    #[clap(long)]
    goals_file: Option<PathBuf>,
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

#[derive(Clone, Debug, ArgEnum)]
enum DashboardFormat {
    Json,
    Text,
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
        MosaicCommand::Machines { command } => run_machines(command, cli.dry_run),
        MosaicCommand::Goals { command } => run_goals(command, cli.dry_run),
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
        MosaicCommand::Dashboard(args) => run_dashboard(cli.session, args),
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

fn run_goals(command: GoalsCommand, dry_run: bool) -> Result<u8, MosaicError> {
    match command {
        GoalsCommand::List(args) => {
            let (mut registry, mut source) = load_goals_registry(args.file.as_deref())?;
            let summary = mosaic_goals::summarize_registry(&registry, args.limit, args.redact);
            if args.redact {
                mosaic_goals::redact_registry(&mut registry);
                redact_dashboard_source(&mut source);
            }
            print_value(json!({
                "schema_version": SCHEMA_VERSION,
                "event": "goals.list",
                "goal_schema_version": mosaic_goals::GOALS_SCHEMA_VERSION,
                "timestamp_ms": now_millis(),
                "source": source,
                "summary": summary,
                "data": registry,
            }))?;
            Ok(0)
        },
        GoalsCommand::Validate(args) => {
            let registry = read_goals_registry(&args.file)?;
            print_value(json!({
                "schema_version": SCHEMA_VERSION,
                "event": "goals.validate",
                "goal_schema_version": mosaic_goals::GOALS_SCHEMA_VERSION,
                "timestamp_ms": now_millis(),
                "valid": true,
                "registry": registry,
            }))?;
            Ok(0)
        },
        GoalsCommand::TodosPlan(args) => run_goals_todos_plan(args, dry_run),
    }
}

fn run_goals_todos_plan(args: GoalsTodosPlanArgs, dry_run: bool) -> Result<u8, MosaicError> {
    let plan = mosaic_goals::build_todos_command_plan(&args.todos_bin, &args.project, &args.plan)
        .map_err(|e| MosaicError::new("invalid_goals_todos_command", e))?;
    let id = format!("mosaic-goals-{}-{}", std::process::id(), now_millis());
    if dry_run {
        let event = goals_todos_event(
            &id,
            "dry_run",
            "none",
            None,
            None,
            &plan,
            args.redact,
            None,
            None,
        );
        audit(&goals_todos_audit_record(
            &id, "dry_run", "none", None, None, &plan, None,
        ));
        print_value(event)?;
        return Ok(0);
    }

    let output = Command::new(&plan.program)
        .args(&plan.args)
        .output()
        .map_err(|e| {
            let message = if args.redact {
                format!("failed to spawn goals adapter: {e}")
            } else {
                format!("failed to spawn {}: {e}", plan.program)
            };
            MosaicError::new("goals_todos_failed", message)
        })?;
    let exit_code = output.status.code().unwrap_or(1) as u8;
    if !output.status.success() {
        let error = Some(format!("todos command exited with status {exit_code}"));
        let mut event = goals_todos_event(
            &id,
            "error",
            "process_exited",
            Some(exit_code),
            error.clone(),
            &plan,
            true,
            None,
            None,
        );
        event["stderr"] = if args.redact {
            json!("[redacted]")
        } else {
            json!(String::from_utf8_lossy(&output.stderr).to_string())
        };
        audit(&goals_todos_audit_record(
            &id,
            "error",
            "process_exited",
            Some(exit_code),
            error,
            &plan,
            None,
        ));
        print_value(event)?;
        return Ok(exit_code);
    }

    let todos_json = serde_json::from_slice::<Value>(&output.stdout).map_err(|e| {
        MosaicError::new(
            "invalid_goals_todos_json",
            format!("todos output was not valid JSON: {e}"),
        )
    })?;
    let mut registry = mosaic_goals::registry_from_todos_plan(&todos_json, &args.project)
        .map_err(|e| MosaicError::new("invalid_goals_todos_data", e))?;
    let summary = mosaic_goals::summarize_registry(&registry, args.limit, args.redact);
    let audit_summary = mosaic_goals::summarize_registry(&registry, 0, true);
    if args.redact {
        mosaic_goals::redact_registry(&mut registry);
    }
    let event = goals_todos_event(
        &id,
        "completed",
        "process_exited",
        Some(exit_code),
        None,
        &plan,
        args.redact,
        Some(summary),
        Some(registry),
    );
    audit(&goals_todos_audit_record(
        &id,
        "completed",
        "process_exited",
        Some(exit_code),
        None,
        &plan,
        Some(audit_summary),
    ));
    print_value(event)?;
    Ok(0)
}

fn load_goals_registry(requested_path: Option<&Path>) -> Result<(Value, Value), MosaicError> {
    let path = requested_path
        .map(Path::to_path_buf)
        .unwrap_or_else(mosaic_goals::default_config_path);
    if requested_path.is_none() && !path.exists() {
        return Ok((
            mosaic_goals::empty_registry(),
            json!({
                "path": path.display().to_string(),
                "loaded": false,
                "missing": true,
            }),
        ));
    }
    let registry = read_goals_registry(&path)?;
    Ok((
        registry,
        json!({
            "path": path.display().to_string(),
            "loaded": true,
        }),
    ))
}

fn read_goals_registry(path: &Path) -> Result<Value, MosaicError> {
    let raw = fs::read_to_string(path).map_err(|e| {
        MosaicError::new(
            "goals_registry_read_failed",
            format!("failed to read {}: {e}", path.display()),
        )
    })?;
    let value = serde_json::from_str::<Value>(&raw).map_err(|e| {
        MosaicError::new(
            "invalid_goals_registry_json",
            format!("{}: {e}", path.display()),
        )
    })?;
    mosaic_goals::normalize_registry_input(value)
        .map_err(|e| MosaicError::new("invalid_goals_registry", format!("{}: {e}", path.display())))
}

fn goals_todos_event(
    id: &str,
    status: &str,
    ack: &str,
    exit_code: Option<u8>,
    error: Option<String>,
    plan: &mosaic_goals::TodosCommandPlan,
    redact_command: bool,
    summary: Option<Value>,
    data: Option<Value>,
) -> Value {
    json!({
        "schema_version": SCHEMA_VERSION,
        "event": "goals.todos_plan",
        "goal_schema_version": mosaic_goals::GOALS_SCHEMA_VERSION,
        "id": id,
        "operation": "goals.todos_plan",
        "adapter": "todos",
        "status": status,
        "ack": ack,
        "timestamp_ms": now_millis(),
        "exit_code": exit_code,
        "error": error,
        "command": if redact_command {
            mosaic_goals::redact_todos_command_plan(plan)
        } else {
            plan.to_json()
        },
        "summary": summary,
        "data": data,
    })
}

fn goals_todos_audit_record(
    id: &str,
    status: &str,
    ack: &str,
    exit_code: Option<u8>,
    error: Option<String>,
    plan: &mosaic_goals::TodosCommandPlan,
    summary: Option<Value>,
) -> Value {
    json!({
        "schema_version": SCHEMA_VERSION,
        "event": "receipt",
        "id": id,
        "operation": "goals.todos_plan",
        "adapter": "todos",
        "status": status,
        "ack": ack,
        "timestamp_ms": now_millis(),
        "exit_code": exit_code,
        "error": error,
        "command": mosaic_goals::redact_todos_command_plan(plan),
        "summary": summary,
    })
}

fn run_machines(command: MachineCommand, dry_run: bool) -> Result<u8, MosaicError> {
    match command {
        MachineCommand::Local => {
            print_value(json!({
                "schema_version": SCHEMA_VERSION,
                "event": "machines.local",
                "machine_schema_version": mosaic_machines::MACHINE_SCHEMA_VERSION,
                "timestamp_ms": now_millis(),
                "data": mosaic_machines::local_machine(),
            }))?;
            Ok(0)
        },
        MachineCommand::List(args) => {
            let (machines, registry) = load_machine_registry(args.file.as_deref(), true)?;
            print_value(json!({
                "schema_version": SCHEMA_VERSION,
                "event": "machines.list",
                "machine_schema_version": mosaic_machines::MACHINE_SCHEMA_VERSION,
                "timestamp_ms": now_millis(),
                "registry": registry,
                "data": machines,
            }))?;
            Ok(0)
        },
        MachineCommand::Validate(args) => {
            let registry = read_machine_registry(&args.file)?;
            mosaic_machines::validate_registry(&registry).map_err(|e| {
                MosaicError::new(
                    "invalid_machine_registry",
                    format!("{}: {e}", args.file.display()),
                )
            })?;
            print_value(json!({
                "schema_version": SCHEMA_VERSION,
                "event": "machines.validate",
                "machine_schema_version": mosaic_machines::MACHINE_SCHEMA_VERSION,
                "timestamp_ms": now_millis(),
                "valid": true,
                "registry": registry,
            }))?;
            Ok(0)
        },
        MachineCommand::Exec(args) => run_machine_exec(args, dry_run),
    }
}

fn run_machine_exec(args: MachineExecArgs, dry_run: bool) -> Result<u8, MosaicError> {
    let (machines, registry) = load_machine_registry(args.file.as_deref(), true)?;
    let machine = mosaic_machines::find_machine(&machines, &args.machine).ok_or_else(|| {
        MosaicError::new(
            "machine_not_found",
            format!(
                "machine {:?} not found in local descriptor or configured registry",
                args.machine
            ),
        )
    })?;
    let plan =
        mosaic_machines::build_command_plan(machine, &args.command, args.mosaic_bin.as_deref())
            .map_err(|e| MosaicError::new("invalid_machine_command", e))?;
    let id = format!("mosaic-machine-{}-{}", std::process::id(), now_millis());
    if dry_run {
        let event = machine_exec_event(
            &id,
            "dry_run",
            "none",
            None,
            None,
            &plan,
            &registry,
            args.redact_command,
        );
        audit(&machine_exec_audit_record(
            &id, "dry_run", "none", None, None, &plan,
        ));
        print_value(event)?;
        return Ok(0);
    }

    let output = Command::new(&plan.program)
        .args(&plan.args)
        .output()
        .map_err(|e| {
            MosaicError::new(
                "machine_exec_failed",
                format!("failed to spawn {}: {e}", plan.program),
            )
        })?;
    let exit_code = output.status.code().unwrap_or(1) as u8;
    let status = if output.status.success() {
        "completed"
    } else {
        "error"
    };
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let error = if output.status.success() {
        None
    } else {
        Some(format!("transport command exited with status {exit_code}"))
    };
    let event = machine_exec_event(
        &id,
        status,
        "process_exited",
        Some(exit_code),
        error.clone(),
        &plan,
        &registry,
        args.redact_command,
    );
    let mut event = event;
    event["stdout"] = json!(stdout);
    event["stderr"] = json!(stderr);
    if let Ok(stdout_json) = serde_json::from_slice::<Value>(&output.stdout) {
        event["stdout_json"] = stdout_json;
    }
    audit(&machine_exec_audit_record(
        &id,
        status,
        "process_exited",
        Some(exit_code),
        error,
        &plan,
    ));
    print_value(event)?;
    Ok(exit_code)
}

fn load_machine_registry(
    requested_path: Option<&Path>,
    include_local: bool,
) -> Result<(Vec<Value>, Value), MosaicError> {
    let path = requested_path
        .map(Path::to_path_buf)
        .unwrap_or_else(mosaic_machines::default_config_path);
    let mut machines = Vec::new();
    if include_local {
        machines.push(mosaic_machines::local_machine());
    }
    if requested_path.is_none() && !path.exists() {
        return Ok((
            machines,
            json!({
                "path": path.display().to_string(),
                "loaded": false,
                "missing": true,
            }),
        ));
    }
    match read_machine_registry(&path) {
        Ok(registry) => {
            let configured = mosaic_machines::machines_from_registry(&registry).map_err(|e| {
                MosaicError::new(
                    "invalid_machine_registry",
                    format!("{}: {e}", path.display()),
                )
            })?;
            if include_local
                && configured
                    .iter()
                    .any(|machine| machine.get("id").and_then(Value::as_str) == Some("local"))
            {
                return Err(MosaicError::new(
                    "invalid_machine_registry",
                    format!(
                        "{}: configured machine id \"local\" is reserved for the built-in local descriptor",
                        path.display()
                    ),
                ));
            }
            machines.extend(configured);
            Ok((
                machines,
                json!({
                    "path": path.display().to_string(),
                    "loaded": true,
                }),
            ))
        },
        Err(error) => Err(error),
    }
}

fn read_machine_registry(path: &Path) -> Result<Value, MosaicError> {
    let raw = fs::read_to_string(path).map_err(|e| {
        MosaicError::new(
            "machine_registry_read_failed",
            format!("failed to read {}: {e}", path.display()),
        )
    })?;
    serde_json::from_str::<Value>(&raw).map_err(|e| {
        MosaicError::new(
            "invalid_machine_registry_json",
            format!("{}: {e}", path.display()),
        )
    })
}

fn machine_exec_event(
    id: &str,
    status: &str,
    ack: &str,
    exit_code: Option<u8>,
    error: Option<String>,
    plan: &mosaic_machines::MachineCommandPlan,
    registry: &Value,
    redact_command: bool,
) -> Value {
    let mut command = plan.to_json();
    if redact_command {
        redact_machine_command_plan(&mut command);
    }
    json!({
        "schema_version": SCHEMA_VERSION,
        "event": "machines.exec",
        "id": id,
        "operation": "machines.exec",
        "machine": &plan.machine_id,
        "transport": &plan.transport_kind,
        "status": status,
        "ack": ack,
        "timestamp_ms": now_millis(),
        "exit_code": exit_code,
        "error": error,
        "registry": registry,
        "command": command,
    })
}

fn machine_exec_audit_record(
    id: &str,
    status: &str,
    ack: &str,
    exit_code: Option<u8>,
    error: Option<String>,
    plan: &mosaic_machines::MachineCommandPlan,
) -> Value {
    let mut command = plan.to_json();
    redact_machine_command_plan(&mut command);
    json!({
        "schema_version": SCHEMA_VERSION,
        "event": "receipt",
        "id": id,
        "operation": "machines.exec",
        "machine": &plan.machine_id,
        "transport": &plan.transport_kind,
        "status": status,
        "ack": ack,
        "timestamp_ms": now_millis(),
        "exit_code": exit_code,
        "error": error,
        "command": command,
    })
}

fn redact_machine_command_plan(command: &mut Value) {
    if let Value::Object(object) = command {
        let transport = object
            .get("transport")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        for field in ["mosaic_command", "argv", "args"] {
            if let Some(Value::Array(values)) = object.get_mut(field) {
                if transport == "ssh" && matches!(field, "argv" | "args") {
                    if let Some(last) = values.last_mut() {
                        *last = json!("[redacted]");
                    }
                } else {
                    let segments = values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect::<Vec<_>>();
                    *values = mosaic_machines::redact_mosaic_command(&segments)
                        .into_iter()
                        .map(Value::String)
                        .collect();
                }
            }
        }
        if object.contains_key("remote_shell_command") {
            object.insert("remote_shell_command".to_owned(), json!("[redacted]"));
        }
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

fn run_dashboard(session: Option<String>, args: DashboardArgs) -> Result<u8, MosaicError> {
    let format = args.format.clone();
    let snapshot = build_dashboard_snapshot(session, args)?;
    match format {
        DashboardFormat::Json => print_value(snapshot)?,
        DashboardFormat::Text => print_dashboard_text(&snapshot)?,
    }
    Ok(0)
}

fn build_dashboard_snapshot(
    requested_session: Option<String>,
    args: DashboardArgs,
) -> Result<Value, MosaicError> {
    let sessions = list_sessions_values()?;
    let mut partial = false;
    let mut errors = Vec::new();

    let show_prompt_bodies = args.show_prompts && !args.redact;
    let queue_summary = match read_queue_records(requested_session.as_deref(), None) {
        Ok(mut queue_records) => {
            sort_values_by_timestamp(&mut queue_records);
            summarize_queue_records(&queue_records, args.limit, show_prompt_bodies)
        },
        Err(error) => {
            let section_error = dashboard_section_error("queues", &error);
            partial = true;
            errors.push(section_error);
            summarize_queue_records(&[], args.limit, false)
        },
    };

    let mut audit_summary = match read_ndjson_file(&audit_path()) {
        Ok(mut audit_records) => {
            if let Some(session) = requested_session.as_deref() {
                audit_records.retain(|record| record_matches_session(record, session));
            }
            sort_values_by_timestamp(&mut audit_records);
            summarize_audit_records(&audit_records, args.limit)
        },
        Err(error) => {
            let section_error = dashboard_section_error("audit", &error);
            partial = true;
            errors.push(section_error);
            summarize_audit_records(&[], args.limit)
        },
    };
    if !show_prompt_bodies {
        redact_prompt_value(&mut audit_summary);
    }

    let goals = match load_goals_registry(args.goals_file.as_deref()) {
        Ok((registry, mut source)) => {
            if args.redact {
                redact_dashboard_source(&mut source);
            }
            let loaded = source
                .get("loaded")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            json!({
                "status": if loaded { "loaded" } else { "not_configured" },
                "source": source,
                "summary": mosaic_goals::summarize_registry(&registry, args.limit, args.redact),
            })
        },
        Err(error) => {
            let section_error = dashboard_section_error("goals", &error);
            partial = true;
            errors.push(section_error.clone());
            json!({
                "status": "error",
                "error": section_error,
                "summary": mosaic_goals::summarize_registry(
                    &mosaic_goals::empty_registry(),
                    args.limit,
                    args.redact,
                ),
            })
        },
    };

    let live = if args.live {
        match resolve_session(requested_session.clone())
            .and_then(|session| build_live_dashboard_snapshot(&session, args.redact))
        {
            Ok(live) => live,
            Err(error) => {
                let error = dashboard_section_error("live", &error);
                partial = true;
                errors.push(error.clone());
                json!({
                    "requested": true,
                    "session": requested_session.clone(),
                    "status": "error",
                    "error": error.clone(),
                    "agents": {
                        "total": 0,
                        "by_kind": [],
                        "panes": []
                    }
                })
            },
        }
    } else {
        json!({
            "requested": false,
            "session": requested_session.clone(),
            "status": "not_requested",
            "agents": {
                "total": 0,
                "by_kind": [],
                "panes": []
            }
        })
    };

    Ok(json!({
        "schema_version": SCHEMA_VERSION,
        "event": "dashboard.snapshot",
        "timestamp_ms": now_millis(),
        "partial": partial,
        "errors": errors,
        "session": requested_session,
        "state_scope": "local_user",
        "sessions": sessions,
        "queues": queue_summary,
        "audit": audit_summary,
        "goals": goals,
        "live": live,
    }))
}

fn dashboard_section_error(section: &'static str, error: &MosaicError) -> Value {
    json!({
        "section": section,
        "code": error.code,
        "message": error.message,
    })
}

fn redact_dashboard_source(source: &mut Value) {
    if let Value::Object(object) = source {
        if object.contains_key("path") {
            object.insert("path".to_owned(), json!("[redacted]"));
        }
    }
}

fn list_sessions_values() -> Result<Vec<Value>, MosaicError> {
    Ok(get_sessions()
        .map_err(|e| MosaicError::new("sessions_list_failed", format!("{e:?}")))?
        .into_iter()
        .map(|(name, age)| {
            json!({
                "name": name,
                "age_seconds": age.as_secs(),
                "status": "running",
            })
        })
        .collect())
}

fn build_live_dashboard_snapshot(session: &str, redact: bool) -> Result<Value, MosaicError> {
    let panes_output = dispatch_cli_action_capture(
        session,
        CliAction::ListPanes {
            tab: true,
            command: true,
            state: true,
            geometry: true,
            all: true,
            json: true,
        },
    )?;
    let panes = mosaic_agent::enrich_panes_data(parse_server_json(panes_output.lines)?);
    let tabs_output = dispatch_cli_action_capture(
        session,
        CliAction::ListTabs {
            state: true,
            dimensions: true,
            panes: true,
            layout: true,
            all: true,
            json: true,
        },
    )?;
    let tabs = parse_server_json(tabs_output.lines)?;
    let agents = summarize_agent_panes(&panes, redact);
    Ok(json!({
        "requested": true,
        "session": session,
        "status": "captured",
        "pane_count": value_array_len(&panes),
        "tab_count": value_array_len(&tabs),
        "agents": agents,
    }))
}

fn summarize_queue_records(records: &[Value], limit: usize, show_prompts: bool) -> Value {
    let mut by_session: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for record in records {
        let session = value_string_field(record, "session").unwrap_or_else(|| "unknown".to_owned());
        let pane_id = value_string_field(record, "pane_id").unwrap_or_else(|| "unknown".to_owned());
        *by_session
            .entry(session)
            .or_default()
            .entry(pane_id)
            .or_default() += 1;
    }
    let by_session = by_session
        .into_iter()
        .map(|(session, panes)| {
            let by_pane = panes
                .into_iter()
                .map(|(pane_id, pending)| {
                    json!({
                        "pane_id": pane_id,
                        "pending": pending,
                    })
                })
                .collect::<Vec<_>>();
            let pending = by_pane
                .iter()
                .filter_map(|pane| pane.get("pending").and_then(Value::as_u64))
                .sum::<u64>();
            json!({
                "session": session,
                "pending": pending,
                "by_pane": by_pane,
            })
        })
        .collect::<Vec<_>>();
    let mut recent = last_n_values(records.to_vec(), Some(limit));
    if !show_prompts {
        redact_prompts(&mut recent);
    }
    json!({
        "total_pending": records.len(),
        "by_session": by_session,
        "recent": recent,
        "prompt_bodies": if show_prompts { "included" } else { "redacted" },
    })
}

fn summarize_audit_records(records: &[Value], limit: usize) -> Value {
    let mut by_operation: BTreeMap<String, usize> = BTreeMap::new();
    for record in records {
        let operation = value_string_field(record, "operation")
            .or_else(|| value_string_field(record, "event"))
            .unwrap_or_else(|| "unknown".to_owned());
        *by_operation.entry(operation).or_default() += 1;
    }
    let by_operation = by_operation
        .into_iter()
        .map(|(operation, count)| {
            json!({
                "operation": operation,
                "count": count,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "total_records": records.len(),
        "by_operation": by_operation,
        "recent": last_n_values(records.to_vec(), Some(limit)),
    })
}

fn summarize_agent_panes(panes: &Value, redact: bool) -> Value {
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut summaries = Vec::new();
    if let Some(panes) = panes.as_array() {
        for pane in panes {
            let agent = pane.get("mosaic_agent").unwrap_or(&Value::Null);
            let kind = value_string_field(agent, "kind").unwrap_or_else(|| "unknown".to_owned());
            *by_kind.entry(kind.clone()).or_default() += 1;
            summaries.push(summarize_agent_pane(pane, agent, &kind, redact));
        }
    }
    let by_kind = by_kind
        .into_iter()
        .map(|(kind, count)| json!({ "kind": kind, "count": count }))
        .collect::<Vec<_>>();
    json!({
        "total": summaries.len(),
        "by_kind": by_kind,
        "panes": summaries,
    })
}

fn summarize_agent_pane(pane: &Value, agent: &Value, kind: &str, redact: bool) -> Value {
    let cwd = value_string_field(agent, "cwd").map(|cwd| redact_string(cwd, redact));
    let command =
        value_string_field(agent, "command").map(|command| redact_string(command, redact));
    let title = value_string_field(pane, "title")
        .or_else(|| value_string_field(pane, "pane_title"))
        .map(|title| redact_string(title, redact));
    let current_task = agent
        .get("current_task")
        .and_then(Value::as_str)
        .map(|current_task| redact_string(current_task.to_owned(), redact));
    let repo = match agent.get("repo") {
        Some(Value::Object(repo)) => json!({
            "name": repo.get("name").cloned().unwrap_or(Value::Null),
            "path": repo
                .get("path")
                .and_then(Value::as_str)
                .map(|path| redact_string(path.to_owned(), redact)),
        }),
        _ => Value::Null,
    };
    json!({
        "pane_id": dashboard_pane_id(pane),
        "title": title,
        "kind": kind,
        "confidence": agent.get("confidence").cloned().unwrap_or(Value::Null),
        "status": value_string_field(agent, "status"),
        "composer_state": value_string_field(agent, "composer_state"),
        "submit_keys": agent.get("submit_keys").cloned().unwrap_or_else(|| json!([])),
        "cwd": cwd,
        "repo": repo,
        "command": command,
        "current_task": current_task,
    })
}

fn dashboard_pane_id(pane: &Value) -> Value {
    pane.get("pane_id")
        .cloned()
        .or_else(|| pane.get("id").cloned())
        .unwrap_or(Value::Null)
}

fn value_array_len(value: &Value) -> usize {
    value.as_array().map(Vec::len).unwrap_or(0)
}

fn record_matches_session(record: &Value, session: &str) -> bool {
    value_string_field(record, "session").as_deref() == Some(session)
        || record
            .get("receipt")
            .and_then(|receipt| value_string_field(receipt, "session"))
            .as_deref()
            == Some(session)
}

fn value_string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn redact_string(value: String, redact: bool) -> String {
    if redact {
        "[redacted]".to_owned()
    } else {
        value
    }
}

fn print_dashboard_text(snapshot: &Value) -> Result<(), MosaicError> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    map_stdout_write_result(write_dashboard_text(&mut stdout, snapshot))
}

fn write_dashboard_text(writer: &mut dyn Write, snapshot: &Value) -> io::Result<()> {
    writeln!(writer, "Open Mosaic Dashboard")?;
    writeln!(
        writer,
        "Sessions: {} running",
        snapshot
            .get("sessions")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0)
    )?;
    if snapshot.get("partial").and_then(Value::as_bool) == Some(true) {
        writeln!(writer, "Partial: true")?;
        for error in snapshot
            .get("errors")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            writeln!(
                writer,
                "  {} error: {}",
                dashboard_text_cell(
                    error
                        .get("section")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                ),
                dashboard_text_cell(
                    error
                        .get("code")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                )
            )?;
        }
    }
    if let Some(session) = snapshot.get("session").and_then(Value::as_str) {
        writeln!(writer, "Filter: session {}", dashboard_text_cell(session))?;
    }
    let queues = &snapshot["queues"];
    writeln!(
        writer,
        "Queues: {} pending ({})",
        queues
            .get("total_pending")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        queues
            .get("prompt_bodies")
            .and_then(Value::as_str)
            .unwrap_or("redacted")
    )?;
    for session in queues
        .get("by_session")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        writeln!(
            writer,
            "  {}: {} pending",
            dashboard_text_cell(
                session
                    .get("session")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            ),
            session.get("pending").and_then(Value::as_u64).unwrap_or(0)
        )?;
    }
    let audit = &snapshot["audit"];
    writeln!(
        writer,
        "Audit: {} records",
        audit
            .get("total_records")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    )?;
    let goals = &snapshot["goals"];
    let goal_summary = &goals["summary"];
    writeln!(
        writer,
        "Goals: {} goals, {} tasks ({})",
        goal_summary
            .get("total_goals")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        goal_summary
            .get("total_tasks")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        dashboard_text_cell(
            goals
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        )
    )?;
    for task in goal_summary
        .get("active")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        writeln!(
            writer,
            "  {} [{}]",
            dashboard_text_cell(task.get("id").and_then(Value::as_str).unwrap_or("unknown")),
            dashboard_text_cell(
                task.get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            )
        )?;
    }
    let live = &snapshot["live"];
    writeln!(
        writer,
        "Live: {}",
        dashboard_text_cell(
            live.get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        )
    )?;
    if live.get("requested").and_then(Value::as_bool) == Some(true) {
        writeln!(
            writer,
            "Panes: {}  Tabs: {}",
            live.get("pane_count").and_then(Value::as_u64).unwrap_or(0),
            live.get("tab_count").and_then(Value::as_u64).unwrap_or(0)
        )?;
    }
    let agents = &live["agents"];
    writeln!(
        writer,
        "Agent Metadata: {} panes",
        agents.get("total").and_then(Value::as_u64).unwrap_or(0)
    )?;
    for kind in agents
        .get("by_kind")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        writeln!(
            writer,
            "  {}: {}",
            dashboard_text_cell(
                kind.get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            ),
            kind.get("count").and_then(Value::as_u64).unwrap_or(0)
        )?;
    }
    writer.flush()
}

fn dashboard_text_cell(value: &str) -> String {
    const MAX_CHARS: usize = 80;
    let mut sanitized = String::new();
    let mut count = 0;
    for character in value.chars() {
        if count >= MAX_CHARS {
            sanitized.push_str("...");
            break;
        }
        if character.is_control() {
            sanitized.push('?');
        } else {
            sanitized.push(character);
        }
        count += 1;
    }
    sanitized
}

fn run_sessions(command: SessionCommand, dry_run: bool) -> Result<u8, MosaicError> {
    match command {
        SessionCommand::List => {
            let sessions = list_sessions_values()?;
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

#[cfg(test)]
mod dashboard_tests {
    use super::*;

    #[test]
    fn live_agent_redaction_hides_sensitive_summary_fields() {
        let summary = summarize_agent_panes(
            &json!([
                {
                    "id": 7,
                    "is_plugin": false,
                    "title": "working in /secret/repo",
                    "mosaic_agent": {
                        "kind": "codewith",
                        "confidence": 0.95,
                        "status": "running",
                        "composer_state": "working",
                        "submit_keys": ["Tab", "Enter"],
                        "cwd": "/secret/repo",
                        "repo": {
                            "path": "/secret/repo",
                            "name": "repo"
                        },
                        "command": "codewith --token secret",
                        "current_task": "ship private task"
                    }
                }
            ]),
            true,
        );

        let pane = &summary["panes"][0];
        assert_eq!(pane["title"], "[redacted]");
        assert_eq!(pane["cwd"], "[redacted]");
        assert_eq!(pane["repo"]["path"], "[redacted]");
        assert_eq!(pane["command"], "[redacted]");
        assert_eq!(pane["current_task"], "[redacted]");
        assert_eq!(pane["repo"]["name"], "repo");
        let serialized = serde_json::to_string(&summary).expect("summary json");
        assert!(!serialized.contains("/secret"));
        assert!(!serialized.contains("private task"));
        assert!(!serialized.contains("secret"));
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
    map_stdout_write_result(write_json_line(&mut stdout, value))
}

fn map_stdout_write_result(result: io::Result<()>) -> Result<(), MosaicError> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(MosaicError::new("stdout_write_failed", error.to_string())),
    }
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
