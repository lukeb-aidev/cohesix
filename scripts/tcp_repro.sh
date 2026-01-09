#!/usr/bin/env bash
# Author: Lukas Bower

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "${SCRIPT_DIR}/.." && pwd)
cd "$REPO_ROOT"

OUT_DIR="${OUT_DIR:-out/cohesix}"
TCP_PORT="${TCP_PORT:-31337}"
PROFILE="${PROFILE:-release}"
CARGO_TARGET="${CARGO_TARGET:-aarch64-unknown-none}"
SEL4_BUILD_DIR="${SEL4_BUILD_DIR:-$HOME/seL4/build}"
STARTUP_TIMEOUT="${STARTUP_TIMEOUT:-120}"
COHSH_TIMEOUT="${COHSH_TIMEOUT:-20}"
RUNS="${RUNS:-1}"
RUN_ID_BASE="${RUN_ID_BASE:-run}"
RUN_DIR="${OUT_DIR}/tcp_repro"
QEMU_PIDFILE="${OUT_DIR}/qemu.pid"

QEMU_PID=""

ERROR_PATTERNS="virtio: zero sized buffers|virtio: bogus descriptor|entered bad status"
LISTEN_LINE="TCP console listening on 0.0.0.0:${TCP_PORT}"

OUT_DIR_ABS=""
CPIO_PATH=""

cleanup() {
    stop_qemu
}
trap cleanup EXIT

mkdir -p "${OUT_DIR}"
OUT_DIR_ABS="$(cd "${OUT_DIR}" && pwd)"
CPIO_PATH="${OUT_DIR_ABS}/cohesix-system.cpio"
mkdir -p "${RUN_DIR}"

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
        echo "[tcp-repro] killing stale QEMU: ${stale_pids[*]}"
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
        print(f"[tcp-repro] error: {label} failed to start", file=sys.stderr)
        sys.exit(1)
    while True:
        if time.time() - start > timeout_s:
            proc.kill()
            print(f"[tcp-repro] error: {label} timed out after {timeout_s}s", file=sys.stderr)
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

run_once() {
    local run_idx="$1"
    local run_id="${RUN_ID_BASE}_${run_idx}"
    local run_path="${RUN_DIR}/${run_id}"
    local run_log="${run_path}/run.tcp.log"
    local cohsh_log="${run_path}/cohsh.tcp.log"
    rm -rf "${run_path}"
    mkdir -p "${run_path}"
    rm -f "${QEMU_PIDFILE}"

    stop_qemu
    kill_stale_qemu

    echo "[tcp-repro] starting run ${run_id}"
    SEL4_BUILD_DIR="${SEL4_BUILD_DIR}" \
        "${SCRIPT_DIR}/cohesix-build-run.sh" \
        --sel4-build "${SEL4_BUILD_DIR}" \
        --out-dir "${OUT_DIR}" \
        --profile "${PROFILE}" \
        --root-task-features cohesix-dev \
        --cargo-target "${CARGO_TARGET}" \
        --raw-qemu \
        --transport tcp \
        > >(tee "${run_log}") 2>&1 &
    QEMU_PID=$!
    echo "${QEMU_PID}" > "${QEMU_PIDFILE}"

    local deadline=$((SECONDS + STARTUP_TIMEOUT))
    while [[ $SECONDS -lt $deadline ]]; do
        if rg -q "${ERROR_PATTERNS}" "${run_log}"; then
            echo "[tcp-repro] error: QEMU reported virtio failure during startup (${run_id})" >&2
            break
        fi
        if rg -q "${LISTEN_LINE}" "${run_log}"; then
            break
        fi
        sleep 1
    done

    if rg -q "${ERROR_PATTERNS}" "${run_log}"; then
        echo "[tcp-repro] error: QEMU reported virtio failure during startup (${run_id})" >&2
        stop_qemu
        return 1
    fi

    if ! rg -q "${LISTEN_LINE}" "${run_log}"; then
        echo "[tcp-repro] error: TCP console did not become ready within ${STARTUP_TIMEOUT}s (${run_id})" >&2
        tail -n 200 "${run_log}" >&2 || true
        stop_qemu
        return 1
    fi

    local cohsh_script
    cohsh_script=$(mktemp)
    cat >"${cohsh_script}" <<'COMMANDS'
attach queen
ping
quit
COMMANDS

    if ! run_with_timeout "cohsh (${run_id})" "${COHSH_TIMEOUT}" "${cohsh_log}" \
        cargo run -p cohsh --features tcp -- \
        --transport tcp \
        --tcp-host 127.0.0.1 \
        --tcp-port "${TCP_PORT}" \
        --script "${cohsh_script}"; then
        echo "[tcp-repro] error: cohsh returned non-zero status (${run_id})" >&2
        tail -n 200 "${cohsh_log}" >&2 || true
        rm -f "${cohsh_script}"
        stop_qemu
        return 1
    fi
    rm -f "${cohsh_script}"

    if rg -q "${ERROR_PATTERNS}" "${run_log}"; then
        echo "[tcp-repro] error: QEMU reported virtio failure (${run_id})" >&2
        stop_qemu
        return 1
    fi

    echo "[tcp-repro] summary (${run_id}):"
    echo "forwarded host port:"
    rg -n "hostfwd|forwarded|tcp::|tcp-port" "${run_log}" | head -n 1 || true
    echo "first virtio error:"
    rg -n "${ERROR_PATTERNS}" "${run_log}" | head -n 1 || true
    echo "last net-console/virtio-net lines:"
    rg -n "net-console|virtio-net|virtio:|TCP console" "${run_log}" | tail -n 50 || true

    stop_qemu
    echo "[tcp-repro] ok: TCP console and cohsh session completed (${run_id})"
}

run_failures=0
for run_idx in $(seq 1 "${RUNS}"); do
    if ! run_once "${run_idx}"; then
        run_failures=$((run_failures + 1))
    fi
done

if [[ "${run_failures}" -ne 0 ]]; then
    echo "[tcp-repro] error: ${run_failures} run(s) failed" >&2
    exit 1
fi
