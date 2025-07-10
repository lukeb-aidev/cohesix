# CLASSIFICATION: COMMUNITY
# Filename: init.sh v0.6
# Author: Lukas Bower
# Date Modified: 2027-12-06

set -euo pipefail
log(){ echo "[init] $1"; }

TS="$(date +%Y%m%d_%H%M%S)"
REPORT="/tmp/USERLAND_REPORT_$TS"
ln -sf "$REPORT" /tmp/USERLAND_REPORT
: > "$REPORT"
START_TS=$(date +%s)
DEBUG="${INIT_SH_DEBUG:-0}"
SKIP_CUDA="${INIT_SKIP_CUDA:-0}"
CUDA_SEEN=0
TELEMETRY_SEEN=0
SECURE9P_SEEN=0
if [ "$DEBUG" -eq 1 ]; then
  set -x
fi

log "starting Cohesix userland init"
ROLE="$(cat /srv/cohrole 2>/dev/null || echo unknown)"
TELEMETRY="${COHTELEMETRY:-quiet}"
log "role=$ROLE telemetry=$TELEMETRY"
echo "role=$ROLE" >> "$REPORT"
echo "telemetry=$TELEMETRY" >> "$REPORT"
echo "debug=$DEBUG skip_cuda=$SKIP_CUDA" >> "$REPORT"

NS_FILE="/etc/plan9.ns"
if [ -f "$NS_FILE" ]; then
  log "loading namespace from $NS_FILE"
  while IFS= read -r line; do
    line="${line%%#*}"
    [ -z "$(echo "$line" | tr -d '[:space:]')" ] && continue
    set -- $line
    cmd="$1"; shift
    case "$cmd" in
      bind)
        if ! bind "$@"; then
          log "bind $* failed"
        fi;;
      srv)
        if ! srv "$@"; then
          log "srv $* failed"
        fi;;
      srv?)
        name="$1"; dest="$2"
        if [ -e "$dest" ]; then
          if ! srv "$name" "$dest"; then
            log "srv $name $dest failed"
          fi
        else
          log "optional service $name not present"
        fi;;
    esac
  done < "$NS_FILE"
else
  log "missing $NS_FILE"
fi


BOOT_OK=1
required=(/bin /usr /tmp /srv /mnt)
for p in "${required[@]}"; do
  if [ ! -e "$p" ]; then
    log "missing mount: $p"
    BOOT_OK=0
  fi
done

if [ ! -e /srv/cohrole ]; then
  log "missing /srv/cohrole"
  BOOT_OK=0
else
  log "cohrole present"
  echo "cohrole=present" >> "$REPORT"
fi

if [ -e /srv/cuda ]; then
  log "cuda service available"
  echo "cuda=present" >> "$REPORT"
  CUDA_SEEN=1
else
  if [ "$SKIP_CUDA" -eq 1 ]; then
    log "cuda service missing but skipping check"
    echo "cuda=skipped" >> "$REPORT"
  else
    log "cuda service missing"
    echo "cuda=missing" >> "$REPORT"
  fi
fi

if [ -e /srv/telemetry ]; then
  log "telemetry service available"
  echo "telemetry_srv=present" >> "$REPORT"
  TELEMETRY_SEEN=1
else
  log "telemetry service missing; disabling"
  TELEMETRY="disabled"
  echo "telemetry_srv=missing" >> "$REPORT"
fi

if [ -e /srv/secure9p ]; then
  log "secure9p available"
  echo "secure9p=present" >> "$REPORT"
  SECURE9P_SEEN=1
else
  log "secure9p service missing"
  echo "secure9p=missing" >> "$REPORT"
fi

# snapshot current srv and mnt state for trace replay
srv_list=$(ls /srv 2>/dev/null | tr '\n' ' ')
mnt_list=$(ls /mnt 2>/dev/null | tr '\n' ' ')
printf '{"srv":"%s","mnt":"%s"}' "$srv_list" "$mnt_list" > /tmp/BOOT_ENV.json

if command -v free >/dev/null 2>&1; then
  free -h >> "$REPORT"
elif [ -f /proc/meminfo ]; then
  grep -E 'MemAvailable|MemFree' /proc/meminfo >> "$REPORT"
fi
if command -v cohtrace >/dev/null 2>&1; then
  cohtrace dump heap_offset >> "$REPORT" 2>&1 || cohtrace dump >> "$REPORT" 2>&1
fi

if [ "$BOOT_OK" -eq 1 ]; then
  touch /tmp/BOOT_OK
  echo "status=ok" >> "$REPORT"
  mkdir -p /history/pivot_runs
  cat "$REPORT" >> "/history/pivot_runs/${TS}.log"
else
  echo "boot failed" > /tmp/BOOT_FAIL
  echo "status=fail" >> "$REPORT"
fi

# periodic check for critical services
( while true; do
  sleep 30
  now=$(date +%s)
  if [ "$CUDA_SEEN" -eq 1 ] && [ ! -e /srv/cuda ]; then
    delta=$((now-START_TS))
    echo "warn:cuda_lost_${delta}s" >> "$REPORT"
    CUDA_SEEN=0
  fi
  if [ "$TELEMETRY_SEEN" -eq 1 ] && [ ! -e /srv/telemetry ]; then
    delta=$((now-START_TS))
    echo "warn:telemetry_lost_${delta}s" >> "$REPORT"
    TELEMETRY_SEEN=0
  fi
  if [ "$SECURE9P_SEEN" -eq 1 ] && [ ! -e /srv/secure9p ]; then
    delta=$((now-START_TS))
    echo "warn:secure9p_lost_${delta}s" >> "$REPORT"
    SECURE9P_SEEN=0
  fi
done ) &

if command -v rc >/dev/null 2>&1; then
  log "launching rc"
  echo "Cohesix shell started"
  echo "shell=rc" >> "$REPORT"
  exec rc
elif command -v sh >/dev/null 2>&1; then
  log "launching sh"
  echo "Cohesix shell started"
  echo "shell=sh" >> "$REPORT"
  exec sh
else
  log "no shell found"
  exit 1
fi
