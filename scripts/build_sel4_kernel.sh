// CLASSIFICATION: COMMUNITY
// Filename: build_sel4_kernel.sh v0.4
// Author: Lukas Bower
// Date Modified: 2026-01-07
#!/usr/bin/env bash
## Auto-detect target architecture and configure seL4 build
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
SEL4_DIR="$ROOT/third_party/sel4"
TOOLS="$ROOT/third_party/sel4_tools"
BUILD_DIR="$ROOT/out/sel4_build"
OUT_ELF="$ROOT/out/sel4.elf"

SETTINGS="$TOOLS/cmake-tool/settings.cmake"
NINJA="$TOOLS/bin/ninja"
if [ ! -x "$NINJA" ]; then
    NINJA="$(command -v ninja)"
fi

CMAKE="$(command -v cmake)"

msg() { printf "\e[32m==>\e[0m %s\n" "$*"; }
die() { printf "\e[31m[ERR]\e[0m %s\n" "$*" >&2; exit 1; }

[ -x "$CMAKE" ] || die "cmake not found"
[ -x "$NINJA" ] || die "Missing ninja at $NINJA"
[ -d "$SEL4_DIR" ] || die "Missing seL4 repo at $SEL4_DIR"

mkdir -p "$BUILD_DIR"
pushd "$BUILD_DIR" >/dev/null

if [ ! -f "$SETTINGS" ]; then
    msg "Creating basic settings.cmake"
    mkdir -p "$(dirname "$SETTINGS")"
    touch "$SETTINGS"
fi

ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64)
        KERNEL_PLATFORM="pc99"
        KERNEL_ARCH="x86_64"
        ;;
    aarch64|arm64)
        KERNEL_PLATFORM="imx8mm_evk"
        KERNEL_ARCH="aarch64"
        ;;
    *)
        die "Unsupported architecture: $ARCH"
        ;;
esac

msg "Configuring seL4 kernel ($KERNEL_PLATFORM, $KERNEL_ARCH)"
"$CMAKE" -G Ninja -C "$SETTINGS" \
    -DKernelArch="$KERNEL_ARCH" -DKernelPlatform="$KERNEL_PLATFORM" \
    "$SEL4_DIR" || die "CMake failed"

msg "Building kernel"
"$NINJA" kernel || die "Kernel build failed"

KERN_SRC="$BUILD_DIR/kernel/kernel.elf"
[ -f "$KERN_SRC" ] || die "Kernel ELF not found"
cp "$KERN_SRC" "$OUT_ELF"
popd >/dev/null

[ -s "$OUT_ELF" ] || die "Output ELF empty"
msg "KERNEL BUILD OK: $OUT_ELF"
