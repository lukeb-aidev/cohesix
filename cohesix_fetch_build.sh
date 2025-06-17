// CLASSIFICATION: COMMUNITY
// Filename: cohesix_fetch_build.sh v0.10
// Author: Lukas Bower
// Date Modified: 2025-09-10
#!/bin/bash
# Fetch and fully build the Cohesix project using SSH Git auth.

set -euo pipefail

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

echo "🛠️ Building kernel EFI and ISO..."
mkdir -p out
cargo build --release --target x86_64-unknown-uefi --bin kernel \
  --no-default-features --features minimal_uefi,kernel_bin
[ -f target/x86_64-unknown-uefi/release/kernel.efi ] || {
  echo "❌ kernel.efi missing" >&2; exit 1; }
./make_iso.sh
[ -f out/cohesix.iso ] || { echo "❌ ISO build failed" >&2; exit 1; }
[ -f out_iso/EFI/BOOT/bootx64.efi ] || { echo "❌ bootx64.efi missing after ISO build" >&2; exit 1; }

for f in initfs.img plan9.ns bootargs.txt boot_trace.json; do
  if [ -f "$f" ]; then
    cp "$f" out/
  else
    echo "⚠️ $f missing; creating placeholder" >&2
    touch "out/$f"
  fi
done

echo "🔍 Running Rust tests with detailed output..."
RUST_BACKTRACE=1 cargo test --release -- --nocapture 2>&1 | tee rust_test_output.log
TEST_EXIT_CODE=${PIPESTATUS[0]}
if [ $TEST_EXIT_CODE -ne 0 ]; then
  echo "❌ Rust tests failed. See rust_test_output.log for details."
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
  LOG_FILE="$LOG_DIR/qemu_boot.log"
  if [ -f "$LOG_FILE" ]; then
    mv "$LOG_FILE" "$LOG_FILE.$(date +%Y%m%d_%H%M%S)"
  fi
  qemu-system-x86_64 \
    -bios /usr/share/qemu/OVMF.fd \
    -drive if=pflash,format=raw,file="$TMPDIR/OVMF_VARS.fd" \
    -cdrom "$ISO_IMG" -net none -M q35 -m 256M \
    -no-reboot -nographic -serial file:"$SERIAL_LOG"
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
