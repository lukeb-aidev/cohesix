# CLASSIFICATION: COMMUNITY
# Filename: test_secure_relay.sh v0.1
# Author: Lukas Bower
# Date Modified: 2026-02-11
#!/usr/bin/env bash
set -euo pipefail
mkdir -p /log/trace
bin/demo_secure_relay >/dev/null 2>&1
[ -s /log/trace/secure_relay.log ]

