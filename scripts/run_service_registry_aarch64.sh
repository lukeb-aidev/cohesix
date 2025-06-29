# CLASSIFICATION: COMMUNITY
# Filename: run_service_registry_aarch64.sh v0.1
# Author: Lukas Bower
# Date Modified: 2026-11-17
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"
BIN="target/aarch64-unknown-linux-gnu/debug/test_service_registry"
if [[ ! -f "$BIN" ]]; then
  echo "Binary $BIN not found" >&2
  exit 1
fi
exec qemu-aarch64 -L /usr/aarch64-linux-gnu/ "$BIN"
