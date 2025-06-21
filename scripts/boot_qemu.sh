# CLASSIFICATION: COMMUNITY
# Filename: boot_qemu.sh v0.1
# Author: Lukas Bower
# Date Modified: 2025-12-29
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"
QEMU=$(command -v qemu-system-x86_64 || true)
if [[ -z "$QEMU" ]]; then
  echo "ERROR: qemu-system-x86_64 not found" >&2
  exit 1
fi
exec "$QEMU" -cdrom out/cohesix_grub.iso -nographic -serial mon:stdio -m 256
