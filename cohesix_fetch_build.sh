#!/bin/bash
# CLASSIFICATION: COMMUNITY
# Filename: cohesix_fetch_build.sh v0.90
# Author: Lukas Bower
# Date Modified: 2025-07-05

set -euxo pipefail

# == Initialization ==
HOST_ARCH="$(uname -m)"
VENV_DIR=".venv_${HOST_ARCH}"
LOG_DIR="$HOME/cohesix_logs"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/build_$(date +%Y%m%d_%H%M%S).log"
SUMMARY_ERRORS="$LOG_DIR/summary_errors.log"
SUMMARY_TEST_FAILS="$LOG_DIR/summary_test_failures.log"
: > "$SUMMARY_ERRORS"
: > "$SUMMARY_TEST_FAILS"
exec 3>&1
exec > >(tee -a "$LOG_FILE") 2>&1
trap 'echo "❌ Build failed. Last 40 log lines:" >&3; tail -n 40 "$LOG_FILE" >&3' ERR
log(){ echo "[$(date +%H:%M:%S)] $1" | tee -a "$LOG_FILE" >&3; }

# == Python environment ==
if [ -z "${VIRTUAL_ENV:-}" ] || [[ "$VIRTUAL_ENV" != *"/${VENV_DIR}" ]]; then
  if [ -d "$VENV_DIR" ]; then
    echo "🔄 Activating existing virtualenv: $VENV_DIR"
    source "$VENV_DIR/bin/activate"
  else
    echo "⚙️ Creating new virtualenv: $VENV_DIR"
    python3 -m venv "$VENV_DIR"
    source "$VENV_DIR/bin/activate"
  fi
fi

# == Clone repo fresh ==
cd "$HOME"
log "📦 Cloning repository..."
rm -rf cohesix
rm -rf cohesix_logs
for i in {1..3}; do
  git clone git@github.com:lukeb-aidev/cohesix.git && break || sleep 1
done
log "✅ Clone complete ..."
cd cohesix
ROOT="$(pwd)"

# == Architecture config ==
if [ -f "$ROOT/scripts/load_arch_config.sh" ]; then
  source "$ROOT/scripts/load_arch_config.sh"
else
  echo "❌ Missing: $ROOT/scripts/load_arch_config.sh" >&2
  exit 1
fi
log "Architecture: $COHESIX_ARCH (seL4+ELF only, no UEFI/PE32 build)"

# == Toolchains ==
command -v rustup >/dev/null 2>&1 || { echo "❌ rustup not found." >&2; exit 1; }
rustup target list --installed | grep -q "^aarch64-unknown-linux-musl$" || rustup target add aarch64-unknown-linux-musl
command -v aarch64-linux-musl-gcc >/dev/null 2>&1 || { echo "❌ aarch64-linux-musl-gcc missing" >&2; exit 1; }
command -v ld.lld >/dev/null 2>&1 || { echo "❌ ld.lld not found" >&2; exit 1; }
ld.lld --version >&3

# == CUDA detection ==
if [ -z "${CUDA_HOME:-}" ]; then
  if command -v nvcc >/dev/null 2>&1; then
    CUDA_HOME="$(dirname "$(dirname "$(command -v nvcc)")")"
  else
    CUDA_HOME="/usr"
  fi
fi
export CUDA_HOME PATH="$CUDA_HOME/bin:$PATH"
[ -f "$CUDA_HOME/include/cuda.h" ] || { echo "❌ cuda.h not found." >&2; exit 1; }
log "✅ Found cuda.h in $CUDA_HOME/include"

# == Build sequence ==
log "📦 Updating submodules..."
git submodule update --init --recursive

log "🐍 Setting up Python environment..."
pip install ply lxml --break-system-packages
python -m pip install --upgrade pip setuptools wheel --break-system-packages

# == CMake, BusyBox, Rust ==
log "🧱 Building C components..."
[ -f CMakeLists.txt ] && { mkdir -p build; (cd build && cmake .. && make -j$(nproc)); }

log "📦 Building BusyBox..."
scripts/build_busybox.sh "$COHESIX_ARCH"
BUSYBOX_BIN="out/busybox/$COHESIX_ARCH/bin/busybox"
if [ -x "$BUSYBOX_BIN" ]; then
  cp "$BUSYBOX_BIN" out/bin/busybox
  for app in sh ls cat echo mount umount; do
    ln -sf busybox out/bin/$app
  done
else
  echo "❌ BusyBox build failed" >&2
  exit 1
fi

log "🧱 Building Rust binaries..."
FEATURES="std,busybox"
RUSTFLAGS="-C debuginfo=2" \
  cargo build --release --bin cohesix_root \
  --no-default-features --features "$FEATURES" \
  --target aarch64-unknown-linux-musl

# == Config, roles, manifest ==
mkdir -p out/etc/cohesix
cat > out/etc/cohesix/config.yaml <<EOF
# CLASSIFICATION: COMMUNITY
# Filename: config.yaml
role: QueenPrimary
network:
  enabled: true
EOF
log "✅ Config written to out/etc/cohesix/config.yaml"

mkdir -p out/etc
cat > out/etc/plan9.ns <<'EOF'
// CLASSIFICATION: COMMUNITY
bind -a /bin /bin
bind -a /usr/py /usr/py
bind -a /srv /srv
bind -a /mnt/9root /
EOF
log "✅ plan9.ns written"

if ls setup/roles/*.yaml >/dev/null 2>&1; then
  for cfg in setup/roles/*.yaml; do
    role="$(basename "$cfg" .yaml)"
    mkdir -p "out/roles/$role"
    cp "$cfg" "out/roles/$role/config.yaml"
  done
else
  echo "❌ No role configs found" >&2
  exit 1
fi

MANIFEST="out/manifest.json"
echo '{"binaries":[' > "$MANIFEST"
first=1
for bin in $(find out/bin -type f -perm -111); do
  hash=$(sha256sum "$bin" | awk '{print $1}')
  ver=$(git rev-parse --short HEAD)
  [ $first -eq 0 ] && echo ',' >> "$MANIFEST"
  first=0
  printf '{"file":"%s","hash":"%s","version":"%s"}' "${bin#out/}" "$hash" "$ver" >> "$MANIFEST"
done
echo ']}' >> "$MANIFEST"
log "✅ Manifest created at $MANIFEST"

# == ELF validation ==
ROOT_SIZE=$(stat -c%s out/cohesix_root.elf)
[ "$ROOT_SIZE" -gt $((100*1024*1024)) ] && { echo "❌ cohesix_root ELF exceeds 100MB." >&2; exit 1; }
readelf -l out/cohesix_root.elf | tee -a "$LOG_FILE" >&3

# == Direct QEMU test ==
log "🧪 Booting elfloader + cohesix_root ELF in QEMU directly..."
QEMU_LOG="$LOG_DIR/qemu_direct_$(date +%Y%m%d_%H%M%S).log"
qemu-system-aarch64 -M virt -cpu cortex-a57 -m 512M \
  -kernel "out/bin/elfloader" \
  -serial mon:stdio -nographic \
  -d int,mmu,page,guest_errors,unimp,cpu_reset \
  -D "$QEMU_LOG" | tee "$LOG_DIR/qemu_serial_direct.log"

QEMU_EXIT=${PIPESTATUS[0]}
tail -n 20 "$LOG_DIR/qemu_serial_direct.log" || echo "❌ Could not tail QEMU log"
[ "$QEMU_EXIT" -ne 0 ] && { echo "❌ QEMU exited with code $QEMU_EXIT" >&2; exit 1; }
grep -q "BOOT_OK" "$LOG_DIR/qemu_serial_direct.log" && log "✅ Direct QEMU boot confirmed" \
  || { echo "❌ BOOT_OK not found in QEMU log" >&2; exit 1; }

log "✅ Build complete and direct QEMU boot verified."