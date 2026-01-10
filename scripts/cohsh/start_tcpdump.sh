#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Start a tcpdump capture for Cohesix TCP console traffic.

set -euo pipefail

LOG_ROOT="${LOG_ROOT:-logs}"
IFACE="${IFACE:-lo0}"
PORT="${PORT:-31337}"
TS=$(date +%Y%m%d-%H%M%S)
LOG_FILE="${LOG_ROOT}/tcpdump-${TS}.log"
ERR_FILE="${LOG_ROOT}/tcpdump-${TS}.err"
PID_FILE="${LOG_ROOT}/tcpdump-${TS}.pid"

mkdir -p "$LOG_ROOT"

sudo tcpdump -n -tt -l -i "$IFACE" "tcp port ${PORT}" > "$LOG_FILE" 2> "$ERR_FILE" &
echo $! > "$PID_FILE"

echo "tcpdump started"
echo "pid: ${PID_FILE}"
echo "log: ${LOG_FILE}"
echo "err: ${ERR_FILE}"
