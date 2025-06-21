# CLASSIFICATION: COMMUNITY
# Filename: test_sensor_world.sh v0.1
# Author: Lukas Bower
# Date Modified: 2026-02-11
#!/usr/bin/env bash
set -euo pipefail
mkdir -p /log/trace
bin/demo_sensor_world >/dev/null 2>&1
[ -s /log/trace/sensor_world.log ]

