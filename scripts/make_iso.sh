// CLASSIFICATION: COMMUNITY
// Filename: scripts/make_iso.sh v0.1
// Author: Lukas Bower
// Date Modified: 2025-09-07

#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
ISO_DIR="$ROOT/out/iso_root"
ISO_OUT="$ROOT/out/cohesix.iso"
KERNEL_SRC="$ROOT/out/kernel.efi"
INIT_SRC="$ROOT/out/bin/init.efi"

error() {
  echo "Error: $1" >&2
  exit 1
}

if command -v xorriso >/dev/null 2>&1; then
  MKISO=xorriso
elif command -v mkisofs >/dev/null 2>&1; then
  MKISO=mkisofs
else
  error "xorriso or mkisofs required"
fi

[ -f "$KERNEL_SRC" ] || error "Missing $KERNEL_SRC"
[ -f "$INIT_SRC" ] || error "Missing $INIT_SRC"

rm -rf "$ISO_DIR"
mkdir -p "$ISO_DIR/EFI/BOOT" "$ISO_DIR/bin" "$ISO_DIR/etc/cohesix" "$ISO_DIR/roles"

cp "$KERNEL_SRC" "$ISO_DIR/EFI/BOOT/bootx64.efi"
cp "$KERNEL_SRC" "$ISO_DIR/kernel.efi"
cp "$INIT_SRC" "$ISO_DIR/bin/init.efi"

if [ -f "$ROOT/out/etc/cohesix/config.yaml" ]; then
  cp "$ROOT/out/etc/cohesix/config.yaml" "$ISO_DIR/etc/cohesix/config.yaml"
fi
if [ -d "$ROOT/out/roles" ]; then
  cp -a "$ROOT/out/roles/." "$ISO_DIR/roles/"
fi

$MKISO -as mkisofs -R -J -no-emul-boot -eltorito-boot EFI/BOOT/bootx64.efi \
  -boot-load-size 4 -boot-info-table -o "$ISO_OUT" "$ISO_DIR"
