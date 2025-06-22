# CLASSIFICATION: COMMUNITY
# Filename: make_iso.sh v0.12
# Author: Lukas Bower
# Date Modified: 2026-06-22
#!/bin/bash
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR"

ARCH="$(uname -m)"
mkdir -p "$ROOT/out/boot"
if [[ "$ARCH" == "x86_64" ]]; then
    echo "➡️ Using x86_64 kernel from build_pc99"
    cp /sel4_workspace/build_pc99/kernel/kernel.elf "$ROOT/out/boot/kernel.elf"
elif [[ "$ARCH" == "aarch64" ]]; then
    echo "➡️ Using aarch64 kernel from build_qemu_arm"
    cp /sel4_workspace/build_qemu_arm/kernel/kernel.elf "$ROOT/out/boot/kernel.elf"
else
    echo "❌ Unknown architecture: $ARCH"
    exit 1
fi

exec bash "$SCRIPT_DIR/scripts/make_grub_iso.sh" "$@"
