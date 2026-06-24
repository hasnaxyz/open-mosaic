# Optional open-dispatch Integration

Open Mosaic does not require dispatch. Dispatch integrations must be optional
adapters or backends that call the native `mosaic` CLI/API. The current tmux
backend should remain the default backend for dispatch users unless they
explicitly select Mosaic.

This document describes the expected contract for an optional Mosaic backend in
open-dispatch or similar controllers.

## Backend Boundary

A dispatch Mosaic backend should use these native commands instead of scraping
interactive terminal output:

```sh
mosaic sessions list
mosaic --session work panes list --all
mosaic --session work tabs list --all
mosaic --session work prompt send --pane-id 1 --text "..." --submit enter
mosaic --session work prompt send --pane-id 1 --queue --text "..."
mosaic --session work queue list --pane-id 1 --redact
mosaic --session work observe pane --pane-id 1 --last-lines 80 --redact
mosaic --session work subscribe --pane-id 1 --scrollback 40
mosaic audit list --redact
```

The backend should treat `schema_version: "mosaic.control.v1"` as the stable
control envelope and tolerate unknown JSON fields. It should not call internal
Zellij IPC types directly.

## Target Selection

Before delivery, the backend should verify:

- the selected session exists
- the selected pane exists in `panes list --all`
- the pane is not exited or held
- `mosaic_agent.kind`, `status`, `composer_state`, and `confidence` support the
  intended delivery mode
- the user or controller policy allows prompt bodies to be sent to that pane

If the pane is unknown, busy, low-confidence, or not clearly an agent composer,
the backend should observe or queue instead of forcing delivery.

## Delivery Semantics

Immediate delivery:

```sh
mosaic --session work prompt send --pane-id 1 --text "$PROMPT" --submit enter
```

Steered/no-submit delivery:

```sh
mosaic --session work prompt send --pane-id 1 --text "$PROMPT" --no-submit
```

Queued delivery:

```sh
mosaic --session work prompt send --pane-id 1 --queue --text "$PROMPT"
```

The backend should store Mosaic receipt IDs in its own task records. An
accepted immediate receipt means the server accepted the write, paste, or key
action. It does not mean the agent consumed or completed the prompt. Use
`observe pane`, `capture`, or `subscribe` for follow-up evidence.

For dry-run validation:

```sh
mosaic --session work --dry-run prompt send --pane-id 1 --text "$PROMPT" --submit enter
```

## Recent Output And Streaming

For snapshots:

```sh
mosaic --session work observe pane --pane-id 1 --last-lines 80 --redact
```

For ongoing monitoring:

```sh
mosaic --session work subscribe --pane-id 1 --scrollback 40
```

NDJSON subscription events are the machine-readable stream surface. Raw stream
mode is suitable for human terminal views but may contain terminal-controlled
text and should not be parsed as trusted structured data.

## Configuration

A portable dispatch configuration can model Mosaic as an optional backend:

```json
{
  "backend": "tmux",
  "optional_backends": {
    "mosaic": {
      "command": "mosaic",
      "default_submit": "enter",
      "observe_last_lines": 80
    }
  }
}
```

The exact dispatch config shape belongs to dispatch. The key requirements are:
tmux remains the default/current backend, Mosaic is opt-in, and all Mosaic
operations go through the documented Mosaic control surface.

## Tests For A Dispatch Backend

A Mosaic backend should add tests for:

- session and pane discovery from representative `mosaic` JSON
- prompt send, no-submit, dry-run, and queued delivery receipts
- refusal or queueing for unrecognized, exited, held, or low-confidence panes
- recent output capture through `observe pane`
- NDJSON subscription parsing
- redaction behavior for prompt bodies and observed output
- backend selection that preserves tmux as the default

The Open Mosaic smoke script is useful for an end-to-end local target:

```sh
cargo build --bin mosaic --bin zellij --no-default-features --features vendored_curl
scripts/mosaic-workflow-smoke.sh
```

## OSS Portability

The optional backend must work on a normal developer machine with only Open
Mosaic installed. Organization-specific identity, task, machine, or transport
systems should be represented as optional adapters, not as backend
requirements.
