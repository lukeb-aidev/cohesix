# CLASSIFICATION: COMMUNITY
# Filename: boot_qemu.sh v0.5
# Author: Lukas Bower
# Date Modified: 2029-02-20
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"
QEMU=$(command -v qemu-system-aarch64 || true)
if [[ -z "$QEMU" ]]; then
  echo "ERROR: qemu-system-aarch64 not found" >&2
  exit 1
fi
HOST_OS="$(uname -s)"
HOST_ARCH="$(uname -m)"
ACCEL_ARGS=()
CPU_MODEL="cortex-a57"
if [[ "$HOST_OS" = "Darwin" && "$HOST_ARCH" = "arm64" ]]; then
  if "$QEMU" -accel help 2>/dev/null | grep -qi hvf; then
    ACCEL_ARGS=(-accel hvf)
    CPU_MODEL="host"
    echo "Using HVF acceleration" >&2
  else
    echo "HVF accelerator unavailable; using TCG" >&2
  fi
fi
LOG_DIR="$ROOT/log"
mkdir -p "$LOG_DIR"
QEMU_LOG="$LOG_DIR/qemu_debug_$(date +%Y%m%d_%H%M%S).log"
exec "$QEMU" -M virt,gic-version=2 "${ACCEL_ARGS[@]}" -cpu "$CPU_MODEL" -m 512M \
  -kernel out/bin/elfloader \
  -serial mon:stdio -nographic \
  -d in_asm,exec,int,mmu,page,guest_errors,unimp,cpu_reset \
  -D "$QEMU_LOG"
