# Open Mosaic Product Contract

Open Mosaic is an OSS-first agentic terminal workspace forked from Zellij. It
preserves Zellij's MIT license and attribution while adding agent-native
control APIs, structured observation, prompt receipts, audit records, and
portable goal/task context and adapter points.

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
- Goal/task context must use portable schemas and optional adapters; private
  task systems must not be required for core Mosaic control commands.
- Web oversight must distinguish read-only watcher links from control links
  and must not place raw authentication tokens in URLs.
- Upstream sync must keep Zellij attribution and MIT notices intact.
- Maintainers must keep the `upstream` remote pull-only and run the upstream
  hygiene check before upstream-sync and release PRs.

The first native surface is the `mosaic` binary, documented in
`docs/MOSAIC_CLI.md`. Getting started commands are in
`docs/GETTING_STARTED.md`. Portable adapter manifests are documented in
`docs/MOSAIC_ADAPTERS.md`. Migration notes from Zellij and tmux are in
`docs/MIGRATION.md`, with an agent-focused tmux comparison in
`docs/TMUX_FOR_AGENTS.md`. Optional dispatch backend guidance is in
`docs/DISPATCH_INTEGRATION.md`. Optional machine and transport registries are
documented in `docs/MOSAIC_MACHINES.md`. Portable goal/task registries and the
optional external todos adapter are documented in `docs/MOSAIC_GOALS.md`.
Bookmarkable web oversight links are documented in `docs/MOSAIC_WEB.md`.
Upstream sync and license hygiene are documented in
`docs/UPSTREAM_MAINTENANCE.md`.
Existing Zellij-derived crate and runtime names are implementation
compatibility unless the relevant document says otherwise.
