# CLASSIFICATION: COMMUNITY
# Filename: build_sel4_kernel.sh v0.18
# Author: Lukas Bower
# Date Modified: 2026-02-27
#!/bin/bash
# Auto-detect target architecture and configure seL4 build
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
bash "$ROOT/scripts/setup_build_env.sh"
SEL4_DIR="$ROOT/third_party/sel4"
BUILD_DIR="$ROOT/out/sel4_build"
OUT_ELF="$ROOT/out/sel4.elf"

NINJA="$(command -v ninja || true)"
CMAKE="$(command -v cmake || true)"
msg() { printf "\e[32m==>\e[0m %s\n" "$*"; }
die() { printf "\e[31m[⚠️]\e[0m %s\n" "$*" >&2; exit 1; }

[ -x "$NINJA" ] || die "ninja not found"
[ -x "$CMAKE" ] || die "cmake not found"

# Fetch seL4 sources and install Python tooling
bash "$ROOT/scripts/bootstrap_sel4_tools.sh"

[ -d "$SEL4_DIR" ] || die "Missing seL4 repo at $SEL4_DIR"

rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"
rm -f "$BUILD_DIR/CMakeCache.txt" "$BUILD_DIR"/*toolchain*.cmake 2>/dev/null
rm -rf "$BUILD_DIR/CMakeFiles" 2>/dev/null

ARCH="${host_arch:-${COH_ARCH:-$(uname -m)}}"
ARCH="${ARCH,,}"
case "$ARCH" in
    aarch64|arm64)
        CROSS_COMPILER_PREFIX=aarch64-linux-gnu-
        PLATFORM=qemu_arm_virt
        KernelArch=aarch64
        KernelSel4Arch=aarch64
        KernelWordSize=64
        ;;
    x86_64|amd64|x86)
        CROSS_COMPILER_PREFIX=""
        PLATFORM=pc99
        KernelArch=x86_64
        KernelSel4Arch=x86_64
        KernelWordSize=64
        ;;
    *)
        die "Unsupported architecture: $ARCH"
        ;;
esac

command -v "${CROSS_COMPILER_PREFIX}gcc" >/dev/null \
    || die "Cross compiler ${CROSS_COMPILER_PREFIX}gcc not found"

msg "Using toolchain prefix: ${CROSS_COMPILER_PREFIX:-native}"

msg "Building seL4 for $ARCH"
pushd "$BUILD_DIR" >/dev/null
"$CMAKE" -G Ninja "$SEL4_DIR" \
    -DKernelArch="$KernelArch" \
    -DKernelPlatform="$PLATFORM" \
    -DKernelSel4Arch="$KernelSel4Arch" \
    -DKernelWordSize="$KernelWordSize" \
    -DCROSS_COMPILER_PREFIX="$CROSS_COMPILER_PREFIX" \
    >/dev/null || die "CMake failed"

"$NINJA" kernel.elf >/dev/null || die "Kernel build failed"
cp kernel.elf "$OUT_ELF"
popd >/dev/null

if [ -s "$OUT_ELF" ]; then
    printf "✅ Kernel built successfully\n"
else
    die "Output ELF empty"
fi
