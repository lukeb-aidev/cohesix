# CLASSIFICATION: COMMUNITY
# Filename: send-heartbeat.sh v0.2
# Date Modified: 2025-06-17
# Author: Lukas Bower

#!/usr/bin/env bash
###############################################################################
# send-heartbeat.sh – Cohesix helper
#
# Periodically updates a heartbeat file to satisfy the watchdog
# monitoring process. The interval defaults to the environment
# variable HEARTBEAT_INTERVAL (seconds) or 300 if unset.
#
# Usage:
#   ./scripts/send-heartbeat.sh /tmp/cohesix.heartbeat [interval]
#
# Arguments:
#   1. Path to heartbeat file
#   2. Optional interval in seconds (overrides HEARTBEAT_INTERVAL)
###############################################################################
set -euo pipefail

usage() {
  grep -E '^#' "$0" | sed -E 's/^#[ ]?//'
  exit 1
}

[[ $# -lt 1 ]] && usage

HB_FILE="$1"
INTERVAL="${2:-${HEARTBEAT_INTERVAL:-300}}"

log() {
  local ts
  ts="$(date '+%Y-%m-%d %H:%M:%S')"
  echo "[$ts] $1" >> "${LOG_FILE:-/dev/stdout}"
}

log "Starting heartbeat to $HB_FILE every ${INTERVAL}s"
while true; do
  touch "$HB_FILE"
  log "pulse"
  sleep "$INTERVAL"
done


