#!/bin/bash
# CLASSIFICATION: COMMUNITY
# Filename: scripts/debug_qemu_boot.sh v0.5
# Author: Lukas Bower
# Date Modified: 2029-10-07
# SAFe Epic: E5-F13 Boot Telemetry | Feature: F15 QEMU Trace Instrumentation
# Ensures this script runs cleanly under Bash for CI use
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

LOG_DIR="$ROOT/logs"
mkdir -p "$LOG_DIR"
INVOC_LOG="$LOG_DIR/qemu_invocation.log"
TRACE_LOG="$LOG_DIR/qemu_boot_trace.log"

ELFLOADER="$ROOT/out/bin/elfloader"
ROOT_ELF="$ROOT/out/cohesix_root.elf"
CPIO_PAYLOAD="${COHESIX_CPIO_PAYLOAD:-${CPIO_IMAGE:-$ROOT/boot/cohesix.cpio}}"

log() {
  printf '%s\n' "$*" | tee -a "$INVOC_LOG"
}

missing=0
check_file() {
  local f="$1"
  if [[ -f "$f" ]]; then
    stat -c "CHECK %n size=%s perm=%a" "$f" | tee -a "$INVOC_LOG"
  else
    log "MISSING $f"
    missing=1
  fi
}

log "Working directory: $(pwd)"
log "Using CPIO payload: $CPIO_PAYLOAD"
check_file "$ELFLOADER"
check_file "$ROOT_ELF"
check_file "$CPIO_PAYLOAD"

du -sh out 2>/dev/null | tee -a "$INVOC_LOG" || true
if command -v sha256sum >/dev/null; then
  for artifact in "$ELFLOADER" "$ROOT_ELF" "$CPIO_PAYLOAD"; do
    if [[ -f "$artifact" ]]; then
      sha256sum "$artifact" 2>/dev/null | tee -a "$INVOC_LOG" || true
    fi
  done
fi

QEMU=$(command -v qemu-system-aarch64 || true)
if [[ -z "$QEMU" ]]; then
  log "QEMU not found"
  exit 1
fi
"$QEMU" --version | tee -a "$INVOC_LOG"

TRACE_FLAGS="in_asm,exec,int,mmu,page,guest_errors,unimp,cpu_reset"
QEMU_CMD=(
  "$QEMU"
  -M virt
  -cpu cortex-a57
  -m 1024
  -kernel "$ELFLOADER"
  -initrd "$CPIO_PAYLOAD"
  -serial mon:stdio
  -nographic
  -d "$TRACE_FLAGS"
  -D "$TRACE_LOG"
  -S
  -snapshot
)

log "Invoking QEMU: $(printf '%q ' "${QEMU_CMD[@]}")"
timeout 5 "${QEMU_CMD[@]}" 2>&1 | tee -a "$INVOC_LOG" || true
# Maintains -serial mon:stdio for direct console output in SSH

if [[ $missing -eq 0 ]]; then
  log "DEBUG_BOOT_READY"
else
  log "DEBUG_BOOT_FAILED"
  exit 1
fi
