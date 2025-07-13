# CLASSIFICATION: COMMUNITY
# Filename: fetch_sel4.sh v0.4
# Author: Lukas Bower
# Date Modified: 2027-12-30

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
COMMIT="$(cat "$SCRIPT_DIR/COMMIT")"
DEST="workspace"

if [ -d "$DEST" ]; then
    echo "🧹 Cleaning existing $DEST"
    rm -rf "$DEST"
fi

echo "📥 Syncing seL4 repos into $DEST..."

# Clone seL4 into workspace directly
git clone https://github.com/seL4/seL4.git $DEST
cd $DEST
git fetch --tags
git checkout 13.0.0

# Now add tools and projects inside workspace
#git clone https://github.com/seL4/seL4_tools.git tools
git clone https://github.com/seL4/seL4_libs.git projects/seL4_libs
git clone https://github.com/seL4/musllibc.git projects/musllibc
git clone https://github.com/seL4/util_libs.git projects/util_libs
git clone https://github.com/seL4/sel4runtime.git projects/sel4runtime
git clone https://github.com/seL4/sel4test.git projects/sel4test

echo "✅ seL4 workspace ready at $DEST"