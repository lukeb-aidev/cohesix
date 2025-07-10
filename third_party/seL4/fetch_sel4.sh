# CLASSIFICATION: COMMUNITY
# Filename: fetch_sel4.sh v0.1
# Author: Lukas Bower
# Date Modified: 2027-12-28

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
COMMIT="$(cat "$SCRIPT_DIR/COMMIT")"
DEST="${SEL4_WORKSPACE:-$HOME/sel4_workspace}"
if [ -d "$DEST" ] && [ -f "$DEST/.git/HEAD" ]; then
    echo "seL4 workspace already exists at $DEST"
    exit 0
fi

echo "Cloning seL4 commit $COMMIT to $DEST"
git clone --depth 1 https://github.com/seL4/sel4.git "$DEST"
(cd "$DEST" && git fetch --depth 1 origin "$COMMIT" && git checkout -q "$COMMIT")
