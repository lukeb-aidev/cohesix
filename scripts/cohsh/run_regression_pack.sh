#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run the cohsh .coh regression pack with kill/clean/rebuild between scripts.

set -euo pipefail

SCRIPTS=(
    "boot_v0.coh"
    "9p_batch.coh"
    "telemetry_ring.coh"
    "observe_watch.coh"
    "cas_roundtrip.coh"
    "tcp_basic.coh"
)

LOG_ROOT="out/cohesix/logs"
ARCHIVE_ROOT="out/regression-logs"

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

wait_port_free() {
    local host="$1"
    local port="$2"
    local timeout="$3"
    local deadline=$((SECONDS + timeout))
    while (( SECONDS < deadline )); do
        if ! check_port_open "$host" "$port"; then
            return 0
        fi
        sleep 0.2
    done
    return 1
}

log_has() {
    local file="$1"
    local pattern="$2"
    if [[ ! -f "$file" ]]; then
        return 1
    fi
    if command -v rg >/dev/null 2>&1; then
        rg -q "$pattern" "$file"
    else
        grep -q "$pattern" "$file"
    fi
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

run_cohsh() {
    local script="$1"
    case "$script" in
        boot_v0.coh)
            ./out/cohesix/host-tools/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/boot_v0.coh
            ;;
        9p_batch.coh)
            ./out/cohesix/host-tools/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/9p_batch.coh
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

mkdir -p "${ARCHIVE_ROOT}"

for script in "${SCRIPTS[@]}"; do
    name="${script%.coh}"
    echo "=== Running ${script} ==="

    pkill -f "qemu-system-aarch64" || true
    pkill -f "/host-tools/cohsh" || true

    if ! wait_port_free 127.0.0.1 31337 5; then
        echo "FAIL: port 31337 still busy before ${script}" >&2
        exit 1
    fi

    rm -rf target out/cohesix
    mkdir -p "${LOG_ROOT}"

    qemu_log="${LOG_ROOT}/${name}.qemu.log"
    coh_log="${LOG_ROOT}/${name}.out.log"

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

    if ! wait_log_marker "$qemu_log" "Cohesix console ready" 180 "$qemu_pid"; then
        echo "FAIL: console ready marker not seen for ${script}" >&2
        cp "$qemu_log" "${ARCHIVE_ROOT}/${name}.qemu.log" || true
        exit 1
    fi

    if ! wait_port_ready 127.0.0.1 31337 30 "$qemu_pid"; then
        echo "FAIL: TCP console not ready for ${script}" >&2
        cp "$qemu_log" "${ARCHIVE_ROOT}/${name}.qemu.log" || true
        exit 1
    fi

    if ! run_cohsh "$script" > "$coh_log" 2>&1; then
        echo "FAIL: cohsh script ${script}" >&2
        cp "$qemu_log" "${ARCHIVE_ROOT}/${name}.qemu.log" || true
        cp "$coh_log" "${ARCHIVE_ROOT}/${name}.out.log" || true
        exit 1
    fi

    cp "$qemu_log" "${ARCHIVE_ROOT}/${name}.qemu.log"
    cp "$coh_log" "${ARCHIVE_ROOT}/${name}.out.log"
    echo "PASS: ${script}"

done

pkill -f "qemu-system-aarch64" || true
pkill -f "/host-tools/cohsh" || true

echo "regression pack complete: ${#SCRIPTS[@]} scripts passed"
