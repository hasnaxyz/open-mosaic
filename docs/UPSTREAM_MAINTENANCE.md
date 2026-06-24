# Open Mosaic Upstream Maintenance

Open Mosaic is a derivative of Zellij. Upstream sync work should make it easy
to accept Zellij fixes while keeping Open Mosaic-specific agent surfaces clear,
portable, and accurately attributed.

Architecture boundaries and fork-base decisions are recorded in
`docs/ZELLIJ_ARCHITECTURE_AUDIT.md`.

## Ground Rules

- Keep `upstream` as a fetch-only remote for `zellij-org/zellij`.
- Do not push Open Mosaic branches, tags, releases, or package artifacts to
  upstream Zellij repositories.
- Preserve `LICENSE.md`, `NOTICE.md`, and upstream copyright notices.
- Keep fork-specific user-facing behavior documented as Open Mosaic behavior,
  not as upstream Zellij behavior.
- Keep Hasna or open-* integrations optional. The product core must keep
  working without private services, private registries, or organization-owned
  machines.

Recommended remote setup for maintainers:

```sh
git remote add upstream https://github.com/zellij-org/zellij.git
git remote set-url --push upstream DISABLED
git remote -v
```

`origin` can point at `hasnaxyz/open-mosaic` or a maintainer's fork. The
important invariant is that `upstream` is pull-only.

## Before A Sync

Run the hygiene guardrail before and after sync work:

```sh
scripts/check-upstream-hygiene.sh
```

The check is maintainer-oriented and expects the Open Mosaic upstream remote
shape shown above. Contributors working from personal forks can still run it
after adding the same pull-only `upstream` remote locally.

Fetch upstream without mutating local branches:

```sh
git fetch upstream --tags --prune
git log --oneline --decorate main..upstream/main
```

Inspect upstream changes before choosing a merge strategy:

```sh
git diff --stat main..upstream/main
git diff --name-only main..upstream/main
```

## Sync Strategy

Use a dedicated branch:

```sh
git switch main
git pull --ff-only origin main
git switch -c maintenance/sync-zellij-YYYY-MM-DD
```

For normal maintenance, prefer merging upstream into the sync branch so the
fork history remains explicit:

```sh
git merge upstream/main
```

Rebase only for local unpublished cleanup branches. Do not rewrite published
Open Mosaic history.

## Conflict Hotspots

Review these files and areas carefully during every upstream sync:

- `LICENSE.md`
- `NOTICE.md`
- `Cargo.toml`
- `.github/workflows/release.yml`
- `src/bin/mosaic.rs`
- `src/bin/mosaic/*`
- `docs/OPEN_MOSAIC.md`
- `docs/GETTING_STARTED.md`
- `docs/MOSAIC_*.md`
- `docs/UPSTREAM_MAINTENANCE.md`
- `docs/ZELLIJ_ARCHITECTURE_AUDIT.md`
- `scripts/check-upstream-hygiene.sh`

If upstream changes licensing, authorship, release metadata, web sharing, IPC,
plugin loading, session management, or CLI action semantics, reconcile those
changes manually instead of accepting conflict markers mechanically.

## Attribution Checklist

Before opening a sync PR, verify:

- `LICENSE.md` still contains the upstream MIT license and Zellij copyright.
- `NOTICE.md` still states Open Mosaic is derived from Zellij.
- Fork-specific features are described as Open Mosaic additions.
- Release artifacts are named Open Mosaic artifacts and are published only from
  Open Mosaic repositories.
- Compatibility names such as `zellij`, `zellij-*` crates, socket names, config
  paths, and plugin URLs remain documented as compatibility layers when they
  appear in user-facing docs.

## Validation Checklist

Run the smallest useful validation first, then broaden when the sync touches
runtime behavior:

```sh
scripts/check-upstream-hygiene.sh
cargo fmt --check
cargo check --bin mosaic --no-default-features --features vendored_curl
cargo test --bin mosaic --no-default-features --features vendored_curl
cargo test --test mosaic_cli --no-default-features --features vendored_curl
cargo package --list
```

For runtime, IPC, session, plugin, or terminal behavior changes, also run:

```sh
cargo build --bin mosaic --bin zellij --no-default-features --features vendored_curl
cargo xtask integration-test
scripts/mosaic-workflow-smoke.sh
```

Open a PR against Open Mosaic after validation. If a sync requires changes that
would be useful upstream, send them to Zellij separately as upstream-focused
patches without Open Mosaic branding or fork-only assumptions.
