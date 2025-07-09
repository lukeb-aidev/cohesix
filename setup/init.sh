# CLASSIFICATION: COMMUNITY
# Filename: init.sh v0.3
# Author: Lukas Bower
# Date Modified: 2027-12-01

set -euo pipefail
log(){ echo "[init] $1"; }

log "starting Cohesix userland init"
ROLE="$(cat /srv/cohrole 2>/dev/null || echo unknown)"
TELEMETRY="${COHTELEMETRY:-quiet}"
log "role=$ROLE telemetry=$TELEMETRY"

NS_FILE="/etc/plan9.ns"
if [ -f "$NS_FILE" ]; then
  log "loading namespace from $NS_FILE"
  while read -r cmd a b; do
    case "$cmd" in
      bind)
        if ! bind "$a" "$b"; then
          log "bind $a $b failed"
        fi;;
      srv)
        if ! srv "$a" "$b"; then
          log "srv $a $b failed"
        fi;;
    esac
  done < "$NS_FILE"
else
  log "missing $NS_FILE"
fi

required=(/bin /usr /tmp /srv /mnt /srv/cuda /srv/telemetry /srv/cohrole)
for p in "${required[@]}"; do
  if [ ! -e "$p" ]; then
    log "missing mount: $p"
  fi
done

if command -v rc >/dev/null 2>&1; then
  log "launching rc"
  echo "Cohesix shell started"
  exec rc
elif command -v sh >/dev/null 2>&1; then
  log "launching sh"
  echo "Cohesix shell started"
  exec sh
else
  log "no shell found"
  exit 1
fi
