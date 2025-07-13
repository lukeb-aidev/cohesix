# CLASSIFICATION: COMMUNITY
# Filename: build_sel4.sh v0.2
# Author: Lukas Bower
# Date Modified: 2027-12-31
#!/usr/bin/env bash
set -euxo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
ROOT="${ROOT//\/\//\/}"


echo "Fetching seL4 sources ..." >&2
SEL4_SRC="${SEL4_SRC:-$ROOT/third_party/seL4/workspace}"

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
git clone https://github.com/seL4/seL4_libs.git projects/seL4_libs
git clone https://github.com/seL4/musllibc.git projects/musllibc
git clone https://github.com/seL4/util_libs.git projects/util_libs
git clone https://github.com/seL4/sel4runtime.git projects/sel4runtime
git clone https://github.com/seL4/sel4test.git projects/sel4testß

echo "✅ seL4 workspace ready at $DEST"

BUILD_DIR="$ROOT/third_party/seL4/workspace/build"

for cmd in cmake ninja aarch64-linux-gnu-gcc aarch64-linux-gnu-g++ rustup cargo readelf nm objdump dtc; do
    command -v "$cmd" >/dev/null 2>&1 || { echo "Missing $cmd" >&2; exit 1; }
done

mkdir -p "$BUILD_DIR"

cd "$BUILD_DIR"
cmake -G Ninja \
  -C "$ROOT/third_party/seL4/workspace/configs/AARCH64_verified.cmake" \
  -DSIMULATION=TRUE \
  -DCROSS_COMPILER_PREFIX=aarch64-linux-gnu- \
  "$SEL4_SRC"
ninja kernel.elf
cp "$BUILD_DIR/kernel.elf" "$ROOT/out/bin/kernel.elf"

 mkdir -p "$ROOT/out/boot"
 cd "$ROOT/out/bin"
 DTB="$BUILD_DIR/kernel.dtb"
 if [ ! -f "$DTB" ]; then
 echo "Error - DTB not found"  >&2
 exit 1
fi

[ -f kernel.elf ] || { echo "Missing kernel.elf" >&2; exit 1; }
[ -f cohesix_root.elf ] || { echo "Missing cohesix_root.elf" >&2; exit 1; }
find kernel.elf cohesix_root.elf $( [ -f "$DTB" ] && echo "$DTB" ) | cpio -o -H newc > ../boot/cohesix.cpio
cd "$ROOT"

echo "✅ build complete"  >&2