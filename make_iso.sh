# CLASSIFICATION: COMMUNITY
# Filename: make_iso.sh v0.3
# Author: Lukas Bower
# Date Modified: 2025-09-03
#!/bin/sh
set -eu

ROOT="$(cd "$(dirname "$0")" && pwd)"
ISO_DIR="$ROOT/out_iso"
KERNEL_SRC="$ROOT/out/kernel.efi"
ISO_OUT="$ROOT/out/cohesix.iso"

error() {
  echo "Error: $1" >&2
  exit 1
}

command -v xorriso >/dev/null 2>&1 || error "xorriso not found"

[ -f "$KERNEL_SRC" ] || error "Missing kernel EFI at $KERNEL_SRC"

rm -rf "$ISO_DIR"
mkdir -p "$ISO_DIR/EFI/BOOT" "$ISO_DIR/etc/cohesix" "$ISO_DIR/roles"

cp -f "$KERNEL_SRC" "$ISO_DIR/EFI/BOOT/bootx64.efi" || error "copy kernel"
cp -f "$KERNEL_SRC" "$ISO_DIR/kernel.efi" || error "copy kernel root"
[ -f "$ISO_DIR/kernel.efi" ] || error "kernel.efi missing"
[ -f "$ISO_DIR/EFI/BOOT/bootx64.efi" ] || error "bootx64.efi missing"
[ -d "$ROOT/out/bin" ] && cp -a "$ROOT/out/bin" "$ISO_DIR/"
[ -f "$ROOT/out/etc/cohesix/config.yaml" ] && \
  cp "$ROOT/out/etc/cohesix/config.yaml" "$ISO_DIR/etc/cohesix/config.yaml"
[ -d "$ROOT/out/roles" ] && cp -a "$ROOT/out/roles/." "$ISO_DIR/roles/"
[ -f "$ISO_DIR/etc/cohesix/config.yaml" ] || error "config.yaml missing in ISO"
[ -d "$ROOT/out/setup" ] && cp -a "$ROOT/out/setup" "$ISO_DIR/"

xorriso -as mkisofs -R -o "$ISO_OUT" "$ISO_DIR"
