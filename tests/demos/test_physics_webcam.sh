# CLASSIFICATION: COMMUNITY
# Filename: test_physics_webcam.sh v0.1
# Author: Lukas Bower
# Date Modified: 2026-02-11
#!/usr/bin/env bash
set -euo pipefail
mkdir -p /log/trace
bin/demo_physics_webcam >/dev/null 2>&1
[ -s /log/trace/physics_webcam.log ]

