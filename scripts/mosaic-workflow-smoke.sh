#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MOSAIC_BIN="${MOSAIC_BIN:-$ROOT_DIR/target/debug/mosaic}"
ZELLIJ_BIN="${ZELLIJ_BIN:-$ROOT_DIR/target/debug/zellij}"
PYTHON_BIN="${PYTHON_BIN:-python3}"
SESSION="${MOSAIC_SMOKE_SESSION:-mosaic-smoke-$$}"
SESSION_CREATED=0

if [[ "$SESSION" != mosaic-smoke-* && "${MOSAIC_SMOKE_ALLOW_DANGEROUS_SESSION_NAME:-0}" != "1" ]]; then
  echo "refusing smoke session name outside mosaic-smoke-* prefix: $SESSION" >&2
  echo "set MOSAIC_SMOKE_ALLOW_DANGEROUS_SESSION_NAME=1 to override the name check" >&2
  exit 2
fi

if [[ -n "${MOSAIC_SMOKE_STATE_HOME:-}" ]]; then
  STATE_HOME="$MOSAIC_SMOKE_STATE_HOME"
  REMOVE_STATE=0
else
  STATE_HOME="$(mktemp -d)"
  REMOVE_STATE=1
fi
WORK_DIR="$(mktemp -d)"
STREAM_PID=""

cleanup() {
  if [[ -n "$STREAM_PID" ]] && kill -0 "$STREAM_PID" >/dev/null 2>&1; then
    kill "$STREAM_PID" >/dev/null 2>&1 || true
    wait "$STREAM_PID" >/dev/null 2>&1 || true
  fi
  if [[ "$SESSION_CREATED" == "1" ]]; then
    "$MOSAIC_BIN" sessions close "$SESSION" --delete >/dev/null 2>&1 || true
  fi
  rm -rf "$WORK_DIR"
  if [[ "$REMOVE_STATE" == "1" && "${MOSAIC_SMOKE_KEEP_STATE:-0}" != "1" ]]; then
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

export XDG_STATE_HOME="$STATE_HOME"
export MOSAIC_ZELLIJ_BIN="$ZELLIJ_BIN"

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
"$MOSAIC_BIN" --session "$SESSION" pane create --name mosaic-smoke -- bash -lc 'printf "mosaic-smoke-start\n"; while IFS= read -r line; do printf "mosaic-smoke-received:%s\n" "$line"; done' >"$WORK_DIR/pane-create.json"

PANE_JSON="$WORK_DIR/panes.json"
PANE_ID=""
for _ in {1..50}; do
  if "$MOSAIC_BIN" --session "$SESSION" panes list --all >"$PANE_JSON" 2>"$WORK_DIR/panes.err"; then
    PANE_ID="$("$PYTHON_BIN" - "$PANE_JSON" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    envelope = json.load(handle)
for pane in envelope.get("data", []):
    if not pane.get("is_plugin"):
        print(f"terminal_{pane['id']}")
        raise SystemExit(0)
raise SystemExit(1)
PY
)" && break
  fi
  sleep 0.2
done

if [[ -z "$PANE_ID" ]]; then
  echo "failed to find a terminal pane in $SESSION" >&2
  cat "$WORK_DIR/panes.err" >&2 || true
  exit 1
fi

wait_for_observation() {
  local needle="$1"
  local output="$2"
  for _ in {1..50}; do
    "$MOSAIC_BIN" --session "$SESSION" observe pane --pane-id "$PANE_ID" --last-lines 40 >"$output"
    if "$PYTHON_BIN" - "$output" "$needle" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    envelope = json.load(handle)
needle = sys.argv[2]
lines = "\n".join(envelope.get("lines", []))
raise SystemExit(0 if needle in lines else 1)
PY
    then
      return 0
    fi
    sleep 0.2
  done
  echo "timed out waiting for pane output: $needle" >&2
  cat "$output" >&2 || true
  return 1
}

wait_for_observation "mosaic-smoke-start" "$WORK_DIR/observe-start.json"

"$MOSAIC_BIN" --session "$SESSION" prompt send --pane-id "$PANE_ID" --text "mosaic-smoke-prompt" --submit enter >"$WORK_DIR/prompt-receipt.json"
"$PYTHON_BIN" - "$WORK_DIR/prompt-receipt.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    receipt = json.load(handle)
assert receipt["operation"] == "prompt.send", receipt
assert receipt["status"] == "accepted", receipt
assert receipt["ack"] == "server_accepted", receipt
PY
wait_for_observation "mosaic-smoke-received:mosaic-smoke-prompt" "$WORK_DIR/observe-prompt.json"

"$MOSAIC_BIN" --session "$SESSION" prompt send --pane-id "$PANE_ID" --queue --text "mosaic-smoke-queued-secret" >"$WORK_DIR/queue-receipt.json"
"$PYTHON_BIN" - "$WORK_DIR/queue-receipt.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    receipt = json.load(handle)
assert receipt["operation"] == "prompt.queue", receipt
assert receipt["status"] == "queued", receipt
PY

"$MOSAIC_BIN" --session "$SESSION" capture --pane-id "$PANE_ID" --scrollback >"$WORK_DIR/capture.txt"
grep -q "mosaic-smoke-received:mosaic-smoke-prompt" "$WORK_DIR/capture.txt"

"$MOSAIC_BIN" --session "$SESSION" subscribe --pane-id "$PANE_ID" --scrollback 10 --format ndjson >"$WORK_DIR/stream.ndjson" &
STREAM_PID="$!"
sleep 0.5
"$MOSAIC_BIN" --session "$SESSION" prompt send --pane-id "$PANE_ID" --text "mosaic-smoke-stream" --submit enter >"$WORK_DIR/stream-receipt.json"
for _ in {1..50}; do
  if grep -q "mosaic-smoke-stream" "$WORK_DIR/stream.ndjson"; then
    break
  fi
  sleep 0.2
done
kill "$STREAM_PID" >/dev/null 2>&1 || true
wait "$STREAM_PID" >/dev/null 2>&1 || true
STREAM_PID=""
"$PYTHON_BIN" - "$WORK_DIR/stream.ndjson" <<'PY'
import json
import sys

updates = 0
with open(sys.argv[1], encoding="utf-8") as handle:
    for line in handle:
        if not line.strip():
            continue
        event = json.loads(line)
        if event.get("event") == "pane_update":
            updates += 1
assert updates > 0, "no pane_update events in stream"
PY
grep -q "mosaic-smoke-stream" "$WORK_DIR/stream.ndjson"

"$MOSAIC_BIN" --session "$SESSION" dashboard --live --redact >"$WORK_DIR/dashboard.json"
"$PYTHON_BIN" - "$WORK_DIR/dashboard.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    dashboard = json.load(handle)
assert dashboard["event"] == "dashboard.snapshot", dashboard
assert dashboard["live"]["status"] == "captured", dashboard
assert dashboard["queues"]["total_pending"] >= 1, dashboard
serialized = json.dumps(dashboard)
assert "mosaic-smoke-queued-secret" not in serialized, dashboard
assert dashboard["queues"]["prompt_bodies"] == "redacted", dashboard
PY

"$MOSAIC_BIN" sessions close "$SESSION" --delete >"$WORK_DIR/session-close.json"
SESSION_CREATED=0
echo "mosaic workflow smoke passed: session=$SESSION pane=$PANE_ID state=$STATE_HOME"
