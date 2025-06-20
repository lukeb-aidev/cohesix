// CLASSIFICATION: COMMUNITY
// Filename: build_sel4_kernel.sh v0.1
// Author: Lukas Bower
// Date Modified: 2025-12-26
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
SEL4_DIR="$ROOT/third_party/sel4"
TOOLS="$ROOT/third_party/sel4_tools"
BUILD_DIR="$ROOT/out/sel4_build"
OUT_ELF="$ROOT/out/sel4.elf"

CMAKE_INIT="$TOOLS/cmake-tool/init-build.sh"
NINJA="$TOOLS/bin/ninja"

msg() { printf "\e[32m==>\e[0m %s\n" "$*"; }
die() { printf "\e[31m[ERR]\e[0m %s\n" "$*" >&2; exit 1; }

[ -x "$CMAKE_INIT" ] || die "Missing init-build.sh at $CMAKE_INIT"
[ -x "$NINJA" ] || die "Missing ninja at $NINJA"
[ -d "$SEL4_DIR" ] || die "Missing seL4 repo at $SEL4_DIR"

mkdir -p "$BUILD_DIR"
pushd "$BUILD_DIR" >/dev/null

msg "Configuring seL4 kernel (pc99, x86_64)"
"$CMAKE_INIT" -DPLATFORM=pc99 -DKernelSel4Arch=x86_64 "$SEL4_DIR" || die "CMake failed"

msg "Building kernel"
"$NINJA" kernel || die "Kernel build failed"

KERN_SRC="$BUILD_DIR/kernel/kernel.elf"
[ -f "$KERN_SRC" ] || die "Kernel ELF not found"
cp "$KERN_SRC" "$OUT_ELF"
popd >/dev/null

[ -s "$OUT_ELF" ] || die "Output ELF empty"
msg "KERNEL BUILD OK: $OUT_ELF"
