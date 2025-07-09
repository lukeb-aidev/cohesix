# CLASSIFICATION: COMMUNITY
# Filename: init.sh v0.5
# Author: Lukas Bower
# Date Modified: 2027-12-05

set -euo pipefail
log(){ echo "[init] $1"; }

TS="$(date +%Y%m%d_%H%M%S)"
REPORT="/tmp/USERLAND_REPORT_$TS"
ln -sf "$REPORT" /tmp/USERLAND_REPORT
: > "$REPORT"

log "starting Cohesix userland init"
ROLE="$(cat /srv/cohrole 2>/dev/null || echo unknown)"
TELEMETRY="${COHTELEMETRY:-quiet}"
log "role=$ROLE telemetry=$TELEMETRY"
echo "role=$ROLE" >> "$REPORT"
echo "telemetry=$TELEMETRY" >> "$REPORT"

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
else
  log "cuda service missing"
  echo "cuda=missing" >> "$REPORT"
fi

if [ -e /srv/telemetry ]; then
  log "telemetry service available"
  echo "telemetry_srv=present" >> "$REPORT"
else
  log "telemetry service missing; disabling"
  TELEMETRY="disabled"
  echo "telemetry_srv=missing" >> "$REPORT"
fi

# snapshot current srv and mnt state for trace replay
srv_list=$(ls /srv 2>/dev/null | tr '\n' ' ')
mnt_list=$(ls /mnt 2>/dev/null | tr '\n' ' ')
printf '{"srv":"%s","mnt":"%s"}' "$srv_list" "$mnt_list" > /tmp/BOOT_ENV.json

if [ "$BOOT_OK" -eq 1 ]; then
  touch /tmp/BOOT_OK
  echo "status=ok" >> "$REPORT"
else
  echo "boot failed" > /tmp/BOOT_FAIL
  echo "status=fail" >> "$REPORT"
fi

# periodic check for critical services
( while true; do
  sleep 30
  if [ ! -e /srv/cuda ]; then
    echo "warn:cuda_missing" >> "$REPORT"
  fi
  if [ ! -e /srv/telemetry ]; then
    echo "warn:telemetry_missing" >> "$REPORT"
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
