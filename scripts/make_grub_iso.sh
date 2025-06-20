// CLASSIFICATION: COMMUNITY
// Filename: make_grub_iso.sh v0.1
// Author: Lukas Bower
// Date Modified: 2025-12-28
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
ISO_ROOT="$ROOT/out/grub_iso_root"
ISO_OUT="$ROOT/out/cohesix_grub.iso"

# Prepare staging directory
rm -rf "$ISO_ROOT"
mkdir -p "$ISO_ROOT/boot/grub"

# Copy kernel, userland, and config
cp "$ROOT/out/sel4.elf" "$ISO_ROOT/boot/kernel.elf"
cp "$ROOT/out/cohesix_root.elf" "$ISO_ROOT/boot/userland.elf"
cp "$ROOT/config/config.yaml" "$ISO_ROOT/boot/config.yaml"

# Generate grub.cfg
cat >"$ISO_ROOT/boot/grub/grub.cfg" <<'CFG'
set default=0
set timeout=0
menuentry "Cohesix" {
  multiboot2 /boot/kernel.elf
  module /boot/userland.elf CohRole=QueenPrimary
  module /boot/config.yaml
}
CFG

# Build ISO using grub-mkrescue
if ! command -v grub-mkrescue >/dev/null 2>&1; then
    echo "ERROR: grub-mkrescue not found" >&2
    exit 1
fi

grub-mkrescue -o "$ISO_OUT" "$ISO_ROOT" >/dev/null 2>&1

if [ -f "$ISO_OUT" ] && [ -s "$ISO_OUT" ]; then
    echo "GRUB ISO OK: $ISO_OUT"
else
    echo "ERROR: ISO build failed" >&2
    exit 1
fi
