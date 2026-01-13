#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run the cohsh .coh regression pack in a single QEMU boot (no kill/clean).

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

SCRIPTS=(
    "boot_v0.coh"
    "9p_batch.coh"
    "host_absent.coh"
    "telemetry_ring.coh"
    "observe_watch.coh"
    "cas_roundtrip.coh"
    "tcp_basic.coh"
)

LOG_ROOT="out/cohesix/logs"
ARCHIVE_ROOT="out/regression-logs"
READY_MARKER="Cohesix console ready"
READY_TIMEOUT="${READY_TIMEOUT:-180}"
PORT_TIMEOUT="${PORT_TIMEOUT:-30}"
QUIT_CLOSE_TIMEOUT="${QUIT_CLOSE_TIMEOUT:-30}"

check_port_open() {
    local host="$1"
    local port="$2"
    python3 - "$host" "$port" <<'PY'
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
}

wait_port_ready() {
    local host="$1"
    local port="$2"
    local timeout="$3"
    local pid="$4"
    local deadline=$((SECONDS + timeout))
    while (( SECONDS < deadline )); do
        if ! kill -0 "$pid" 2>/dev/null; then
            return 1
        fi
        if check_port_open "$host" "$port"; then
            return 0
        fi
        sleep 0.2
    done
    return 2
}

log_has() {
    local file="$1"
    local pattern="$2"
    python3 - "$file" "$pattern" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
pattern = sys.argv[2]
if not path.exists():
    sys.exit(1)
data = path.read_bytes()
try:
    text = data.decode(errors="ignore")
except Exception:
    sys.exit(1)
sys.exit(0 if pattern in text else 1)
PY
}

wait_log_marker() {
    local file="$1"
    local pattern="$2"
    local timeout="$3"
    local pid="$4"
    local deadline=$((SECONDS + timeout))
    while (( SECONDS < deadline )); do
        if ! kill -0 "$pid" 2>/dev/null; then
            return 1
        fi
        if log_has "$file" "$pattern"; then
            return 0
        fi
        sleep 0.2
    done
    return 2
}

count_log_pattern() {
    local file="$1"
    local pattern="$2"
    python3 - "$file" "$pattern" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
pattern = sys.argv[2].encode()
if not path.exists():
    print(0)
    sys.exit(0)
data = path.read_bytes()
print(data.count(pattern))
PY
}

wait_log_count_increase() {
    local file="$1"
    local pattern="$2"
    local start_count="$3"
    local timeout="$4"
    local deadline=$((SECONDS + timeout))
    while (( SECONDS < deadline )); do
        local current
        current=$(count_log_pattern "$file" "$pattern")
        if (( current > start_count )); then
            return 0
        fi
        sleep 0.2
    done
    return 1
}

run_cohsh() {
    local script="$1"
    case "$script" in
        boot_v0.coh)
            ./out/cohesix/host-tools/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/boot_v0.coh
            ;;
        9p_batch.coh)
            ./out/cohesix/host-tools/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/9p_batch.coh
            ;;
        host_absent.coh)
            ./out/cohesix/host-tools/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/host_absent.coh
            ;;
        telemetry_ring.coh)
            ./out/cohesix/host-tools/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/telemetry_ring.coh
            ;;
        observe_watch.coh)
            ./out/cohesix/host-tools/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/observe_watch.coh
            ;;
        cas_roundtrip.coh)
            ./out/cohesix/host-tools/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/cas_roundtrip.coh
            ;;
        tcp_basic.coh)
            ./out/cohesix/host-tools/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/tcp_basic.coh
            ;;
        *)
            echo "Unknown script: $script" >&2
            return 2
            ;;
    esac
}

if check_port_open 127.0.0.1 31337; then
    echo "Port 31337 already in use; stop the running QEMU TCP console and retry." >&2
    exit 1
fi

rm -rf target out/cohesix
mkdir -p "${LOG_ROOT}" "${ARCHIVE_ROOT}"

qemu_log="${LOG_ROOT}/regression_batch.qemu.log"

cargo run -p coh-rtc -- \
    "$PROJECT_ROOT/configs/root_task.toml" \
    --out "$PROJECT_ROOT/apps/root-task/src/generated" \
    --manifest "$PROJECT_ROOT/out/manifests/root_task_resolved.json" \
    --cli-script "$PROJECT_ROOT/scripts/cohsh/boot_v0.coh" \
    --doc-snippet "$PROJECT_ROOT/docs/snippets/root_task_manifest.md"

SEL4_BUILD_DIR=$HOME/seL4/build ./scripts/cohesix-build-run.sh \
    --sel4-build "$HOME/seL4/build" \
    --out-dir out/cohesix \
    --profile release \
    --root-task-features cohesix-dev \
    --cargo-target aarch64-unknown-none \
    --raw-qemu \
    --transport tcp \
    > "$qemu_log" 2>&1 &
qemu_pid=$!

cleanup() {
    if kill -0 "$qemu_pid" 2>/dev/null; then
        kill "$qemu_pid" || true
    fi
}
trap cleanup EXIT

if ! wait_log_marker "$qemu_log" "$READY_MARKER" "$READY_TIMEOUT" "$qemu_pid"; then
    echo "FAIL: console ready marker not seen" >&2
    tail -n 50 "$qemu_log" >&2 || true
    exit 1
fi

if ! wait_port_ready 127.0.0.1 31337 "$PORT_TIMEOUT" "$qemu_pid"; then
    echo "FAIL: TCP console not ready" >&2
    tail -n 50 "$qemu_log" >&2 || true
    exit 1
fi

for script in "${SCRIPTS[@]}"; do
    name="${script%.coh}"
    echo "=== Running ${script} ==="

    close_count_before=$(count_log_pattern "$qemu_log" "audit tcp.conn.close")
    coh_log="${LOG_ROOT}/${name}.out.log"

    if ! run_cohsh "$script" > "$coh_log" 2>&1; then
        echo "FAIL: cohsh script ${script}" >&2
        cp "$qemu_log" "${ARCHIVE_ROOT}/${name}.qemu.log" || true
        cp "$coh_log" "${ARCHIVE_ROOT}/${name}.out.log" || true
        exit 1
    fi

    if ! wait_log_count_increase "$qemu_log" "audit tcp.conn.close" "$close_count_before" "$QUIT_CLOSE_TIMEOUT"; then
        echo "FAIL: connection did not close after ${script} within ${QUIT_CLOSE_TIMEOUT}s" >&2
        cp "$qemu_log" "${ARCHIVE_ROOT}/${name}.qemu.log" || true
        cp "$coh_log" "${ARCHIVE_ROOT}/${name}.out.log" || true
        exit 1
    fi

    cp "$qemu_log" "${ARCHIVE_ROOT}/${name}.qemu.log"
    cp "$coh_log" "${ARCHIVE_ROOT}/${name}.out.log"
    echo "PASS: ${script}"
done

echo "regression batch complete: ${#SCRIPTS[@]} scripts passed"
