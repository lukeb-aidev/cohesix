# CLASSIFICATION: COMMUNITY
# Filename: demo_edge_failover.sh v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12

#!/usr/bin/env bash
# Simulate queen disconnect and worker promotion.
set -euo pipefail
mkdir -p /srv/queen
: > /srv/queen/heartbeat
sleep 0.1
rm /srv/queen/heartbeat
mkdir -p /srv
./target/debug/cohrun orchestrator status >"${TMPDIR:-$(mktemp -d)}/edge_failover.log" 2>&1 || true
