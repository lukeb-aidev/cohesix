# CLASSIFICATION: COMMUNITY
# Filename: fetch_sel4.sh v0.2
# Author: Lukas Bower
# Date Modified: 2027-12-30

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
COMMIT="$(cat "$SCRIPT_DIR/COMMIT")"
DEST="${SEL4_WORKSPACE:-$HOME/sel4_workspace}"

if [ -d "$DEST" ]; then
    echo "seL4 workspace already exists at $DEST"
    exit 0
fi

if ! command -v repo >/dev/null 2>&1; then
    echo "ERROR: repo tool not found. Install with: sudo apt install repo" >&2
    exit 1
fi

mkdir -p "$DEST"
cd "$DEST"
repo init -u https://github.com/seL4/sel4test-manifest.git --depth=1
repo sync
cd "$DEST/sel4"
git fetch origin "$COMMIT" --depth 1
git checkout -q "$COMMIT"
echo "✅ seL4 workspace ready at $DEST"
