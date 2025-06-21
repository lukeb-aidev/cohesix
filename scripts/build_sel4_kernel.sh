// CLASSIFICATION: COMMUNITY
// Filename: build_sel4_kernel.sh v0.12
// Author: Lukas Bower
// Date Modified: 2026-02-12
#!/bin/bash
# Auto-detect target architecture and configure seL4 build
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
source "$ROOT/scripts/setup_build_env.sh"
SEL4_DIR="$ROOT/third_party/sel4"
TOOLS="$ROOT/third_party/sel4_tools"
BUILD_DIR="$ROOT/out/sel4_build"
OUT_ELF="$ROOT/out/sel4.elf"

SETTINGS="$TOOLS/cmake-tool/settings.cmake"
NINJA="$(command -v ninja || true)"
CMAKE="$(command -v cmake || true)"
GEN="Ninja"
if [ -z "$NINJA" ]; then
    GEN="Unix Makefiles"
else
    export CMAKE_MAKE_PROGRAM="$NINJA"
fi

msg() { printf "\e[32m==>\e[0m %s\n" "$*"; }
die() { printf "\e[31m[ERR]\e[0m %s\n" "$*" >&2; exit 1; }

[ -d "$SEL4_DIR" ] || die "Missing seL4 repo at $SEL4_DIR"

# Install Python tooling required by seL4 build scripts
"$ROOT/scripts/bootstrap_sel4_tools.sh"

mkdir -p "$BUILD_DIR"
pushd "$BUILD_DIR" >/dev/null

if [ ! -f "$SETTINGS" ]; then
    msg "Creating basic settings.cmake"
    mkdir -p "$(dirname "$SETTINGS")"
    tmp_arch="$(uname -m)"
    case "$tmp_arch" in
        aarch64|arm64)
            echo "set(KernelWordSize 64 CACHE STRING \"Default word size\" FORCE)" > "$SETTINGS"
            echo "set(KernelSel4Arch aarch64 CACHE STRING \"Default seL4 arch\" FORCE)" >> "$SETTINGS"
            ;;
        *)
            echo "set(KernelWordSize 64 CACHE STRING \"Default word size\" FORCE)" > "$SETTINGS"
            echo "set(KernelSel4Arch x86_64 CACHE STRING \"Default seL4 arch\" FORCE)" >> "$SETTINGS"
            ;;
    esac
fi

ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64)
        KERNEL_PLATFORM="pc99"
        KERNEL_ARCH="x86_64"
        KERNEL_SEL4_ARCH="x86_64"
        KERNEL_WORD_SIZE=64
        CC="gcc"
        ;;
    aarch64|arm64)
        KERNEL_PLATFORM="imx8mm_evk"
        KERNEL_ARCH="aarch64"
        KERNEL_SEL4_ARCH="aarch64"
        KERNEL_WORD_SIZE=64
        CC="gcc"
        ;;
    *)
        die "Unsupported architecture: $ARCH"
        ;;
esac

if [ "$KERNEL_PLATFORM" = "imx8mm_evk" ]; then
    if command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
        CC="aarch64-linux-gnu-gcc"
    else
        die "aarch64-linux-gnu-gcc not found"
    fi
fi


msg "Host arch: $ARCH, target platform: $KERNEL_PLATFORM"
msg "Using compiler: $(command -v $CC)"
[ -x "$CMAKE" ] || die "cmake not found"
if [ "$GEN" = "Ninja" ]; then
    msg "Using Ninja at $NINJA"
else
    msg "Ninja not found; using Unix Makefiles"
fi

# Update settings.cmake with defaults
cat > "$SETTINGS" <<EOF
set(KernelWordSize ${KERNEL_WORD_SIZE} CACHE STRING "Default word size" FORCE)
set(KernelSel4Arch ${KERNEL_SEL4_ARCH} CACHE STRING "Default seL4 arch" FORCE)
EOF

msg "Configuring seL4 kernel ($KERNEL_PLATFORM, $KERNEL_ARCH)"
"$CMAKE" -G "$GEN" -C "$SETTINGS" \
    -DKernelArch="$KERNEL_ARCH" -DKernelPlatform="$KERNEL_PLATFORM" \
    -DKernelSel4Arch="$KERNEL_SEL4_ARCH" -DKernelWordSize="$KERNEL_WORD_SIZE" \
    -DCMAKE_C_COMPILER="$CC" -DCMAKE_ASM_COMPILER="$CC" \
    "$SEL4_DIR" || die "CMake failed"

msg "Building kernel"
if [ "$GEN" = "Ninja" ]; then
    "$NINJA" kernel.elf || die "Kernel build failed"
else
    make -j"$(nproc)" kernel.elf || die "Kernel build failed"
fi

[ -f "$BUILD_DIR/kernel.elf" ] && KERN_SRC="$BUILD_DIR/kernel.elf" || KERN_SRC="$BUILD_DIR/kernel/kernel.elf"
[ -f "$KERN_SRC" ] || die "Kernel ELF not found"
cp "$KERN_SRC" "$OUT_ELF"
popd >/dev/null

[ -s "$OUT_ELF" ] || die "Output ELF empty"
msg "KERNEL BUILD OK: $OUT_ELF"
