#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: scripts/qemu_smoke_x86.sh v0.1
# Author: Lukas Bower
# Date Modified: 2028-12-12

set -euo pipefail
ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

if ! command -v qemu-system-x86_64 >/dev/null 2>&1; then
  echo "⚠️ qemu-system-x86_64 not installed; skipping" >&2
  exit 0
fi

if [ ! -f boot/elfloader ] || [ ! -f boot/cohesix.cpio ]; then
  echo "⚠️ Boot files not found; skipping" >&2
  exit 0
fi

qemu-system-x86_64 \
  -machine q35 \
  -m 512M \
  -nographic -serial mon:stdio \
  -kernel boot/elfloader \
  -initrd boot/cohesix.cpio \
  -no-reboot -display none &
PID=$!
# Wait briefly then terminate
sleep 5
kill "$PID" 2>/dev/null || true
wait "$PID" 2>/dev/null || true

echo "✅ QEMU x86 smoke test completed"
