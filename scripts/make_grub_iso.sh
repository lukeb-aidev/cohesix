// CLASSIFICATION: COMMUNITY
// Filename: make_grub_iso.sh v0.5
// Author: Lukas Bower
// Date Modified: 2026-01-02
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
ISO_ROOT="$ROOT/out/stage"
ISO_OUT="$ROOT/out/cohesix_grub.iso"
ROLE="${1:-${COHROLE:-QueenPrimary}}"

# Create stage directory if missing
mkdir -p "$ISO_ROOT/boot/grub"

# Ensure kernel and root task ELFs exist
KERNEL_ELF="$ROOT/out/sel4.elf"
ROOT_ELF="$ROOT/out/cohesix_root.elf"
if [ ! -s "$KERNEL_ELF" ]; then
    bash "$ROOT/scripts/build_sel4_kernel.sh"
fi
if [ ! -s "$ROOT_ELF" ]; then
    bash "$ROOT/scripts/build_root_elf.sh"
fi

# Copy kernel, userland, and config
cp "$KERNEL_ELF" "$ISO_ROOT/boot/kernel.elf"
cp "$ROOT_ELF" "$ISO_ROOT/boot/userland.elf"
cp "$ROOT/config/config.yaml" "$ISO_ROOT/boot/config.yaml"

# Generate grub.cfg
cat >"$ISO_ROOT/boot/grub/grub.cfg" <<CFG
set default=0
set timeout=0
set CohRole=${ROLE}
menuentry "Cohesix" {
  multiboot2 /boot/kernel.elf
  module /boot/userland.elf CohRole=${ROLE}
  module /boot/config.yaml
}
CFG

# Build ISO using grub-mkrescue
if ! command -v grub-mkrescue >/dev/null 2>&1; then
    echo "ERROR: grub-mkrescue not found" >&2
    exit 1
fi


grub-mkrescue -o "$ISO_OUT" "$ISO_ROOT" >/dev/null 2>&1

# Ensure summary directories exist before scanning
mkdir -p "$ISO_ROOT/bin" "$ISO_ROOT/roles"

if [ -f "$ISO_OUT" ] && [ -s "$ISO_OUT" ]; then
    BIN_COUNT=0
    if [ -d "$ISO_ROOT/bin" ]; then
        BIN_COUNT=$(find "$ISO_ROOT/bin" -type f -perm -111 | wc -l)
    fi
    ROLE_COUNT=0
    if [ -d "$ISO_ROOT/roles" ]; then
        ROLE_COUNT=$(find "$ISO_ROOT/roles" -name '*.yaml' | wc -l)
    fi
    SIZE_MB=$(du -m "$ISO_OUT" | awk '{print $1}')
    echo "ISO BUILD OK: ${BIN_COUNT} binaries, ${ROLE_COUNT} roles, ${SIZE_MB}MB total"
else
    echo "ERROR: ISO build failed" >&2
    exit 1
fi
