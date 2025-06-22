# CLASSIFICATION: COMMUNITY
# Filename: make_iso.sh v0.13
# Author: Lukas Bower
# Date Modified: 2026-06-23
#!/bin/bash
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR"

ARCH="$(uname -m)"
if [[ ! -f /sel4_workspace/build_pc99/kernel/kernel.elf && ! -f /sel4_workspace/build_qemu_arm/kernel/kernel.elf ]]; then
    echo "ERROR: Missing /sel4_workspace. Run the official seL4 setup and build for x86_64 or aarch64 before continuing."
    exit 1
fi
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
