# CLASSIFICATION: COMMUNITY
# Filename: scripts/validate_iso_build.sh v0.1
# Author: Lukas Bower
# Date Modified: 2025-12-19
#!/bin/bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
EFI="$ROOT/out/BOOTX64.EFI"
ISO="$ROOT/out/cohesix.iso"
KERNEL="$ROOT/out/boot/kernel.elf"
log(){ echo "[validate_iso] $*"; }
error(){ echo "ERROR: $*" >&2; exit 1; }

[[ -f "$EFI" ]] || error "BOOTX64.EFI not found at $EFI"
[[ -s "$ISO" ]] || error "cohesix.iso missing or empty at $ISO"
[[ -f "$KERNEL" ]] || error "kernel.elf missing at $KERNEL"

log "All required files present."
ls -l "$EFI" "$ISO" "$KERNEL"
du -h "$ISO"
