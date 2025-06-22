// CLASSIFICATION: COMMUNITY
// Filename: official_sel4_build.sh v0.1
// Author: Lukas Bower
// Date Modified: 2026-03-01
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
SEL4_DIR="$ROOT/third_party/sel4"
TOOLS_DIR="$ROOT/third_party/sel4_tools"
OUT="$ROOT/out"

log(){ printf '\e[32m[sel4]\e[0m %s\n' "$1"; }
die(){ printf '\e[31m[ERR]\e[0m %s\n' "$1" >&2; exit 1; }

check_tool(){ command -v "$1" >/dev/null 2>&1 || die "$1 not found"; }

# Step 1: verify host tools
for t in git ninja cmake python3; do check_tool "$t"; done
check_tool gcc
check_tool qemu-system-x86_64
check_tool qemu-system-aarch64
check_tool aarch64-linux-gnu-gcc

cmake_ver=$(cmake --version | head -n1 | awk '{print $3}')
if [ "$(printf '%s\n' 3.20 "$cmake_ver" | sort -V | head -n1)" != "3.20" ]; then
    die "cmake >=3.20 required"
fi

# Step 2: clone repositories at tag 13.0.0
clone(){
    local dir=$1 url=$2 branch=$3
    if [ ! -d "$dir/.git" ]; then
        git clone --depth 1 --branch "$branch" "$url" "$dir"
    else
        git -C "$dir" fetch origin "$branch" --depth 1
        git -C "$dir" reset --hard FETCH_HEAD
    fi
}
clone "$SEL4_DIR" https://github.com/seL4/seL4.git 13.0.0
clone "$TOOLS_DIR" https://github.com/seL4/sel4_tools.git 13.0.x-compatible

for req in "$SEL4_DIR/configs" "$SEL4_DIR/gcc.cmake" \
           "$SEL4_DIR/src/arch/arm" "$SEL4_DIR/src/arch/aarch64" "$SEL4_DIR/src/arch/x86"; do
    [ -e "$req" ] || die "missing $req"
done

build(){
    local arch=$1 cross=$2 cfg=$3 plat=$4
    local builddir="$OUT/sel4_build_${arch}"
    rm -rf "$builddir" && mkdir -p "$builddir"
    pushd "$builddir" >/dev/null
    cmake -DCMAKE_TOOLCHAIN_FILE=../../third_party/sel4/gcc.cmake \
          -DCROSS_COMPILER_PREFIX="$cross" \
          -C ../../third_party/sel4/configs/include/"$cfg" \
          -DPLATFORM="$plat" -DKernelSel4Arch="$arch" -G Ninja \
          ../../third_party/sel4 || die "cmake failed for $arch"
    ninja kernel.elf image || die "ninja failed for $arch"
    [ -f kernel.elf ] || die "kernel.elf missing for $arch"
    cp kernel.elf "$OUT/sel4_${arch}.elf"
    popd >/dev/null
    log "built $arch kernel"
}

build x86_64 "" X64_verified.cmake pc99
build aarch64 aarch64-linux-gnu- ARM_verified.cmake qemu_arm_virt

log "kernels available at $OUT/sel4_x86_64.elf and $OUT/sel4_aarch64.elf"
