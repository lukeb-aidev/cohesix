#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Verify UART prompt stability under log spam and confirm /log/queen.log content.

set -euo pipefail

if ! command -v python3 >/dev/null 2>&1; then
    echo "[prompt-stability] ERROR: python3 is required" >&2
    exit 1
fi

PROJECT_ROOT="$(git rev-parse --show-toplevel)"
SEL4_BUILD_DIR="${SEL4_BUILD_DIR:-$HOME/seL4/build}"
OUT_DIR="${COHESIX_OUT_DIR:-$PROJECT_ROOT/out/cohesix}"
TCP_PORT="${COHSH_TCP_PORT:-31337}"
TIMEOUT_SECONDS="${COHESIX_QEMU_TIMEOUT:-60}"
COHSH_BIN="${COHSH_BIN:-$OUT_DIR/host-tools/cohsh}"
LOG_PATH="${COHESIX_PROMPT_LOG:-$OUT_DIR/qemu.prompt.log}"
COHSH_LOG="${COHESIX_PROMPT_COHSH_LOG:-$OUT_DIR/cohsh.prompt.log}"

if [[ ! -d "$SEL4_BUILD_DIR" ]]; then
    echo "[prompt-stability] ERROR: seL4 build directory not found: $SEL4_BUILD_DIR" >&2
    exit 2
fi

mkdir -p "$OUT_DIR"
: > "$LOG_PATH"
: > "$COHSH_LOG"

QEMU_PID=""
COHSH_SCRIPT=""

cleanup() {
    if [[ -n "$COHSH_SCRIPT" && -f "$COHSH_SCRIPT" ]]; then
        rm -f "$COHSH_SCRIPT"
    fi
    if [[ -n "$QEMU_PID" ]]; then
        if kill -0 "$QEMU_PID" >/dev/null 2>&1; then
            kill "$QEMU_PID" >/dev/null 2>&1 || true
            wait "$QEMU_PID" >/dev/null 2>&1 || true
        fi
    fi
}
trap cleanup EXIT

BUILD_CMD=("$PROJECT_ROOT/scripts/cohesix-build-run.sh"
    --sel4-build "$SEL4_BUILD_DIR"
    --out-dir "$OUT_DIR"
    --profile release
    --root-task-features cohesix-dev
    --cargo-target aarch64-unknown-none
    --raw-qemu
    --transport tcp
    --tcp-port "$TCP_PORT")

echo "[prompt-stability] Launching QEMU (raw) and capturing UART output..."
"${BUILD_CMD[@]}" >"$LOG_PATH" 2>&1 &
QEMU_PID=$!

wait_for_prompt() {
    local deadline=$((SECONDS + TIMEOUT_SECONDS))
    while (( SECONDS < deadline )); do
        if ! kill -0 "$QEMU_PID" >/dev/null 2>&1; then
            echo "[prompt-stability] ERROR: QEMU exited before prompt" >&2
            return 1
        fi
        if grep -q "cohesix>" "$LOG_PATH"; then
            return 0
        fi
        sleep 1
    done
    echo "[prompt-stability] ERROR: timed out waiting for prompt" >&2
    return 1
}

wait_for_port() {
    local host="127.0.0.1"
    local port="$1"
    local deadline=$((SECONDS + TIMEOUT_SECONDS))
    while (( SECONDS < deadline )); do
        if ! kill -0 "$QEMU_PID" >/dev/null 2>&1; then
            echo "[prompt-stability] ERROR: QEMU exited before TCP console was ready" >&2
            return 1
        fi
        if python3 - "$host" "$port" <<'PY'
import socket
import sys

host = sys.argv[1]
port = int(sys.argv[2])
try:
    with socket.create_connection((host, port), timeout=0.5):
        sys.exit(0)
except OSError:
    sys.exit(1)
PY
        then
            return 0
        fi
        sleep 1
    done
    echo "[prompt-stability] ERROR: timed out waiting for TCP console" >&2
    return 1
}

wait_for_prompt
wait_for_port "$TCP_PORT"

if [[ ! -x "$COHSH_BIN" ]]; then
    echo "[prompt-stability] ERROR: cohsh binary not found: $COHSH_BIN" >&2
    exit 3
fi

COHSH_SCRIPT="$(mktemp)"
cat >"$COHSH_SCRIPT" <<'COMMANDS'
attach queen
ping
netstats
netstats
    tail /log/queen.log
quit
COMMANDS

echo "[prompt-stability] Driving TCP console burst + log tail..."
if ! "$COHSH_BIN" --transport tcp --tcp-port "$TCP_PORT" --script "$COHSH_SCRIPT" >"$COHSH_LOG" 2>&1; then
    echo "[prompt-stability] ERROR: cohsh script failed" >&2
    cat "$COHSH_LOG" >&2
    exit 4
fi

python3 - "$COHSH_LOG" <<'PY'
import sys

log_path = sys.argv[1]
data = open(log_path, encoding="utf-8", errors="ignore").read()
needle = "log.channel=LOGFILE path=/log/queen.log"
if needle not in data:
    print("[prompt-stability] ERROR: log channel handoff line missing from cohsh output", file=sys.stderr)
    sys.exit(1)
PY

python3 - "$LOG_PATH" <<'PY'
import re
import sys

log_path = sys.argv[1]
pattern = re.compile(r'^\[(INFO|WARN|ERROR|DEBUG|TRACE)')
lines = open(log_path, encoding="utf-8", errors="ignore").read().splitlines()
prompt_line = None
for idx, line in enumerate(lines, 1):
    if "cohesix>" in line:
        prompt_line = idx
if prompt_line is None:
    print("[prompt-stability] ERROR: serial prompt not found", file=sys.stderr)
    sys.exit(1)
for idx in range(prompt_line, len(lines) + 1):
    if pattern.search(lines[idx - 1]):
        print(
            f"[prompt-stability] ERROR: log line after prompt at line {idx}: {lines[idx - 1]}",
            file=sys.stderr,
        )
        sys.exit(1)
print("[prompt-stability] OK: prompt stable and UART log noise suppressed")
PY

echo "[prompt-stability] PASS: UART prompt stable and /log/queen.log populated"
