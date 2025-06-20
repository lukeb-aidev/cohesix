# CLASSIFICATION: COMMUNITY
# Filename: scripts/make_iso.sh v0.7
# Author: Lukas Bower
# Date Modified: 2025-12-18
#!/bin/bash
# ISO layout:
#   bin/               - runtime binaries
#   usr/bin/           - CLI wrappers and Go tools
#   usr/cli/           - Python CLI modules
#   home/cohesix/      - Python libraries
#   etc/               - system configuration
#   roles/             - role definitions
#   userland/          - minimal shell utilities
#   usr/src/           - test sources
#   tmp/               - writable temp space
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
ISO_DIR="$ROOT/out/iso_root"
ISO_OUT="$ROOT/out/cohesix.iso"
TARGET="${TARGET:-$(uname -m)}"
KERNEL_SRC="$ROOT/out/boot/kernel.elf"
INIT_SRC="$ROOT/out/boot/init"
BOOTLOADER_SRC=""

log() { echo "$(date +%H:%M:%S) $1"; }
error() { echo "Error: $1" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

case "$TARGET" in
  aarch64*|arm64*) EFI_NAME="BOOTAA64.EFI" ;;
  x86_64*) EFI_NAME="BOOTX64.EFI" ;;
  *) error "Unsupported TARGET: $TARGET" ;;
esac
BOOTLOADER_SRC="$ROOT/out/$EFI_NAME"

if have xorriso; then
  MKISO=(xorriso -as mkisofs)
elif have grub-mkrescue; then
  MKISO=(grub-mkrescue -o)
elif have mkisofs; then
  MKISO=(mkisofs)
else
  log "ISO tools missing"
  echo "ISO Build SKIPPED (tool not found)"
  exit 0
fi
if ! have mformat; then
  log "mtools missing"
  echo "ISO Build SKIPPED (tool not found)"
  exit 0
fi

[ -f "$BOOTLOADER_SRC" ] || {
  KERNEL_EFI="$ROOT/target/${TARGET}-unknown-uefi/release/kernel.efi"
  if [ -f "$KERNEL_EFI" ]; then
    log "Generating $BOOTLOADER_SRC from kernel EFI"
    objcopy --target=efi-app-$TARGET "$KERNEL_EFI" "$BOOTLOADER_SRC" \
      || error "Failed to create $BOOTLOADER_SRC"
  else
    error "Missing $BOOTLOADER_SRC and $KERNEL_EFI"
  fi
}
[ -f "$KERNEL_SRC" ] || error "Missing $KERNEL_SRC"
[ -x "$INIT_SRC" ] || error "Missing $INIT_SRC"

rm -rf "$ISO_DIR"
mkdir -p "$ISO_DIR"/{bin,usr/bin,usr/cli,home/cohesix,etc/cohesix,roles,userland,usr/src,tmp,srv,boot/efi/EFI/BOOT}
chmod 1777 "$ISO_DIR/tmp"

log "📦 Adding bootloader..."
cp "$BOOTLOADER_SRC" "$ISO_DIR/boot/efi/EFI/BOOT/$EFI_NAME"
log "📦 Adding kernel..."
cp "$KERNEL_SRC" "$ISO_DIR/kernel.elf"
log "📦 Adding init..."
cp "$INIT_SRC" "$ISO_DIR/init"

# Runtime binaries compiled during build
if [ -d "$ROOT/out/bin" ]; then
  cp -a "$ROOT/out/bin/." "$ISO_DIR/bin/"
fi

# Python CLI modules
if [ -d "$ROOT/cli" ]; then
  cp "$ROOT"/cli/*.py "$ISO_DIR/usr/cli/"
fi

# CLI wrappers and helper scripts
for tool in cohcli cohcap cohtrace cohrun cohbuild cohup cohpkg; do
  if [ -f "$ROOT/bin/$tool" ]; then
    cp "$ROOT/bin/$tool" "$ISO_DIR/usr/bin/$tool"
    chmod +x "$ISO_DIR/usr/bin/$tool"
  fi
done
ln -sf cohcli "$ISO_DIR/usr/bin/cohesix"

# Ensure python3 is discoverable
ln -sf /usr/bin/python3 "$ISO_DIR/usr/bin/python3"

# Python libraries for runtime
if [ -d "$ROOT/python" ]; then
  cp -a "$ROOT/python" "$ISO_DIR/home/cohesix/python"
fi

# Minimal userland utilities
if [ -d "$ROOT/userland/miniroot" ]; then
  cp -a "$ROOT/userland/miniroot" "$ISO_DIR/userland/miniroot"
fi

# Example source files
if [ -f "$ROOT/usr/src/example.coh" ]; then
  mkdir -p "$ISO_DIR/usr/src"
  cp "$ROOT/usr/src/example.coh" "$ISO_DIR/usr/src/example.coh"
fi

if [ -d "$ROOT/out/etc/cohesix" ]; then
  log "📦 Copying config files"
  cp -a "$ROOT/out/etc/cohesix/." "$ISO_DIR/etc/cohesix/"
fi
if [ -d "$ROOT/out/roles" ]; then
  log "📦 Copying roles"
  cp -a "$ROOT/out/roles/." "$ISO_DIR/roles/"
fi

"${MKISO[@]}" -R -J -no-emul-boot \
  -eltorito-platform efi -eltorito-boot boot/efi/EFI/BOOT/$EFI_NAME \
  -boot-load-size 4 -boot-info-table -o "$ISO_OUT" "$ISO_DIR"

if [ ! -f "$ISO_OUT" ]; then
  error "ISO not created at $ISO_OUT"
fi

SIZE=$(du -h "$ISO_OUT" | awk '{print $1}')
log "ISO Build PASS ($SIZE) at $ISO_OUT"
log "Staged files:" && find "$ISO_DIR" -type f | sort
