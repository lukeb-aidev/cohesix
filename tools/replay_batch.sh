#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: replay_batch.sh v0.1
# Author: Lukas Bower
# Date Modified: 2029-01-26
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: tools/replay_batch.sh [options] <hydration-log> [hydration-log ...]

Replay Cohesix hydration logs and reproduce the recorded actions.

Options:
  -h, --help           Show this help message and exit
  --dry-run            Print the actions without executing them
  --base-dir PATH      Resolve relative log paths from PATH before applying entries
  --log-file PATH      Write script output to PATH (defaults to TMPDIR-aware path)
USAGE
}

log() {
  printf '[replay-batch] %s\n' "$1"
}

warn() {
  printf '[replay-batch][warn] %s\n' "$1" >&2
}

ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT_DIR"

DRY_RUN=0
BASE_DIR=""
LOG_FILE=""
LOG_INPUTS=()

while (($#)); do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --base-dir)
      if [[ $# -lt 2 ]]; then
        warn "--base-dir requires a directory argument"
        exit 1
      fi
      BASE_DIR="$2"
      shift 2
      ;;
    --log-file)
      if [[ $# -lt 2 ]]; then
        warn "--log-file requires a path argument"
        exit 1
      fi
      LOG_FILE="$2"
      shift 2
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
      LOG_INPUTS+=("$1")
      shift
      ;;
  esac
done

if (($#)); then
  for arg in "$@"; do
    LOG_INPUTS+=("$arg")
  done
fi

if [[ ${#LOG_INPUTS[@]} -eq 0 ]]; then
  warn "No hydration logs provided"
  usage
  exit 1
fi

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

TMP_ROOT="$(select_tmp_root)"
if [[ -n "$LOG_FILE" ]]; then
  mkdir -p "$(dirname "$LOG_FILE")"
else
  mkdir -p "$TMP_ROOT/cohesix_batch_logs"
  LOG_FILE="$TMP_ROOT/cohesix_batch_logs/replay_batch_$(date +%Y%m%d_%H%M%S).log"
fi

exec > >(tee "$LOG_FILE") 2>&1
log "Transcript: $LOG_FILE"

if [[ -n "$BASE_DIR" ]]; then
  if [[ ! -d "$BASE_DIR" ]]; then
    warn "Base directory $BASE_DIR does not exist"
    exit 1
  fi
  BASE_DIR="$(cd "$BASE_DIR" && pwd)"
fi

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

execute_command() {
  local context="$1"
  local -n env_map="$2"
  local cwd="$3"
  shift 3
  local -a cmd=("$@")
  local -a env_assignments=()
  local key
  for key in "${!env_map[@]}"; do
    env_assignments+=("$key=${env_map[$key]}")
  done
  local start
  start="$(now_ms)"
  if ((DRY_RUN)); then
    log "DRY-RUN $context :: ${cmd[*]}"
    return 0
  fi
  (
    cd "$cwd"
    if [[ ${#env_assignments[@]} -gt 0 ]]; then
      env "${env_assignments[@]}" "${cmd[@]}"
    else
      "${cmd[@]}"
    fi
  )
  local status=$?
  local end
  end="$(now_ms)"
  if (( status != 0 )); then
    warn "Command failed ($context) :: ${cmd[*]}"
    exit $status
  fi
  log "Executed $context in $(( end - start )) ms :: ${cmd[*]}"
}

process_log() {
  local log_path="$1"
  local resolved="$log_path"
  if [[ ! -f "$resolved" && -n "$BASE_DIR" ]]; then
    resolved="$BASE_DIR/$log_path"
  fi
  if [[ ! -f "$resolved" ]]; then
    warn "Missing hydration log: $log_path"
    return 1
  fi
  resolved="$(cd "$(dirname "$resolved")" && pwd)/$(basename "$resolved")"
  log "Processing $resolved"
  local current_cwd="$ROOT_DIR"
  if [[ -n "$BASE_DIR" ]]; then
    current_cwd="$BASE_DIR"
  fi
  declare -A env_vars=()
  local actions=0
  local line_no=0
  while IFS= read -r raw_line || [[ -n "$raw_line" ]]; do
    line_no=$(( line_no + 1 ))
    local line="${raw_line%$'\r'}"
    [[ -z "$line" ]] && continue
    [[ "$line" =~ ^# ]] && continue
    IFS='|' read -r -a parts <<< "$line"
    local action="${parts[0]}"
    case "$action" in
      CWD)
        if [[ ${#parts[@]} -lt 2 ]]; then
          warn "$resolved:$line_no Missing CWD target"
          continue
        fi
        local new_cwd="${parts[1]}"
        if [[ "$new_cwd" != /* ]]; then
          new_cwd="$current_cwd/$new_cwd"
        fi
        if [[ ! -d "$new_cwd" ]]; then
          warn "$resolved:$line_no Directory not found: $new_cwd"
          exit 1
        fi
        current_cwd="$new_cwd"
        log "Changed directory -> $current_cwd"
        ;;
      ENV)
        if [[ ${#parts[@]} -lt 3 ]]; then
          warn "$resolved:$line_no Missing ENV arguments"
          continue
        fi
        env_vars["${parts[1]}"]="${parts[2]}"
        log "Set ENV ${parts[1]}"
        ;;
      RUN)
        if [[ ${#parts[@]} -lt 2 ]]; then
          warn "$resolved:$line_no RUN requires at least one argument"
          continue
        fi
        local -a cmd=("${parts[@]:1}")
        actions=$(( actions + 1 ))
        execute_command "$resolved:$line_no" env_vars "$current_cwd" "${cmd[@]}"
        ;;
      COPY)
        if [[ ${#parts[@]} -lt 3 ]]; then
          warn "$resolved:$line_no COPY requires source and destination"
          continue
        fi
        local src="${parts[1]}"
        local dst="${parts[2]}"
        [[ "$src" != /* ]] && src="$current_cwd/$src"
        [[ "$dst" != /* ]] && dst="$current_cwd/$dst"
        if [[ ! -f "$src" ]]; then
          warn "$resolved:$line_no Source not found: $src"
          exit 1
        fi
        if ((DRY_RUN)); then
          log "DRY-RUN $resolved:$line_no :: COPY $src -> $dst"
        else
          mkdir -p "$(dirname "$dst")"
          cp "$src" "$dst"
          log "Copied $src -> $dst"
        fi
        actions=$(( actions + 1 ))
        ;;
      SLEEP)
        if [[ ${#parts[@]} -lt 2 ]]; then
          warn "$resolved:$line_no SLEEP requires a duration"
          continue
        fi
        local duration="${parts[1]}"
        actions=$(( actions + 1 ))
        if ((DRY_RUN)); then
          log "DRY-RUN $resolved:$line_no :: SLEEP $duration"
        else
          sleep "$duration"
          log "Slept for $duration (s)"
        fi
        ;;
      *)
        warn "$resolved:$line_no Unknown action '$action'"
        ;;
    esac
  done < "$resolved"
  log "Completed $resolved with $actions action(s)"
}

total_actions=0
for hydration_log in "${LOG_INPUTS[@]}"; do
  process_log "$hydration_log"
  (( total_actions += 1 ))
done

log "Processed ${#LOG_INPUTS[@]} log file(s); output captured at $LOG_FILE"
