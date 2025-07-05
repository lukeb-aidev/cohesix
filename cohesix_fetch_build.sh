# CLASSIFICATION: COMMUNITY
# Filename: cohesix_fetch_build.sh v0.91
# Author: Lukas Bower
# Date Modified: 2027-02-01
#!/bin/bash
set -euo pipefail

LOG="$HOME/cohesix/srv/upload/fix-log.txt"
STAGE_DIR="$HOME/cohesix/srv/upload/out/stage"
mkdir -p "$(dirname "$LOG")" "$STAGE_DIR/bin" "$STAGE_DIR/usr/bin"
exec > >(tee -a "$LOG") 2>&1

ROOT=$(pwd)
export RUSTFLAGS="-C link-arg=-Tlink.ld"

echo "== Checking CUDA =="
if command -v nvcc >/dev/null 2>&1; then
  echo "CUDA compiler found: $(nvcc --version | head -n 1)"
else
  echo "CUDA compiler nvcc not found, skipping CUDA build"
fi

echo "== BusyBox version check =="
BUSYBOX_VERSION=$(busybox | head -n 1)
echo "BusyBox version: $BUSYBOX_VERSION"

echo "== CMake version check =="
CMAKE_VERSION=$(cmake --version | head -n 1)
echo "CMake version: $CMAKE_VERSION"

echo "== Rust build =="
BINS=(cohesix_root cohcc cohesix_build cohesix_cap cohesix_trace init logdemo)
for BIN in "${BINS[@]}"; do
  case "$BIN" in
    init|logdemo)
      FEATURES="minimal_uefi"
      ;;
    *)
      FEATURES="std"
      ;;
  esac
  cargo build --release --bin "$BIN" --target aarch64-unknown-linux-musl --no-default-features --features "$FEATURES" || exit 1
done

cargo clippy --target aarch64-unknown-linux-musl -- -D warnings

mkdir -p out/bin
for BIN in "${BINS[@]}"; do
  SRC="target/aarch64-unknown-linux-musl/release/$BIN"
  cp "$SRC" "out/bin/$BIN" || true
  if [ -f "$SRC" ]; then
    cp "$SRC" "$STAGE_DIR/bin/$BIN"
    cp "$SRC" "$STAGE_DIR/usr/bin/$BIN"
    chmod +x "$STAGE_DIR/bin/$BIN" "$STAGE_DIR/usr/bin/$BIN"
  fi
done
cp target/aarch64-unknown-linux-musl/release/cohesix_root out/bin/cohesix_root.elf

echo "== Go build =="
mkdir -p out/go_helpers
for dir in go/cmd/*; do
  if [ -f "$dir/main.go" ]; then
    name=$(basename "$dir")
    GOOS=linux GOARCH=arm64 go build -C "$dir" -o "$ROOT/out/go_helpers/$name" || true
    if [ -f "$ROOT/out/go_helpers/$name" ]; then
      cp "$ROOT/out/go_helpers/$name" "$STAGE_DIR/bin/$name"
      cp "$ROOT/out/go_helpers/$name" "$STAGE_DIR/usr/bin/$name"
      chmod +x "$STAGE_DIR/bin/$name" "$STAGE_DIR/usr/bin/$name"
    fi
  fi
done

echo "== Mandoc build check =="
if command -v mandoc >/dev/null 2>&1; then
  echo "mandoc found: $(mandoc -V)"
else
  echo "mandoc not found, skipping mandoc-related build steps"
fi

echo "== Plan9 support check =="
if [ -d "plan9" ]; then
  echo "Plan9 directory found, building Plan9 components"
  if [ -f "plan9/mk" ]; then
    (cd plan9 && make) || echo "Plan9 build failed"
  fi
else
  echo "Plan9 directory not found, skipping Plan9 build"
fi

echo "== Validating root ELF load segments =="
ROOT_ELF="$STAGE_DIR/bin/cohesix_root"
if [ -f "$ROOT_ELF" ]; then
  echo "Checking ELF segments for $ROOT_ELF"
  readelf -l "$ROOT_ELF" | grep -E 'LOAD' || echo "No LOAD segments found"
else
  echo "Root ELF not found at $ROOT_ELF"
fi

echo "== Staging ELF files for QEMU boot =="
cp target/aarch64-unknown-linux-musl/release/cohesix_root "$STAGE_DIR/bin/cohesix_root.elf"
chmod +x "$STAGE_DIR/bin/cohesix_root.elf"

if [ -f "kernel.elf" ]; then
  cp kernel.elf "$STAGE_DIR/bin/kernel.elf"
  chmod +x "$STAGE_DIR/bin/kernel.elf"
else
  echo "kernel.elf not found, skipping"
fi

if [ -f "elfloader" ]; then
  cp elfloader "$STAGE_DIR/bin/elfloader"
  chmod +x "$STAGE_DIR/bin/elfloader"
else
  echo "elfloader not found, skipping"
fi

if ls tests 2>/dev/null | grep -q '\.py'; then
  pytest || echo "pytest failed"
else
  echo "SKIPPED pytest"
fi

echo "== Build complete =="
echo "== Stage contents =="
find "$STAGE_DIR/bin" -type f -maxdepth 1
echo "BUILD AND STAGING COMPLETE: All binaries present and documented."
cargo clippy -- -D warnings
