# CLASSIFICATION: COMMUNITY
#!/usr/bin/env bash
# Filename: collect_boot_logs.sh v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12

###############################################################################
# collect_boot_logs.sh – gather hardware boot logs
#
# Connects to reference devices (Jetson Orin Nano and Raspberry Pi 5) via SSH
# and retrieves /srv/boot.log and /trace/boot.log. The logs are saved under the
# local 'logs/' directory for CI artifact upload.
###############################################################################
set -euo pipefail

JETSON_HOST=${JETSON_HOST:?JETSON_HOST not set}
PI_HOST=${PI_HOST:?PI_HOST not set}

collect() {
  local host="$1"
  echo "Collecting logs from $host"
  ssh "$host" 'cat /srv/boot.log /trace/boot.log 2>/dev/null' \
    > "logs/${host}_boot.log"
}

mkdir -p logs
collect "$JETSON_HOST"
collect "$PI_HOST"

