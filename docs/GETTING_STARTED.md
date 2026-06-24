# Getting Started With Open Mosaic

Open Mosaic currently ships two local binaries:

- `mosaic`: agent-facing control CLI with JSON output.
- `zellij`: compatibility terminal workspace binary inherited from Zellij.

The compatibility binary is still required to start and attach terminal
sessions. The `mosaic` CLI controls those sessions through Mosaic JSON envelopes.

## Build Locally

Build the CLI only:

```sh
cargo build --release --bin mosaic --no-default-features --features vendored_curl
install -Dm755 target/release/mosaic "$HOME/.local/bin/mosaic"
```

Build the full local workspace:

```sh
cargo build --release --bin mosaic --bin zellij
install -Dm755 target/release/mosaic "$HOME/.local/bin/mosaic"
install -Dm755 target/release/zellij "$HOME/.local/bin/zellij"
```

If `zellij` is not on `PATH`, point Mosaic at it:

```sh
export MOSAIC_ZELLIJ_BIN="$HOME/.local/bin/zellij"
```

## First Session

```sh
mosaic sessions create demo
mosaic sessions list
mosaic --session demo panes list
```

Attach interactively with the compatibility binary:

```sh
zellij attach demo
```

## Agent Workflow Smoke

Find a pane ID:

```sh
mosaic --session demo panes list
```

Send a prompt with a delivery receipt:

```sh
mosaic --session demo prompt send --pane-id 1 --text "pwd" --submit enter
```

Queue a prompt without delivering it:

```sh
mosaic --session demo prompt send --pane-id 1 --text "next task" --queue
mosaic --session demo queue list
```

Observe recent output:

```sh
mosaic --session demo capture --pane-id 1 --scrollback
mosaic --session demo observe pane --pane-id 1 --last-lines 40 --redact
```

Inspect audit records:

```sh
mosaic audit list --redact
```

Render a compact local dashboard:

```sh
mosaic dashboard --format text
mosaic --session demo dashboard --live --redact
```

The dashboard reads local queues and audit records without private services.
Queued prompt bodies are redacted by default; use `--show-prompts` only when
the caller is allowed to inspect queued prompt content.

Create a read-only web oversight link for the same session:

```sh
zellij web --create-read-only-token observer
mosaic web link --session demo --mode watch --token-name observer
```

The generated link contains no raw token. See `docs/MOSAIC_WEB.md` for the
watch/control distinction and web-server security notes.

To run the same flow against a disposable local session:

```sh
cargo build --bin mosaic --bin zellij --no-default-features --features vendored_curl
scripts/mosaic-workflow-smoke.sh
```

The smoke script creates a disposable `mosaic-smoke-*` background session,
refuses to reuse an existing session name, and closes only the session it
created.

If you have a supported agent CLI installed (`codewith`, `codex`, `claude`, or
`opencode`), you can also run the guarded real-agent smoke:

```sh
scripts/mosaic-agent-workflow-smoke.sh
```

By default this launches a real agent pane, verifies Mosaic classified it as an
agent, writes a marker prompt without submitting it, queues a follow-up,
captures recent output, streams pane updates, checks audit/queue redaction, and
then closes the disposable `mosaic-agent-smoke-*` session. It does not start a
model turn unless `MOSAIC_AGENT_SMOKE_SUBMIT=enter` is set.

## State

Mosaic stores local queue, observation, and audit records under:

```sh
"${XDG_STATE_HOME:-$HOME/.local/state}/open-mosaic"
```

State is local to the user account. Do not store private service credentials in
Mosaic core state; integrations should use documented adapter boundaries.
