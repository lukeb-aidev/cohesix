#!/usr/bin/env bash
# ISO layout:
#   bin/               - runtime binaries
#   usr/bin/           - CLI wrappers and Go tools
#   usr/cli/           - Python CLI modules
#   home/cohesix/      - Python libraries
#   etc/               - system configuration
#   roles/             - role definitions
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
[ ! -f "$ROOT/out/kernel.efi" ] && echo "❌ Missing kernel.efi" && exit 1
[ -f "$INIT_SRC" ] || error "Missing $INIT_SRC"

rm -rf "$ISO_DIR"
mkdir -p "$ISO_DIR"/{bin,usr/bin,usr/cli,home/cohesix,etc/cohesix,roles,EFI/BOOT}

cp "$KERNEL_SRC" "$ISO_DIR/EFI/BOOT/bootx64.efi"
cp "$KERNEL_SRC" "$ISO_DIR/kernel.efi"
cp "$INIT_SRC" "$ISO_DIR/bin/init.efi"

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

if [ -f "$ROOT/out/etc/cohesix/config.yaml" ]; then
  cp "$ROOT/out/etc/cohesix/config.yaml" "$ISO_DIR/etc/cohesix/config.yaml"
fi
if [ -d "$ROOT/out/roles" ]; then
  cp -a "$ROOT/out/roles/." "$ISO_DIR/roles/"
fi

$MKISO -as mkisofs -R -J -no-emul-boot -eltorito-boot EFI/BOOT/bootx64.efi \
  -boot-load-size 4 -boot-info-table -o "$ISO_OUT" "$ISO_DIR"
