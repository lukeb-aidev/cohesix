// CLASSIFICATION: COMMUNITY
// Filename: make_iso.sh v0.1
// Author: Lukas Bower
// Date Modified: 2025-08-28
#!/bin/sh
set -eu

ROOT="$(cd "$(dirname "$0")" && pwd)"
ISO_DIR="$ROOT/out_iso"
EFI_DIR="$ISO_DIR/EFI/BOOT"
BIN_DIR="$ISO_DIR/bin"
ETC_DIR="$ISO_DIR/etc"
KERNEL_SRC="$ROOT/target/x86_64-unknown-uefi/release/kernel.efi"
ISO_OUT="$ROOT/out/cohesix.iso"

error() {
  echo "Error: $1" >&2
  exit 1
}

command -v xorriso >/dev/null 2>&1 || error "xorriso not found"

[ -f "$KERNEL_SRC" ] || error "Missing kernel EFI at $KERNEL_SRC"

mkdir -p "$EFI_DIR" "$BIN_DIR" "$ETC_DIR" "$ROOT/out"

cp -f "$KERNEL_SRC" "$EFI_DIR/bootx64.efi"

user_bins_found=0
for f in "$ROOT"/target/x86_64-unknown-uefi/release/*.efi; do
  [ -e "$f" ] || break
  base="$(basename "$f")"
  [ "$base" = "kernel.efi" ] && continue
  mv -f "$f" "$BIN_DIR/$base"
  user_bins_found=1
done

if [ $user_bins_found -eq 0 ] && ! ls "$BIN_DIR"/*.efi >/dev/null 2>&1; then
  error "No userland EFI binaries found"
fi

CONFIG_SRC="$ROOT/etc/init.conf"
[ -f "$CONFIG_SRC" ] && cp -f "$CONFIG_SRC" "$ETC_DIR/"

xorriso -as mkisofs -R -o "$ISO_OUT" "$ISO_DIR"
