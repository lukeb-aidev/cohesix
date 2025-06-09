#!/bin/sh
# CLASSIFICATION: COMMUNITY
# Filename: slm_dispatch_agent.sh v0.2
# Author: Lukas Bower
# Date Modified: 2025-07-22
# SPDX-License-Identifier: Apache-2.0
# SLM Action: dispatch
# Target: agent

set -e
mkdir -p /srv/slm/dispatch/worker1
python3 cli/cohcli.py dispatch-slm --target worker1 --model demo
[ -f /srv/slm/dispatch/worker1/demo.req ]
