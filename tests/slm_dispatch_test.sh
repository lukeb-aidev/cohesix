#!/bin/sh
# CLASSIFICATION: COMMUNITY
# Filename: slm_dispatch_test.sh v0.1
# Date Modified: 2025-07-08
# Author: Lukas Bower

set -e
mkdir -p /srv/slm/dispatch/worker1
python3 cli/cohcli.py dispatch-slm --target worker1 --model demo
[ -f /srv/slm/dispatch/worker1/demo.req ]
