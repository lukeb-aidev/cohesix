// CLASSIFICATION: COMMUNITY
// Filename: build_busybox.sh v0.1
// Date Modified: 2025-06-18
// Author: Lukas Bower
#!/usr/bin/env bash

set -e

if [ ! -d busybox ]; then
    git clone https://github.com/mirror/busybox
fi
cd busybox
make defconfig >/dev/null
sed -i 's/.*CONFIG_STATIC.*/CONFIG_STATIC=y/' .config
CROSS_COMPILE=aarch64-linux-gnu- make -j4 >/dev/null
mkdir -p /mnt/firmware
cp busybox /mnt/firmware/busybox

