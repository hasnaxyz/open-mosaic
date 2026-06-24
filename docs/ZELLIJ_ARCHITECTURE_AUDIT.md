# Zellij Architecture Audit For Open Mosaic

Open Mosaic is forked from Zellij and keeps Zellij as the upstream architecture
base. This document records the fork boundary so maintainers can add
agent-native behavior without losing upstream mergeability or attribution.

## Fork Base Decision

Open Mosaic currently keeps the Zellij workspace shape and crate names, with
the root package still named `zellij` for compatibility with upstream build,
test, plugin, and packaging assumptions. The fork adds a first-class `mosaic`
binary and Open Mosaic docs, schemas, scripts, and packaging metadata.

This is intentional:

- Keeping upstream crate/module names reduces upstream sync conflicts.
- Keeping the `zellij` binary preserves compatibility for existing Zellij users
  and scripts while Open Mosaic grows the native `mosaic` control surface.
- Open Mosaic-specific behavior must be documented as Open Mosaic behavior and
  must not imply that it is available in upstream Zellij.
- `upstream` remains a pull-only remote for `zellij-org/zellij`; release and
  package publishing target Open Mosaic repositories only.

## Upstream Architecture Map

### Process Entrypoints

- `src/main.rs` is the upstream-derived Zellij entrypoint and remains the
  compatibility binary surface.
- `src/commands.rs` owns broad command parsing and command dispatch for the
  Zellij-compatible binary.
- `src/bin/mosaic.rs` is the Open Mosaic-native control binary. Its helper
  modules live under `src/bin/mosaic/` and are the preferred place for
  agent-facing JSON control additions.

### CLI And Control Dispatch

Zellij's existing CLI control path is action-oriented:

- `zellij-utils/src/cli.rs` defines `CliAction` variants and CLI-facing action
  data.
- `zellij-client/src/cli_client.rs` connects a CLI invocation to a running
  session and sends the requested action over IPC.
- `zellij-utils/src/ipc.rs` and `zellij-utils/src/client_server_contract/*.proto`
  define the client/server message contract.
- `zellij-server/src/route.rs`, `zellij-server/src/screen.rs`, and
  `zellij-server/src/tab/mod.rs` route actions into session, tab, and pane
  behavior.

Open Mosaic should use this path for operations that already exist in Zellij,
such as session, pane, tab, dump-screen, and key/paste actions. It should add
Open Mosaic-only JSON envelopes, local queues, audit records, adapter metadata,
and safety checks in `src/bin/mosaic.rs` unless the server must expose a new
primitive.

### Terminal, Pane, And Session Runtime

The upstream runtime centers on the server:

- `zellij-server/src/lib.rs` coordinates server state, connected clients, and
  watcher/read-only client handling.
- `zellij-server/src/route.rs` receives IPC instructions and queues or forwards
  them to screen/tab handlers.
- `zellij-server/src/screen.rs` coordinates tabs, focus, client state, and
  screen-level actions.
- `zellij-server/src/tab/mod.rs` owns pane layout, focus, floating panes,
  terminal/plugin panes, rendering state, search, and input routing.
- `zellij-server/src/pty_writer.rs` writes bytes to terminal PTYs and handles
  queued write behavior.

Open Mosaic prompt delivery must treat server acceptance as distinct from agent
consumption. Receipts from `mosaic prompt send` prove acceptance or local queue
mutation, not that an agent has read or completed the prompt. Controllers
should pair receipts with `observe`, `capture`, or `subscribe` output.

### Plugin System

Zellij plugins remain an upstream subsystem:

- `zellij-tile/` exposes the plugin SDK surface.
- `zellij-utils/src/plugin_api/` defines plugin API types and protobufs.
- `default-plugins/` contains the bundled status bar, tab bar, session manager,
  share, layout manager, mobile, and related plugins.

Open Mosaic can add an agent workspace plugin, but it should use the existing
plugin SDK and permission model. Product-core agent control must not require a
private plugin registry or private service. Optional Hasna/open-* integrations
belong behind adapter/plugin boundaries documented in `docs/MOSAIC_ADAPTERS.md`.

### Web And Remote Oversight

Zellij's web client and remote attach code is mostly upstream-derived:

- `zellij-client/src/web_client/` contains HTTP handlers, websocket handlers,
  connection/session management, authentication, IPC listener code, and web
  control message types.
- `zellij-client/src/remote_attach/` contains remote attach configuration,
  authentication, HTTP, and websocket client helpers.

Open Mosaic's web oversight additions should preserve secure defaults:
bookmarkable links should distinguish watch from control, raw tokens should not
be embedded in URLs, and read-only watcher semantics should remain clear.
Open Mosaic web helper behavior is documented in `docs/MOSAIC_WEB.md`.

### Build, Test, And Release

The upstream-derived build tooling is still authoritative for broad validation:

- `xtask/src/` implements CI, build, distribution, integration-test, format,
  and metadata helpers.
- `.github/workflows/rust.yml` runs formatting, build, unit, integration, and
  platform test jobs.
- `.github/workflows/e2e.yml` builds a generic binary and runs end-to-end tests.
- `.github/workflows/release.yml` builds release artifacts.
- `Cargo.toml` controls package metadata and Debian asset inclusion.

Open Mosaic-specific docs, schemas, and scripts must be included in
`Cargo.toml` package include and relevant package asset metadata when they are
needed by users or maintainers.

## Open Mosaic-Owned Surfaces

These surfaces are intentionally fork-owned and should stay portable:

- `src/bin/mosaic.rs` and `src/bin/mosaic/*`
- `schemas/mosaic.control.v1.schema.json`
- `docs/MOSAIC_*.md`, `docs/OPEN_MOSAIC.md`,
  `docs/ZELLIJ_ARCHITECTURE_AUDIT.md`, and
  `docs/UPSTREAM_MAINTENANCE.md`
- `scripts/check-upstream-hygiene.sh`
- `scripts/mosaic-workflow-smoke.sh`
- `scripts/mosaic-agent-workflow-smoke.sh`
- local Open Mosaic state under `$XDG_STATE_HOME/open-mosaic` or
  `~/.local/state/open-mosaic`

These surfaces must not hardcode private paths, private services, private
machine names, or Hasna-only assumptions. Optional integrations are acceptable
only when the generic path works without them.

## When To Touch Upstream-Derived Core

Prefer adding Open Mosaic functionality in the `mosaic` binary, schemas, docs,
scripts, or optional adapters. Touch upstream-derived core only when the native
control surface needs a primitive that cannot be expressed through existing
Zellij CLI actions or IPC.

Examples that can usually stay outside core:

- JSON envelopes and stable schemas
- agent pane detection based on existing pane list output
- local prompt queues, audit logs, and redaction
- machine, goals, task, identity, and transport adapter descriptors
- dashboard aggregation of existing session, queue, audit, and goal state

Examples that may require core changes:

- new server-side terminal observation data not available through dump-screen
- new watcher/read-only client permissions
- plugin API additions
- terminal input semantics that must be handled at PTY write time
- session persistence changes

Core changes require broader validation because they can affect non-agent
terminal users and upstream sync conflict risk.

## Validation Expectations

For docs/schema/control-only slices, run at least:

```sh
python3 -m json.tool schemas/mosaic.control.v1.schema.json >/dev/null
cargo fmt --check
cargo check --bin mosaic --no-default-features --features vendored_curl
cargo test --bin mosaic --no-default-features --features vendored_curl
cargo test --test mosaic_cli --no-default-features --features vendored_curl
scripts/check-upstream-hygiene.sh
git diff --check
```

For runtime, IPC, plugin, web, terminal, or release changes, add the relevant
broader checks:

```sh
cargo build --bin mosaic --bin zellij --no-default-features --features vendored_curl
cargo xtask integration-test
scripts/mosaic-workflow-smoke.sh
scripts/mosaic-agent-workflow-smoke.sh
```

PR CI must pass the Rust matrix and generic binary E2E before merge.

## Upstream Sync Watchpoints

During every upstream sync, inspect these areas before resolving conflicts:

- command/action definitions in `zellij-utils/src/cli.rs`
- IPC protobufs and conversions under `zellij-utils/src/client_server_contract/`
  and `zellij-utils/src/ipc/`
- client CLI dispatch in `zellij-client/src/cli_client.rs`
- server routing, screen, tab, and PTY writer behavior under `zellij-server/src/`
- web client authentication, watcher, and websocket behavior under
  `zellij-client/src/web_client/`
- plugin SDK and bundled plugin changes under `zellij-tile/`,
  `zellij-utils/src/plugin_api/`, and `default-plugins/`
- release workflows and package metadata
- Open Mosaic-owned `mosaic` binary, schema, docs, and smoke scripts

If upstream accepts a change that supersedes a Mosaic fork patch, prefer the
upstream implementation and keep only the Open Mosaic JSON/control wrapper
needed by agents.

## Known Remaining Risks

- Open Mosaic still carries Zellij-derived crate and runtime names for
  compatibility, which can confuse users if docs do not explain the fork
  boundary.
- The native `mosaic` binary currently wraps many existing Zellij CLI actions;
  future server primitives should be added carefully and tested across terminal
  edge cases.
- Web oversight depends on upstream web-client security behavior. Token,
  watcher, and control changes require dedicated review.
- A richer agent dashboard/plugin should stay optional and should not become a
  hidden dependency for CLI automation.
