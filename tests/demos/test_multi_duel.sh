# CLASSIFICATION: COMMUNITY
# Filename: test_multi_duel.sh v0.1
# Author: Lukas Bower
# Date Modified: 2026-02-11
#!/usr/bin/env bash
set -euo pipefail
LOG_DIR="${TMPDIR:-$(mktemp -d)}/trace"
mkdir -p "$LOG_DIR"
bin/demo_multi_duel >"$LOG_DIR/multi_duel.log" 2>&1
[ -s "$LOG_DIR/multi_duel.log" ]

