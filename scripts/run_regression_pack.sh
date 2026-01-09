#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run the Milestone ≤8a regression pack with a fresh QEMU per script.

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "${SCRIPT_DIR}/.." && pwd)
cd "${REPO_ROOT}"

OUT_DIR="${OUT_DIR:-out/cohesix}"
TCP_PORT="${TCP_PORT:-31337}"
PROFILE="${PROFILE:-release}"
CARGO_TARGET="${CARGO_TARGET:-aarch64-unknown-none}"
SEL4_BUILD_DIR="${SEL4_BUILD_DIR:-$HOME/seL4/build}"
STARTUP_TIMEOUT="${STARTUP_TIMEOUT:-120}"
COHSH_TIMEOUT="${COHSH_TIMEOUT:-20}"
RUN_ID="${RUN_ID:-run1}"
LOG_DIR="${OUT_DIR}/logs"
QEMU_PIDFILE="${OUT_DIR}/qemu.pid"

RUN_DIR="${OUT_DIR}/regression_pack"

ERROR_PATTERNS="virtio: zero sized buffers|virtio: bogus descriptor|entered bad status"
LISTEN_LINE="TCP console listening on 0.0.0.0:${TCP_PORT}"

OUT_DIR_ABS=""
CPIO_PATH=""
QEMU_PID=""

cleanup() {
    stop_qemu
}
trap cleanup EXIT

mkdir -p "${OUT_DIR}"
mkdir -p "${LOG_DIR}"
mkdir -p "${RUN_DIR}"
OUT_DIR_ABS="$(cd "${OUT_DIR}" && pwd)"
CPIO_PATH="${OUT_DIR_ABS}/cohesix-system.cpio"

kill_stale_qemu() {
    local pid
    local stale_pids=()
    if command -v pgrep >/dev/null 2>&1; then
        while IFS= read -r pid; do
            [[ -n "$pid" ]] && stale_pids+=("$pid")
        done < <(pgrep -f "qemu-system-aarch64.*${CPIO_PATH}" || true)
    else
        while IFS= read -r pid; do
            [[ -n "$pid" ]] && stale_pids+=("$pid")
        done < <(ps -ax -o pid= -o command= | rg "qemu-system-aarch64.*${CPIO_PATH}" | awk '{print $1}')
    fi

    if [[ ${#stale_pids[@]} -gt 0 ]]; then
        echo "[regression-pack] killing stale QEMU: ${stale_pids[*]}"
        kill "${stale_pids[@]}" >/dev/null 2>&1 || true
        sleep 1
        for pid in "${stale_pids[@]}"; do
            if kill -0 "$pid" >/dev/null 2>&1; then
                kill -9 "$pid" >/dev/null 2>&1 || true
            fi
        done
    fi
}

stop_qemu() {
    local pid="${QEMU_PID}"
    if [[ -z "${pid}" ]] && [[ -f "${QEMU_PIDFILE}" ]]; then
        pid="$(cat "${QEMU_PIDFILE}" 2>/dev/null || true)"
    fi
    if [[ -n "${pid}" ]]; then
        pkill -P "${pid}" >/dev/null 2>&1 || true
        kill "${pid}" >/dev/null 2>&1 || true
        wait "${pid}" >/dev/null 2>&1 || true
    fi
    QEMU_PID=""
    rm -f "${QEMU_PIDFILE}"
}

run_with_timeout() {
    local label="$1"
    local timeout_s="$2"
    local log_path="$3"
    shift 3
    python3 - "$timeout_s" "$log_path" "$label" "$@" <<'PY'
import os
import select
import subprocess
import sys
import time

timeout_s = float(sys.argv[1])
log_path = sys.argv[2]
label = sys.argv[3]
cmd = sys.argv[4:]

start = time.time()
with open(log_path, "wb") as log:
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    fd = proc.stdout.fileno() if proc.stdout else None
    if fd is None:
        print(f"[regression-pack] error: {label} failed to start", file=sys.stderr)
        sys.exit(1)
    while True:
        if time.time() - start > timeout_s:
            proc.kill()
            print(f"[regression-pack] error: {label} timed out after {timeout_s}s", file=sys.stderr)
            sys.exit(124)
        rlist, _, _ = select.select([fd], [], [], 0.1)
        if rlist:
            data = os.read(fd, 4096)
            if not data:
                break
            sys.stdout.buffer.write(data)
            sys.stdout.buffer.flush()
            log.write(data)
            log.flush()
        if proc.poll() is not None:
            remaining = os.read(fd, 4096)
            while remaining:
                sys.stdout.buffer.write(remaining)
                sys.stdout.buffer.flush()
                log.write(remaining)
                log.flush()
                remaining = os.read(fd, 4096)
            break
    sys.exit(proc.wait())
PY
}

run_qemu_for_script() {
    local script_name="$1"
    local qemu_log="${LOG_DIR}/${script_name}.${RUN_ID}.qemu.log"
    rm -f "${qemu_log}"

    stop_qemu
    kill_stale_qemu

    echo "[regression-pack] starting QEMU for ${script_name}"
    SEL4_BUILD_DIR="${SEL4_BUILD_DIR}" \
        "${SCRIPT_DIR}/cohesix-build-run.sh" \
        --sel4-build "${SEL4_BUILD_DIR}" \
        --out-dir "${OUT_DIR}" \
        --profile "${PROFILE}" \
        --root-task-features cohesix-dev \
        --cargo-target "${CARGO_TARGET}" \
        --raw-qemu \
        --transport tcp \
        > >(tee "${qemu_log}") 2>&1 &
    QEMU_PID=$!
    echo "${QEMU_PID}" > "${QEMU_PIDFILE}"

    local deadline=$((SECONDS + STARTUP_TIMEOUT))
    while [[ $SECONDS -lt $deadline ]]; do
        if rg -q "${ERROR_PATTERNS}" "${qemu_log}"; then
            echo "[regression-pack] error: QEMU reported virtio failure during startup (${script_name})" >&2
            break
        fi
        if rg -q "${LISTEN_LINE}" "${qemu_log}"; then
            return 0
        fi
        sleep 1
    done

    if rg -q "${ERROR_PATTERNS}" "${qemu_log}"; then
        echo "[regression-pack] error: QEMU reported virtio failure during startup (${script_name})" >&2
    else
        echo "[regression-pack] error: TCP console did not become ready within ${STARTUP_TIMEOUT}s (${script_name})" >&2
        tail -n 200 "${qemu_log}" >&2 || true
    fi
    return 1
}

run_cohsh_script() {
    local script_name="$1"
    local script_path="${SCRIPT_DIR}/cohsh/${script_name}"
    local cohsh_log="${LOG_DIR}/${script_name}.${RUN_ID}.log"
    if [[ ! -f "${script_path}" ]]; then
        echo "[regression-pack] error: missing script ${script_path}" >&2
        return 1
    fi

    echo "[regression-pack] running ${script_name}"
    if ! run_with_timeout "cohsh ${script_name}" "${COHSH_TIMEOUT}" "${cohsh_log}" \
        cargo run -p cohsh --features tcp -- \
        --transport tcp \
        --tcp-host 127.0.0.1 \
        --tcp-port "${TCP_PORT}" \
        --script "${script_path}"; then
        echo "[regression-pack] error: ${script_name} failed (log: ${cohsh_log})" >&2
        rg -n "script failure at line|ERROR|Error:" "${cohsh_log}" >&2 || true
        tail -n 40 "${cohsh_log}" >&2 || true
        return 1
    fi
    return 0
}

scripts=(
    "boot_v0.coh"
    "9p_batch.coh"
    "telemetry_ring.coh"
    "observe_watch.coh"
    "cas_roundtrip.coh"
)

echo "[regression-pack] running tcp repro harness"
./scripts/tcp_repro.sh

for script in "${scripts[@]}"; do
    if ! run_qemu_for_script "${script}"; then
        stop_qemu
        exit 1
    fi
    if ! run_cohsh_script "${script}"; then
        stop_qemu
        exit 1
    fi
    stop_qemu
    echo "[regression-pack] ok: ${script}"
done

echo "[regression-pack] complete: ${#scripts[@]} scripts passed"
