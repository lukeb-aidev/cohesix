// CLASSIFICATION: COMMUNITY
# Filename: make_iso.sh v0.9
# Author: Lukas Bower
# Date Modified: 2026-01-06
#!/bin/bash
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec bash "$SCRIPT_DIR/scripts/make_grub_iso.sh" "$@"
