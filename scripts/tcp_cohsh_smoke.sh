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
STARTUP_TIMEOUT="${STARTUP_TIMEOUT:-90}"

QEMU_LOG="${OUT_DIR}/run.tcp.log"
COHSH_LOG="${OUT_DIR}/cohsh.tcp.log"
QEMU_PID=""

cleanup() {
    if [[ -n "${QEMU_PID}" ]]; then
        kill "${QEMU_PID}" >/dev/null 2>&1 || true
        wait "${QEMU_PID}" >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT

mkdir -p "$OUT_DIR"

"${SCRIPT_DIR}/cohesix-build-run.sh" \
    --sel4-build "${SEL4_BUILD_DIR}" \
    --out-dir "${OUT_DIR}" \
    --profile "${PROFILE}" \
    --cargo-target "${CARGO_TARGET}" \
    --transport tcp \
    --tcp-port "${TCP_PORT}" \
    --raw-qemu \
    >"${QEMU_LOG}" 2>&1 &
QEMU_PID=$!

deadline=$((SECONDS + STARTUP_TIMEOUT))
while [[ $SECONDS -lt $deadline ]]; do
    if rg -q "TCP console listening on 0.0.0.0:${TCP_PORT}" "${QEMU_LOG}"; then
        break
    fi
    sleep 1
done

if ! rg -q "TCP console listening on 0.0.0.0:${TCP_PORT}" "${QEMU_LOG}"; then
    echo "[tcp-smoke] error: TCP console did not become ready within ${STARTUP_TIMEOUT}s" >&2
    tail -n 200 "${QEMU_LOG}" >&2 || true
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
    >"${COHSH_LOG}" 2>&1; then
    echo "[tcp-smoke] error: cohsh returned non-zero status" >&2
    tail -n 200 "${COHSH_LOG}" >&2 || true
    exit 1
fi

rm -f "${COHSH_SCRIPT}"

if rg -q "virtio: zero sized buffers are not allowed" "${QEMU_LOG}"; then
    echo "[tcp-smoke] error: QEMU reported zero-sized buffers" >&2
    exit 1
fi

if rg -q "entered bad status" "${QEMU_LOG}"; then
    echo "[tcp-smoke] error: virtio-net entered bad status" >&2
    exit 1
fi

if rg -q "auth/handshake: timeout|timeout waiting for server response|recv: 0 bytes" "${COHSH_LOG}"; then
    echo "[tcp-smoke] error: cohsh handshake timed out or saw empty reads" >&2
    exit 1
fi

if ! rg -q "ping:" "${COHSH_LOG}"; then
    echo "[tcp-smoke] error: cohsh did not complete ping" >&2
    tail -n 200 "${COHSH_LOG}" >&2 || true
    exit 1
fi

echo "[tcp-smoke] ok: cohsh ping completed and virtio-net stayed healthy"
