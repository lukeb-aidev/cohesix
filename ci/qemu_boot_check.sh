# CLASSIFICATION: COMMUNITY
#!/usr/bin/env bash
# Filename: qemu_boot_check.sh v0.9
# Author: Lukas Bower
# Date Modified: 2029-02-20
# This script boots Cohesix under QEMU for CI. Firmware assumptions:
# - x86_64 uses OVMF for UEFI.
# - aarch64 requires QEMU_EFI.fd provided by system packages
#   (often from qemu-efi-aarch64) and is passed via -bios.
set -euo pipefail

OS_NAME="$(uname -s)"
homebrew_roots=()

if [ "$OS_NAME" = "Darwin" ]; then
  if [ -n "${HOMEBREW_PREFIX:-}" ]; then
    homebrew_roots+=("$HOMEBREW_PREFIX")
  fi
  if command -v brew >/dev/null 2>&1; then
    brew_prefix="$(brew --prefix 2>/dev/null || true)"
    if [ -n "$brew_prefix" ]; then
      homebrew_roots+=("$brew_prefix")
    fi
    brew_qemu_prefix="$(brew --prefix qemu 2>/dev/null || true)"
    if [ -n "$brew_qemu_prefix" ]; then
      homebrew_roots+=("$brew_qemu_prefix")
    fi
  fi
  homebrew_roots+=("/opt/homebrew" "/usr/local")
fi

find_first_file() {
  for candidate in "$@"; do
    if [ -f "$candidate" ]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

ARCH_RAW="${BOOT_ARCH:-$(uname -m)}"
case "$ARCH_RAW" in
  arm64) ARCH="aarch64" ;;
  amd64) ARCH="x86_64" ;;
  *)     ARCH="$ARCH_RAW" ;;
esac
LOG_DIR="${TMPDIR:-$(mktemp -d)}"
LOG_FILE="$LOG_DIR/qemu_serial.log"
SUCCESS_MARKER="Cohesix shell started"
ACCEL_ARGS=()
QEMU_CPU_OPTS=()

if [ "$ARCH" = "aarch64" ]; then
  if ! command -v qemu-system-aarch64 >/dev/null 2>&1; then
    echo "⚠️ qemu-system-aarch64 not installed; skipping ARM boot" >&2
    exit 0
  fi
  QEMU_CPU="cortex-a53"
  if [ "$OS_NAME" = "Darwin" ]; then
    if qemu-system-aarch64 -accel help 2>/dev/null | grep -qi hvf; then
      ACCEL_ARGS=(-accel hvf)
      QEMU_CPU="host"
      echo "✅ Using HVF acceleration for qemu-system-aarch64" >&2
    else
      echo "⚠️ HVF accelerator unavailable; using TCG" >&2
    fi
  fi
  QEMU_CPU_OPTS=(-cpu "$QEMU_CPU")
  if [ -n "${QEMU_EFI:-}" ] && [ ! -f "$QEMU_EFI" ]; then
    echo "⚠️ QEMU_EFI override '$QEMU_EFI' not found; probing default firmware paths" >&2
  fi
  declare -a qemu_efi_candidates=()
  if [ -n "${QEMU_EFI:-}" ]; then
    qemu_efi_candidates+=("$QEMU_EFI")
  fi
  qemu_efi_candidates+=(
    /usr/share/qemu-efi-aarch64/QEMU_EFI.fd
    /usr/share/qemu-efi/QEMU_EFI.fd
    /usr/share/edk2/aarch64/QEMU_EFI.fd
    /usr/share/AAVMF/AAVMF_CODE.fd
    /usr/share/edk2-ovmf/AAVMF_CODE.fd
  )
  for root in "${homebrew_roots[@]}"; do
    qemu_efi_candidates+=(
      "$root/share/qemu/QEMU_EFI.fd"
      "$root/share/qemu/edk2-aarch64-code.fd"
      "$root/share/qemu/edk2-arm-code.fd"
      "$root/share/AAVMF/AAVMF_CODE.fd"
    )
  done
  if QEMU_EFI_PATH="$(find_first_file "${qemu_efi_candidates[@]}")"; then
    QEMU_EFI="$QEMU_EFI_PATH"
  else
    echo "QEMU_EFI.fd not found" >&2
    exit 1
  fi
  ARM_LOG="$LOG_DIR/qemu_arm.log"
  (
    timeout 30s qemu-system-aarch64 \
      -machine virt \
      "${ACCEL_ARGS[@]}" \
      "${QEMU_CPU_OPTS[@]}" \
      -bios "$QEMU_EFI" \
      -drive format=raw,file=fat:rw:out/ \
      -m 256M -net none -nographic -serial mon:stdio -no-reboot 2>&1 | tee "$ARM_LOG" &
  ) # Switched to -serial mon:stdio for direct console output in SSH
  QEMU_PID=$!
  LOG_PATH="$ARM_LOG"
else
  declare -a ovmf_code_candidates=(
    /usr/share/qemu/OVMF.fd
    /usr/share/OVMF/OVMF_CODE.fd
    /usr/share/OVMF/OVMF.fd
    /usr/share/edk2/ovmf/OVMF_CODE.fd
    /usr/share/edk2/x64/OVMF_CODE.fd
    /usr/share/OVMF/OVMF_CODE.fd
  )
  declare -a ovmf_vars_candidates=(
    /usr/share/OVMF/OVMF_VARS.fd
    /usr/share/edk2/ovmf/OVMF_VARS.fd
    /usr/share/edk2/x64/OVMF_VARS.fd
  )
  for root in "${homebrew_roots[@]}"; do
    ovmf_code_candidates+=(
      "$root/share/qemu/OVMF.fd"
      "$root/share/qemu/OVMF_CODE.fd"
      "$root/share/qemu/edk2-x86_64-code.fd"
      "$root/share/OVMF/OVMF_CODE.fd"
      "$root/share/edk2-ovmf/OVMF_CODE.fd"
    )
    ovmf_vars_candidates+=(
      "$root/share/qemu/OVMF_VARS.fd"
      "$root/share/qemu/edk2-x86_64-vars.fd"
      "$root/share/OVMF/OVMF_VARS.fd"
      "$root/share/edk2-ovmf/OVMF_VARS.fd"
    )
  done
  if OVMF_CODE_PATH="$(find_first_file "${ovmf_code_candidates[@]}")"; then
    OVMF_CODE="$OVMF_CODE_PATH"
  else
    echo "⚠️ OVMF firmware not found; skipping x86_64 boot" >&2
    exit 0
  fi
  if OVMF_VARS_PATH="$(find_first_file "${ovmf_vars_candidates[@]}")"; then
    OVMF_VARS="$OVMF_VARS_PATH"
  else
    echo "⚠️ OVMF_VARS firmware not found; skipping x86_64 boot" >&2
    exit 0
  fi
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
