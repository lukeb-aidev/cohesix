# CLASSIFICATION: COMMUNITY
# Filename: cohesix_fetch_build.sh v0.87
# Author: Lukas Bower
# Date Modified: 2026-12-31
#!/bin/bash
#
# Bare metal seL4 build flow (no UEFI):
# 1. Run init-build.sh with debug flags to configure seL4 for qemu-arm-virt.
# 2. Build kernel.elf and elfloader via ninja.
# 3. Stage kernel.elf, elfloader, and root ELF under $ROOT/out/bin/ for QEMU.


HOST_ARCH="$(uname -m)"
if [[ "$HOST_ARCH" = "aarch64" ]] && ! command -v aarch64-linux-musl-gcc >/dev/null 2>&1; then
  if command -v sudo >/dev/null 2>&1; then
    SUDO=sudo
  else
    SUDO=""
  fi
  echo "Missing aarch64-linux-musl-gcc. Attempting install via apt" >&2
  if ! $SUDO apt update && ! $SUDO apt install -y musl-tools gcc-aarch64-linux-musl; then
    echo "ERROR: Missing aarch64-linux-musl-gcc. Install with:\nsudo apt update && sudo apt install musl-tools gcc-aarch64-linux-musl" >&2
    exit 1
  fi
  if ! command -v aarch64-linux-musl-gcc >/dev/null 2>&1; then
    echo "ERROR: Missing aarch64-linux-musl-gcc. Install with:\nsudo apt update && sudo apt install musl-tools gcc-aarch64-linux-musl" >&2
    exit 1
  fi
fi
# Fetch and fully build the Cohesix project using SSH Git auth.

set -euxo pipefail
# Early virtualenv setup
VENV_DIR=".venv_${HOST_ARCH}"
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
export PYTHONPATH="/home/ubuntu/sel4_workspace/kernel:/usr/local/lib/python3.12/dist-packages:${PYTHONPATH:-}"
export MEMCHR_DISABLE_RUNTIME_CPU_FEATURE_DETECTION=1
export CUDA_HOME="${CUDA_HOME:-/usr}"
export CUDA_INCLUDE_DIR="${CUDA_INCLUDE_DIR:-$CUDA_HOME/include}"
export CUDA_LIBRARY_PATH="${CUDA_LIBRARY_PATH:-/usr/lib/x86_64-linux-gnu}"
export PATH="$CUDA_HOME/bin:$PATH"
export LD_LIBRARY_PATH="$CUDA_LIBRARY_PATH:${LD_LIBRARY_PATH:-}"
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

# Clone repository before sourcing any configuration so a fresh checkout
# is available even when $HOME is empty.
cd "$HOME"
log "📦 Cloning repository..."
rm -rf cohesix
for i in {1..3}; do
  git clone git@github.com:lukeb-aidev/cohesix.git && break || sleep 3
done
log "✅ Clone complete ..."

cd cohesix
ROOT="$(pwd)"
if [ -f "$ROOT/scripts/load_arch_config.sh" ]; then
  source "$ROOT/scripts/load_arch_config.sh"
else
  echo "❌ Missing: $ROOT/scripts/load_arch_config.sh" >&2
  exit 1
fi

COH_ARCH="$COHESIX_ARCH"
log "Architecture: $COH_ARCH (seL4+ELF only, no UEFI/PE32 build)"


# Toolchain sanity checks
if ! command -v rustup >/dev/null 2>&1; then
  echo "❌ rustup not found. Install Rust toolchains before running" >&2
  exit 1
fi
if ! rustup target list --installed | grep -q "^aarch64-unknown-linux-musl$"; then
  echo "🔧 Installing missing Rust target aarch64-unknown-linux-musl" >&2
  rustup target add aarch64-unknown-linux-musl
fi
command -v aarch64-linux-musl-gcc >/dev/null 2>&1 || { echo "❌ aarch64-linux-musl-gcc missing" >&2; exit 1; }
command -v ld.lld >/dev/null 2>&1 || { echo "❌ ld.lld not found" >&2; exit 1; }
ld.lld --version >&3


# Optional seL4 entry build flag
SEL4_ENTRY=0
if [[ ${1:-} == --sel4-entry ]]; then
  SEL4_ENTRY=1
  shift
fi


# CUDA detection and environment setup
CUDA_HOME="${CUDA_HOME:-}"
if [ -z "$CUDA_HOME" ]; then
  if command -v nvcc >/dev/null 2>&1; then
    NVCC_PATH="$(command -v nvcc)"
    CUDA_HOME="$(dirname "$(dirname "$NVCC_PATH")")"
  elif [ -d /usr/local/cuda ]; then
    CUDA_HOME="/usr/local/cuda"
  else
    shopt -s nullglob
    CUDA_MATCHES=(/usr/local/cuda-*arm64 /usr/local/cuda-*)
    CUDA_HOME="${CUDA_MATCHES[0]:-}"
    # Manual override for environments where cuda.h is in /usr/include but no nvcc exists
    if [ "$CUDA_HOME" = "/usr" ] && [ -f "/usr/include/cuda.h" ]; then
      export CUDA_INCLUDE_DIR="/usr/include"
      export CUDA_LIBRARY_PATH="/usr/lib/x86_64-linux-gnu"
      export LD_LIBRARY_PATH="$CUDA_LIBRARY_PATH:$LD_LIBRARY_PATH"
      log "✅ Manually set CUDA paths for cust_raw: CUDA_HOME=$CUDA_HOME"
    fi
    shopt -u nullglob
    if [ -z "$CUDA_HOME" ] || [ ! -d "$CUDA_HOME" ]; then
      CUDA_HOME="/usr"
    fi
  fi
fi

# Log CUDA fallback paths
log "CUDA fallback paths tried: ${CUDA_MATCHES[*]:-none found}"

export CUDA_HOME
export PATH="$CUDA_HOME/bin:$PATH"
if [ -d "$CUDA_HOME/lib64" ]; then
  export LD_LIBRARY_PATH="$CUDA_HOME/lib64:${LD_LIBRARY_PATH:-}"
elif [ -d "$CUDA_HOME/lib" ]; then
  export LD_LIBRARY_PATH="$CUDA_HOME/lib:${LD_LIBRARY_PATH:-}"
fi
# Add robust library path fallback for common distros
if [ -d "/usr/lib/x86_64-linux-gnu" ]; then
  export LD_LIBRARY_PATH="/usr/lib/x86_64-linux-gnu:$LD_LIBRARY_PATH"
fi
export CUDA_LIBRARY_PATH="$LD_LIBRARY_PATH"

if [ -f "$CUDA_HOME/include/cuda.h" ]; then
  log "✅ Found cuda.h in $CUDA_HOME/include"
else
  echo "❌ cuda.h not found in $CUDA_HOME/include. Check CUDA installation." >&2
  exit 1
fi

if [ -n "$CUDA_HOME" ] && [ -f "$CUDA_HOME/bin/nvcc" ]; then
  log "CUDA detected at $CUDA_HOME"
  if nvcc --version >/tmp/nvcc_check.log 2>&1; then
    log "nvcc OK: $(grep -m1 release /tmp/nvcc_check.log)"
  else
    log "⚠️ nvcc failed: $(cat /tmp/nvcc_check.log)"
  fi
  if command -v nvidia-smi >/dev/null 2>&1; then
    if nvidia-smi >/tmp/nvidia_smi.log 2>&1; then
      log "nvidia-smi OK: $(grep -m1 'Driver Version' /tmp/nvidia_smi.log)"
    else
      log "⚠️ nvidia-smi failed: $(cat /tmp/nvidia_smi.log)"
    fi
  else
    log "⚠️ nvidia-smi not found"
  fi
else
  log "⚠️ CUDA toolkit not detected. nvcc not found or invalid CUDA_HOME: $CUDA_HOME"
fi

if [ "$COH_ARCH" = "aarch64" ] && command -v rustup >/dev/null 2>&1; then
  if ! rustup target list --installed | grep -q '^aarch64-unknown-linux-musl$'; then
    rustup target add aarch64-unknown-linux-musl
    log "✅ Rust target aarch64-unknown-linux-musl installed"
  fi
fi

if [ "$COH_ARCH" != "x86_64" ]; then
  CROSS_X86="x86_64-linux-gnu-"
else
  CROSS_X86=""
fi

log "📦 Installing build dependencies..."
if command -v sudo >/dev/null 2>&1; then
  SUDO="sudo"
else
  SUDO=""
fi
$SUDO apt-get update -y
$SUDO apt-get install -y build-essential ninja-build git wget \
  python3 python3-pip cmake gcc-aarch64-linux-gnu

CMAKE_VER=$(cmake --version 2>/dev/null | head -n1 | awk '{print $3}')
if ! dpkg --compare-versions "$CMAKE_VER" ge 3.20; then
  log "cmake $CMAKE_VER too old; installing newer release binary"
  TMP_CMAKE="$(mktemp -d)"
  CMAKE_V=3.28.1
  ARCH=$(uname -m)
  case "$ARCH" in
    aarch64|arm64)
      CMAKE_PKG="cmake-${CMAKE_V}-linux-aarch64.tar.gz";;
    *)
      CMAKE_PKG="cmake-${CMAKE_V}-linux-x86_64.tar.gz";;
  esac
  wget -q "https://github.com/Kitware/CMake/releases/download/v${CMAKE_V}/${CMAKE_PKG}" -O "$TMP_CMAKE/$CMAKE_PKG"
  tar -xf "$TMP_CMAKE/$CMAKE_PKG" -C "$TMP_CMAKE"
  $SUDO cp -r "$TMP_CMAKE"/cmake-${CMAKE_V}-linux-*/{bin,share} /usr/local/
  rm -rf "$TMP_CMAKE"
  hash -r
fi

cd "$ROOT"
STAGE_DIR="$ROOT/out/iso"
GO_HELPERS_DIR="$ROOT/out/go_helpers"
mkdir -p "$ROOT/out/bin" "$GO_HELPERS_DIR"
# Clean up artifacts from previous builds
rm -f "$ROOT/out/bin/init.efi" "$ROOT/out/boot/kernel.elf" 2>/dev/null || true

# Detect platform and GPU availability
COH_PLATFORM="$(uname -m)"
case "$COH_PLATFORM" in
  x86_64|amd64)
    COH_ARCH="x86_64"
    ;;
  aarch64|arm64)
    COH_ARCH="aarch64"
    ;;
  *)
    COH_ARCH="$COH_PLATFORM"
    ;;
esac

COH_GPU=0
if command -v nvidia-smi >/dev/null 2>&1; then
  if nvidia-smi > /tmp/nvidia_smi.log 2>&1; then
    COH_GPU=1
    log "nvidia-smi output:" && cat /tmp/nvidia_smi.log
  else
    log "nvidia-smi present but failed: $(cat /tmp/nvidia_smi.log)"
  fi
elif [ -c /dev/nvidia0 ]; then
  COH_GPU=1
elif command -v lspci >/dev/null 2>&1 && lspci | grep -qi nvidia; then
  COH_GPU=1
fi
export COH_ARCH COH_GPU
export COH_PLATFORM="$COH_ARCH"
log "Detected platform: $COH_ARCH, GPU=$COH_GPU"

log "📦 Updating submodules (if any)..."
git submodule update --init --recursive

log "🐍 Setting up Python environment..."
pip install ply lxml --break-system-packages
# Ensure $HOME/.local/bin is included for user installs
export PATH="$HOME/.local/bin:$PATH"
# Upgrade pip and base tooling; fall back to ensurepip if needed
python -m pip install --upgrade pip setuptools wheel --break-system-packages \
  || python -m ensurepip --upgrade

if [ -f requirements.txt ]; then
  python -m pip install -r requirements.txt --break-system-packages
fi
if [ -f tests/requirements.txt ]; then
  python -m pip install -r tests/requirements.txt --break-system-packages
fi


log "🔧 Checking C compiler..."
if ! command -v gcc >/dev/null 2>&1; then
  echo "❌ gcc not found. Install with: sudo apt install build-essential" >&2
  exit 1
fi
CC_TEST_C="$(mktemp --suffix=.c cohesix_cc_test.XXXX)"
cat <<'EOF' > "$CC_TEST_C"
#include <stdio.h>
int main(void){ printf("hello\n"); return 0; }
EOF
CC_TEST_BIN="${CC_TEST_C%.c}"
if gcc "$CC_TEST_C" -o "$CC_TEST_BIN" >/dev/null 2>&1 && "./$CC_TEST_BIN" >/dev/null; then
  log "✅ C compiler operational"
  rm -f "$CC_TEST_C" "$CC_TEST_BIN"
else
  echo "❌ C compiler or linker failed" >&2
  rm -f "$CC_TEST_C" "$CC_TEST_BIN"
  exit 1
fi

log "🧱 Building C components..."
if [ -f CMakeLists.txt ]; then
  mkdir -p build
  (cd build && cmake .. && make -j$(nproc))
fi

log "📦 Building BusyBox..."
scripts/build_busybox.sh "$COH_ARCH"
BUSYBOX_BIN="out/busybox/$COH_ARCH/bin/busybox"
if [ -x "$BUSYBOX_BIN" ]; then
  cp "$BUSYBOX_BIN" "$STAGE_DIR/bin/busybox"
  for app in sh ls cat echo mount umount; do
    ln -sf busybox "$STAGE_DIR/bin/$app"
  done
  if [ -f "userland/miniroot/bin/init" ]; then
    cp "userland/miniroot/bin/init" "$STAGE_DIR/bin/init"
    chmod +x "$STAGE_DIR/bin/init"
  fi
  if [ -f "userland/miniroot/bin/rc" ]; then
    cp "userland/miniroot/bin/rc" "$STAGE_DIR/bin/rc"
    chmod +x "$STAGE_DIR/bin/rc"
  fi
else
  echo "❌ BusyBox build failed" >&2
  exit 1
fi

log "🧱 Building Rust components for seL4 rootserver ELF..."
FEATURES="std,rapier,physics,busybox"
if [ "$SEL4_ENTRY" = 1 ]; then
  FEATURES+=",sel4,kernel_bin,minimal_uefi"
fi
RUSTFLAGS="-C debuginfo=2" \
  cargo build --release --bin cohesix_root \
  --no-default-features --features "$FEATURES" \
  --target aarch64-unknown-linux-musl
grep -Ei 'error|fail|panic|permission denied|warning' "$LOG_FILE" > "$SUMMARY_ERRORS" || true

# Ensure output directory exists before copying Rust binaries
mkdir -p "$STAGE_DIR/bin" "$STAGE_DIR/usr/bin" "$STAGE_DIR/usr/cli" "$STAGE_DIR/home/cohesix"

# Copy Rust CLI binaries into out/bin for ISO staging (copy only, skip build)
for bin in cohcc cohesix_build cohesix_cap cohesix_trace cohrun_cli cohagent cohrole cohrun cohup cohesix_root kernel logdemo init sel4_entry; do
  BIN_PATH="target/aarch64-unknown-linux-musl/release/$bin"
  if [ -f "$BIN_PATH" ]; then
    cp "$BIN_PATH" "$STAGE_DIR/bin/$bin"
    cp "$BIN_PATH" "$ROOT/out/bin/$bin"
  else
    echo "⚠️ $bin not found at $BIN_PATH" >&2
  fi
done

# Stage shell wrappers for Python CLI tools
for script in cohcli cohcap cohtrace cohrun cohbuild cohup cohpkg; do
  if [ -f "bin/$script" ]; then
    cp "bin/$script" "$STAGE_DIR/bin/$script"
    cp "bin/$script" "$STAGE_DIR/usr/bin/$script"
    sed -i '1c #!/usr/bin/env python3' "$STAGE_DIR/bin/$script"
    sed -i '1c #!/usr/bin/env python3' "$STAGE_DIR/usr/bin/$script"
    chmod +x "$STAGE_DIR/bin/$script" "$STAGE_DIR/usr/bin/$script"
  fi
done


cd "$ROOT"
log "🧱 Staging root ELF for seL4..."
# Copy root ELF from cargo build output to out/cohesix_root.elf
ROOT_ELF_SRC="target/aarch64-unknown-linux-musl/release/cohesix_root"
if [ -f "$ROOT_ELF_SRC" ]; then
  cp "$ROOT_ELF_SRC" out/cohesix_root.elf
  mkdir -p "$ROOT/out/bin"
  cp "$ROOT_ELF_SRC" "$ROOT/out/bin/cohesix_root.elf"
  log "Root ELF size: $(stat -c%s "$ROOT/out/bin/cohesix_root.elf") bytes"
else
  echo "❌ $ROOT_ELF_SRC missing" >&2
  exit 1
fi
[ -f out/cohesix_root.elf ] || { echo "❌ out/cohesix_root.elf missing" >&2; exit 1; }

# Bulletproof ELF validation
log "🔍 Validating cohesix_root ELF memory layout..."
readelf -l out/cohesix_root.elf | tee -a "$LOG_FILE" >&3
if readelf -l out/cohesix_root.elf | grep -q 'LOAD' && \
   ! readelf -l out/cohesix_root.elf | awk '/LOAD/ {print $3}' | grep -q -E '^0xffffff80[0-9a-fA-F]{8}$'; then
  echo "❌ cohesix_root ELF LOAD segments not aligned with expected seL4 virtual space" >&2
  exit 1
fi

ROOT_SIZE=$(stat -c%s out/cohesix_root.elf)
if [ "$ROOT_SIZE" -gt $((100*1024*1024)) ]; then
  echo "❌ cohesix_root ELF exceeds 100MB. Increase KernelElfVSpaceSizeBits or reduce binary size." >&2
  exit 1
fi

log "✅ cohesix_root ELF memory layout and size validated"

# Ensure staging directories exist for config and roles
mkdir -p "$STAGE_DIR/etc" "$STAGE_DIR/roles" "$STAGE_DIR/init" \
         "$STAGE_DIR/usr/bin" "$STAGE_DIR/usr/cli" "$STAGE_DIR/home/cohesix"
if [ -d python ]; then
  cp -r python "$STAGE_DIR/home/cohesix" 2>/dev/null || true
  mkdir -p "$ROOT/out/home"
  cp -r python "$ROOT/out/home/cohesix" 2>/dev/null || true
fi

#
#
# -----------------------------------------------------------
# seL4 kernel build using standard sel4_workspace layout
# -----------------------------------------------------------
log "🧱 Building seL4 kernel using existing /home/ubuntu/sel4_workspace workspace..."

KERNEL_DIR="/home/ubuntu/sel4_workspace"
COHESIX_OUT="${COHESIX_OUT:-$ROOT/out}"

cd "$KERNEL_DIR"

# Dynamically adjust KernelElfVSpaceSizeBits if ELF is large
KERNEL_VSPACE_BITS=42
if [ "$ROOT_SIZE" -gt $((50*1024*1024)) ]; then
  KERNEL_VSPACE_BITS=43
fi
./init-build.sh \
  -DPLATFORM=qemu-arm-virt \
  -DAARCH64=TRUE \
  -DKernelPrinting=ON \
  -DKernelDebugBuild=TRUE \
  -DKernelLogBuffer=ON \
  -DKernelElfVSpaceSizeBits="$KERNEL_VSPACE_BITS" \
  -DKernelRootCNodeSizeBits=18 \
  -DKernelVirtualEnd=0xffffff80e0000000 \
  -DKernelArmGICV2=ON \
  -DKernelArmPL011=ON \
  -DKernelVerificationBuild=ON \
  -DROOT_SERVER="$ROOT/out/cohesix_root.elf"

# Now run ninja in the workspace root
ninja

# Log kernel configuration for debugging
CACHE_FILE=$(find . -name CMakeCache.txt | head -n1)
if [ -f "$CACHE_FILE" ]; then
  log "Kernel configuration summary:" && \
  grep -E 'KernelPrinting|KernelDebugBuild|KernelLogBuffer|KernelVerificationBuild|KernelElfVSpaceSizeBits|KernelRootCNodeSizeBits|KernelVirtualEnd|KernelArmGICV2|KernelArmPL011' "$CACHE_FILE" || true
fi

# Copy kernel.elf and elfloader
cp "$KERNEL_DIR/kernel/kernel.elf" "$COHESIX_OUT/bin/kernel.elf"
log "✅ Kernel ELF staged to $COHESIX_OUT/bin/kernel.elf, size: $(stat -c%s "$COHESIX_OUT/bin/kernel.elf") bytes"

cp "$KERNEL_DIR/elfloader/elfloader" "$COHESIX_OUT/bin/elfloader"
log "✅ Elfloader staged to $COHESIX_OUT/bin/elfloader, size: $(stat -c%s "$COHESIX_OUT/bin/elfloader") bytes"

cd "$ROOT"

# -----------------------------------------------------------
# QEMU bare metal boot test (aarch64)
# -----------------------------------------------------------
log "🧪 Booting elfloader + kernel in QEMU..."
QEMU_LOG="$LOG_DIR/qemu_debug_$(date +%Y%m%d_%H%M%S).log"
qemu-system-aarch64 -M virt,gic-version=2 -cpu cortex-a57 -m 512M \
  -kernel "$COHESIX_OUT/bin/elfloader" \
  -serial mon:stdio -nographic \
  -d int,mmu,page,guest_errors,unimp,cpu_reset \
  -D "$QEMU_LOG" || true
log "QEMU log saved to $QEMU_LOG"


log "📂 Staging boot files..."
mkdir -p "$STAGE_DIR/boot"
cp "$COHESIX_OUT/bin/kernel.elf" "$STAGE_DIR/boot/kernel.elf"
cp out/cohesix_root.elf "$STAGE_DIR/boot/userland.elf"
for f in initfs.img bootargs.txt boot_trace.json; do
  [ -f "$f" ] && cp "$f" "$STAGE_DIR/boot/"
done

ensure_plan9_ns() {
  local ns_path="$ROOT/config/plan9.ns"
  if [ ! -f "$ns_path" ]; then
    log "⚠️ config/plan9.ns missing. Generating default..."
    mkdir -p "$ROOT/config"
  cat > "$ns_path" <<'EOF'
// CLASSIFICATION: COMMUNITY
// Filename: config/plan9.ns v0.1
// Author: Lukas Bower
// Date Modified: 2026-08-04
# mount -b /dev /dev  # Removed legacy Linux mount - not needed for UEFI
# mount -b /proc /proc  # Removed legacy Linux mount - not needed for UEFI
bind -a /bin /bin
bind -a /usr/py /usr/py
bind -a /srv /srv
bind -a /mnt/9root /
EOF
  fi
  mkdir -p "$STAGE_DIR/etc"
  if cp "$ns_path" "$STAGE_DIR/etc/plan9.ns"; then
    log "✅ plan9.ns staged"
  else
    log "⚠️ plan9.ns staging failed"
  fi
}

ensure_plan9_ns

# Always create a robust config/config.yaml and stage it
log "📂 Ensuring configuration file exists..."
CONFIG_PATH="$ROOT/config/config.yaml"
mkdir -p "$(dirname "$CONFIG_PATH")"
cat > "$CONFIG_PATH" <<EOF
# CLASSIFICATION: COMMUNITY
# Filename: config.yaml
# Author: Lukas Bower
# Date Modified: $(date +%Y-%m-%d)
role: QueenPrimary
network:
  enabled: true
  interfaces:
    - eth0
logging:
  level: info
EOF
log "✅ config.yaml created at $CONFIG_PATH"

# Also stage config.yaml into out for ISO build
mkdir -p "$ROOT/out/etc/cohesix"
cp "$CONFIG_PATH" "$ROOT/out/etc/cohesix/config.yaml"
log "✅ config.yaml staged to $ROOT/out/etc/cohesix/config.yaml"

# Stage config.yaml to ISO
mkdir -p "$STAGE_DIR/etc/cohesix"
cp "$CONFIG_PATH" "$STAGE_DIR/etc/cohesix/config.yaml"
log "✅ config.yaml staged to ISO"
if ls setup/roles/*.yaml >/dev/null 2>&1; then
  for cfg in setup/roles/*.yaml; do
    role="$(basename "$cfg" .yaml)"
    mkdir -p "$STAGE_DIR/roles/$role"
    cp "$cfg" "$STAGE_DIR/roles/$role/config.yaml"
  done
else
  echo "❌ No role configs found in setup/roles" >&2
  exit 1
fi
for shf in setup/init.sh setup/*.sh; do
  [ -f "$shf" ] && cp "$shf" "$STAGE_DIR/init/"
done

# Generate manifest of staged binaries
MANIFEST="$STAGE_DIR/manifest.json"
echo '{"binaries":[' > "$MANIFEST"
first=1
for bin in $(find "$STAGE_DIR/bin" -type f -perm -111); do
  hash=$(sha256sum "$bin" | awk '{print $1}')
  ver=$(git rev-parse --short HEAD)
  if [ $first -eq 0 ]; then echo ',' >> "$MANIFEST"; fi
  first=0
  printf '{"file":"%s","hash":"%s","version":"%s"}' "${bin#$STAGE_DIR/}" "$hash" "$ver" >> "$MANIFEST"
done
echo ']}' >> "$MANIFEST"


# 🗂 Prepare /srv namespace for tests (clean and set role)
log "🗂 Preparing /srv namespace for tests..."
sudo rm -rf /srv
sudo mkdir -p /srv
echo "DroneWorker" | sudo tee /srv/cohrole


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

  # Ensure GOARCH maps correctly
  if [ "$COH_ARCH" = "aarch64" ]; then
    GOARCH="arm64"
  elif [ "$COH_ARCH" = "x86_64" ]; then
    GOARCH="amd64"
  else
    GOARCH="$COH_ARCH"
  fi

  mkdir -p "$GO_HELPERS_DIR"

  for dir in go/cmd/*; do
    if [ -f "$dir/main.go" ]; then
      name="$(basename "$dir")"
      log "  ensuring modules for $name"
      (cd "$dir" && go mod tidy)
      log "  compiling $name for Linux as GOARCH=$GOARCH"
      if GOOS=linux GOARCH="$GOARCH" go build -tags unix -C "$dir" -o "$GO_HELPERS_DIR/$name"; then
        chmod +x "$GO_HELPERS_DIR/$name"
      else
        log "  build failed for $name"
      fi
    fi
  done

  if (cd go && go test ./...); then
    log "✅ Go tests passed"
  else
    echo "❌ Go tests failed" | tee -a "$SUMMARY_TEST_FAILS" >&3
  fi
  log "[INFO] Go helpers built and staged in $GO_HELPERS_DIR (excluded from ISO)"
else
  log "⚠️ Go not found; skipping Go build"
fi



log "📖 Building mandoc and staging man pages..."
scripts/build_mandoc.sh
MANDOC_BIN="prebuilt/mandoc/mandoc.$COH_ARCH"
if [ -x "$MANDOC_BIN" ]; then
  mkdir -p "$STAGE_DIR/prebuilt/mandoc"
  cp "$MANDOC_BIN" "$STAGE_DIR/prebuilt/mandoc/"
  chmod +x "$STAGE_DIR/prebuilt/mandoc/mandoc.$COH_ARCH"
  cp bin/mandoc "$STAGE_DIR/bin/mandoc"
  chmod +x "$STAGE_DIR/bin/mandoc"
  cp bin/man "$STAGE_DIR/bin/man"
  chmod +x "$STAGE_DIR/bin/man"
  if [ -d docs/man ]; then
    mkdir -p "$STAGE_DIR/usr/share/cohesix/man"
    cp docs/man/*.1 "$STAGE_DIR/usr/share/cohesix/man/" 2>/dev/null || true
    cp docs/man/*.8 "$STAGE_DIR/usr/share/cohesix/man/" 2>/dev/null || true
  fi
else
  echo "❌ mandoc build failed" >&2
  exit 1
fi

echo "✅ All builds complete."

echo "[🧪] Checking boot prerequisites..."
if [ ! -x "$STAGE_DIR/bin/init" ]; then
  echo "❌ init binary missing in $STAGE_DIR/bin" >&2
  exit 1
fi
if [ ! -f "$STAGE_DIR/boot/kernel.elf" ]; then
  echo "❌ Kernel ELF missing. Expected at $STAGE_DIR/boot/kernel.elf" >&2
  exit 1
fi
if [ ! -f "$STAGE_DIR/etc/plan9.ns" ]; then
  echo "❌ plan9.ns missing at $STAGE_DIR/etc/plan9.ns" >&2
  exit 1
fi

log "📀 Creating ISO..."
# ISO root layout:
#   out/iso/bin            - runtime binaries (kernel, init, busybox)
#   out/iso/usr/bin        - CLI wrappers and Go tools
#   out/iso/usr/cli        - Python CLI modules
#   out/iso/home/cohesix   - Python libraries
#   out/iso/etc            - configuration files
#   out/iso/roles          - role definitions
# Already ensured and activated the Python venv at the top of the script.
# No redundant venv creation or activation here.
if [[ "${VIRTUAL_ENV:-}" != *"/${VENV_DIR}" ]]; then
  echo "❌ Python venv not active before ISO build" >&2
  exit 1
fi

bash tools/make_iso.sh
ISO_OUT="out/cohesix.iso"
if [ ! -f "$ISO_OUT" ]; then
  echo "❌ ISO build failed: $ISO_OUT missing" >&2
  exit 1
fi
# Before cleanup deletes ISO_ROOT
if [ -d "$STAGE_DIR/bin" ]; then
  find "$STAGE_DIR/bin" -type f -print | tee -a "$LOG_FILE" >&3 || true
fi
if [ -f "$ISO_OUT" ]; then
  du -h "$ISO_OUT" | tee -a "$LOG_FILE" >&3
fi

if [ ! -d "/srv/cuda" ] || ! command -v nvidia-smi >/dev/null 2>&1 || ! nvidia-smi >/dev/null 2>&1; then
  echo "⚠️ CUDA hardware or /srv/cuda not detected" | tee -a "$LOG_FILE" >&3
fi


#
# Additional ISO checks before QEMU boot
log "🔍 Validating ISO with isoinfo..."
if command -v isoinfo >/dev/null 2>&1; then
  isoinfo -i "$ISO_OUT" -l | tee -a "$LOG_FILE" >&3 || true
  isoinfo -i "$ISO_OUT" -R -f | tee -a "$LOG_FILE" >&3 || true
else
  log "⚠️ isoinfo not installed, skipping detailed ISO listing"
fi

# Optional QEMU boot check (architecture-aware)
ISO_IMG="$ISO_OUT"
case "$COH_ARCH" in
  x86_64)
    if [ -x "$(command -v qemu-system-x86_64 2>/dev/null)" ]; then
      if [ ! -f "$ISO_IMG" ]; then
        echo "❌ ${ISO_IMG} missing in out" >&2
        exit 1
      fi
      TMPDIR="${TMPDIR:-$(mktemp -d)}"
      LOG_DIR="$PWD/logs"
      mkdir -p "$LOG_DIR"
      SERIAL_LOG="$TMPDIR/qemu_boot.log"
      QEMU_LOG="$LOG_DIR/qemu_boot.log"
      [ -f "$QEMU_LOG" ] && mv "$QEMU_LOG" "$QEMU_LOG.$(date +%Y%m%d_%H%M%S)"
      log "🧪 Booting ISO in QEMU for x86_64..."
      qemu-system-x86_64 -bios OVMF.fd -cdrom "$ISO_IMG" -serial mon:stdio -nographic 2>&1 | tee "$SERIAL_LOG"
      # Switched to -serial mon:stdio for direct console output in SSH
      QEMU_EXIT=${PIPESTATUS[0]}
      cat "$SERIAL_LOG" >> "$QEMU_LOG" 2>/dev/null || true
      cat "$SERIAL_LOG" >> "$LOG_FILE" 2>/dev/null || true
      echo "📜 Boot log (tail):"
      tail -n 20 "$SERIAL_LOG" || echo "❌ Could not read QEMU log"
      if [ "$QEMU_EXIT" -ne 0 ]; then
        echo "❌ QEMU exited with code $QEMU_EXIT" >&2
        exit 1
      fi
      if grep -q "BOOT_OK" "$SERIAL_LOG"; then
        log "✅ QEMU boot succeeded"
      else
        echo "❌ BOOT_OK not found in log" >&2
        exit 1
      fi
    else
      log "⚠️ qemu-system-x86_64 not installed; skipping boot test"
    fi
    ;;
  aarch64)
    if [ -x "$(command -v qemu-system-aarch64 2>/dev/null)" ]; then
      if [ ! -f "$ISO_IMG" ]; then
        echo "❌ ${ISO_IMG} missing in out" >&2
        exit 1
      fi
      TMPDIR="${TMPDIR:-$(mktemp -d)}"
      LOG_DIR="$PWD/logs"
      mkdir -p "$LOG_DIR"
      SERIAL_LOG="$TMPDIR/qemu_boot.log"
      QEMU_LOG="$LOG_DIR/qemu_boot.log"
      [ -f "$QEMU_LOG" ] && mv "$QEMU_LOG" "$QEMU_LOG.$(date +%Y%m%d_%H%M%S)"
      log "🧪 Booting ISO in QEMU for aarch64..."
      QEMU_EFI="/usr/share/qemu-efi-aarch64/QEMU_EFI.fd"
      if [ -f "$QEMU_EFI" ]; then
        qemu-system-aarch64 -M virt -cpu cortex-a57 -bios "$QEMU_EFI" \
          -serial mon:stdio -cdrom "$ISO_IMG" -nographic 2>&1 | tee "$SERIAL_LOG"
      else
        qemu-system-aarch64 -M virt -cpu cortex-a57 -bios none \
          -serial mon:stdio -cdrom "$ISO_IMG" -nographic 2>&1 | tee "$SERIAL_LOG"
      fi
      QEMU_EXIT=${PIPESTATUS[0]}
      cat "$SERIAL_LOG" >> "$QEMU_LOG" 2>/dev/null || true
      cat "$SERIAL_LOG" >> "$LOG_FILE" 2>/dev/null || true
      echo "📜 Boot log (tail):"
      tail -n 20 "$SERIAL_LOG" || echo "❌ Could not read QEMU log"
      if [ "$QEMU_EXIT" -ne 0 ]; then
        echo "❌ QEMU exited with code $QEMU_EXIT" >&2
        exit 1
      fi
      if grep -q "BOOT_OK" "$SERIAL_LOG"; then
        log "✅ QEMU boot succeeded"
      else
        echo "❌ BOOT_OK not found in log" >&2
        exit 1
      fi
    else
      log "⚠️ qemu-system-aarch64 not installed; skipping boot test"
    fi
    ;;
  *)
    echo "Unsupported architecture for QEMU boot: $COH_ARCH" >&2
    ;;
esac


BIN_COUNT=$(find "$STAGE_DIR/bin" -type f -perm -111 | wc -l)
ROLE_COUNT=$(find "$STAGE_DIR/roles" -name '*.yaml' | wc -l)
ISO_SIZE_MB=$(du -m "$ISO_OUT" | awk '{print $1}')
echo "ISO BUILD OK: ${BIN_COUNT} binaries, ${ROLE_COUNT} roles, ${ISO_SIZE_MB}MB total" >&3

cleanup() {
  log "🧹 Cleanup completed."
}
cleanup

log "✅ [Build Complete] $(date)"

grep -Ei 'error|fail|panic|permission denied|warning' "$LOG_FILE" > "$SUMMARY_ERRORS" || true
grep -A 5 -E '^failures:|thread .* panicked at' "$LOG_FILE" > "$SUMMARY_TEST_FAILS" || true

echo "⚠️  Summary of Errors and Warnings:" | tee -a "$LOG_FILE" >&3
tail -n 10 "$SUMMARY_ERRORS" || echo "✅ No critical issues found" | tee -a "$LOG_FILE" >&3

echo "🪵 Full log saved to $LOG_FILE" >&3
echo "✅ ISO build complete. Run QEMU with:" >&3
if [ "$COH_ARCH" = "x86_64" ]; then
  echo "qemu-system-x86_64 -bios OVMF.fd -cdrom $ISO_OUT -serial mon:stdio -nographic" >&3
else
  echo "qemu-system-aarch64 -M virt -cpu cortex-a57 -bios QEMU_EFI.fd -cdrom $ISO_OUT -serial mon:stdio -nographic" >&3
fi
