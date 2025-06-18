# CLASSIFICATION: COMMUNITY
# Filename: cohesix_fetch_build.sh v0.13
# Author: Lukas Bower
# Date Modified: 2025-09-17
#!/bin/bash
# Fetch and fully build the Cohesix project using SSH Git auth.

set -euo pipefail
LOG_FILE=~/cohesix_build.log
rm -f "$LOG_FILE"
exec 3>&1  # Save original stdout
exec > "$LOG_FILE" 2>&1
trap 'echo "❌ Build failed. Last 40 log lines:" >&3; tail -n 40 "$LOG_FILE" >&3' ERR

cd "$HOME"
echo "[1/5] 🧹 Cleaning workspace..." >&3
rm -rf cohesix

echo "[2/5] 📦 Cloning repository..." >&3
git clone git@github.com:lukeb-aidev/cohesix.git
cd cohesix

echo "📦 Updating submodules (if any)..."
git submodule update --init --recursive

echo "[3/5] 🐍 Setting up Python environment..." >&3
command -v python3 >/dev/null || { echo "❌ python3 not found"; exit 1; }
python3 -m venv .venv
source .venv/bin/activate
pip install --upgrade pip setuptools wheel

if [ -f requirements.txt ]; then
  pip install -r requirements.txt
fi

echo "[4/5] 🧱 Building Rust components..." >&3
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

echo "[5/5] 📀 Creating ISO image..." >&3
./scripts/make_iso.sh
[ -f out/cohesix.iso ] || { echo "❌ ISO build failed" >&2; exit 1; }

echo "🔍 Running Rust tests with detailed output..."
TEST_LOG="$HOME/cohesix_test.log"
ERROR_LOG="$HOME/cohesix_test_errors.log"
RUST_BACKTRACE=1 cargo test --release -- --nocapture > "$TEST_LOG" 2>&1
TEST_EXIT_CODE=$?
if [ $TEST_EXIT_CODE -ne 0 ]; then
  echo "❌ Rust tests failed. See $TEST_LOG for details." >&2
  grep -i "error" "$TEST_LOG" > "$ERROR_LOG" 2>&1 || true
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

echo "✅ Cohesix build completed successfully." >&3
echo "🪵 Full log saved to $LOG_FILE" >&3
