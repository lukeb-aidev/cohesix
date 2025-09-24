#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: perf_log.sh v0.1
# Author: Lukas Bower
# Date Modified: 2029-01-26
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: tools/perf_log.sh [options]

Capture timing information for build and boot stages, wrapping existing scripts
such as scripts/boot_qemu.sh.

Options:
  -h, --help           Show this help message and exit
  --build-cmd CMD      Command to measure for the build stage
  --boot-cmd CMD       Command to measure for the boot stage (default: scripts/boot_qemu.sh if available)
  --skip-build         Skip the build stage entirely
  --skip-boot          Skip the boot stage entirely
  --log-file PATH      Write JSON summary to PATH (defaults to TMPDIR-aware path)
  --tag NAME           Tag to associate with the measurement (used for log names)
  --quiet              Reduce console output
USAGE
}

log() {
  if [[ ${QUIET:-0} -eq 0 ]]; then
    printf '[perf-log] %s\n' "$1"
  fi
}

warn() {
  printf '[perf-log][warn] %s\n' "$1" >&2
}

select_tmp_root() {
  local candidate
  for candidate in "${COHESIX_TRACE_TMP:-}" "${COHESIX_ENS_TMP:-}" "${TMPDIR:-}"; do
    if [[ -n "$candidate" ]]; then
      printf '%s' "$candidate"
      return 0
    fi
  done
  mktemp -d
}

if date +%s%3N >/dev/null 2>&1; then
  now_ms() {
    date +%s%3N
  }
else
  now_ms() {
    python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
  }
fi

ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT_DIR"

BUILD_CMD=""
BOOT_CMD=""
SKIP_BUILD=0
SKIP_BOOT=0
LOG_FILE=""
TAG="batch"
QUIET=0

while (($#)); do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    --build-cmd)
      if [[ $# -lt 2 ]]; then
        warn "--build-cmd requires a command string"
        exit 1
      fi
      BUILD_CMD="$2"
      shift 2
      ;;
    --boot-cmd)
      if [[ $# -lt 2 ]]; then
        warn "--boot-cmd requires a command string"
        exit 1
      fi
      BOOT_CMD="$2"
      shift 2
      ;;
    --skip-build)
      SKIP_BUILD=1
      shift
      ;;
    --skip-boot)
      SKIP_BOOT=1
      shift
      ;;
    --log-file)
      if [[ $# -lt 2 ]]; then
        warn "--log-file requires a path"
        exit 1
      fi
      LOG_FILE="$2"
      shift 2
      ;;
    --tag)
      if [[ $# -lt 2 ]]; then
        warn "--tag requires a value"
        exit 1
      fi
      TAG="$2"
      shift 2
      ;;
    --quiet)
      QUIET=1
      shift
      ;;
    --)
      shift
      break
      ;;
    -* )
      warn "Unknown option: $1"
      usage
      exit 1
      ;;
    *)
      warn "Unexpected positional argument: $1"
      usage
      exit 1
      ;;
  esac
done

if (( $# )); then
  warn "Unexpected arguments: $*"
  usage
  exit 1
fi

if [[ -z "$BOOT_CMD" && -x scripts/boot_qemu.sh ]]; then
  BOOT_CMD="scripts/boot_qemu.sh"
fi

TMP_ROOT="$(select_tmp_root)"
if [[ -n "$LOG_FILE" ]]; then
  mkdir -p "$(dirname "$LOG_FILE")"
  LOG_DIR="$(cd "$(dirname "$LOG_FILE")" && pwd)"
else
  mkdir -p "$TMP_ROOT/cohesix_perf_logs"
  LOG_DIR="$TMP_ROOT/cohesix_perf_logs"
  LOG_FILE="$LOG_DIR/perf_$(date +%Y%m%d_%H%M%S).json"
fi

TAG_SAFE="${TAG//[^A-Za-z0-9_.-]/_}"
BUILD_LOG_PATH="$LOG_DIR/${TAG_SAFE}_build.log"
BOOT_LOG_PATH="$LOG_DIR/${TAG_SAFE}_boot.log"

BUILD_STATUS="skipped"
BUILD_DURATION=""
BUILD_EXIT=""
BUILD_LOG=""

BOOT_STATUS="skipped"
BOOT_DURATION=""
BOOT_EXIT=""
BOOT_LOG=""

run_stage() {
  local stage="$1"
  local command="$2"
  local stage_log="$3"
  local upper
  upper="$(printf '%s' "$stage" | tr '[:lower:]' '[:upper:]')"
  local status_var="${upper}_STATUS"
  local duration_var="${upper}_DURATION"
  local exit_var="${upper}_EXIT"
  local log_var="${upper}_LOG"

  if [[ -z "$command" ]]; then
    eval "$status_var='skipped'"
    eval "$log_var=''"
    log "Skipping $stage stage (no command provided)"
    return 0
  fi

  : > "$stage_log"
  log "Starting $stage stage: $command"
  local start end exit_code
  start="$(now_ms)"
  set +e
  bash -lc "$command" |& tee "$stage_log"
  exit_code=${PIPESTATUS[0]:-0}
  set -e
  end="$(now_ms)"
  local duration=$(( end - start ))
  eval "$duration_var=$duration"
  eval "$exit_var=$exit_code"
  eval "$log_var=$stage_log"
  if (( exit_code == 0 )); then
    eval "$status_var='success'"
    log "$stage stage completed in ${duration} ms (log: $stage_log)"
  else
    eval "$status_var='failed'"
    warn "$stage stage failed after ${duration} ms (exit $exit_code, log: $stage_log)"
  fi
  return $exit_code
}

RC=0
if (( SKIP_BUILD )); then
  log "Build stage skipped by flag"
else
  if ! run_stage "build" "$BUILD_CMD" "$BUILD_LOG_PATH"; then
    RC=$?
  fi
fi

if (( SKIP_BOOT )); then
  log "Boot stage skipped by flag"
else
  if (( RC == 0 )); then
    if ! run_stage "boot" "$BOOT_CMD" "$BOOT_LOG_PATH"; then
      RC=$?
    fi
  else
    warn "Skipping boot stage because build failed"
  fi
fi

export PERF_TIMESTAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
export PERF_TAG="$TAG"
export PERF_WORKDIR="$ROOT_DIR"
export PERF_LOG_FILE="$LOG_FILE"
export PERF_BUILD_CMD="$BUILD_CMD"
export PERF_BUILD_STATUS="$BUILD_STATUS"
export PERF_BUILD_DURATION="$BUILD_DURATION"
export PERF_BUILD_EXIT="$BUILD_EXIT"
export PERF_BUILD_LOG="$BUILD_LOG"
export PERF_BOOT_CMD="$BOOT_CMD"
export PERF_BOOT_STATUS="$BOOT_STATUS"
export PERF_BOOT_DURATION="$BOOT_DURATION"
export PERF_BOOT_EXIT="$BOOT_EXIT"
export PERF_BOOT_LOG="$BOOT_LOG"

python3 - "$LOG_FILE" <<'PY'
import json
import os
import sys
from pathlib import Path

log_path = Path(sys.argv[1])

def parse_optional_int(value):
    if value and value.isdigit():
        return int(value)
    return None

payload = {
    "timestamp": os.environ["PERF_TIMESTAMP"],
    "tag": os.environ["PERF_TAG"],
    "working_dir": os.environ["PERF_WORKDIR"],
    "build": {
        "command": os.environ.get("PERF_BUILD_CMD") or None,
        "status": os.environ.get("PERF_BUILD_STATUS") or "skipped",
        "duration_ms": parse_optional_int(os.environ.get("PERF_BUILD_DURATION")),
        "exit_code": parse_optional_int(os.environ.get("PERF_BUILD_EXIT")),
        "log_path": os.environ.get("PERF_BUILD_LOG") or None,
    },
    "boot": {
        "command": os.environ.get("PERF_BOOT_CMD") or None,
        "status": os.environ.get("PERF_BOOT_STATUS") or "skipped",
        "duration_ms": parse_optional_int(os.environ.get("PERF_BOOT_DURATION")),
        "exit_code": parse_optional_int(os.environ.get("PERF_BOOT_EXIT")),
        "log_path": os.environ.get("PERF_BOOT_LOG") or None,
    },
}
log_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
PY

log "Performance summary written to $LOG_FILE"
exit $RC
