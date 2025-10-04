#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: scripts/qemu_smoke_x86.sh v0.2
# Author: Lukas Bower
# Date Modified: 2029-10-10
# SAFe Epic: E5-F13 Boot Telemetry | Feature: F15 QEMU Trace Instrumentation

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

EPIC_ID="E5-F13"
TRACE_ID="${COHESIX_TRACE_ID:-}"
if [[ -z "$TRACE_ID" ]]; then
  TRACE_ID="$(python3 - <<'PY'
import uuid
print(uuid.uuid4().hex)
PY
  )"
fi
TRACE_ID="${TRACE_ID//[$'\r\n\t ']}"
if [[ -z "$TRACE_ID" ]]; then
  TRACE_ID="manual$(date -u +%s)"
fi

ts() {
  date -u +%Y-%m-%dT%H:%M:%SZ
}

LOG_ROOT="$ROOT/log/boot"
mkdir -p "$LOG_ROOT"
LOG_TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_LOG="$LOG_ROOT/qemu_smoke_aarch64_${LOG_TIMESTAMP}.log"
CONSOLE_LOG="$LOG_ROOT/qemu_smoke_aarch64_console_${LOG_TIMESTAMP}.log"

log_line() {
  local level="$1"
  shift
  local msg="$*"
  local line="$(ts) [$EPIC_ID][$TRACE_ID][$level] $msg"
  printf '%s\n' "$line" | tee -a "$RUN_LOG"
}

log_info() {
  log_line INFO "$*"
}

log_error() {
  log_line ERROR "$*" >&2
}

require_command() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    log_error "Required command '$cmd' not found. Install QEMU aarch64 support (e.g., sudo apt install qemu-system-arm)."
    exit 1
  fi
}

locate_artifact() {
  local result_var="$1"
  shift
  local candidate
  for candidate in "$@"; do
    if [[ -f "$candidate" ]]; then
      printf -v "$result_var" '%s' "$candidate"
      return 0
    fi
  done
  return 1
}

validate_override() {
  local value="$1"
  local label="$2"
  if [[ -n "$value" ]]; then
    if [[ ! -f "$value" ]]; then
      log_error "${label} override '$value' is not a file. Ensure the build completed (see third_party/seL4/README_BUILD.md)."
      exit 1
    fi
    printf '%s' "$value"
    return 0
  fi
  return 1
}

require_command qemu-system-aarch64

ELFLOADER_CANDIDATES=(
  "$ROOT/out/bin/elfloader"
  "$ROOT/out/elfloader"
  "$ROOT/boot/elfloader"
  "$ROOT/third_party/seL4/elfloader"
)
ROOT_ELF_CANDIDATES=(
  "$ROOT/out/cohesix_root.elf"
  "$ROOT/out/bin/cohesix_root.elf"
  "$ROOT/boot/cohesix_root.elf"
  "$ROOT/target/sel4-aarch64/release/cohesix_root"
)
CPIO_CANDIDATES=(
  "$ROOT/out/cohesix_root.cpio"
  "$ROOT/out/cohesix.cpio"
  "$ROOT/boot/cohesix.cpio"
  "$ROOT/out/initrd.cpio"
)

ELFLOADER_PATH=""
ROOT_ELF_PATH=""
CPIO_PATH=""

ELFLOADER_OVERRIDE="${COHESIX_ELFLOADER:-${ELFLOADER:-}}"
ROOT_ELF_OVERRIDE="${COHESIX_ROOT_ELF:-${ROOT_ELF:-}}"
CPIO_OVERRIDE="${COHESIX_INITRD:-${CPIO_IMAGE:-}}"

if ! validate_override "$ELFLOADER_OVERRIDE" "Elfloader" >/dev/null 2>&1; then
  if ! locate_artifact ELFLOADER_PATH "${ELFLOADER_CANDIDATES[@]}"; then
    log_error "Elfloader binary not found. Run 'third_party/seL4/build_sel4.sh' to produce out/bin/elfloader."
    exit 1
  fi
else
  ELFLOADER_PATH="$ELFLOADER_OVERRIDE"
fi

if ! validate_override "$ROOT_ELF_OVERRIDE" "Root ELF" >/dev/null 2>&1; then
  if ! locate_artifact ROOT_ELF_PATH "${ROOT_ELF_CANDIDATES[@]}"; then
    log_error "Rootserver ELF missing. Build cohesix_root via 'cargo +nightly build -p cohesix_root --release --target=workspace/cohesix_root/sel4-aarch64.json'."
    exit 1
  fi
else
  ROOT_ELF_PATH="$ROOT_ELF_OVERRIDE"
fi

if ! validate_override "$CPIO_OVERRIDE" "Initrd" >/dev/null 2>&1; then
  if ! locate_artifact CPIO_PATH "${CPIO_CANDIDATES[@]}"; then
    log_error "CPIO bundle absent. Package the governed initrd with 'third_party/seL4/build_sel4.sh' or consult workspace/docs/community/diagnostics/USERLAND_BOOT.md."
    exit 1
  fi
else
  CPIO_PATH="$CPIO_OVERRIDE"
fi

log_info "Elfloader: $ELFLOADER_PATH"
log_info "Root ELF: $ROOT_ELF_PATH"
log_info "CPIO: $CPIO_PATH"
log_info "Run log: $RUN_LOG"
log_info "Console log: $CONSOLE_LOG"

BOOT_WINDOW="${COHESIX_QEMU_TIMEOUT:-8}"
if [[ "$BOOT_WINDOW" =~ ^[0-9]+$ ]]; then
  BOOT_WINDOW=$(( BOOT_WINDOW > 0 ? BOOT_WINDOW : 8 ))
else
  log_info "Non-numeric COHESIX_QEMU_TIMEOUT '$BOOT_WINDOW' ignored; defaulting to 8 seconds."
  BOOT_WINDOW=8
fi

QEMU_CMD=(
  qemu-system-aarch64
  -M virt
  -cpu cortex-a57
  -m 1024
  -kernel "$ELFLOADER_PATH"
  -initrd "$CPIO_PATH"
  -serial mon:stdio
  -nographic
)

log_info "Launching QEMU (timeout ${BOOT_WINDOW}s): $(printf '%q ' "${QEMU_CMD[@]}")"

set +e
if command -v timeout >/dev/null 2>&1; then
  timeout "$BOOT_WINDOW" "${QEMU_CMD[@]}" \
    > >(tee -a "$CONSOLE_LOG") \
    2> >(tee -a "$CONSOLE_LOG" >&2)
  STATUS=$?
else
  log_info "timeout command unavailable; manual termination after ${BOOT_WINDOW}s"
  "${QEMU_CMD[@]}" \
    > >(tee -a "$CONSOLE_LOG") \
    2> >(tee -a "$CONSOLE_LOG" >&2) &
  QEMU_PID=$!
  sleep "$BOOT_WINDOW" || true
  if kill "$QEMU_PID" 2>/dev/null; then
    wait "$QEMU_PID"
    STATUS=0
  else
    wait "$QEMU_PID"
    STATUS=$?
  fi
fi
set -e

if [[ $STATUS -eq 0 ]]; then
  log_info "QEMU session completed within ${BOOT_WINDOW}s"
  exit 0
elif [[ $STATUS -eq 124 ]]; then
  log_info "QEMU timeout reached after ${BOOT_WINDOW}s; review console log at $CONSOLE_LOG"
  exit 0
else
  log_error "QEMU exited with status $STATUS. Inspect $CONSOLE_LOG and rebuild artefacts if necessary."
  exit "$STATUS"
fi
