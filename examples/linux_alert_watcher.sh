#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: linux_alert_watcher.sh v0.1
# Author: Lukas Bower
# Date Modified: 2026-12-31

set -euo pipefail

MNT=/mnt/cohesix
mkdir -p "$MNT"

if ! mountpoint -q "$MNT"; then
    9pfuse localhost "$MNT"
fi

echo "Watching alerts..."

inotifywait -m "$MNT/srv/alerts" -e create --format '%w%f' |
while read -r path; do
    cat "$path"
    echo "[simulated] upload $path to S3"
    rm "$path"
done
