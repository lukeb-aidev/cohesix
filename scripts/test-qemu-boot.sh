#!/usr/bin/env bash
# Author: Lukas Bower

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SEL4_BUILD_DIR="${SEL4_BUILD:-$ROOT_DIR/seL4/build}"
OUT_DIR="${OUT_DIR:-$ROOT_DIR/out/boot-test}"
CARGO_TARGET="${CARGO_TARGET:-aarch64-unknown-none}"
TCP_PORT="${TCP_PORT:-31337}"
QEMU_TIMEOUT="${QEMU_TIMEOUT:-600}"

LOG_FILE="$OUT_DIR/qemu.log"
mkdir -p "$OUT_DIR"

COMMAND=(
    bash "$ROOT_DIR/scripts/cohesix-build-run.sh"
    --sel4-build "$SEL4_BUILD_DIR"
    --out-dir "$OUT_DIR"
    --cargo-target "$CARGO_TARGET"
    --profile release
    --transport tcp
    --tcp-port "$TCP_PORT"
    --raw-qemu
)

if [[ ${COHESIX_BOOT_FEATURES:-} != "" ]]; then
    IFS=',' read -ra REQ_FEATURES <<<"$COHESIX_BOOT_FEATURES"
    for feature in "${REQ_FEATURES[@]}"; do
        COMMAND+=(--features "$feature")
    done
fi

status=0
if ! timeout "$QEMU_TIMEOUT" "${COMMAND[@]}" >"$LOG_FILE" 2>&1; then
    status=$?
fi

if [[ "$status" -eq 124 ]]; then
    echo "[test-qemu-boot] QEMU timed out after ${QEMU_TIMEOUT}s" >&2
    exit 1
fi

if [[ "$status" -ne 0 ]]; then
    echo "[test-qemu-boot] boot command failed with status $status" >&2
    echo "--- qemu log (tail) ---" >&2
    tail -n 120 "$LOG_FILE" >&2 || true
    exit 1
fi

if grep -q "Cohesix console ready" "$LOG_FILE" \
    && grep -q "console tcp listen" "$LOG_FILE" \
    && grep -q "cohsh" "$LOG_FILE"; then
    echo "[test-qemu-boot] boot succeeded"
    exit 0
fi

echo "[test-qemu-boot] boot markers missing" >&2
tail -n 120 "$LOG_FILE" >&2 || true
exit 1
