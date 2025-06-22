# CLASSIFICATION: COMMUNITY
#!/usr/bin/env bash
# Filename: qemu_boot_check.sh v0.2
# Author: Lukas Bower
# Date Modified: 2026-02-21
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

LOG_DIR="${TMPDIR:-$(mktemp -d)}"
LOG_FILE="$LOG_DIR/qemu_serial.log"
SUCCESS_MARKER="Cohesix shell started"

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
[ -f "$OVMF_CODE" ] || { echo "OVMF firmware not found" >&2; exit 1; }
[ -n "$OVMF_VARS" ] || { echo "OVMF_VARS.fd not found" >&2; exit 1; }
cp "$OVMF_VARS" "$LOG_DIR/OVMF_VARS.fd"

(
  timeout 30s qemu-system-x86_64 \
    -bios "$OVMF_CODE" \
    -pflash "$LOG_DIR/OVMF_VARS.fd" \
    -drive format=raw,file=fat:rw:out/ \
    -m 256M -net none -nographic -serial file:"$LOG_FILE" -no-reboot &
)
QEMU_PID=$!

BOOT_OK=0
for _ in {1..30}; do
  if grep -q "$SUCCESS_MARKER" "$LOG_FILE" 2>/dev/null; then
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

if [ "$BOOT_OK" -eq 1 ]; then
  echo "✅ x86_64 boot success"
else
  echo "❌ x86_64 boot failed. Log tail:" >&2
  tail -n 20 "$LOG_FILE" >&2 || true
  exit 1
fi

if command -v qemu-system-aarch64 >/dev/null 2>&1; then
  ARM_LOG="$LOG_DIR/qemu_arm.log"
  (
    timeout 30s qemu-system-aarch64 \
      -machine virt \
      -cpu cortex-a53 \
      -drive format=raw,file=fat:rw:out/ \
      -m 256M -net none -nographic -serial file:"$ARM_LOG" -no-reboot &
  )
  ARM_PID=$!
  ARM_OK=0
  for _ in {1..30}; do
    if grep -q "$SUCCESS_MARKER" "$ARM_LOG" 2>/dev/null; then
      ARM_OK=1
      break
    fi
    if ! ps -p "$ARM_PID" >/dev/null 2>&1; then
      break
    fi
    sleep 1
  done
  kill "$ARM_PID" 2>/dev/null || true
  wait "$ARM_PID" 2>/dev/null || true
  if [ "$ARM_OK" -eq 1 ]; then
    echo "✅ aarch64 boot success"
    exit 0
  else
    echo "❌ aarch64 boot failed. Log tail:" >&2
    tail -n 20 "$ARM_LOG" >&2 || true
    exit 1
  fi
else
  echo "⚠️ qemu-system-aarch64 not installed; skipping ARM boot" >&2
fi
