# CLASSIFICATION: COMMUNITY
# Filename: test_physics_webcam.sh v0.1
# Author: Lukas Bower
# Date Modified: 2026-02-11
#!/usr/bin/env bash
set -euo pipefail
LOG_DIR="${TMPDIR:-$(mktemp -d)}/trace"
mkdir -p "$LOG_DIR"
bin/demo_physics_webcam >"$LOG_DIR/physics_webcam.log" 2>&1
[ -s "$LOG_DIR/physics_webcam.log" ]

