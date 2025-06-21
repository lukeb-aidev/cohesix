# CLASSIFICATION: COMMUNITY
# Filename: test_bee_learns.sh v0.1
# Author: Lukas Bower
# Date Modified: 2026-02-11
#!/usr/bin/env bash
set -euo pipefail
mkdir -p /log/trace
bin/demo_bee_learns >/dev/null 2>&1
[ -s /log/trace/bee_learns.log ]


