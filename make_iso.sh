# CLASSIFICATION: COMMUNITY
# Filename: make_iso.sh v0.6
# Author: Lukas Bower
# Date Modified: 2025-12-02
#!/bin/sh
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$SCRIPT_DIR/scripts/make_iso.sh" "$@"
