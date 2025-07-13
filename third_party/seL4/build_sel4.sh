# CLASSIFICATION: COMMUNITY
# Filename: build_sel4.sh v0.2
# Author: Lukas Bower
# Date Modified: 2027-12-31
#!/usr/bin/env bash
set -euxo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
ROOT="${ROOT//\/\//\/}"

echo "Fetching seL4 sources ..." >&2
bash "$ROOT/third_party/seL4/fetch_sel4.sh"

SEL4_SRC="${SEL4_SRC:-$ROOT/third_party/seL4/workspace}"
BUILD_DIR="$ROOT/third_party/seL4/build"

for cmd in cmake ninja aarch64-linux-gnu-gcc aarch64-linux-gnu-g++ rustup cargo readelf nm objdump dtc; do
    command -v "$cmd" >/dev/null 2>&1 || { echo "Missing $cmd" >&2; exit 1; }
done

mkdir -p "$SEL4_SRC" "$BUILD_DIR"


cd "$BUILD_DIR"
cmake -G Ninja -C "$ROOT/build_config.cmake" "$SEL4_SRC"
ninja kernel.elf
cp kernel.elf "$ROOT/out/bin/kernel.elf"

cd "$ROOT"
"$ROOT/scripts/build_root_elf.sh"

 mkdir -p out/boot
 cd out/bin
 DTB="$BUILD_DIR/kernel/kernel.dtb"
 if [ ! -f "$DTB" ]; then
     DTC_SRC="$SEL4_SRC/projects/sel4test/tools/dts/qemu-arm-virt.dts"
    [ -f "$DTC_SRC" ] && dtc -I dts -O dtb "$DTC_SRC" -o "$DTB"
fi
find kernel.elf cohesix_root.elf $( [ -f "$DTB" ] && echo "$DTB" ) | cpio -o -H newc > ../boot/cohesix.cpio
cd "$ROOT"

readelf -h out/bin/cohesix_root.elf | grep -q 'AArch64'
readelf -h out/bin/kernel.elf | grep -q 'AArch64'

if nm -u out/bin/cohesix_root.elf | grep -q " U "; then
    echo "Unresolved symbols in cohesix_root.elf" >&2
     exit 1
fi

objdump -x out/bin/cohesix_root.elf | grep -q "_start"
objdump -x out/bin/kernel.elf | grep -q "_start"

echo "✅ build complete"
