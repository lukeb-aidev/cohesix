# CLASSIFICATION: COMMUNITY
# Filename: scripts/validate_iso_build.sh v0.2
# Author: Lukas Bower
# Date Modified: 2026-10-16
#!/bin/bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
ISO="$ROOT/out/cohesix.iso"
KERNEL="$ROOT/out/boot/kernel.elf"
ROOT_ELF="$ROOT/out/cohesix_root.elf"
log(){ echo "[validate_iso] $*"; }
error(){ echo "ERROR: $*" >&2; exit 1; }

[[ -s "$ISO" ]] || error "cohesix.iso missing or empty at $ISO"
[[ -f "$KERNEL" ]] || error "kernel.elf missing at $KERNEL"
[[ -f "$ROOT_ELF" ]] || error "userland.elf missing at $ROOT_ELF"

log "All required files present."
ls -l "$ISO" "$KERNEL" "$ROOT_ELF"
du -h "$ISO"
