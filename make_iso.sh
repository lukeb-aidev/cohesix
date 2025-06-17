// CLASSIFICATION: COMMUNITY
// Filename: make_iso.sh v0.2
// Author: Lukas Bower
// Date Modified: 2025-09-02
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
mkdir -p "$ISO_DIR/EFI/BOOT"

cp -f "$KERNEL_SRC" "$ISO_DIR/EFI/BOOT/bootx64.efi" || error "copy kernel"
[ -d "$ROOT/out/bin" ] && cp -a "$ROOT/out/bin" "$ISO_DIR/"
[ -d "$ROOT/out/etc" ] && cp -a "$ROOT/out/etc" "$ISO_DIR/"
[ -d "$ROOT/out/roles" ] && cp -a "$ROOT/out/roles" "$ISO_DIR/"
[ -d "$ROOT/out/setup" ] && cp -a "$ROOT/out/setup" "$ISO_DIR/"

xorriso -as mkisofs -R -o "$ISO_OUT" "$ISO_DIR"
