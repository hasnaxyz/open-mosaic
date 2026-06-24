# Open Mosaic Release Guide

Open Mosaic releases are built from this fork and should be published as
Open Mosaic artifacts. The project is derived from Zellij, so every release
must preserve `LICENSE.md`, `NOTICE.md`, and upstream attribution.

Do not publish Open Mosaic artifacts to upstream Zellij repositories, package
registries, or release pages.

## Artifact Names

The GitHub release workflow publishes Open Mosaic archives:

```text
open-mosaic-<target>.tar.gz
open-mosaic-<target>.zip
open-mosaic-<target>.sha256sum
open-mosaic-no-web-<target>.tar.gz
open-mosaic-no-web-<target>.zip
open-mosaic-no-web-<target>.sha256sum
```

Windows installer artifacts use:

```text
open-mosaic-<target>-installer.msi
open-mosaic-<target>-installer.sha256sum
open-mosaic-no-web-<target>-installer.msi
open-mosaic-no-web-<target>-installer.sha256sum
```

Archives include both:

- `mosaic`: native Open Mosaic control CLI.
- `zellij`: compatibility workspace binary inherited from Zellij.

The compatibility binary remains part of the distribution until the interactive
runtime has a separate Mosaic entrypoint.

## Local Release Build

Build and stage a local Open Mosaic distribution:

```sh
cargo xtask dist
```

This creates:

```text
target/dist/open-mosaic/
target/dist/open-mosaic-<host>.tar.gz
target/dist/open-mosaic-<host>.tar.gz.sha256sum
```

The staged directory and archive include `bin/mosaic`, `bin/zellij`,
`LICENSE.md`, `NOTICE.md`, user-facing docs, schemas, and smoke scripts.

For a direct binary-only build:

```sh
cargo build --release --bin mosaic --bin zellij
```

Install for a local smoke test:

```sh
install -Dm755 target/release/mosaic "$HOME/.local/bin/mosaic"
install -Dm755 target/release/zellij "$HOME/.local/bin/zellij"
mosaic --version
zellij --version
```

Run a basic control-plane smoke:

```sh
mosaic --dry-run --session no-such-session prompt send --pane-id 1 --text "hello"
mosaic adapters list
```

## GitHub Release

The release workflow runs on tags matching `v*.*.*` and on manual dispatch.
Manual dispatch creates a draft release for `main`.

Before tagging:

```sh
cargo fmt --check
cargo check --bin mosaic --no-default-features --features vendored_curl
cargo test --bin mosaic --no-default-features --features vendored_curl
cargo test --test mosaic_cli --no-default-features --features vendored_curl
cargo package --list
scripts/check-upstream-hygiene.sh
```

When a maintainer has a supported local agent CLI installed, also run the
guarded real-agent smoke before a release candidate:

```sh
scripts/mosaic-agent-workflow-smoke.sh
```

It is intentionally not a CI requirement because it depends on an operator's
local agent installation and auth state. By default it validates no-submit
prompt delivery and does not submit a model turn.

After the workflow publishes assets, verify:

- Asset names start with `open-mosaic-`.
- Normal and no-web archives contain `mosaic` and `zellij`.
- Source/package manifests include `schemas/mosaic.control.v1.schema.json`.
- `.sha256sum` files hash the uploaded archive or installer artifact.
- Windows MSI identifies itself as Open Mosaic and installs into an Open Mosaic
  folder, not an upstream Zellij folder.

## Upstream Maintenance

Keep the `upstream` remote pull-only. Pull from
`https://github.com/zellij-org/zellij.git` when syncing, but do not push Open
Mosaic branches or tags upstream.

When accepting upstream changes, preserve Open Mosaic-specific files and review
conflicts around:

- `src/bin/mosaic.rs`
- `src/bin/mosaic/*`
- `docs/MOSAIC_*.md`
- `docs/OPEN_MOSAIC.md`
- `docs/GETTING_STARTED.md`
- `docs/ZELLIJ_ARCHITECTURE_AUDIT.md`
- `.github/workflows/release.yml`
- `NOTICE.md`

If upstream Zellij changes licensing or attribution files, reconcile them
manually and keep notices intact.

The detailed sync workflow and attribution checklist live in
`docs/UPSTREAM_MAINTENANCE.md`. Run `scripts/check-upstream-hygiene.sh` before
opening upstream-sync or release PRs.
