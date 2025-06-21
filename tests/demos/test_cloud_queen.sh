# CLASSIFICATION: COMMUNITY
# Filename: test_cloud_queen.sh v0.1
# Author: Lukas Bower
# Date Modified: 2026-02-11
#!/usr/bin/env bash
set -euo pipefail
mkdir -p /log/trace
bin/demo_cloud_queen >/dev/null 2>&1
[ -s /log/trace/cloud_queen.log ]

