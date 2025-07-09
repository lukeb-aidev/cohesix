# CLASSIFICATION: COMMUNITY
# Filename: init.sh v0.2
# Author: Lukas Bower
# Date Modified: 2027-11-30

set -euo pipefail
log(){ echo "[init] $1"; }

log "starting Cohesix userland init"
ROLE="$(cat /srv/cohrole 2>/dev/null || echo unknown)"
log "role=$ROLE"

if [ -f /etc/plan9.ns ]; then
  log "loading namespace from /etc/plan9.ns"
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
  done < /etc/plan9.ns
else
  log "missing /etc/plan9.ns"
fi

if [ "$(grep -c /bin /etc/plan9.ns 2>/dev/null)" = 0 ]; then
  log "warning: /bin not bound"
fi

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
