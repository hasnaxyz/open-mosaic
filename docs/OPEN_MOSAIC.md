# Open Mosaic Product Contract

Open Mosaic is an OSS-first agentic terminal workspace forked from Zellij. It
preserves Zellij's MIT license and attribution while adding agent-native
control APIs, structured observation, prompt receipts, audit records, and
portable adapter points.

Core requirements:

- The product must work on a normal machine without Hasna private services.
- Hasna/open-* integrations must be optional adapters or plugins.
- The public CLI/API must expose stable JSON or NDJSON where agents consume it.
- Agent metadata must be best-effort, portable, and based on generic pane
  fields unless an optional adapter is explicitly configured.
- Prompt delivery must support dry-run, no-submit, queued, and immediate modes.
- Immediate delivery receipts mean the Mosaic server accepted the action; they
  do not claim the terminal process has read or completed the prompt.
- Audit records must include operation, target session/pane, status, timestamp,
  and receipt ID, with redaction hooks for prompt bodies.
- Upstream sync must keep Zellij attribution and MIT notices intact.

The first native surface is the `mosaic` binary, documented in
`docs/MOSAIC_CLI.md`. Existing Zellij-derived crate and runtime names are
implementation compatibility unless the relevant document says otherwise.
