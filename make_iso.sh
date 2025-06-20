# CLASSIFICATION: COMMUNITY
# Filename: make_iso.sh v0.8
# Author: Lukas Bower
# Date Modified: 2025-12-17
#!/bin/bash
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec bash "$SCRIPT_DIR/scripts/make_iso.sh" "$@"
