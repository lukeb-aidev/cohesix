# CLASSIFICATION: COMMUNITY
# Filename: cohesix_fetch_build.sh v0.12
# Author: Lukas Bower
# Date Modified: 2025-09-15
#!/bin/bash
# Fetch and fully build the Cohesix project using SSH Git auth.

set -euo pipefail

LOG_FILE="$HOME/cohesix_build.log"
: > "$LOG_FILE"
trap 'echo "❌ Build failed. Last 40 lines:" && tail -n 40 "$LOG_FILE"' ERR
exec > >(tee -a "$LOG_FILE") 2>&1

cd "$HOME"
echo "📦 Cloning Git repo via SSH..."

# Remove any existing clone to ensure a clean build
rm -rf cohesix
# Clone using SSH key (assumes GitHub SSH auth already configured)
git clone git@github.com:lukeb-aidev/cohesix.git
cd cohesix

echo "📦 Updating submodules (if any)..."
git submodule update --init --recursive

echo "🐍 Setting up Python venv..."
command -v python3 >/dev/null || { echo "❌ python3 not found"; exit 1; }
python3 -m venv .venv
source .venv/bin/activate
pip install --upgrade pip setuptools wheel

if [ -f requirements.txt ]; then
  pip install -r requirements.txt
fi

echo "🦀 Building Rust components..."
cargo build --all-targets --release

TARGET="x86_64-unknown-uefi"
echo "🛠️ Building kernel EFI..."
mkdir -p out/bin out/etc/cohesix out/roles
cargo build --release --target "$TARGET" --bin kernel \
  --no-default-features --features minimal_uefi,kernel_bin
KERNEL_EFI="target/${TARGET}/release/kernel.efi"
[ -f "$KERNEL_EFI" ] || { echo "❌ kernel.efi missing" >&2; exit 1; }
cp "$KERNEL_EFI" out/kernel.efi

echo "🛠️ Building init EFI..."
cargo build --release --target "$TARGET" --bin init \
  --no-default-features --features minimal_uefi
INIT_EFI="target/${TARGET}/release/init.efi"
if [ ! -f "$INIT_EFI" ]; then
  echo "❌ init EFI missing at $INIT_EFI" >&2
  exit 1
fi
mkdir -p out/bin
cp "$INIT_EFI" out/bin/init.efi
cp "$INIT_EFI" out/init.efi
if [[ ! -f out/init.efi ]]; then
  echo "❌ init EFI missing after build. Check target path or build.rs logic." >&2
  find ./target -name '*.efi' >&2
  exit 1
fi

for f in initfs.img plan9.ns bootargs.txt boot_trace.json; do
  if [ -f "$f" ]; then
    cp "$f" out/
  fi
done

echo "📀 Creating ISO..."
./scripts/make_iso.sh
[ -f out/cohesix.iso ] || { echo "❌ ISO build failed" >&2; exit 1; }

echo "🔍 Running Rust tests with detailed output..."
TEST_LOG="$HOME/cohesix_test.log"
ERROR_LOG="$HOME/cohesix_test_errors.log"
RUST_BACKTRACE=1 cargo test --release -- --nocapture 2>&1 | tee "$TEST_LOG"
TEST_EXIT_CODE=${PIPESTATUS[0]}
if [ $TEST_EXIT_CODE -ne 0 ]; then
  echo "❌ Rust tests failed. See $TEST_LOG for details." >&2
  grep -i "error" "$TEST_LOG" | tee "$ERROR_LOG" >&2 || true
  exit $TEST_EXIT_CODE
else
  echo "✅ Rust tests passed."
fi

if command -v go &> /dev/null; then
  echo "🐹 Building Go components..."
  if [ -f go.mod ]; then
    go build ./...
    go test ./...
  fi
else
  echo "⚠️ Go not found; skipping Go build"
fi

echo "🐍 Running Python tests (pytest)..."
if command -v pytest &> /dev/null; then
  pytest -v || true
fi
if command -v flake8 &> /dev/null; then
  flake8 python tests || true
fi

echo "🧱 CMake config (if present)..."
if [ -f CMakeLists.txt ]; then
  mkdir -p build && cd build
  cmake ..
  make -j$(nproc)
  ctest --output-on-failure || true
  cd ..
fi

echo "✅ All builds complete."

# Optional QEMU boot check
if command -v qemu-system-x86_64 >/dev/null; then
  ISO_IMG="out/cohesix.iso"
  if [ ! -f "$ISO_IMG" ]; then
    echo "❌ cohesix.iso missing in out" >&2
    exit 1
  fi
  TMPDIR="${TMPDIR:-$(mktemp -d)}"
  LOG_DIR="$PWD/logs"
  mkdir -p "$LOG_DIR"
  SERIAL_LOG="$TMPDIR/qemu_boot.log"
  QEMU_LOG="$LOG_DIR/qemu_boot.log"
  if [ -f "$QEMU_LOG" ]; then
    mv "$QEMU_LOG" "$QEMU_LOG.$(date +%Y%m%d_%H%M%S)"
  fi
  OVMF_CODE="/usr/share/qemu/OVMF.fd"
  if [ ! -f "$OVMF_CODE" ]; then
    for p in /usr/share/OVMF/OVMF_CODE.fd /usr/share/OVMF/OVMF.fd /usr/share/edk2/ovmf/OVMF_CODE.fd; do
      if [ -f "$p" ]; then
        OVMF_CODE="$p"
        break
      fi
    done
  fi
  OVMF_VARS=""
  for p in /usr/share/OVMF/OVMF_VARS.fd /usr/share/edk2/ovmf/OVMF_VARS.fd; do
    if [ -f "$p" ]; then
      OVMF_VARS="$p"
      break
    fi
  done
  [ -f "$OVMF_CODE" ] || { echo "OVMF firmware not found" >&2; exit 1; }
  [ -n "$OVMF_VARS" ] || { echo "OVMF_VARS.fd not found" >&2; exit 1; }
  cp "$OVMF_VARS" "$TMPDIR/OVMF_VARS.fd"
  qemu-system-x86_64 \
    -bios "$OVMF_CODE" \
    -drive if=pflash,format=raw,file="$TMPDIR/OVMF_VARS.fd" \
    -cdrom "$ISO_IMG" -net none -M q35 -m 256M \
    -no-reboot -nographic -serial file:"$SERIAL_LOG"
  cat "$SERIAL_LOG" >> "$QEMU_LOG" 2>/dev/null || true
  cat "$SERIAL_LOG" >> "$LOG_FILE" 2>/dev/null || true
  echo "📜 Boot log (tail):"
  tail -n 20 "$SERIAL_LOG" || echo "❌ Could not read QEMU log"
  if grep -q "BOOT_OK" "$SERIAL_LOG"; then
    echo "✅ QEMU boot succeeded"
  else
    echo "❌ BOOT_OK not found in log"
    exit 1
  fi
else
  echo "⚠️ qemu-system-x86_64 not installed; skipping boot test"
fi

echo "✅ Build artifacts:"
echo " - Kernel EFI: $(realpath out/kernel.efi)"
echo " - Init EFI: $(realpath out/bin/init.efi)"
echo " - ISO image: $(realpath out/cohesix.iso)"
echo "Full build log: $LOG_FILE"
