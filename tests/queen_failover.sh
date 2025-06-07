#!/bin/sh
# CLASSIFICATION: COMMUNITY
# Filename: queen_failover.sh v0.1
# Date Modified: 2025-07-08
# Author: Lukas Bower

set -e
rm -f /srv/queen/primary_up
export COHROLE=QueenBackup
timeout 2 python3 - <<'PY'
import os, time
if os.environ.get('COHROLE') == 'QueenBackup' and not os.path.exists('/srv/queen/primary_up'):
    time.sleep(1)
    open('/srv/queen/role','w').write('QueenPrimary')
PY
[ "$(cat /srv/queen/role)" = "QueenPrimary" ]
