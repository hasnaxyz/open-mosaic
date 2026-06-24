# Open Mosaic

Open Mosaic is an OSS-first agentic terminal workspace forked from
[Zellij](https://github.com/zellij-org/zellij). It keeps Zellij's terminal
multiplexer foundation and adds native Mosaic control APIs for agent workflows:
structured session/pane discovery, prompt delivery receipts, prompt queues,
agent metadata, pane observation, audit records, portable goal/task context,
and portable adapter manifests.

Open Mosaic is intended to work on normal developer machines without private
Hasna infrastructure. Optional Hasna or open-* integrations must live behind
adapters or plugins.

## Status

This repository is a derivative work in active development. The current public
surface is the `mosaic` CLI plus the Zellij-compatible `zellij` terminal
workspace binary. Internal crate, module, plugin, socket, and compatibility
binary names still use Zellij-derived names where that keeps upstream sync and
existing behavior stable.

## Install From Source

For the agent-facing CLI:

```sh
cargo build --release --bin mosaic --no-default-features --features vendored_curl
install -Dm755 target/release/mosaic "$HOME/.local/bin/mosaic"
```

For the full local workspace, build both binaries:

```sh
cargo build --release --bin mosaic --bin zellij
install -Dm755 target/release/mosaic "$HOME/.local/bin/mosaic"
install -Dm755 target/release/zellij "$HOME/.local/bin/zellij"
```

The `mosaic sessions create` command currently launches the compatibility
`zellij` binary. If it is not on `PATH`, set `MOSAIC_ZELLIJ_BIN=/path/to/zellij`.

See [docs/GETTING_STARTED.md](docs/GETTING_STARTED.md) for a first workflow.

## Native Mosaic CLI

The `mosaic` CLI emits versioned JSON for automation:

```sh
mosaic sessions list
mosaic sessions create demo
mosaic --session demo panes list
mosaic --session demo prompt send --pane-id 1 --text "pwd" --submit enter
mosaic --session demo observe pane --pane-id 1 --last-lines 40
mosaic --session demo queue list
mosaic --session demo dashboard --live --redact
mosaic web link --session demo --mode watch
mosaic goals list --redact
mosaic audit list --redact
mosaic adapters list
```

Reference docs:

- [Mosaic CLI](docs/MOSAIC_CLI.md)
- [Mosaic JSON schemas](docs/MOSAIC_SCHEMAS.md)
- [Adapter manifests](docs/MOSAIC_ADAPTERS.md)
- [Machines and transports](docs/MOSAIC_MACHINES.md)
- [Goals and tasks](docs/MOSAIC_GOALS.md)
- [Web oversight links](docs/MOSAIC_WEB.md)
- [Migration notes from Zellij and tmux](docs/MIGRATION.md)
- [Open Mosaic and tmux for agents](docs/TMUX_FOR_AGENTS.md)
- [Optional dispatch integration](docs/DISPATCH_INTEGRATION.md)
- [Open Mosaic product contract](docs/OPEN_MOSAIC.md)
- [Upstream maintenance](docs/UPSTREAM_MAINTENANCE.md)
- [Zellij architecture audit](docs/ZELLIJ_ARCHITECTURE_AUDIT.md)

## How Open Mosaic Differs From Zellij

Zellij is the upstream terminal workspace foundation. Open Mosaic preserves that
foundation while adding agent-native control and observability surfaces. These
Mosaic additions are fork-specific unless they are separately accepted upstream
by Zellij.

The fork does not claim to be upstream Zellij, and it preserves Zellij's MIT
license notices and attribution. See [NOTICE.md](NOTICE.md) and
[LICENSE.md](LICENSE.md).

Migration guidance from upstream Zellij and tmux is in
[docs/MIGRATION.md](docs/MIGRATION.md). A focused comparison for agent
controllers is in [docs/TMUX_FOR_AGENTS.md](docs/TMUX_FOR_AGENTS.md).

## Development

```sh
cargo xtask build
cargo xtask test
cargo test --test mosaic_cli --no-default-features --features vendored_curl
```

For the CLI-only slice during development:

```sh
cargo check --bin mosaic --no-default-features --features vendored_curl
cargo test --bin mosaic --no-default-features --features vendored_curl
cargo test --test mosaic_cli --no-default-features --features vendored_curl
cargo build --bin mosaic --bin zellij --no-default-features --features vendored_curl
scripts/mosaic-workflow-smoke.sh
```

## Upstream

The upstream project is [zellij-org/zellij](https://github.com/zellij-org/zellij).
Keep upstream attribution and MIT notices intact when syncing or modifying the
fork. Do not push to the upstream remote from this repository.
The maintainer workflow is documented in
[docs/UPSTREAM_MAINTENANCE.md](docs/UPSTREAM_MAINTENANCE.md), with fork
architecture boundaries in
[docs/ZELLIJ_ARCHITECTURE_AUDIT.md](docs/ZELLIJ_ARCHITECTURE_AUDIT.md). Run
`scripts/check-upstream-hygiene.sh` before release or upstream-sync PRs.

## License

MIT. Open Mosaic is derived from Zellij and preserves the upstream license and
copyright notices.
