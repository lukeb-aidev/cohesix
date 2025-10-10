# CLASSIFICATION: COMMUNITY
#!/usr/bin/env bash
# Filename: qemu_boot_check.sh v1.1
# Author: Lukas Bower
# Date Modified: 2030-08-09
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

PLATFORM_GEN_HEADER="$ROOT/third_party/seL4/include/generated/plat/platform_gen.h"
EXPECTED_SMMU_IOPT_LEVELS="${EXPECTED_SMMU_IOPT_LEVELS:-1}"

determine_expected_iopt_levels() {
  if [ ! -f "$PLATFORM_GEN_HEADER" ]; then
    echo "❌ Missing seL4 platform header at $PLATFORM_GEN_HEADER" >&2
    exit 1
  fi
  if grep -q '^#define CONFIGURE_SMMU' "$PLATFORM_GEN_HEADER"; then
    printf '%s\n' "$EXPECTED_SMMU_IOPT_LEVELS"
  else
    printf '0\n'
  fi
}

ROOTSERVER_CHECK="$ROOT/ci/rootserver_release_check.sh"
if [ -x "$ROOTSERVER_CHECK" ]; then
  "$ROOTSERVER_CHECK" "$ROOT/out/bin/cohesix_root.elf"
fi

ARCH_RAW="${BOOT_ARCH:-$(uname -m)}"
case "$ARCH_RAW" in
  arm64) ARCH="aarch64" ;;
  amd64) ARCH="x86_64" ;;
  *)     ARCH="$ARCH_RAW" ;;
esac
LOG_DIR="${TMPDIR:-$ROOT/tmp/qemu_ci}"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/qemu_serial.log"
SUCCESS_MARKER="Cohesix shell started"
ACCEL_ARGS=()
QEMU_CPU_OPTS=()
EXPECTED_IOPT_LEVELS="$(determine_expected_iopt_levels)"

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

BOOT_TIMEOUT_S=30
BOOT_BUDGET_MS=200

BOOT_ELAPSED_MS=""
if ! BOOT_ELAPSED_MS="$(
  BOOT_TIMEOUT_S="$BOOT_TIMEOUT_S" \
  SUCCESS_MARKER="$SUCCESS_MARKER" \
  LOG_PATH="$LOG_PATH" \
  QEMU_PID="$QEMU_PID" \
  python3 <<'PY'
import os
import sys
import time

timeout_s = float(os.environ.get("BOOT_TIMEOUT_S", "30"))
marker = os.environ["SUCCESS_MARKER"]
log_path = os.environ["LOG_PATH"]
pid = int(os.environ["QEMU_PID"])

start = time.perf_counter()
deadline = start + timeout_s
last_pos = 0

def qemu_alive() -> bool:
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True

while time.perf_counter() < deadline:
    exists = os.path.exists(log_path)
    if exists:
        with open(log_path, "r", encoding="utf-8", errors="ignore") as handle:
            handle.seek(last_pos)
            chunk = handle.read()
            last_pos = handle.tell()
        if marker in chunk:
            elapsed_ms = int((time.perf_counter() - start) * 1000)
            print(elapsed_ms)
            sys.exit(0)
    if not qemu_alive():
        break
    time.sleep(0.01)

sys.stderr.write("Boot marker not observed before timeout or QEMU exit\n")
sys.exit(1)
PY
)"; then
  kill "$QEMU_PID" 2>/dev/null || true
  wait "$QEMU_PID" 2>/dev/null || true
  echo "❌ boot failed. Log tail:" >&2
  tail -n 20 "$LOG_PATH" >&2 || true
  exit 1
fi

kill "$QEMU_PID" 2>/dev/null || true
wait "$QEMU_PID" 2>/dev/null || true

BOOT_OK=1

if [ "$BOOT_ELAPSED_MS" -gt "$BOOT_BUDGET_MS" ]; then
  echo "❌ boot exceeded latency budget: ${BOOT_ELAPSED_MS}ms (budget ${BOOT_BUDGET_MS}ms)" >&2
  tail -n 20 "$LOG_PATH" >&2 || true
  exit 1
fi

# detect MMU faults or data aborts in the serial log
if grep -qiE "(data abort|mmu fault|prefetch abort)" "$LOG_PATH"; then
  echo "❌ MMU fault detected. Log tail:" >&2
  tail -n 20 "$LOG_PATH" >&2 || true
  exit 1
fi

# ensure interrupt controller configured correctly
if grep -q "Could not infer GIC interrupt target ID" "$LOG_PATH"; then
  echo "❌ GIC target inference warning detected; verify kernel.dts configuration" >&2
  tail -n 20 "$LOG_PATH" >&2 || true
  exit 1
fi

# validate expected IOPT levels based on seL4 configuration
iopt_line="$(grep -E 'IOPT levels:' "$LOG_PATH" | tail -n 1)"
if [ -z "$iopt_line" ]; then
  echo "❌ Unable to locate IOPT levels line in boot log" >&2
  tail -n 20 "$LOG_PATH" >&2 || true
  exit 1
fi
iopt_reported="$(printf '%s' "$iopt_line" | sed -E 's/.*IOPT levels:[[:space:]]*([0-9]+).*/\1/')"
if [ -z "$iopt_reported" ]; then
  echo "❌ Failed to parse IOPT level from line: $iopt_line" >&2
  exit 1
fi
if [ "$iopt_reported" != "$EXPECTED_IOPT_LEVELS" ]; then
  echo "❌ Reported IOPT levels $iopt_reported differ from expected $EXPECTED_IOPT_LEVELS" >&2
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
    echo "✅ aarch64 boot success (${BOOT_ELAPSED_MS}ms)"
  else
    echo "✅ x86_64 boot success (${BOOT_ELAPSED_MS}ms)"
  fi
else
  echo "❌ boot failed. Log tail:" >&2
  tail -n 20 "$LOG_PATH" >&2 || true
  exit 1
fi
