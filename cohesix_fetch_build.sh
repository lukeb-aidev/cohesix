// CLASSIFICATION: COMMUNITY
// Filename: cohesix_fetch_build.sh v0.3
// Author: Lukas Bower
// Date Modified: 2025-06-15
#!/bin/bash
# Fetch and fully build the Cohesix project using SSH Git auth.

set -euo pipefail

timestamp=$(date +%Y%m%d_%H%M%S)
cd ~

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
python3 -m venv .venv
source .venv/bin/activate
pip install --upgrade pip setuptools wheel

if [ -f requirements.txt ]; then
  pip install -r requirements.txt
fi

echo "🦀 Building Rust components..."
cargo build --release

echo "🔍 Running Rust tests with detailed output..."
RUST_BACKTRACE=1 cargo test --release -- --nocapture 2>&1 | tee rust_test_output.log
TEST_EXIT_CODE=${PIPESTATUS[0]}
if [ $TEST_EXIT_CODE -ne 0 ]; then
  echo "❌ Rust tests failed. See rust_test_output.log for details."
  exit $TEST_EXIT_CODE
else
  echo "✅ Rust tests passed."
fi

echo "🐹 Building Go components..."
if [ -f go.mod ]; then
  go build ./...
  go test ./...
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
  TMPDIR="${TMPDIR:-$(mktemp -d)}"
  DISK_DIR="$TMPDIR/qemu_disk"
  LOG_FILE="$TMPDIR/qemu_boot.log"
  mkdir -p "$DISK_DIR"
  qemu-system-x86_64 -kernel out/kernel.elf -nographic -serial file:"$LOG_FILE" &
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
