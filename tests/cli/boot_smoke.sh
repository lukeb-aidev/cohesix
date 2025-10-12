#!/usr/bin/env bash
# Author: Lukas Bower

set -euo pipefail

if ! command -v timeout >/dev/null 2>&1; then
    echo "[boot-smoke] ERROR: timeout command is required" >&2
    exit 1
fi

PROJECT_ROOT="$(git rev-parse --show-toplevel)"
SEL4_BUILD_DIR="${SEL4_BUILD:-$HOME/seL4/build}"
TARGET_TRIPLE="${COHESIX_CARGO_TARGET:-aarch64-unknown-none}"
CARGO_PROFILE="${COHESIX_CARGO_PROFILE:-release}"
TIMEOUT_SECONDS="${COHESIX_QEMU_TIMEOUT:-30}"
OUT_DIR="${COHESIX_BOOT_SMOKE_OUT:-$PROJECT_ROOT/out/boot-smoke}"
LOG_PATH="$OUT_DIR/qemu.log"

if [[ ! -d "$SEL4_BUILD_DIR" ]]; then
    echo "[boot-smoke] ERROR: seL4 build directory not found: $SEL4_BUILD_DIR" >&2
    exit 2
fi

mkdir -p "$OUT_DIR"
: > "$LOG_PATH"

BUILD_RUN_CMD=("$PROJECT_ROOT/scripts/cohesix-build-run.sh"
    --sel4-build "$SEL4_BUILD_DIR"
    --out-dir "$OUT_DIR"
    --cargo-target "$TARGET_TRIPLE"
    --profile "$CARGO_PROFILE")

set +e
{ timeout "${TIMEOUT_SECONDS}s" "${BUILD_RUN_CMD[@]}"; } | tee "$LOG_PATH"
STATUS=${PIPESTATUS[0]}
set -e

if [[ "$STATUS" -ne 0 && "$STATUS" -ne 124 ]]; then
    echo "[boot-smoke] ERROR: build/run failed with status $STATUS" >&2
    exit "$STATUS"
fi

for token in \
    "[cohesix:root-task] Cohesix v0 (AArch64/virt)" \
    "[cohesix:root-task] tick: 1" \
    "[cohesix:root-task] tick: 2" \
    "[cohesix:root-task] tick: 3" \
    "[cohesix:root-task] PING" \
    "[cohesix:root-task] PONG"; do
    if ! grep -q "$token" "$LOG_PATH"; then
        echo "[boot-smoke] ERROR: expected marker missing: $token" >&2
        exit 3
    fi
done

echo "[boot-smoke] PASS: Cohesix banner, ticks, and IPC markers detected"
