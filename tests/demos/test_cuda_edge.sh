# CLASSIFICATION: COMMUNITY
# Filename: test_cuda_edge.sh v0.1
# Author: Lukas Bower
# Date Modified: 2026-02-11
#!/usr/bin/env bash
set -euo pipefail
mkdir -p /log/trace
bin/demo_cuda_edge >/dev/null 2>&1
[ -s /log/trace/cuda_edge.log ]

