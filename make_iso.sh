// CLASSIFICATION: COMMUNITY
# Filename: make_iso.sh v0.11
# Author: Lukas Bower
# Date Modified: 2026-02-11
#!/bin/bash
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec bash "$SCRIPT_DIR/scripts/make_grub_iso.sh" "$@"
