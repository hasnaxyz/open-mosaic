# Open Mosaic Migration Notes

Open Mosaic is a derivative of Zellij. It keeps the Zellij terminal workspace
foundation and adds a native `mosaic` control CLI for agent workflows. Migration
should be incremental: keep existing Zellij or tmux workflows working, then add
Mosaic JSON control where automation benefits from receipts, queues,
observation, metadata, and audit records.

Open Mosaic does not require private services. Optional integrations should use
the adapter boundaries documented in `docs/MOSAIC_ADAPTERS.md`.

## From Zellij

The interactive terminal workspace is still started by the Zellij-derived
compatibility binary:

```sh
cargo build --release --bin mosaic --bin zellij
install -Dm755 target/release/mosaic "$HOME/.local/bin/mosaic"
install -Dm755 target/release/zellij "$HOME/.local/bin/zellij"
```

If `zellij` is not on `PATH`, point the Mosaic CLI at it:

```sh
export MOSAIC_ZELLIJ_BIN="$HOME/.local/bin/zellij"
```

Existing Zellij layouts, configs, plugins, and session behavior remain the
compatibility baseline unless a Mosaic document describes a new surface. Mosaic
does not rename every internal crate, socket, asset, or compatibility path
because that would make upstream sync harder and risk breaking stable Zellij
behavior.

Use Mosaic beside the compatibility binary:

```sh
mosaic sessions create work --background
mosaic sessions list
mosaic --session work panes list --all
zellij attach work
```

Agent-facing additions are available through JSON commands:

```sh
mosaic --session work prompt send --pane-id 1 --text "cargo test" --submit enter
mosaic --session work prompt send --pane-id 1 --queue --text "next task"
mosaic --session work queue list --redact
mosaic --session work observe pane --pane-id 1 --last-lines 40 --redact
mosaic --session work dashboard --live --redact
mosaic audit list --redact
```

Important compatibility rules:

- `status: "accepted"` means the Mosaic/Zellij server accepted the action. It
  does not prove that the terminal process consumed the prompt or completed it.
- `mosaic_agent` metadata is best-effort. Low confidence, unknown, exited, or
  held panes should be treated as observation targets until a human or adapter
  confirms they are safe prompt targets.
- Queue and audit state is local observer state under
  `${XDG_STATE_HOME:-$HOME/.local/state}/open-mosaic`.
- `--redact` and `MOSAIC_AUDIT_REDACT=1` protect command output and local queue
  or audit views, but they do not retroactively remove text already written to a
  terminal pane by another process.

Rollback is simple: stop using `mosaic` commands and continue using the
compatibility `zellij` binary or upstream Zellij. Open Mosaic preserves upstream
Zellij attribution and MIT license notices; fork-specific behavior should not be
reported upstream as if it were upstream Zellij behavior.

## From tmux

tmux and Open Mosaic use similar terminal-workspace concepts, but the names and
automation contracts differ.

| tmux concept | Open Mosaic concept | Notes |
| --- | --- | --- |
| server | Zellij-derived server | Managed by the compatibility runtime. |
| session | session | Discover with `mosaic sessions list`. |
| window | tab | Discover with `mosaic --session work tabs list --all`. |
| pane | pane | Discover with `mosaic --session work panes list --all`. |
| `send-keys` | `prompt send` | Emits a receipt and supports submit modes. |
| paste buffer | prompt text/file input | Multi-line prompt delivery uses paste/write paths. |
| `capture-pane` | `capture` or `observe pane` | `observe pane` adds structured activity metadata. |
| control mode | `subscribe` NDJSON | NDJSON is the machine-readable stream surface. |

Typical tmux automation:

```sh
tmux new-session -d -s work
tmux send-keys -t work:0.0 'cargo test' Enter
tmux capture-pane -pt work:0.0 -S -100
```

Equivalent Mosaic automation:

```sh
mosaic sessions create work --background
mosaic --session work panes list --all
mosaic --session work prompt send --pane-id 1 --text "cargo test" --submit enter
mosaic --session work observe pane --pane-id 1 --last-lines 100
```

For agent workflows, prefer `prompt send` over shell-specific key injection:

```sh
mosaic --session work prompt send --pane-id 1 --file prompt.txt --no-submit
mosaic --session work prompt send --pane-id 1 --queue --text "follow-up"
mosaic --session work queue list --pane-id 1 --redact
```

Use `subscribe` when a controller needs ongoing output:

```sh
mosaic --session work subscribe --pane-id 1 --scrollback 40
mosaic --session work subscribe --pane-id 1 --format raw
```

NDJSON mode is the stable automation surface. Raw mode is for humans and may
contain terminal-controlled text.

## Migration Checklist

1. Build or install both `mosaic` and the compatibility `zellij` binary.
2. Start one disposable session and validate basic discovery:

   ```sh
   mosaic sessions create mosaic-migration-demo --background
   mosaic --session mosaic-migration-demo panes list --all
   mosaic --session mosaic-migration-demo dashboard --live --redact
   mosaic sessions close mosaic-migration-demo
   ```

3. Convert read-only automation first: session list, pane list, capture,
   observation, dashboard, and audit views.
4. Convert prompt delivery next, and check receipts plus observed pane output
   before chaining follow-up prompts.
5. Use queued prompts for work that should wait for a controller, human, or
   adapter decision.
6. Keep private credentials and organization-specific state outside Mosaic core;
   connect them through optional adapters when needed.

The reusable workflow smoke test exercises a disposable local session:

```sh
cargo build --bin mosaic --bin zellij --no-default-features --features vendored_curl
scripts/mosaic-workflow-smoke.sh
```
