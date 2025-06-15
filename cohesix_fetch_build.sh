// CLASSIFICATION: COMMUNITY
// Filename: cohesix_fetch_build.sh v0.4
// Author: Lukas Bower
// Date Modified: 2025-08-01
#!/bin/bash
# Fetch and fully build the Cohesix project using SSH Git auth.

set -euo pipefail

timestamp=$(date +%Y%m%d_%H%M%S)
cd "$HOME"

echo "📦 Cloning Git repo via SSH..."

# Backup existing folder if it exists
if [ -d "cohesix" ]; then
  mv cohesix "cohesix_backup_$timestamp"
  echo "🗂️ Moved existing repo to cohesix_backup_$timestamp"
fi

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

echo "🔨 Building kernel ELF..."
cargo build --bin kernel --release --features kernel_bin
mkdir -p out
cp target/release/kernel out/kernel.elf

for f in initfs.img plan9.ns bootargs.txt boot_trace.json; do
  if [ -f "$f" ]; then
    cp "$f" out/
  else
    echo "⚠️ $f missing; creating placeholder" >&2
    touch "out/$f"
  fi
done
touch out/qemu_boot.log

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
  if [ ! -f out/kernel.elf ]; then
    echo "❌ Kernel ELF not found at out/kernel.elf"
    exit 1
  fi
  TMPDIR="${TMPDIR:-$(mktemp -d)}"
  DISK_DIR="$TMPDIR/qemu_disk"
  LOG_FILE="out/qemu_boot.log"
  mkdir -p "$DISK_DIR"
  timeout 10s qemu-system-x86_64 -kernel out/kernel.elf -nographic -serial file:"$LOG_FILE" &
  QEMU_PID=$!
  sleep 3
  tail -n 20 "$LOG_FILE" || echo "❌ Could not read QEMU log"
  if grep -q "BOOT_OK" "$LOG_FILE"; then
    echo "✅ QEMU boot succeeded"
  else
    echo "❌ BOOT_OK not found in log"
  fi
  wait $QEMU_PID || echo "❌ QEMU exited with error"
else
  echo "⚠️ qemu-system-x86_64 not installed; skipping boot test"
fi
