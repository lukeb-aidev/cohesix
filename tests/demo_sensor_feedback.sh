#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: demo_sensor_feedback.sh v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12
# Demonstrate sensor feedback loop.
set -euo pipefail
mkdir -p /srv/sensors
PYTHONPATH=$(pwd)/python python3 -m sensors.sensor_proxy

