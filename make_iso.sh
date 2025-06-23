# CLASSIFICATION: COMMUNITY
# Filename: make_iso.sh v0.18
# Author: Lukas Bower
# Date Modified: 2026-07-23
#!/bin/bash
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR"
SEL4_WORKSPACE="${SEL4_WORKSPACE:-/home/ubuntu/sel4_workspace}"
echo "Using kernel from: $SEL4_WORKSPACE"

ARCH="$(uname -m)"
echo "Detected build arch: $ARCH"

mkdir -p "$ROOT/out/bin" "$ROOT/out/iso/boot"

case "$ARCH" in
    x86_64)
        KERNEL_SRC="$SEL4_WORKSPACE/build_pc99/kernel/kernel.elf"
        ;;
    aarch64)
        KERNEL_SRC="$SEL4_WORKSPACE/build_qemu_arm/kernel/kernel.elf"
        ;;
    *)
        echo "❌ Unknown architecture: $ARCH"
        exit 1
        ;;
esac

echo "ℹ️ Kernel source: $KERNEL_SRC"
if [ ! -f "$KERNEL_SRC" ]; then
    echo "❌ Kernel ELF not found at $KERNEL_SRC. Did you run init-build.sh + ninja?" >&2
    ls -l "$SEL4_WORKSPACE"/build_* || true
    exit 1
fi
cp "$KERNEL_SRC" "$ROOT/out/bin/kernel.elf"
cp "$KERNEL_SRC" "$ROOT/out/iso/boot/kernel.elf"
if [[ ! -f "$ROOT/out/iso/boot/kernel.elf" ]]; then
    echo "❌ Kernel ELF not staged at $ROOT/out/iso/boot/kernel.elf" >&2
    exit 1
fi

exec bash "$SCRIPT_DIR/scripts/make_grub_iso.sh" "$@"
