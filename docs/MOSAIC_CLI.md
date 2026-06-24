# Open Mosaic Native CLI

The `mosaic` binary is the Open Mosaic control surface for agents and scripts.
It maps public Mosaic commands to the current Zellij-derived IPC internally, so
callers do not depend on raw `ClientToServerMsg` or `ServerToClientMsg`
semantics.

Schema-bearing machine output uses `schema_version: "mosaic.control.v1"`.
Unknown JSON fields are reserved for compatible additions.
The JSON Schema contract is published in
`schemas/mosaic.control.v1.schema.json` and documented in
`docs/MOSAIC_SCHEMAS.md`.

## Sessions

```sh
mosaic sessions list
mosaic sessions create work --background
mosaic sessions attach work
mosaic sessions close work
```

Session creation and attach currently use the compatibility binary path because
the TUI client still lives in the Zellij-derived entrypoint. Set
`MOSAIC_ZELLIJ_BIN=/path/to/zellij` if that binary is not on `PATH`.

## Panes And Tabs

```sh
mosaic --session work panes list --all
mosaic --session work tabs list --all
mosaic --session work pane create --name tests -- cargo test
mosaic --session work tab create --name build -- cargo build
```

`panes list` and `tabs list` return a Mosaic envelope whose `data` field holds
the server-provided pane or tab array. `panes list --all` asks the
Zellij-derived server for command and cwd details where available.

`panes list` enriches each pane with a `mosaic_agent` object. The classifier is
best-effort and portable: it uses only generic pane fields such as title,
command, cwd, plugin status, and exit/held state. It does not require Hasna
services or private registries.

```json
{
  "id": 1,
  "is_plugin": false,
  "title": "working: Build Open Mosaic",
  "pane_command": "node /home/user/.bun/bin/codewith --no-alt-screen",
  "pane_cwd": "/work/open-mosaic",
  "mosaic_agent": {
    "schema_version": "mosaic.agent.v1",
    "kind": "codewith",
    "confidence": 0.95,
    "signals": ["command:codewith"],
    "status": "running",
    "composer_state": "unknown",
    "submit_keys": ["Tab", "Enter"],
    "cwd": "/work/open-mosaic",
    "repo": {
      "path": "/work/open-mosaic",
      "name": "open-mosaic"
    },
    "command": "node /home/user/.bun/bin/codewith --no-alt-screen",
    "current_task": null
  }
}
```

Known `mosaic_agent.kind` values are `codewith`, `claude_code`, `opencode`,
`codex`, `shell`, `server`, `log`, `plugin`, and `unknown`. Consumers must
treat low-confidence and unknown values as advisory metadata, not as permission
to force prompt delivery. `repo.path` is local observer metadata and can contain
an absolute path from the machine that produced the observation. `submit_keys`
are hints for adapters, not a promise that prompt submission is safe for a
specific pane.

## Prompt Delivery

```sh
mosaic --session work prompt send --pane-id terminal_1 --text "cargo test"
mosaic --session work prompt send --pane-id terminal_1 --file prompt.txt --no-submit
mosaic --session work --dry-run prompt send --pane-id terminal_1 --text "status?"
mosaic --session work prompt send --pane-id terminal_1 --queue --text "next task"
mosaic --session work queue list --pane-id terminal_1 --redact
mosaic --session work queue clear --pane-id terminal_1 --receipt-id mosaic-123
```

Receipts use this shape:

```json
{
  "schema_version": "mosaic.control.v1",
  "event": "receipt",
  "id": "mosaic-123-1782290000000",
  "operation": "prompt.send",
  "session": "work",
  "pane_id": "terminal_1",
  "status": "accepted",
  "ack": "server_accepted",
  "timestamp_ms": 1782290000000,
  "error": null
}
```

For operations that touch the terminal server, `status: "accepted"` with
`ack: "server_accepted"` means the server accepted the write/paste/key action.
It does not mean the terminal process consumed the bytes or completed work. Use
capture or subscribe to observe results. Local state operations such as
`queue clear` use `ack: "local_state_updated"` when the queue file mutation
has completed.

Queued prompts are stored as NDJSON under `$XDG_STATE_HOME/open-mosaic/queues`
or `~/.local/state/open-mosaic/queues`. Audit records are appended to
`audit.ndjson` in the same state directory. Set `MOSAIC_AUDIT_REDACT=1` to
redact queued prompt bodies from local queue records.

`queue list` returns a `queue.list` envelope with local queued prompt records in
the `data` array, ordered by `timestamp_ms` before `--limit` keeps the newest
records. Omit `--session` to inspect all local queue files, or pass `--pane-id`
to filter. `--redact` replaces prompt bodies with `[redacted]` in the command
output without modifying the queue file. `queue clear` requires `--session` and
`--pane-id`; it removes either the whole pane queue or one record selected by
`--receipt-id`. Queue appends and clears are serialized per queue file so a
receipt-specific clear cannot discard unrelated queued prompts. Top-level
`--dry-run` emits a `queue.clear` receipt without mutating the queue.

```sh
mosaic audit list --limit 20
mosaic audit list --redact
```

`audit list` returns an `audit.list` envelope with local audit records. Audit
records are local observer data; consumers should treat them as append-only
evidence, not as proof that a terminal process consumed a prompt.

## Adapters

```sh
mosaic adapters list
mosaic adapters list --kind agent
mosaic adapters validate --file adapter.json
```

`adapters list` returns built-in portable adapter interface descriptors using
`adapter_schema_version: "mosaic.adapter.v1"`. `adapters validate` validates a
manifest file without executing it. See `docs/MOSAIC_ADAPTERS.md` for the
manifest schema, supported kinds, and capability names.

## Machines

```sh
mosaic machines local
mosaic machines list
mosaic machines list --file machines.json
mosaic machines validate --file machines.json
mosaic --dry-run machines exec --file machines.json --machine dev -- sessions list
```

`machines` is the optional multi-machine surface. It uses portable
`mosaic.machine.v1` registry files and generic transports such as SSH. It does
not require private registries or Open Machines. `machines exec` runs the
normal Mosaic command named after `--` on the target machine; use top-level
`--dry-run` to inspect the local command plan without connecting, and
`--redact-command` when prompt bodies or prompt file paths appear in the
command. See `docs/MOSAIC_MACHINES.md` for the registry shape and SSH safety
rules.

## Goals And Tasks

```sh
mosaic goals list
mosaic goals list --file goals.json --redact
mosaic goals validate --file goals.json
mosaic --dry-run goals todos-plan --project /work/repo --plan plan-id --redact
```

`goals` is the portable task/goal context surface. The generic registry schema
is `mosaic.goals.v1`, stored by default at
`$XDG_CONFIG_HOME/open-mosaic/goals.json` or
`~/.config/open-mosaic/goals.json`. If the default file is absent,
`goals list` returns an empty registry with `configured: false`; explicit
`--file` paths must exist and validate.

`goals todos-plan` is an optional adapter command that runs the external
`todos` CLI only when requested, using argv execution rather than a shell. It
normalizes `todos --project <path> --json plans --show <plan-id>` into a
Mosaic `goals.todos_plan` envelope. Top-level `--dry-run` returns the planned
argv without executing the adapter. `--redact` hides task text and local paths
from returned JSON. See `docs/MOSAIC_GOALS.md` for the full registry shape and
adapter boundary.

## Web Oversight

```sh
mosaic web link --session work
mosaic web link --session work --mode watch --token-name observer
mosaic web link --session work --mode control --base-url https://mosaic.example.test/base/
mosaic web link --session work --redact
```

`web link` returns a `web.link` envelope using `web_schema_version:
"mosaic.web.v1"`. It is a local helper for the existing Zellij-derived web
client: it builds a bookmarkable `/{session}` URL, distinguishes read-only
watcher links from control links, and documents the required token type. It
does not start the web server, create tokens, or verify that the session
exists.

Watch mode is the default and sets `read_only_required: true`,
`watcher: true`, and `control_allowed: false`. It is intended for read-only
tokens created with `zellij web --create-read-only-token <name>`. Control mode
requires a normal token created with `zellij web --create-token <name>` and
sets `control_allowed: true`.

Raw tokens are never accepted by `mosaic web link` and are never embedded in
the URL. `--base-url` accepts only `http` and `https`; credentials, query
strings, and fragments are rejected to avoid leaking secrets into links or
logs. Use `--redact` when a controller should report link metadata without
exposing session names, hostnames, route paths, or token labels. See
`docs/MOSAIC_WEB.md` for web oversight workflows and secure deployment notes.

## Observation

```sh
mosaic --session work observe pane --pane-id terminal_1 --last-lines 40
mosaic --session work observe pane --pane-id terminal_1 --scrollback --last-lines 100
mosaic --session work observe pane --pane-id terminal_1 --redact
mosaic --session work capture --pane-id terminal_1 --scrollback
mosaic --session work subscribe --pane-id terminal_1 --scrollback 50
mosaic --session work subscribe --pane-id terminal_1 --format raw
```

`observe pane` returns an `observe.pane` JSON event for agents and dashboards.
It captures the current pane through the server dump-screen path, optionally
includes full scrollback, applies `--last-lines` after capture, and includes a
deterministic `activity` summary with total/returned line counts, non-empty
line count, character count, truncation status, last non-empty line, and server
exit code. `--last-lines 0` means all captured lines. `--redact` replaces
returned non-empty terminal lines and the last-line summary with `[redacted]`.
Setting `MOSAIC_OBSERVE_REDACT=1` applies the same output redaction by default.

Each successful observation appends an `observation` audit record with the same
ID and audit-safe activity counts, but never stores raw terminal output lines or
the raw last-line summary in the audit record. This keeps the local audit trail
useful for receipts and observation timelines without silently persisting pane
contents.

`subscribe` defaults to NDJSON. Each pane update includes
`schema_version`, `event`, `session`, `pane_id`, `sequence`, `timestamp_ms`,
`is_initial`, `viewport`, and optional `scrollback`. Raw mode is explicitly
human-oriented and can contain pane-controlled terminal text.

## Dashboard

```sh
mosaic dashboard
mosaic dashboard --format text
mosaic --session work dashboard
mosaic --session work dashboard --live --redact
mosaic dashboard --goals-file goals.json --redact
```

`dashboard` returns a `dashboard.snapshot` envelope that combines local Mosaic
state into one compact view for agents, scripts, and terminal dashboard panes.
It does not require private services. Without `--live`, it only reads local
user state and the session list: pending queues, recent audit records, optional
goals/tasks, and running session names. With `--live`, it also asks the target
Mosaic/Zellij session for panes and tabs, enriches panes with `mosaic_agent`
metadata, and summarizes agent kinds without returning full raw pane dumps.

Queued prompt bodies are redacted by default in dashboard JSON and text output.
Pass `--show-prompts` only when the caller is allowed to view queued prompt
content. `--redact` forces prompt-body redaction and redacts live pane titles,
current task text, goals/task text, local paths, and command details from live
agent summaries. When one section cannot be read, for example live panes, a
configured goals registry, local queues, or the audit log, the command still
returns the remaining snapshot with `partial: true`, an `errors` array, and a
section-specific status such as `live.status: "error"` or
`goals.status: "error"`. The text format is intended for a compact terminal
pane and sanitizes control characters from dynamic labels:

```text
Open Mosaic Dashboard
Sessions: 1 running
Queues: 2 pending (redacted)
Audit: 6 records
Goals: 1 goals, 3 tasks (loaded)
Live: not_requested
Agent Metadata: 0 panes
```

Runtime errors are emitted to stderr as JSON:

```json
{
  "schema_version": "mosaic.control.v1",
  "event": "error",
  "code": "no_active_session",
  "message": "no active Mosaic/Zellij session found; pass --session",
  "timestamp_ms": 1782290000000
}
```

## Review Commands

The Zellij-derived default development profile expects prebuilt debug WASM
plugins. For this first Mosaic CLI slice, reviewers can validate the control
surface without those plugin artifacts with:

```sh
cargo fmt --check
cargo check --bin mosaic --no-default-features --features vendored_curl
cargo test --bin mosaic --no-default-features --features vendored_curl
cargo test --test mosaic_cli --no-default-features --features vendored_curl
cargo build --bin mosaic --bin zellij --no-default-features --features vendored_curl
scripts/mosaic-workflow-smoke.sh
```

Default-feature packaging and release validation still need the normal plugin
artifact build path before publishing broader installable artifacts.
When an agent CLI is installed locally and the operator wants to exercise a
real agent pane instead of the deterministic shell fixture, run:

```sh
scripts/mosaic-agent-workflow-smoke.sh
```

The real-agent smoke script launches a disposable `mosaic-agent-smoke-*`
session, verifies `mosaic_agent` metadata before prompt delivery, writes a
no-submit prompt by default, queues a follow-up, observes/captures/subscribes
output, checks audit/queue redaction, and closes the session. Set
`MOSAIC_AGENT_SMOKE_SUBMIT=enter` only when an operator intentionally wants to
submit the prompt to the agent model.
