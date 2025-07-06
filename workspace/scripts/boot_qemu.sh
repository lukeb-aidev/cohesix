# CLASSIFICATION: COMMUNITY
# Filename: boot_qemu.sh v0.3
# Author: Lukas Bower
# Date Modified: 2026-12-31
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"
QEMU=$(command -v qemu-system-aarch64 || true)
if [[ -z "$QEMU" ]]; then
  echo "ERROR: qemu-system-aarch64 not found" >&2
  exit 1
fi
LOG_DIR="$ROOT/log"
mkdir -p "$LOG_DIR"
QEMU_LOG="$LOG_DIR/qemu_debug_$(date +%Y%m%d_%H%M%S).log"
exec "$QEMU" -M virt,gic-version=2 -cpu cortex-a57 -m 512M \
  -kernel out/bin/elfloader \
  -serial mon:stdio -nographic \
  -d int,mmu,page,guest_errors,unimp,cpu_reset \
  -D "$QEMU_LOG"
