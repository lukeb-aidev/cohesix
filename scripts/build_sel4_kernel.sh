// CLASSIFICATION: COMMUNITY
// Filename: build_sel4_kernel.sh v0.10
// Author: Lukas Bower
// Date Modified: 2026-01-29
#!/bin/bash
# Auto-detect target architecture and configure seL4 build
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
SEL4_DIR="$ROOT/third_party/sel4"
TOOLS="$ROOT/third_party/sel4_tools"
BUILD_DIR="$ROOT/out/sel4_build"
OUT_ELF="$ROOT/out/sel4.elf"

SETTINGS="$TOOLS/cmake-tool/settings.cmake"
NINJA="$TOOLS/bin/ninja"
if [ ! -x "$NINJA" ]; then
    NINJA="$(command -v ninja || true)"
fi

CMAKE="$(command -v cmake || true)"

msg() { printf "\e[32m==>\e[0m %s\n" "$*"; }
die() { printf "\e[31m[ERR]\e[0m %s\n" "$*" >&2; exit 1; }

# Ensure required host tools are installed
missing_pkgs=()
for pkg in cmake gcc python3-yaml; do
    dpkg -s "$pkg" >/dev/null 2>&1 || missing_pkgs+=("$pkg")
done

# Install ninja-build if no ninja executable is available
if [ -z "$NINJA" ]; then
    dpkg -s ninja-build >/dev/null 2>&1 || missing_pkgs+=("ninja-build")
fi



[ -d "$SEL4_DIR" ] || die "Missing seL4 repo at $SEL4_DIR"

# Install Python tooling required by seL4 build scripts
"$ROOT/scripts/bootstrap_sel4_tools.sh"

mkdir -p "$BUILD_DIR"
pushd "$BUILD_DIR" >/dev/null

if [ ! -f "$SETTINGS" ]; then
    msg "Creating basic settings.cmake"
    mkdir -p "$(dirname "$SETTINGS")"
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
    command -v aarch64-linux-gnu-gcc >/dev/null 2>&1 || missing_pkgs+=("gcc-aarch64-linux-gnu")
    if command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
        CC="aarch64-linux-gnu-gcc"
    fi
fi

if [ ${#missing_pkgs[@]} -gt 0 ] && command -v apt-get >/dev/null 2>&1; then
    msg "Installing packages: ${missing_pkgs[*]}"
    sudo apt-get update -y >/dev/null
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y ${missing_pkgs[*]} >/dev/null
    NINJA="$(command -v ninja || true)"
    CMAKE="$(command -v cmake || true)"
    export CMAKE_MAKE_PROGRAM="$NINJA"
fi

msg "Host arch: $ARCH, target platform: $KERNEL_PLATFORM"
msg "Using compiler: $(command -v $CC)"
[ -x "$CMAKE" ] || die "cmake not found"
[ -x "$NINJA" ] || die "Missing ninja at $NINJA"
export CMAKE_MAKE_PROGRAM="$(command -v ninja)"

# Update settings.cmake with defaults
cat > "$SETTINGS" <<EOF
set(KernelWordSize ${KERNEL_WORD_SIZE} CACHE STRING "Default word size" FORCE)
set(KernelSel4Arch ${KERNEL_SEL4_ARCH} CACHE STRING "Default seL4 arch" FORCE)
EOF

msg "Configuring seL4 kernel ($KERNEL_PLATFORM, $KERNEL_ARCH)"
"$CMAKE" -G Ninja -C "$SETTINGS" \
    -DKernelArch="$KERNEL_ARCH" -DKernelPlatform="$KERNEL_PLATFORM" \
    -DKernelSel4Arch="$KERNEL_SEL4_ARCH" -DKernelWordSize="$KERNEL_WORD_SIZE" \
    -DCMAKE_C_COMPILER="$CC" -DCMAKE_ASM_COMPILER="$CC" \
    "$SEL4_DIR" || die "CMake failed"

msg "Building kernel"
"$NINJA" kernel.elf || die "Kernel build failed"

KERN_SRC="$BUILD_DIR/kernel/kernel.elf"
[ -f "$KERN_SRC" ] || die "Kernel ELF not found"
cp "$KERN_SRC" "$OUT_ELF"
popd >/dev/null

[ -s "$OUT_ELF" ] || die "Output ELF empty"
msg "KERNEL BUILD OK: $OUT_ELF"
