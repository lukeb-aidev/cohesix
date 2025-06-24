# CLASSIFICATION: COMMUNITY
# Filename: make_iso.sh v0.22
# Author: Lukas Bower
# Date Modified: 2026-08-05
#!/bin/bash
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR"
if [ ! -f "$ROOT/scripts/load_arch_config.sh" ]; then
    echo "❌ Missing load_arch_config.sh in $ROOT/scripts" >&2
    exit 1
fi
source "$ROOT/scripts/load_arch_config.sh"
case "$COHESIX_ARCH" in
    x86_64) COHESIX_TARGET="x86_64-unknown-linux-gnu";;
    aarch64) COHESIX_TARGET="aarch64-unknown-linux-gnu";;
    *) echo "Unsupported architecture: $COHESIX_ARCH" >&2; exit 1;;
esac
SEL4_WORKSPACE="${SEL4_WORKSPACE:-/home/ubuntu/sel4_workspace}"
echo "[INFO] Using kernel from: $SEL4_WORKSPACE"
echo "[INFO] Detected build arch: $COHESIX_ARCH"

mkdir -p "$ROOT/out/bin" "$ROOT/out/iso/boot"
KERNEL_BIN="$ROOT/out/bin/kernel.elf"
KERNEL_BOOT="$ROOT/out/iso/boot/kernel.elf"

if [ "$COHESIX_ARCH" = "x86_64" ]; then
    KERNEL_SRC="$SEL4_WORKSPACE/build_pc99/kernel/kernel.elf"
elif [ "$COHESIX_ARCH" = "aarch64" ]; then
    KERNEL_SRC="$SEL4_WORKSPACE/build_qemu_arm/kernel/kernel.elf"
fi
if [[ "$COHESIX_ARCH" != "x86_64" && "$COHESIX_ARCH" != "aarch64" ]]; then
    echo "❌ Unsupported architecture: $COHESIX_ARCH" >&2
    exit 1
fi

echo "[INFO] Kernel source: $KERNEL_SRC"
echo "[INFO] Checking kernel path: $KERNEL_SRC"
if [ ! -f "$KERNEL_SRC" ]; then
    echo "❌ Kernel ELF not found at $KERNEL_SRC. Did you run init-build.sh + ninja?" >&2
    ls -l "$SEL4_WORKSPACE"/build_* || true
    exit 1
fi
cp "$KERNEL_SRC" "$KERNEL_BIN"
cp "$KERNEL_SRC" "$KERNEL_BOOT" || {
    echo "❌ Failed to stage $KERNEL_SRC to $KERNEL_BOOT" >&2
    exit 1
}
if [[ ! -f "$KERNEL_BOOT" ]]; then
    echo "❌ Kernel ELF not staged at $KERNEL_BOOT" >&2
    exit 1
fi

exec bash "$SCRIPT_DIR/scripts/make_grub_iso.sh" "$@"
