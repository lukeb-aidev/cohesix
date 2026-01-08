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

RUN_LOG="${OUT_DIR}/run.tcp.log"
COHSH_LOG="${OUT_DIR}/cohsh.tcp.log"
NC_LOG="${OUT_DIR}/nc.tcp.log"
QEMU_PID=""

ERROR_PATTERNS="virtio: zero sized buffers|virtio: bogus descriptor|entered bad status"
LISTEN_LINE="TCP console listening on 0.0.0.0:${TCP_PORT}"

cleanup() {
    if [[ -n "${QEMU_PID}" ]]; then
        pkill -P "${QEMU_PID}" >/dev/null 2>&1 || true
        kill "${QEMU_PID}" >/dev/null 2>&1 || true
        wait "${QEMU_PID}" >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT

rm -rf "${OUT_DIR}"
mkdir -p "${OUT_DIR}"

SEL4_BUILD_DIR="${SEL4_BUILD_DIR}" \
    "${SCRIPT_DIR}/cohesix-build-run.sh" \
    --sel4-build "${SEL4_BUILD_DIR}" \
    --out-dir "${OUT_DIR}" \
    --profile "${PROFILE}" \
    --root-task-features dev-virt,cache-trace \
    --cargo-target "${CARGO_TARGET}" \
    --raw-qemu \
    --transport tcp \
    > >(tee "${RUN_LOG}") 2>&1 &
QEMU_PID=$!

deadline=$((SECONDS + STARTUP_TIMEOUT))
while [[ $SECONDS -lt $deadline ]]; do
    if rg -q "${ERROR_PATTERNS}" "${RUN_LOG}"; then
        echo "[tcp-repro] error: QEMU reported virtio failure during startup" >&2
        break
    fi
    if rg -q "${LISTEN_LINE}" "${RUN_LOG}"; then
        break
    fi
    sleep 1
done

if rg -q "${ERROR_PATTERNS}" "${RUN_LOG}"; then
    echo "[tcp-repro] error: QEMU reported virtio failure during startup" >&2
    exit 1
fi

if ! rg -q "${LISTEN_LINE}" "${RUN_LOG}"; then
    echo "[tcp-repro] error: TCP console did not become ready within ${STARTUP_TIMEOUT}s" >&2
    tail -n 200 "${RUN_LOG}" >&2 || true
    exit 1
fi

if ! command -v nc >/dev/null 2>&1; then
    echo "[tcp-repro] error: nc not found on PATH" >&2
    exit 1
fi

printf "help\n" | nc -v 127.0.0.1 "${TCP_PORT}" -w 2 2>&1 | tee "${NC_LOG}"
if [[ ! -s "${NC_LOG}" ]]; then
    echo "[tcp-repro] error: nc session produced no output" >&2
    exit 1
fi

COHSH_SCRIPT=$(mktemp)
cat >"${COHSH_SCRIPT}" <<'COMMANDS'
attach queen
ping
quit
COMMANDS

if ! cargo run -p cohsh --features tcp -- \
    --transport tcp \
    --tcp-host 127.0.0.1 \
    --tcp-port "${TCP_PORT}" \
    --script "${COHSH_SCRIPT}" \
    2>&1 | tee "${COHSH_LOG}"; then
    echo "[tcp-repro] error: cohsh returned non-zero status" >&2
    tail -n 200 "${COHSH_LOG}" >&2 || true
    exit 1
fi

rm -f "${COHSH_SCRIPT}"

if rg -q "${ERROR_PATTERNS}" "${RUN_LOG}"; then
    echo "[tcp-repro] error: QEMU reported virtio failure" >&2
    exit 1
fi

echo "[tcp-repro] summary:"
echo "forwarded host port:"
rg -n "hostfwd|forwarded|tcp::|tcp-port" "${RUN_LOG}" | head -n 1 || true
echo "first virtio error:"
rg -n "${ERROR_PATTERNS}" "${RUN_LOG}" | head -n 1 || true
echo "last net-console/virtio-net lines:"
rg -n "net-console|virtio-net|virtio:|TCP console" "${RUN_LOG}" | tail -n 50 || true

echo "[tcp-repro] ok: TCP console and cohsh session completed"
