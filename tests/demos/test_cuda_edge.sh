# CLASSIFICATION: COMMUNITY
# Filename: test_cuda_edge.sh v0.1
# Author: Lukas Bower
# Date Modified: 2026-02-11
#!/usr/bin/env bash
set -euo pipefail
LOG_DIR="${TMPDIR:-$(mktemp -d)}/trace"
mkdir -p "$LOG_DIR"
bin/demo_cuda_edge >"$LOG_DIR/cuda_edge.log" 2>&1
[ -s "$LOG_DIR/cuda_edge.log" ]

