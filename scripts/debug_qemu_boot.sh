#!/bin/bash
# CLASSIFICATION: COMMUNITY
# Filename: scripts/debug_qemu_boot.sh v0.4
# Author: Lukas Bower
# Date Modified: 2026-11-24
# Ensures this script runs cleanly under Bash for CI use
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

LOG_DIR="$ROOT/logs"
mkdir -p "$LOG_DIR"
INVOC_LOG="$LOG_DIR/qemu_invocation.log"

ISO="$ROOT/out/cohesix.iso"
ROOT_ELF="$ROOT/out/cohesix_root.elf"
CFG="$ROOT/config/config.yaml"

missing=0
check_file() {
  local f="$1"
  if [[ -f "$f" ]]; then
    stat -c "CHECK %n size=%s perm=%a" "$f"
  else
    echo "MISSING $f"
    missing=1
  fi
}

echo "Working directory: $(pwd)"
check_file "$ISO"
check_file "$ROOT_ELF"
check_file "$CFG"

du -sh out 2>/dev/null || true
if command -v sha256sum >/dev/null; then
  sha256sum "$ISO" 2>/dev/null || true
fi

QEMU=$(command -v qemu-system-x86_64 || true)
if [[ -z "$QEMU" ]]; then
  echo "QEMU not found"
  exit 1
fi
"$QEMU" --version

timeout 2 "$QEMU" -cdrom "$ISO" -nographic -serial mon:stdio -d int -D "$LOG_DIR/bootlog.txt" -S -snapshot 2>&1 | tee -a "$INVOC_LOG" || true
# Switched to -serial mon:stdio for direct console output in SSH

if [[ $missing -eq 0 ]]; then
  echo "DEBUG_BOOT_READY"
else
  echo "DEBUG_BOOT_FAILED"
  exit 1
fi
