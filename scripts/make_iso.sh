# CLASSIFICATION: COMMUNITY
# Filename: scripts/make_iso.sh v0.10
# Author: Lukas Bower
# Date Modified: 2025-12-21
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
  error "ISO tools missing"
fi
if ! have mformat; then
  error "mtools missing"
fi

log "Using bootloader source: $BOOTLOADER_SRC"
log "Kernel source: $KERNEL_SRC"
log "Init source: $INIT_SRC"

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
log "ISO staging directory prepared at $ISO_DIR"

log "📦 Adding bootloader..."
cp "$BOOTLOADER_SRC" "$ISO_DIR/boot/efi/EFI/BOOT/$EFI_NAME"
[[ -f "$ISO_DIR/boot/efi/EFI/BOOT/$EFI_NAME" ]] || error "missing bootloader in ISO"
log "✅ Bootloader installed"
log "📦 Adding kernel..."
cp "$KERNEL_SRC" "$ISO_DIR/kernel.elf"
[[ -f "$ISO_DIR/kernel.elf" ]] || error "missing kernel.elf in ISO"
log "📦 Adding init..."
cp "$INIT_SRC" "$ISO_DIR/init"
[[ -f "$ISO_DIR/init" ]] || error "missing init in ISO"
log "✅ Kernel and init installed"

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
log "✅ Files copied"

log "📦 Running mkisofs" 
if ! "${MKISO[@]}" -R -J -no-emul-boot \
  -eltorito-platform efi -eltorito-boot boot/efi/EFI/BOOT/$EFI_NAME \
  -boot-load-size 4 -boot-info-table -o "$ISO_OUT" "$ISO_DIR" \
  >"$ISO_DIR/mkisofs.log" 2>&1; then
  echo "mkisofs failed" >&2
  cat "$ISO_DIR/mkisofs.log" >&2
  exit 1
fi
log "✅ mkisofs finished"

if [ ! -f "$ISO_OUT" ]; then
  error "ISO not created at $ISO_OUT"
fi
[[ -s "$ISO_OUT" ]] || error "ISO file empty at $ISO_OUT"
[[ -r "$ISO_OUT" ]] || error "ISO not readable at $ISO_OUT"

SIZE=$(du -h "$ISO_OUT" | awk '{print $1}')
log "ISO Build PASS ($SIZE) at $ISO_OUT"
log "Staged files:" && find "$ISO_DIR" -type f | sort
