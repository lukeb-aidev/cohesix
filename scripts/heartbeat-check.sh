# CLASSIFICATION: COMMUNITY
# Filename: heartbeat-check.sh v0.2
# Date Modified: 2025-06-01
# Author: Lukas Bower

#!/usr/bin/env bash
###############################################################################
# heartbeat-check.sh – Cohesix watchdog
#
# Purpose
# -------
#   Continuously monitors a *heartbeat file* emitted by long‑running Cohesix
#   tasks (hydration workers, CI loops, etc.).  If the heartbeat file has not
#   been modified within the configured timeout, the script logs an error and
#   (optionally) executes a recovery command.
#
# Usage
# -----
#   ./scripts/heartbeat-check.sh /tmp/cohesix.heartbeat 300 \
#       --recover "systemctl restart cohesix-worker"
#
#   Argument order:
#     1. Path to heartbeat file (created by the monitored process)
#     2. Timeout in seconds
#
#   Optional flags:
#     --recover "<command>"   Command to run when heartbeat is stale
#     --log "<file>"          Append log output to <file> instead of stdout
#
# Exit codes:
#   0  Normal exit (never reached in watch mode)
#   1  Invalid arguments
#   2  Heartbeat file missing
#   3  Recovery command failed
###############################################################################
set -euo pipefail

usage() {
  grep -E '^#' "$0" | sed -E 's/^#[ ]?//'
  exit 1
}

log() {
  local ts msg
  ts="$(date '+%Y-%m-%d %H:%M:%S')"
  msg="$1"
  if [[ -n "${LOG_FILE:-}" ]]; then
    echo "[$ts] $msg" >> "$LOG_FILE"
  else
    echo -e "\e[34m[$ts]\e[0m $msg"
  fi
}

########## Argument parsing ###################################################
[[ $# -lt 2 ]] && usage
HB_FILE="$1"
TIMEOUT="$2"
shift 2

RECOVER_CMD=""
LOG_FILE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --recover)
      RECOVER_CMD="$2"; shift 2 ;;
    --log)
      LOG_FILE="$2"; shift 2 ;;
    *)
      usage ;;
  esac
done

[[ -f $HB_FILE ]] || { log "Heartbeat file not found: $HB_FILE"; exit 2; }

########## Main loop ##########################################################
log "Monitoring heartbeat: $HB_FILE (timeout = ${TIMEOUT}s)"
while true; do
  last_ts=$(stat -c "%Y" "$HB_FILE")
  now_ts=$(date +%s)
  delta=$(( now_ts - last_ts ))

  if (( delta > TIMEOUT )); then
    log "❌ Heartbeat stale ($delta s since last update)"
    if [[ -n $RECOVER_CMD ]]; then
      log "Attempting recovery: $RECOVER_CMD"
      if eval "$RECOVER_CMD"; then
        log "✅ Recovery command succeeded"
      else
        log "⚠️  Recovery command failed"; exit 3
      fi
    fi
  fi

  sleep "$(( TIMEOUT / 2 ))"
done
