# CLASSIFICATION: COMMUNITY
# Filename: cohesix_fetch_build.sh v0.17
# Author: Lukas Bower
# Date Modified: 2025-09-21
#!/bin/bash
# Fetch and fully build the Cohesix project using SSH Git auth.

set -euo pipefail
LOG_DIR="$HOME/cohesix_logs"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/build_$(date +%Y%m%d_%H%M%S).log"
SUMMARY_ERRORS="$LOG_DIR/summary_errors.log"
SUMMARY_TEST_FAILS="$LOG_DIR/summary_test_failures.log"
: > "$SUMMARY_ERRORS"
: > "$SUMMARY_TEST_FAILS"
exec 3>&1  # Save original stdout
exec > >(tee -a "$LOG_FILE") 2>&1
trap 'echo "❌ Build failed. Last 40 log lines:" >&3; tail -n 40 "$LOG_FILE" >&3' ERR

log(){ echo "[$(date +%H:%M:%S)] $1" | tee -a "$LOG_FILE" >&3; }

log "🛠️ [Build Start] $(date)"

cd "$HOME"
log "🧹 Cleaning workspace..."
rm -rf cohesix

log "📦 Cloning repository..."
git clone git@github.com:lukeb-aidev/cohesix.git
cd cohesix
ROOT="$(pwd)"

log "📦 Updating submodules (if any)..."
git submodule update --init --recursive

log "🐍 Setting up Python environment..."
command -v python3 >/dev/null || { echo "❌ python3 not found" >&2; exit 1; }
python3 -m venv .venv
source .venv/bin/activate
pip install --upgrade pip setuptools wheel

[ -f requirements.txt ] && pip install -r requirements.txt

log "🧱 Building Rust components..."
cargo build --all-targets --release
grep -Ei 'error|fail|panic|permission denied|warning' "$LOG_FILE" > "$SUMMARY_ERRORS" || true

# Ensure output directory exists before copying Rust binaries
mkdir -p out/bin

# Confirm cohcc built successfully before copying
rm -f out/bin/cohcc
if [[ -f target/release/cohcc ]]; then
  cp target/release/cohcc out/bin/cohcc
else
  echo "❌ cohcc not found at expected location" >&2
  exit 1
fi

# Copy other Rust CLI binaries into out/bin for ISO staging
for bin in cohbuild cohcap cohtrace cohrun_cli; do
  BIN_PATH="target/release/$bin"
  [ -f "$BIN_PATH" ] && cp "$BIN_PATH" "out/bin/$bin"
done

# Stage shell wrappers for Python CLI tools
for script in cohcli cohcap cohtrace cohrun cohbuild cohup cohpkg; do
  [ -f "bin/$script" ] && cp "bin/$script" "out/bin/$script"
done

TARGET="x86_64-unknown-uefi"
log "🛠️ Building kernel EFI..."
mkdir -p out/bin out/etc/cohesix out/roles out/setup
cargo build --release --target "$TARGET" --bin kernel \
  --no-default-features --features minimal_uefi,kernel_bin
KERNEL_EFI="target/${TARGET}/release/kernel.efi"
[ -f "$KERNEL_EFI" ] || { echo "❌ kernel.efi missing" >&2; exit 1; }
cp "$KERNEL_EFI" out/kernel.efi

log "🛠️ Building init EFI..."
cargo build --release --target "$TARGET" --bin init \
  --no-default-features --features minimal_uefi
INIT_EFI="target/${TARGET}/release/init.efi"
if [ ! -f "$INIT_EFI" ]; then
  echo "❌ init EFI missing at $INIT_EFI" >&2
  exit 1
fi
cp "$INIT_EFI" out/bin/init.efi
cp "$INIT_EFI" out/init.efi
[ -f out/init.efi ] || { echo "❌ init EFI missing after build" >&2; exit 1; }
if [[ ! -f out/init.efi ]]; then
  echo "❌ init.efi missing — build incomplete" | tee -a "$LOG_FILE"
fi

log "📂 Staging boot files..."
for f in initfs.img plan9.ns bootargs.txt boot_trace.json; do
  [ -f "$f" ] && cp "$f" out/
done

log "📂 Staging configuration..."
[ -f setup/config.yaml ] || { echo "❌ setup/config.yaml missing" >&2; exit 1; }
cp setup/config.yaml out/etc/cohesix/config.yaml
if ls setup/roles/*.yaml >/dev/null 2>&1; then
  for cfg in setup/roles/*.yaml; do
    role="$(basename "$cfg" .yaml)"
    mkdir -p "out/roles/$role"
    cp "$cfg" "out/roles/$role/config.yaml"
  done
else
  echo "❌ No role configs found in setup/roles" >&2
  exit 1
fi
for shf in setup/init.sh setup/*.sh; do
  [ -f "$shf" ] && cp "$shf" out/setup/
done


log "🔍 Running Rust tests with detailed output..."
RUST_BACKTRACE=1 cargo test --release -- --nocapture
TEST_EXIT_CODE=$?
grep -A 5 -E '^failures:|thread .* panicked at' "$LOG_FILE" > "$SUMMARY_TEST_FAILS" || true
if [ $TEST_EXIT_CODE -ne 0 ]; then
  echo "❌ Rust tests failed." | tee -a "$LOG_FILE" >&3
fi
grep -Ei 'error|fail|panic|permission denied|warning' "$LOG_FILE" > "$SUMMARY_ERRORS" || true

if command -v go &> /dev/null; then
  log "🐹 Building Go components..."
  mkdir -p out/bin
  (cd go/cmd/coh-9p-helper && go build -o "$ROOT/out/bin/coh-9p-helper")
  (cd go/cmd/gui-orchestrator && go build -o "$ROOT/out/bin/gui-orchestrator")
  (cd go && go test ./...)
else
  log "⚠️ Go not found; skipping Go build"
fi

log "🐍 Running Python tests..."
if command -v pytest &> /dev/null; then
  pytest -v || true
fi
if command -v flake8 &> /dev/null; then
  flake8 python tests || true
fi

log "🧱 Building C components..."
if [ -f CMakeLists.txt ]; then
  mkdir -p build
  (cd build && cmake .. && make -j$(nproc))
fi

echo "✅ All builds complete."

log "📀 Creating ISO..."
# ISO root layout:
#   out/iso_root/bin            - runtime binaries (kernel, init, busybox)
#   out/iso_root/usr/bin        - CLI wrappers and Go tools
#   out/iso_root/usr/cli        - Python CLI modules
#   out/iso_root/home/cohesix   - Python libraries
#   out/iso_root/etc            - configuration files
#   out/iso_root/roles          - role definitions
if [ "${VIRTUAL_ENV:-}" != "$(pwd)/.venv" ]; then
  echo "❌ Python venv not active before ISO build" >&2
  exit 1
fi
./scripts/make_iso.sh
[ -f out/cohesix.iso ] || { echo "❌ ISO build failed" >&2; exit 1; }
ISO_SIZE=$(stat -c %s out/cohesix.iso)
if [ "$ISO_SIZE" -le $((1024*1024)) ]; then
  echo "❌ ISO build incomplete or missing required tools" >&2
  exit 1
fi
if command -v xorriso >/dev/null; then
  xorriso -indev out/cohesix.iso -find / -name kernel.efi -print | grep -q kernel.efi || {
    echo "❌ kernel.efi missing in ISO" >&2; exit 1; }
else
  log "⚠️ xorriso not found; skipping ISO content check"
fi
if [[ ! -f out/init.efi ]]; then
  echo "❌ init.efi missing — build incomplete" | tee -a "$LOG_FILE"
fi
grep -Ei 'error|fail|panic|permission denied|warning' "$LOG_FILE" > "$SUMMARY_ERRORS" || true

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
  log "🧪 Booting ISO in QEMU..."
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
    log "✅ QEMU boot succeeded"
  else
    echo "❌ BOOT_OK not found in log" >&2
    exit 1
  fi
else
  log "⚠️ qemu-system-x86_64 not installed; skipping boot test"
fi

log "✅ [Build Complete] $(date)"

grep -Ei 'error|fail|panic|permission denied|warning' "$LOG_FILE" > "$SUMMARY_ERRORS" || true
grep -A 5 -E '^failures:|thread .* panicked at' "$LOG_FILE" > "$SUMMARY_TEST_FAILS" || true

echo "⚠️  Summary of Errors and Warnings:" | tee -a "$LOG_FILE" >&3
tail -n 10 "$SUMMARY_ERRORS" || echo "✅ No critical issues found" | tee -a "$LOG_FILE" >&3

echo "🪵 Full log saved to $LOG_FILE" >&3
echo "✅ ISO build complete. Run QEMU with:" >&3
echo "qemu-system-x86_64 -cdrom out/cohesix.iso -boot d -m 1024" >&3
