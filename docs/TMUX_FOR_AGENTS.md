# Open Mosaic And tmux For Agents

tmux is a mature terminal multiplexer. Open Mosaic is a Zellij-derived terminal
workspace with additional agent-facing control APIs. The goal is not to make
tmux obsolete; it is to provide a native OSS control surface where agents need
structured session discovery, prompt receipts, queues, observation, metadata,
audit records, and portable adapters.

tmux remains a good default when a workflow only needs a stable terminal
multiplexer. Open Mosaic becomes useful when automation has to reason about
agent panes and produce machine-readable evidence of what it attempted.

## Agent Control Surface

| Capability | tmux | Open Mosaic |
| --- | --- | --- |
| Session discovery | `list-sessions` text formats | `mosaic sessions list` JSON |
| Pane discovery | `list-panes` format strings | `panes list --all` JSON with agent metadata |
| Prompt delivery | key/paste commands | `prompt send` receipts, submit modes, dry-run |
| Queued delivery | external scripts | native per-pane queue records |
| Recent output | `capture-pane` | `capture` plus `observe pane` activity summary |
| Streaming output | control mode or pipe-pane | `subscribe` NDJSON or raw text |
| Agent detection | external conventions | built-in best-effort `mosaic_agent` metadata |
| Audit trail | external logging | native audit records with redaction hooks |
| Dashboard | external tooling | `mosaic dashboard` JSON or compact text |
| Extensibility | shell scripts and plugins | portable adapter manifests |

## Why Receipts Matter

Terminal automation often needs to distinguish these facts:

- the controller intended to send a prompt
- the terminal server accepted the action
- the target process displayed or consumed the bytes
- the agent completed the requested work

Open Mosaic receipts only prove the second fact for immediate delivery:

```json
{
  "schema_version": "mosaic.control.v1",
  "event": "receipt",
  "operation": "prompt.send",
  "status": "accepted",
  "ack": "server_accepted"
}
```

Controllers should pair receipts with `observe pane`, `capture`, or `subscribe`
before deciding whether to send follow-up work. This is especially important for
agent panes whose composer state is unknown or whose metadata confidence is low.

## Safer Prompt Delivery

Open Mosaic exposes explicit delivery modes:

```sh
mosaic --session work --dry-run prompt send --pane-id 1 --text "status?"
mosaic --session work prompt send --pane-id 1 --text "status?" --no-submit
mosaic --session work prompt send --pane-id 1 --text "status?" --submit enter
mosaic --session work prompt send --pane-id 1 --queue --text "next task"
```

Use `--dry-run` to validate targeting, `--no-submit` when the prompt should
land in a composer without execution, and `--queue` when another controller or
human should decide when to deliver. Do not force prompts into unknown, busy, or
low-confidence panes solely because they appear in the pane list.

## Observation And Redaction

For read-only monitoring:

```sh
mosaic --session work observe pane --pane-id 1 --last-lines 80 --redact
mosaic --session work capture --pane-id 1 --scrollback
mosaic --session work subscribe --pane-id 1 --scrollback 40
mosaic --session work dashboard --live --redact
```

`observe pane` returns structured counts and a last-line summary. With
`--redact`, returned terminal lines and summaries are replaced with redaction
markers. The audit log records observation activity counts but does not persist
raw terminal output.

## Remote And Multi-Machine Workflows

tmux often handles remote work by running inside SSH. Open Mosaic supports the
same simple pattern today: install `mosaic` and the compatibility `zellij`
binary on the target machine, then run commands over SSH or inside the remote
shell.

Future multi-machine integrations should use the generic adapter kinds and
capabilities in `docs/MOSAIC_ADAPTERS.md`, such as `transport.ssh` and
`machine.remote`. They should remain optional so local Open Mosaic sessions work
without private registries or organization-specific services.

## When To Keep tmux

Keep tmux as the primary backend when a project depends on established tmux
scripts, when no agent metadata or receipt trail is needed, or when the
deployment target already has tmux but cannot install Open Mosaic. A controller
can support both backends: tmux for existing environments and Mosaic where
native JSON control, queues, receipts, and observation are available.
