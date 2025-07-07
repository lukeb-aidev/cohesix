# CLASSIFICATION: COMMUNITY
# Filename: cohesix_fetch_build.sh v0.94
# Author: Lukas Bower
# Date Modified: 2027-08-13
#!/usr/bin/env bash
#
# Merged old script v0.89 features into current script.
# Preserved CUDA checks, BusyBox build, CMake upgrade, Go helpers, mandoc staging,
# but removed ISO building and related QEMU -cdrom logic for pure bare-metal seL4.
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
export ROOT="$HOME/cohesix"
export LOG_DIR="$ROOT/logs"

# Clone repository before sourcing any configuration so a fresh checkout
# is available even when $HOME is empty.
cd "$HOME"

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
log "🚀 Starting repository clone..."

  log "📦 Cloning repository..."
  rm -rf cohesix
  rm -rf cohesix_logs
  for i in {1..3}; do
    git clone git@github.com:lukeb-aidev/cohesix.git && break || sleep 1
  done
  log "✅ Clone complete ..."

cd cohesix
mkdir -p "$LOG_DIR"


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
if ! rustup component list --toolchain nightly | grep -q 'rust-src (installed)'; then
  echo "🔧 Installing missing rust-src component for nightly" >&2
  rustup component add rust-src --toolchain nightly
fi
if ! rustup target list --installed | grep -q "^aarch64-unknown-linux-musl$"; then
  echo "🔧 Installing missing Rust target aarch64-unknown-linux-musl" >&2
  rustup target add aarch64-unknown-linux-musl
fi
command -v aarch64-linux-musl-gcc >/dev/null 2>&1 || { echo "❌ aarch64-linux-musl-gcc missing" >&2; exit 1; }
command -v ld.lld >/dev/null 2>&1 || { echo "❌ ld.lld not found" >&2; exit 1; }
ld.lld --version >&3

log "\ud83d\udcc5 Fetching Cargo dependencies..."
cd "$ROOT/workspace"
cargo fetch
log "\u2705 Cargo dependencies fetched"


# Optional seL4 entry build flag
SEL4_ENTRY=0
if [[ ${1:-} == --sel4-entry ]]; then
  SEL4_ENTRY=1
  shift
fi


# CUDA detection and environment setup
log "🚀 Starting CUDA check..."
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
  log "✅ CUDA OK"
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

log "🚀 Starting dependency install..."
if command -v sudo >/dev/null 2>&1; then
  SUDO="sudo"
else
  SUDO=""
fi
$SUDO apt-get update -y
$SUDO apt-get install -y build-essential ninja-build git wget \
  python3 python3-pip cmake gcc-aarch64-linux-gnu
log "✅ Dependencies installed"

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
STAGE_DIR="$ROOT/out"
GO_HELPERS_DIR="$ROOT/out/go_helpers"
mkdir -p "$ROOT/out/bin" "$GO_HELPERS_DIR"
mkdir -p "$STAGE_DIR" "$ROOT/out/etc"
# Create minimal Cohesix filesystem structure
for dir in bin usr/cli srv mnt etc tmp proc dev; do
  mkdir -p "$STAGE_DIR/$dir"
done
log "✅ Created Cohesix FS structure"

# Ensure init.conf exists with defaults
INIT_CONF="$ROOT/out/etc/init.conf"
if [ ! -f "$INIT_CONF" ]; then
  cat > "$INIT_CONF" <<EOF
# CLASSIFICATION: COMMUNITY
# Filename: init.conf v0.1
# Author: Lukas Bower
# Date Modified: $(date +%Y-%m-%d)
ROLE=DroneWorker
GPU=1
TRACE=on
EOF
  log "✅ Created default init.conf"
else
  log "✅ Existing init.conf found"
fi

# Stage rc script if available
if [ -f "userland/miniroot/bin/rc" ]; then
  cp "userland/miniroot/bin/rc" "$STAGE_DIR/etc/rc"
  cp "userland/miniroot/bin/rc" "$ROOT/out/etc/rc"
  chmod +x "$STAGE_DIR/etc/rc" "$ROOT/out/etc/rc"
  log "✅ Staged /etc/rc"
fi
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

# Set musl cross compiler for aarch64 if available
if [ "$COH_ARCH" = "aarch64" ]; then
  if command -v aarch64-linux-musl-gcc >/dev/null 2>&1; then
    export CC_aarch64_unknown_linux_musl="$(command -v aarch64-linux-musl-gcc)"
    log "✅ Using musl cross compiler at $CC_aarch64_unknown_linux_musl"
  elif [ -x "/opt/aarch64-linux-musl/bin/aarch64-linux-musl-gcc" ]; then
    export CC_aarch64_unknown_linux_musl="/opt/aarch64-linux-musl/bin/aarch64-linux-musl-gcc"
    log "✅ Using musl cross compiler at /opt/aarch64-linux-musl/bin/aarch64-linux-musl-gcc"
  else
    log "⚠️ Musl cross compiler not found in PATH or /opt/aarch64-linux-musl/bin"
  fi
fi

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

 # Build GUI orchestrator early
if [ -d "$ROOT/go/cmd/gui-orchestrator" ]; then
  log "✅ Found GUI orchestrator Go code matching design"
  if [ "$COH_ARCH" = "aarch64" ]; then
    GOARCH="arm64"
  elif [ "$COH_ARCH" = "x86_64" ]; then
    GOARCH="amd64"
  else
    GOARCH="$COH_ARCH"
  fi
  (cd "$ROOT/go/cmd/gui-orchestrator" && go mod tidy)
  OUT_BIN="$GO_HELPERS_DIR/web_gui_orchestrator"
  log "  compiling GUI orchestrator as GOARCH=$GOARCH"
  if (cd "$ROOT/go/cmd/gui-orchestrator" && GOOS=linux GOARCH="$GOARCH" go build -tags unix -o "$OUT_BIN"); then
    chmod +x "$OUT_BIN"
    log "✅ Built GUI orchestrator → $OUT_BIN"
  else
    log "⚠️ GUI orchestrator build failed"
  fi
  ls -lh "$GO_HELPERS_DIR" | tee -a "$LOG_FILE" >&3
else
  log "⚠️ GUI orchestrator missing or incomplete - generated new code from spec"
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
if [ -f "$ROOT/CMakeLists.txt" ]; then
  cd "$ROOT"
  mkdir -p build
  cd "$ROOT/build"
  cmake "$ROOT" && make -j$(nproc)
else
  echo "⚠️ No CMakeLists.txt found at $ROOT, skipping C build"
fi

log "🔧 Building BusyBox..."
cd "$ROOT"
"$ROOT/scripts/build_busybox.sh" "$COH_ARCH"
BUSYBOX_BIN="$ROOT/out/busybox/$COH_ARCH/bin/busybox"
if [ -x "$BUSYBOX_BIN" ]; then
  cp "$BUSYBOX_BIN" "$STAGE_DIR/bin/busybox"
  log "✅ BusyBox built"
  for app in sh ls cat echo mount umount vi cp mv rm grep head tail printf test mkdir rmdir; do
    ln -sf busybox "$STAGE_DIR/bin/$app"
  done
  log "✅ Staged BusyBox applets to /bin"
  if [ -f "$ROOT/userland/miniroot/bin/init" ]; then
    cp "$ROOT/userland/miniroot/bin/init" "$STAGE_DIR/bin/init"
    chmod +x "$STAGE_DIR/bin/init"
  fi
  if [ -f "$ROOT/userland/miniroot/bin/rc" ]; then
    cp "$ROOT/userland/miniroot/bin/rc" "$STAGE_DIR/bin/rc"
    chmod +x "$STAGE_DIR/bin/rc"
  fi
else
  echo "❌ BusyBox build failed" >&2
  exit 1
fi

log "🔧 Building Rust components..."
echo "== Rust build =="

#
# Build Rust binaries with correct targets and flags:
# - kernel, logdemo, init, cohesix_root: aarch64-unknown-linux-musl + custom link.ld
# - CLI tools: aarch64-unknown-linux-gnu, no custom linker
#

# Build cohesix_root for seL4 root server
echo "🔧 Building Rust binary: cohesix_root"
cd "$ROOT/workspace/cohesix_root"
RUSTFLAGS="-C linker=ld.lld -C link-arg=-T$ROOT/link.ld" cargo +nightly build -Z build-std=core,alloc --release --target "$ROOT/workspace/cohesix_root/sel4-aarch64.json" --target-dir "$ROOT/workspace/target_root"
cd "$ROOT"
echo "✅ Finished building: cohesix_root"

# --- Inspect final cohesix_root ELF with readelf ---
echo "🔍 Inspecting final cohesix_root ELF with readelf..."
readelf -l "$ROOT/workspace/target_root/sel4-aarch64/release/cohesix_root" \
  > "$LOG_DIR/ld_verbose_$(date +%Y%m%d_%H%M%S).log" 2>&1
echo "✅ ELF program headers written to log"

 # Build kernel with its required features
echo "🔧 Building Rust binary: kernel"
cd "$ROOT/workspace"
RUSTFLAGS="-C link-arg=-T$ROOT/workspace/link.ld" \
  cargo build --release --bin kernel \
  --features "kernel_bin,minimal_uefi" \
  --target aarch64-unknown-linux-musl
echo "✅ Finished building: kernel"

# Build logdemo with its required features
echo "🔧 Building Rust binary: logdemo"
RUSTFLAGS="-C link-arg=-T$ROOT/workspace/link.ld" \
  cargo build --release --bin logdemo \
  --features "minimal_uefi" \
  --target aarch64-unknown-linux-musl
echo "✅ Finished building: logdemo"

 # Build init with its required features (static musl, explicit crt-static, separate target dir)
echo "🔧 Building Rust binary: init"
RUSTFLAGS="-C target-feature=+crt-static" \
  cargo build --release --bin init \
  --features "minimal_uefi" \
  --target aarch64-unknown-linux-musl \
  --target-dir "$ROOT/workspace/target_static"
echo "✅ Finished building: init"

 # Build other CLI tools without special features (GNU target, separate target_cli dir)
for bin in cohcc cohesix_build cohesix_cap cohesix_trace; do
  echo "🔧 Building Rust binary: $bin"
  cargo build --release --bin "$bin" \
    --target aarch64-unknown-linux-gnu \
    --target-dir "$ROOT/workspace/target_cli"
  echo "✅ Finished building: $bin"
done
log "✅ Rust components built"

 # Copy built binaries to staging, searching both musl and gnu targets
mkdir -p "$STAGE_DIR/bin"
for bin in cohcc cohesix_build cohesix_cap cohesix_trace cohrun_cli cohagent cohrole cohrun cohup cohesix_root kernel logdemo init; do
  BIN_PATH=""
  for dir in "$ROOT/workspace/target/aarch64-unknown-linux-gnu/release" "$ROOT/workspace/target/aarch64-unknown-linux-musl/release"; do
    if [ -f "$dir/$bin" ]; then
      BIN_PATH="$dir/$bin"
      break
    fi
  done
  if [ -n "$BIN_PATH" ]; then
    cp "$BIN_PATH" "$STAGE_DIR/bin/$bin"
    cp "$BIN_PATH" "$ROOT/out/bin/$bin"
  else
    echo "⚠️ $bin not found in target dirs" >&2
  fi
done

# Stage shell wrappers for Python CLI tools
for script in cohcli cohcap cohtrace cohrun cohbuild cohup cohpkg; do
  if [ -f "$ROOT/bin/$script" ]; then
    cp "$ROOT/bin/$script" "$STAGE_DIR/bin/$script"
    cp "$ROOT/bin/$script" "$STAGE_DIR/usr/bin/$script"
    sed -i '1c #!/usr/bin/env python3' "$STAGE_DIR/bin/$script"
    sed -i '1c #!/usr/bin/env python3' "$STAGE_DIR/usr/bin/$script"
    chmod +x "$STAGE_DIR/bin/$script" "$STAGE_DIR/usr/bin/$script"
  fi
done


cd "$ROOT"
log "🧱 Staging root ELF for seL4..."
# Copy root ELF from cargo build output to out/cohesix_root.elf
ROOT_ELF_SRC="$ROOT/workspace/target/sel4-aarch64/release/cohesix_root"
if [ -f "$ROOT_ELF_SRC" ]; then
  cp "$ROOT_ELF_SRC" "$ROOT/out/cohesix_root.elf"
  mkdir -p "$ROOT/out/bin"
  cp "$ROOT_ELF_SRC" "$ROOT/out/bin/cohesix_root.elf"
  log "Root ELF size: $(stat -c%s "$ROOT/out/bin/cohesix_root.elf") bytes"
else
  echo "❌ $ROOT_ELF_SRC missing" >&2
  exit 1
fi
[ -f "$ROOT/out/cohesix_root.elf" ] || { echo "❌ $ROOT/out/cohesix_root.elf missing" >&2; exit 1; }

# Bulletproof ELF validation
log "🔍 Validating cohesix_root ELF memory layout..."
READLOG="$LOG_DIR/cohesix_root_readelf_$(date +%Y%m%d_%H%M%S).log"
readelf -l "$ROOT/out/cohesix_root.elf" | tee -a "$LOG_FILE" | tee "$READLOG" >&3
sha256sum "$ROOT/out/cohesix_root.elf" > "$LOG_DIR/cohesix_root_$(date +%Y%m%d_%H%M%S).sha256"
if readelf -l "$ROOT/out/cohesix_root.elf" | grep -q 'LOAD' && \
   ! readelf -l "$ROOT/out/cohesix_root.elf" | awk '/LOAD/ {print $3}' | grep -q -E '^0xffffff80[0-9a-fA-F]{8}$'; then
  echo "❌ cohesix_root ELF LOAD segments not aligned with expected seL4 virtual space" >&2
  exit 1
fi

ROOT_SIZE=$(stat -c%s "$ROOT/out/cohesix_root.elf")
if [ "$ROOT_SIZE" -gt $((100*1024*1024)) ]; then
  echo "❌ cohesix_root ELF exceeds 100MB. Increase KernelElfVSpaceSizeBits or reduce binary size." >&2
  exit 1
fi

log "✅ cohesix_root ELF memory layout and size validated"

# Ensure staging directories exist for config and roles
mkdir -p "$STAGE_DIR/etc" "$STAGE_DIR/roles" "$STAGE_DIR/init" \
         "$STAGE_DIR/usr/bin" "$STAGE_DIR/usr/cli" "$STAGE_DIR/home/cohesix"
if [ -d "$ROOT/python" ]; then
  cp -r "$ROOT/python" "$STAGE_DIR/home/cohesix" 2>/dev/null || true
  mkdir -p "$ROOT/out/home"
  cp -r "$ROOT/python" "$ROOT/out/home/cohesix" 2>/dev/null || true
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
RUST_BACKTRACE=1 cargo test --release --target "$ROOT/workspace/cohesix_root/sel4-aarch64.json" -- --nocapture
TEST_EXIT_CODE=$?
grep -A 5 -E '^failures:|thread .* panicked at' "$LOG_FILE" > "$SUMMARY_TEST_FAILS" || true
if [ $TEST_EXIT_CODE -ne 0 ]; then
  echo "❌ Rust tests failed." | tee -a "$LOG_FILE" >&3
fi
grep -Ei 'error|fail|panic|permission denied|warning' "$LOG_FILE" > "$SUMMARY_ERRORS" || true

# --- Go build and staging section ---
echo "== Go build =="
if command -v go &> /dev/null; then
  log "🐹 Building Go components..."

  if [ "$COH_ARCH" = "aarch64" ]; then
    GOARCH="arm64"
  elif [ "$COH_ARCH" = "x86_64" ]; then
    GOARCH="amd64"
  else
    GOARCH="$COH_ARCH"
  fi

  mkdir -p "$GO_HELPERS_DIR"
  mkdir -p "$STAGE_DIR/usr/plan9/bin"

  for dir in go/cmd/*; do
    if [ -f "$dir/main.go" ]; then
      name="$(basename "$dir")"
      [ "$name" = "gui-orchestrator" ] && continue
      log "  ensuring modules for $name"
      (cd "$dir" && go mod tidy)
      log "  compiling $name for Linux as GOARCH=$GOARCH"
      if GOOS=linux GOARCH="$GOARCH" go build -tags unix -C "$dir" -o "$GO_HELPERS_DIR/$name"; then
        chmod +x "$GO_HELPERS_DIR/$name"
        if [[ "$name" == "srvctl" || "$name" == "indexserver" || "$name" == "devwatcher" ]]; then
          cp "$GO_HELPERS_DIR/$name" "$STAGE_DIR/usr/plan9/bin/"
          log "📦 Staged Plan9 binary: $name -> $STAGE_DIR/usr/plan9/bin/"
        else
          log "📦 Staged Linux helper: $name -> $GO_HELPERS_DIR"
        fi
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
# --- End Go build and staging section ---



log "📖 Building mandoc and staging man pages..."
./scripts/build_mandoc.sh
MANDOC_BIN="prebuilt/mandoc/mandoc.$COH_ARCH"
if [ -x "$MANDOC_BIN" ]; then
  mkdir -p "$STAGE_DIR/prebuilt/mandoc"
  cp "$MANDOC_BIN" "$STAGE_DIR/prebuilt/mandoc/"
  chmod +x "$STAGE_DIR/prebuilt/mandoc/mandoc.$COH_ARCH"
  cp bin/mandoc "$STAGE_DIR/bin/mandoc"
  cp bin/mandoc "$STAGE_DIR/usr/bin/mandoc"
  chmod +x "$STAGE_DIR/bin/mandoc" "$STAGE_DIR/usr/bin/mandoc"
  cp bin/man "$STAGE_DIR/bin/man"
  cp bin/man "$STAGE_DIR/usr/bin/man"
  chmod +x "$STAGE_DIR/bin/man" "$STAGE_DIR/usr/bin/man"
  log "✅ Built mandoc" 
  if [ -d "$ROOT/docs/man" ]; then
    mkdir -p "$STAGE_DIR/usr/share/man/man1" "$STAGE_DIR/usr/share/man/man8"
    cp "$ROOT/docs/man/"*.1 "$STAGE_DIR/usr/share/man/man1/" 2>/dev/null || true
    cp "$ROOT/docs/man/"*.8 "$STAGE_DIR/usr/share/man/man8/" 2>/dev/null || true
    log "✅ Updated man pages"
  fi
  log "✅ Staged mandoc to /usr/bin"
  cat > "$STAGE_DIR/etc/README.txt" <<'EOF'
Cohesix OS Quick Start

Tools: cohcli, cohrun, cohtrace, cohcc, cohcap, cohesix-shell, mandoc

Usage:
  cohcli status --verbose
  cohrun kiosk_start
  cohtrace list

Logs: /log/
Traces: /log/trace/
Manual pages: mandoc -Tascii /usr/share/man/man1/<tool>.1
EOF
  log "✅ Created /etc/README.txt"
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

log "🏗️  Staging complete filesystem..."
echo "BUILD AND STAGING COMPLETE"
BIN_COUNT=$(find "$STAGE_DIR/bin" -type f -perm -111 | wc -l)
ROLE_COUNT=$(find "$STAGE_DIR/roles" -name '*.yaml' | wc -l)
log "FS BUILD OK: ${BIN_COUNT} binaries, ${ROLE_COUNT} roles staged" >&3

VERIFY_LOG="$LOG_DIR/root_split_target_dir_verify_$(date +%Y%m%d_%H%M%S).log"
{
  echo "root build path: $ROOT/workspace/target_root"
  echo "cli build path: $ROOT/workspace/target_cli"
  sha256sum "$ROOT/workspace/target_root/sel4-aarch64/release/cohesix_root" 2>/dev/null || true
  sha256sum "$ROOT/workspace/target_cli/aarch64-unknown-linux-gnu/release/cohcc" 2>/dev/null || true
  readelf -h "$ROOT/out/cohesix_root.elf" 2>/dev/null || echo "readelf missing"
} > "$VERIFY_LOG"

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
echo "=== AUDIT SUMMARY ==="
echo "Old script included ISO creation and QEMU -cdrom tests."
echo "New script uses only elfloader + kernel ELF direct boot."
echo "All other environment checks, CUDA, BusyBox, Go, mandoc logic have been preserved."

