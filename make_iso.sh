# CLASSIFICATION: COMMUNITY
# Filename: make_iso.sh v0.14
# Author: Lukas Bower
# Date Modified: 2026-06-30
#!/bin/bash
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR"

ARCH="$(uname -m)"
if [[ ! -f /sel4_workspace/build_pc99/kernel/kernel.elf && ! -f /sel4_workspace/build_qemu_arm/kernel/kernel.elf ]]; then
    echo "ERROR: Missing /sel4_workspace. Run the official seL4 setup and build for x86_64 or aarch64 before continuing."
    exit 1
fi

mkdir -p "$ROOT/out/bin" "$ROOT/out/iso/boot"

case "$ARCH" in
    x86_64)
        KERNEL_SRC="/sel4_workspace/build_pc99/kernel/kernel.elf"
        ;;
    aarch64)
        KERNEL_SRC="/sel4_workspace/build_qemu_arm/kernel/kernel.elf"
        ;;
    *)
        echo "❌ Unknown architecture: $ARCH"
        exit 1
        ;;
esac

echo "ℹ️ Kernel source: $KERNEL_SRC"
[ -f "$KERNEL_SRC" ] || { echo "Missing kernel at $KERNEL_SRC" >&2; exit 1; }
cp "$KERNEL_SRC" "$ROOT/out/bin/kernel.elf"
cp "$KERNEL_SRC" "$ROOT/out/iso/boot/kernel.elf"

exec bash "$SCRIPT_DIR/scripts/make_grub_iso.sh" "$@"
