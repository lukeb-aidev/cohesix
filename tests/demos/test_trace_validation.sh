# CLASSIFICATION: COMMUNITY
# Filename: test_trace_validation.sh v0.1
# Author: Lukas Bower
# Date Modified: 2026-02-11
#!/usr/bin/env bash
set -euo pipefail
mkdir -p /log/trace
bin/demo_trace_validation >/dev/null 2>&1
[ -s /log/trace/trace_validation.log ]

