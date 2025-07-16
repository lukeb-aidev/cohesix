# CLASSIFICATION: COMMUNITY
#!/usr/bin/env bash
# Filename: qemu_boot_check.sh v0.6
# Author: Lukas Bower
# Date Modified: 2027-12-31
# This script boots Cohesix under QEMU for CI. Firmware assumptions:
# - x86_64 uses OVMF for UEFI.
# - aarch64 requires QEMU_EFI.fd provided by system packages
#   (often from qemu-efi-aarch64) and is passed via -bios.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

ARCH="$(uname -m)"
LOG_DIR="${TMPDIR:-$(mktemp -d)}"
LOG_FILE="$LOG_DIR/qemu_serial.log"
SUCCESS_MARKER="Cohesix shell started"

if [ "$ARCH" = "aarch64" ]; then
  if ! command -v qemu-system-aarch64 >/dev/null 2>&1; then
    echo "⚠️ qemu-system-aarch64 not installed; skipping ARM boot" >&2
    exit 0
  fi
  QEMU_EFI="/usr/share/qemu-efi-aarch64/QEMU_EFI.fd"
  if [ ! -f "$QEMU_EFI" ]; then
    for p in /usr/share/qemu-efi/QEMU_EFI.fd /usr/share/edk2/aarch64/QEMU_EFI.fd; do
      if [ -f "$p" ]; then
        QEMU_EFI="$p"
        break
      fi
    done
  fi
  [ -f "$QEMU_EFI" ] || { echo "QEMU_EFI.fd not found" >&2; exit 1; }
  ARM_LOG="$LOG_DIR/qemu_arm.log"
  (
    timeout 30s qemu-system-aarch64 \
      -machine virt \
      -cpu cortex-a53 \
      -bios "$QEMU_EFI" \
      -drive format=raw,file=fat:rw:out/ \
      -m 256M -net none -nographic -serial mon:stdio -no-reboot 2>&1 | tee "$ARM_LOG" &
  ) # Switched to -serial mon:stdio for direct console output in SSH
  QEMU_PID=$!
  LOG_PATH="$ARM_LOG"
else
  OVMF_CODE="/usr/share/qemu/OVMF.fd"
  if [ ! -f "$OVMF_CODE" ]; then
    for p in /usr/share/OVMF/OVMF_CODE.fd /usr/share/OVMF/OVMF.fd /usr/share/edk2/ovmf/OVMF_CODE.fd; do
      if [ -f "$p" ]; then
        OVMF_CODE="$p"
        break
      fi
    done
  fi
  OVMF_VARS=""
  for p in /usr/share/OVMF/OVMF_VARS.fd /usr/share/edk2/ovmf/OVMF_VARS.fd; do
    if [ -f "$p" ]; then
      OVMF_VARS="$p"
      break
    fi
  done
  [ -f "$OVMF_CODE" ] || { echo "⚠️ OVMF firmware not found; skipping x86_64 boot" >&2; exit 0; }
  [ -n "$OVMF_VARS" ] || { echo "⚠️ OVMF_VARS.fd not found; skipping x86_64 boot" >&2; exit 0; }
  cp "$OVMF_VARS" "$LOG_DIR/OVMF_VARS.fd"
  (
    timeout 30s qemu-system-x86_64 \
      -bios "$OVMF_CODE" \
      -pflash "$LOG_DIR/OVMF_VARS.fd" \
      -drive format=raw,file=fat:rw:out/ \
      -m 256M -net none -nographic -serial mon:stdio -no-reboot 2>&1 | tee "$LOG_FILE" &
  ) # Switched to -serial mon:stdio for direct console output in SSH
  QEMU_PID=$!
  LOG_PATH="$LOG_FILE"
fi

BOOT_OK=0
for _ in {1..30}; do
  if grep -q "$SUCCESS_MARKER" "$LOG_PATH" 2>/dev/null; then
    BOOT_OK=1
    break
  fi
  if ! ps -p "$QEMU_PID" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

kill "$QEMU_PID" 2>/dev/null || true
wait "$QEMU_PID" 2>/dev/null || true

# detect MMU faults or data aborts in the serial log
if grep -qiE "(data abort|mmu fault|prefetch abort)" "$LOG_PATH"; then
  echo "❌ MMU fault detected. Log tail:" >&2
  tail -n 20 "$LOG_PATH" >&2 || true
  exit 1
fi

# catch capability copy failures from sel4utils
if grep -qi "Failed to copy cap" "$LOG_PATH"; then
  echo "❌ Capability copy failed. Log tail:" >&2
  tail -n 20 "$LOG_PATH" >&2 || true
  exit 1
fi

if [ "$BOOT_OK" -eq 1 ]; then
  if [ "$ARCH" = "aarch64" ]; then
    echo "✅ aarch64 boot success"
  else
    echo "✅ x86_64 boot success"
  fi
else
  echo "❌ boot failed. Log tail:" >&2
  tail -n 20 "$LOG_PATH" >&2 || true
  exit 1
fi
