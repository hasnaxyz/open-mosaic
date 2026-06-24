# Open Mosaic Native CLI

The `mosaic` binary is the Open Mosaic control surface for agents and scripts.
It maps public Mosaic commands to the current Zellij-derived IPC internally, so
callers do not depend on raw `ClientToServerMsg` or `ServerToClientMsg`
semantics.

Schema-bearing machine output uses `schema_version: "mosaic.control.v1"`.
Unknown JSON fields are reserved for compatible additions.

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
```

Default-feature packaging and release validation still need the normal plugin
artifact build path before publishing broader installable artifacts.
