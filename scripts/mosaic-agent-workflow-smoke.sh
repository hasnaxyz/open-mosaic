#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/mosaic-agent-workflow-smoke.sh

Launch an installed terminal agent CLI inside a disposable Mosaic session and
exercise the real agent-facing control path:

  - detect the pane as codewith/codex/claude_code/opencode metadata
  - dry-run a prompt receipt
  - write a no-submit prompt by default
  - queue a follow-up prompt
  - observe, capture, subscribe, inspect audit/queue redaction
  - close the disposable session

The script does not submit the prompt by default. Set
MOSAIC_AGENT_SMOKE_SUBMIT=enter to send Enter after the prompt.

Environment:
  MOSAIC_BIN                      Path to mosaic binary
  ZELLIJ_BIN                      Path to zellij compatibility binary
  MOSAIC_AGENT_SMOKE_BIN          Agent executable to launch
  MOSAIC_AGENT_SMOKE_KIND         Expected mosaic_agent.kind
  MOSAIC_AGENT_SMOKE_ARGS         Extra agent args, split on whitespace
  MOSAIC_AGENT_SMOKE_SESSION      Disposable session name
  MOSAIC_AGENT_SMOKE_STATE_HOME   State dir to reuse instead of a temp dir
  MOSAIC_AGENT_SMOKE_SUBMIT       none (default), enter, or tab
  MOSAIC_AGENT_SMOKE_RAW_WRITE    Use raw write instead of paste, default 1
  MOSAIC_AGENT_SMOKE_KEEP_STATE   Keep temp XDG_STATE_HOME when set to 1

For complex custom commands, provide a small wrapper executable through
MOSAIC_AGENT_SMOKE_BIN.
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MOSAIC_BIN="${MOSAIC_BIN:-$ROOT_DIR/target/debug/mosaic}"
ZELLIJ_BIN="${ZELLIJ_BIN:-$ROOT_DIR/target/debug/zellij}"
PYTHON_BIN="${PYTHON_BIN:-python3}"
SESSION="${MOSAIC_AGENT_SMOKE_SESSION:-mosaic-agent-smoke-$$}"
PROMPT_MARKER="MOSAIC_AGENT_SMOKE_PROMPT_$$"
QUEUE_MARKER="MOSAIC_AGENT_SMOKE_QUEUE_$$"
STREAM_MARKER="MOSAIC_AGENT_SMOKE_STREAM_$$"
SESSION_CREATED=0
STREAM_PID=""

if [[ "$SESSION" != mosaic-agent-smoke-* && "${MOSAIC_AGENT_SMOKE_ALLOW_DANGEROUS_SESSION_NAME:-0}" != "1" ]]; then
  echo "refusing smoke session name outside mosaic-agent-smoke-* prefix: $SESSION" >&2
  echo "set MOSAIC_AGENT_SMOKE_ALLOW_DANGEROUS_SESSION_NAME=1 to override the name check" >&2
  exit 2
fi

if [[ -n "${MOSAIC_AGENT_SMOKE_STATE_HOME:-}" ]]; then
  STATE_HOME="$MOSAIC_AGENT_SMOKE_STATE_HOME"
  REMOVE_STATE=0
else
  STATE_HOME="$(mktemp -d)"
  REMOVE_STATE=1
fi
WORK_DIR="$(mktemp -d)"

cleanup() {
  if [[ -n "$STREAM_PID" ]] && kill -0 "$STREAM_PID" >/dev/null 2>&1; then
    kill "$STREAM_PID" >/dev/null 2>&1 || true
    wait "$STREAM_PID" >/dev/null 2>&1 || true
  fi
  if [[ "$SESSION_CREATED" == "1" ]]; then
    "$MOSAIC_BIN" sessions close "$SESSION" --delete >/dev/null 2>&1 || true
  fi
  rm -rf "$WORK_DIR"
  if [[ "$REMOVE_STATE" == "1" && "${MOSAIC_AGENT_SMOKE_KEEP_STATE:-0}" != "1" ]]; then
    rm -rf "$STATE_HOME"
  fi
}
trap cleanup EXIT

if [[ ! -x "$MOSAIC_BIN" ]]; then
  echo "missing mosaic binary: $MOSAIC_BIN" >&2
  echo "run: cargo build --bin mosaic --bin zellij --no-default-features --features vendored_curl" >&2
  exit 2
fi
if [[ ! -x "$ZELLIJ_BIN" ]]; then
  echo "missing zellij compatibility binary: $ZELLIJ_BIN" >&2
  echo "run: cargo build --bin mosaic --bin zellij --no-default-features --features vendored_curl" >&2
  exit 2
fi

agent_bin="${MOSAIC_AGENT_SMOKE_BIN:-}"
expected_kind="${MOSAIC_AGENT_SMOKE_KIND:-}"
agent_args=()

if [[ -z "$agent_bin" ]]; then
  if command -v codewith >/dev/null 2>&1; then
    agent_bin="$(command -v codewith)"
    expected_kind="${expected_kind:-codewith}"
    agent_args=(--no-alt-screen -C "$ROOT_DIR")
  elif command -v codex >/dev/null 2>&1; then
    agent_bin="$(command -v codex)"
    expected_kind="${expected_kind:-codex}"
    agent_args=(--no-alt-screen -C "$ROOT_DIR")
  elif command -v claude >/dev/null 2>&1; then
    agent_bin="$(command -v claude)"
    expected_kind="${expected_kind:-claude_code}"
    agent_args=(--ax-screen-reader)
  elif command -v opencode >/dev/null 2>&1; then
    agent_bin="$(command -v opencode)"
    expected_kind="${expected_kind:-opencode}"
  else
    echo "no supported agent CLI found; set MOSAIC_AGENT_SMOKE_BIN and MOSAIC_AGENT_SMOKE_KIND" >&2
    exit 2
  fi
else
  if [[ ! -x "$agent_bin" ]]; then
    echo "agent executable is not executable: $agent_bin" >&2
    exit 2
  fi
  if [[ -z "$expected_kind" ]]; then
    echo "MOSAIC_AGENT_SMOKE_KIND is required when MOSAIC_AGENT_SMOKE_BIN is set" >&2
    exit 2
  fi
fi

if [[ -n "${MOSAIC_AGENT_SMOKE_ARGS:-}" ]]; then
  read -r -a extra_agent_args <<<"$MOSAIC_AGENT_SMOKE_ARGS"
  agent_args+=("${extra_agent_args[@]}")
fi

submit_mode="${MOSAIC_AGENT_SMOKE_SUBMIT:-none}"
case "$submit_mode" in
  none)
    submit_args=(--no-submit)
    ;;
  enter)
    submit_args=(--submit enter)
    ;;
  tab)
    submit_args=(--submit tab)
    ;;
  *)
    echo "invalid MOSAIC_AGENT_SMOKE_SUBMIT: $submit_mode (expected none, enter, or tab)" >&2
    exit 2
    ;;
esac

delivery_args=()
if [[ "${MOSAIC_AGENT_SMOKE_RAW_WRITE:-1}" != "0" ]]; then
  delivery_args=(--raw-write)
fi

export XDG_STATE_HOME="$STATE_HOME"
export MOSAIC_ZELLIJ_BIN="$ZELLIJ_BIN"
export MOSAIC_AUDIT_REDACT=1

"$MOSAIC_BIN" sessions list >"$WORK_DIR/sessions-before.json"
if "$PYTHON_BIN" - "$WORK_DIR/sessions-before.json" "$SESSION" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    envelope = json.load(handle)
target = sys.argv[2]
raise SystemExit(0 if any(session.get("name") == target for session in envelope.get("sessions", [])) else 1)
PY
then
  echo "refusing to reuse existing Mosaic/Zellij session: $SESSION" >&2
  exit 2
fi

"$MOSAIC_BIN" sessions create "$SESSION" --background >"$WORK_DIR/session-create.json"
SESSION_CREATED=1
"$MOSAIC_BIN" --session "$SESSION" pane create --name "$expected_kind-smoke" -- "$agent_bin" "${agent_args[@]}" >"$WORK_DIR/pane-create.json"

PANE_JSON="$WORK_DIR/panes.json"
PANE_ID=""
for _ in {1..80}; do
  if "$MOSAIC_BIN" --session "$SESSION" panes list --all >"$PANE_JSON" 2>"$WORK_DIR/panes.err"; then
    if PANE_ID="$("$PYTHON_BIN" - "$PANE_JSON" "$expected_kind" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    envelope = json.load(handle)
expected = sys.argv[2]
for pane in envelope.get("data", []):
    agent = pane.get("mosaic_agent") or {}
    if (
        agent.get("kind") == expected
        and agent.get("status") == "running"
        and float(agent.get("confidence") or 0) >= 0.75
        and agent.get("composer_state") != "working"
    ):
        print(f"terminal_{pane['id']}")
        raise SystemExit(0)
raise SystemExit(1)
PY
)"; then
      break
    fi
  fi
  sleep 0.25
done

if [[ -z "$PANE_ID" ]]; then
  echo "failed to find a safe running $expected_kind agent pane in $SESSION" >&2
  cat "$PANE_JSON" >&2 || true
  cat "$WORK_DIR/panes.err" >&2 || true
  exit 1
fi

wait_for_agent_screen() {
  local output="$1"
  for _ in {1..80}; do
    "$MOSAIC_BIN" --session "$SESSION" observe pane --pane-id "$PANE_ID" --last-lines 80 >"$output"
    if "$PYTHON_BIN" - "$output" "$expected_kind" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    envelope = json.load(handle)
expected = sys.argv[2].replace("_", " ")
lines = "\n".join(envelope.get("lines", []))
activity = envelope.get("activity") or {}
non_empty = int(activity.get("non_empty_line_count") or 0)
has_prompt_surface = "›" in lines or ">" in lines or "model:" in lines.lower()
has_agent_name = expected in lines.lower() or expected.replace("code", "") in lines.lower()
raise SystemExit(0 if non_empty >= 3 and (has_prompt_surface or has_agent_name) else 1)
PY
    then
      sleep "${MOSAIC_AGENT_SMOKE_READY_SETTLE:-1}"
      return 0
    fi
    sleep 0.25
  done
  echo "timed out waiting for $expected_kind agent screen to settle" >&2
  cat "$output" >&2 || true
  return 1
}

wait_for_observation() {
  local needle="$1"
  local output="$2"
  for _ in {1..60}; do
    "$MOSAIC_BIN" --session "$SESSION" observe pane --pane-id "$PANE_ID" --last-lines 80 >"$output"
    if "$PYTHON_BIN" - "$output" "$needle" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    envelope = json.load(handle)
needle = sys.argv[2]
lines = "\n".join(envelope.get("lines", []))
compact = "".join(line.strip() for line in envelope.get("lines", []))
raise SystemExit(0 if needle in lines or needle in compact else 1)
PY
    then
      return 0
    fi
    sleep 0.25
  done
  echo "timed out waiting for pane output: $needle" >&2
  cat "$output" >&2 || true
  return 1
}

wait_for_agent_screen "$WORK_DIR/observe-ready.json"

"$MOSAIC_BIN" --session "$SESSION" --dry-run prompt send --pane-id "$PANE_ID" --text "$PROMPT_MARKER dry-run" --no-submit "${delivery_args[@]}" >"$WORK_DIR/dry-run-receipt.json"
"$PYTHON_BIN" - "$WORK_DIR/dry-run-receipt.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    receipt = json.load(handle)
assert receipt["operation"] == "prompt.send", receipt
assert receipt["status"] == "dry_run", receipt
assert receipt["ack"] == "none", receipt
PY

"$MOSAIC_BIN" --session "$SESSION" prompt send --pane-id "$PANE_ID" --text "$PROMPT_MARKER no-submit" "${submit_args[@]}" "${delivery_args[@]}" >"$WORK_DIR/prompt-receipt.json"
"$PYTHON_BIN" - "$WORK_DIR/prompt-receipt.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    receipt = json.load(handle)
assert receipt["operation"] == "prompt.send", receipt
assert receipt["status"] == "accepted", receipt
assert receipt["ack"] == "server_accepted", receipt
PY

if [[ "$submit_mode" == "none" ]]; then
  wait_for_observation "$PROMPT_MARKER" "$WORK_DIR/observe-prompt.json"
fi

"$MOSAIC_BIN" --session "$SESSION" prompt send --pane-id "$PANE_ID" --queue --text "$QUEUE_MARKER queued-secret" >"$WORK_DIR/queue-receipt.json"
"$PYTHON_BIN" - "$WORK_DIR/queue-receipt.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    receipt = json.load(handle)
assert receipt["operation"] == "prompt.queue", receipt
assert receipt["status"] == "queued", receipt
PY

"$MOSAIC_BIN" --session "$SESSION" queue list --pane-id "$PANE_ID" --redact >"$WORK_DIR/queue-list.json"
"$PYTHON_BIN" - "$WORK_DIR/queue-list.json" "$QUEUE_MARKER" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    envelope = json.load(handle)
serialized = json.dumps(envelope)
assert envelope["event"] == "queue.list", envelope
assert sys.argv[2] not in serialized, envelope
assert "[redacted]" in serialized, envelope
PY

"$MOSAIC_BIN" --session "$SESSION" capture --pane-id "$PANE_ID" --scrollback >"$WORK_DIR/capture.txt"
if [[ "$submit_mode" == "none" ]]; then
  "$PYTHON_BIN" - "$WORK_DIR/capture.txt" "$PROMPT_MARKER" <<'PY'
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    text = handle.read()
compact = "".join(line.strip() for line in text.splitlines())
assert sys.argv[2] in text or sys.argv[2] in compact, "capture marker not observed"
PY
fi

"$MOSAIC_BIN" --session "$SESSION" subscribe --pane-id "$PANE_ID" --scrollback 10 --format ndjson >"$WORK_DIR/stream.ndjson" &
STREAM_PID="$!"
sleep 0.75
"$MOSAIC_BIN" --session "$SESSION" prompt send --pane-id "$PANE_ID" --text "$STREAM_MARKER" --no-submit "${delivery_args[@]}" >"$WORK_DIR/stream-receipt.json"
for _ in {1..60}; do
  if "$PYTHON_BIN" - "$WORK_DIR/stream.ndjson" "$STREAM_MARKER" <<'PY'
import json
import sys

try:
    with open(sys.argv[1], encoding="utf-8") as handle:
        raw = handle.read()
except FileNotFoundError:
    raise SystemExit(1)
if not raw.strip():
    raise SystemExit(1)
needle = sys.argv[2]
def strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, list):
        for item in value:
            yield from strings(item)
    elif isinstance(value, dict):
        for item in value.values():
            yield from strings(item)

for line in raw.splitlines():
    if not line.strip():
        continue
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        continue
    compact = "".join(part.strip() for part in strings(event))
    if needle in compact:
        raise SystemExit(0)
raise SystemExit(1)
PY
  then
    break
  fi
  sleep 0.25
done
kill "$STREAM_PID" >/dev/null 2>&1 || true
wait "$STREAM_PID" >/dev/null 2>&1 || true
STREAM_PID=""
"$PYTHON_BIN" - "$WORK_DIR/stream.ndjson" "$STREAM_MARKER" <<'PY'
import json
import sys

updates = 0
marker_seen = False
def strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, list):
        for item in value:
            yield from strings(item)
    elif isinstance(value, dict):
        for item in value.values():
            yield from strings(item)

with open(sys.argv[1], encoding="utf-8") as handle:
    for line in handle:
        if not line.strip():
            continue
        event = json.loads(line)
        if event.get("event") == "pane_update":
            updates += 1
            compact = "".join(part.strip() for part in strings(event))
            marker_seen = marker_seen or sys.argv[2] in compact
assert updates > 0, "no pane_update events in stream"
assert marker_seen, "stream marker not observed"
PY

"$MOSAIC_BIN" audit list --limit 20 --redact >"$WORK_DIR/audit.json"
"$PYTHON_BIN" - "$WORK_DIR/audit.json" "$QUEUE_MARKER" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    envelope = json.load(handle)
serialized = json.dumps(envelope)
assert envelope["event"] == "audit.list", envelope
assert "prompt.send" in serialized, envelope
assert "prompt.queue" in serialized, envelope
assert sys.argv[2] not in serialized, envelope
PY

"$MOSAIC_BIN" sessions close "$SESSION" --delete >"$WORK_DIR/session-close.json"
SESSION_CREATED=0
echo "mosaic real agent workflow smoke passed: session=$SESSION pane=$PANE_ID agent=$expected_kind submit=$submit_mode state=$STATE_HOME"
