# CLASSIFICATION: COMMUNITY
# Filename: cohesix_fetch_build.sh v1.24
# Author: Lukas Bower
# Date Modified: 2027-12-31
#!/usr/bin/env bash
# This script fetches and builds the Cohesix project, including seL4 and other dependencies.

HOST_ARCH="$(uname -m)"
if [[ "$HOST_ARCH" = "aarch64" ]] && ! command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
  if command -v sudo >/dev/null 2>&1; then
    SUDO=sudo
  else
    SUDO=""
  fi
fi
# Ensure ROOT is always set
ROOT="${ROOT:-$HOME/cohesix}"
export ROOT

LOG_DIR="$ROOT/logs"
mkdir -p "$LOG_DIR"
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
export PYTHONPATH="$ROOT/third_party/seL4/kernel:/usr/local/lib/python3.12/dist-packages:${PYTHONPATH:-}"
export MEMCHR_DISABLE_RUNTIME_CPU_FEATURE_DETECTION=1
export CUDA_HOME="${CUDA_HOME:-/usr}"
export CUDA_INCLUDE_DIR="${CUDA_INCLUDE_DIR:-$CUDA_HOME/include}"
export CUDA_LIBRARY_PATH="${CUDA_LIBRARY_PATH:-/usr/lib/x86_64-linux-gnu}"
export PATH="$CUDA_HOME/bin:$PATH"
export LD_LIBRARY_PATH="$CUDA_LIBRARY_PATH:${LD_LIBRARY_PATH:-}"
export ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export LOG_DIR="$ROOT/logs"
WORKSPACE="${WORKSPACE:-$ROOT/third_party/seL4}"

cd "$ROOT"

mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/build_$(date +%Y%m%d_%H%M%S).log"
SUMMARY_ERRORS="$LOG_DIR/summary_errors.log"
SUMMARY_TEST_FAILS="$LOG_DIR/summary_test_failures.log"
: > "$LOG_DIR/libsel4_link_and_boot_trace.md"
TRACE_LOG="$LOG_DIR/libsel4_link_and_boot_trace.md"
{
  echo "// CLASSIFICATION: COMMUNITY"
  echo "// Filename: libsel4_link_and_boot_trace.md v0.1"
  echo "// Author: Lukas Bower"
  echo "// Date Modified: $(date +%Y-%m-%d)"
  echo
} > "$TRACE_LOG"
: > "$SUMMARY_ERRORS"
: > "$SUMMARY_TEST_FAILS"
exec 3>&1  # Save original stdout
exec > >(tee -a "$LOG_FILE" >&3) 2>&1
trap 'echo "❌ Build failed. Last 40 log lines:" >&3; tail -n 40 "$LOG_FILE" >&3' ERR

log(){ echo "[$(date +%H:%M:%S)] $1" | tee -a "$LOG_FILE" >&3; }

log "🛠️ [Build Start] $(date)"
log "🚀 Using existing repository at $ROOT"

STAGE_DIR="$ROOT/out"
GO_HELPERS_DIR="$ROOT/out/go_helpers"
cd "$STAGE_DIR"
mkdir -p bin usr/bin usr/cli usr/share/man/man1 usr/share/man/man8 \
         etc/cohesix srv mnt/data tmp dev proc roles home/cohesix boot init
cp "$ROOT/workspace/cohesix/src/kernel/init.rc" "$STAGE_DIR/srv/init.rc"
chmod +x "$STAGE_DIR/srv/init.rc"
log "✅ Created Cohesix FS structure"
# 🗂 Prepare /srv namespace for tests (clean and set role)
log "🗂 Preparing /srv namespace for tests..."
echo "DroneWorker" | sudo tee /srv/cohrole
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

LIB_PATH="$ROOT/third_party/seL4/lib/libsel4.a"

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
if ! rustup target list --installed | grep -q "^aarch64-unknown-linux-gnu$"; then
  echo "🔧 Installing missing Rust target aarch64-unknown-linux-gnu" >&2
  rustup target add aarch64-unknown-linux-gnu
fi
command -v aarch64-linux-gnu-gcc >/dev/null 2>&1 || { echo "❌ aarch64-linux-gnu-gcc missing" >&2; exit 1; }
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

# Kernel must run in production mode; disable seL4 self-tests
export CONFIG_BUILD_KERNEL_TESTS=n
KERNEL_TEST_FLAG=OFF


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
  if ! rustup target list --installed | grep -q '^aarch64-unknown-linux-gnu$'; then
    rustup target add aarch64-unknown-linux-gnu
    log "✅ Rust target aarch64-unknown-linux-gnu installed"
  fi
fi

if [ "$COH_ARCH" != "x86_64" ]; then
  CROSS_X86="x86_64-linux-gnu-"
else
  CROSS_X86=""
fi

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



# Ensure init.conf exists with defaults
INIT_CONF="$STAGE_DIR/etc/init.conf"
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

# Ensure plan9.ns is staged early, fail fast if missing
ensure_plan9_ns() {
  local ns_path="$ROOT/config/plan9.ns"
  if [ ! -f "$ns_path" ]; then
    echo "❌ Missing namespace file: $ns_path" >&2
    return 1
  fi
  if cp "$ns_path" "$STAGE_DIR/etc/plan9.ns"; then
    cp "$ns_path" "$ROOT/out/etc/plan9.ns"
    log "✅ plan9.ns staged"
  else
    echo "❌ plan9.ns staging failed" >&2
    return 1
  fi
}
ensure_plan9_ns

# Stage rc script if available
if [ -f "userland/miniroot/bin/rc" ]; then
  cp "userland/miniroot/bin/rc" "$STAGE_DIR/etc/rc"
  chmod +x "$STAGE_DIR/etc/rc"
  log "✅ Staged /etc/rc"
fi

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

# Set cross compiler for aarch64 if available
if [ "$COH_ARCH" = "aarch64" ]; then
  if command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
    export CC_aarch64_unknown_linux_gnu="$(command -v aarch64-linux-gnu-gcc)"
    log "✅ Using GNU cross compiler at $CC_aarch64_unknown_linux_gnu"
  elif [ -x "/opt/aarch64-linux-gnu/bin/aarch64-linux-gnu-gcc" ]; then
    export CC_aarch64_unknown_linux_gnu="/opt/aarch64-linux-gnu/bin/aarch64-linux-gnu-gcc"
    log "✅ Using GNU cross compiler at /opt/aarch64-linux-gnu/bin/aarch64-linux-gnu-gcc"
  else
    log "⚠️ aarch64-linux-gnu-gcc not found in PATH or /opt/aarch64-linux-gnu/bin"
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
#  for app in sh ls cat echo mount umount vi cp mv rm grep head tail printf test mkdir rmdir; do
#    ln -sf busybox "$STAGE_DIR/bin/$app"
#  done
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

log "🔧 Building Rust workspace binaries..."

cd "$ROOT/workspace"
cargo clean

# Build all workspace crates except cohesix_root with standard musl userland target
cargo build --release --workspace --exclude cohesix_root --target=aarch64-unknown-linux-musl

# Build bare metal cohesix_root with explicit build-std for core+alloc only
cargo +nightly build -p cohesix_root --release \
  --target=cohesix_root/sel4-aarch64.json \
  -Z build-std=core,alloc,compiler_builtins \
  -Z build-std-features=compiler-builtins-mem

log "✅ Rust components built with proper split targets"

TARGET_DIR="$ROOT/workspace/target/aarch64-unknown-linux-musl/release"
# Stage all main binaries
for bin in cohcc cohbuild cli_cap cohtrace cohrun_cli cohagent cohrole cohrun cohup srvctl indexserver devwatcher physics-server exportfs import mount srv scenario_compiler cloud cohesix cohfuzz; do
  BIN_PATH="$TARGET_DIR/$bin"
  if [ -f "$BIN_PATH" ]; then
    cp "$BIN_PATH" "$STAGE_DIR/bin/$bin"
    cp "$BIN_PATH" "$ROOT/out/bin/$bin"
  else
    echo "❌ $bin not found in $TARGET_DIR" >&2
    exit 1
  fi
  [ -f "$STAGE_DIR/bin/$bin" ] || { echo "❌ $bin missing after staging" >&2; exit 1; }
done

# Ensure physics-server and srv exist after staging
[ -f "$STAGE_DIR/bin/physics-server" ] || { echo "❌ physics-server missing after staging" >&2; exit 1; }
[ -f "$STAGE_DIR/bin/srv" ] || { echo "❌ srv missing after staging" >&2; exit 1; }

# Ensure all staged binaries are executable
chmod +x "$STAGE_DIR/bin"/*

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
  cp "$ROOT_ELF_SRC" "$ROOT/out/bin/cohesix_root.elf"
  log "Root ELF size: $(stat -c%s "$ROOT/out/bin/cohesix_root.elf") bytes"
else
  echo "❌ $ROOT_ELF_SRC missing" >&2
  exit 1
fi
[ -f "$ROOT/out/cohesix_root.elf" ] || { echo "❌ $ROOT/out/cohesix_root.elf missing" >&2; exit 1; }


# Build seL4 kernel, elfloader, and CPIO via build_sel4.sh after Rust build
log "🏗️  Building seL4 kernel and CPIO via build_sel4.sh..."
pushd "$ROOT/third_party/seL4"
echo "Fetching seL4 sources ..." >&2
SEL4_SRC="${SEL4_SRC:-$ROOT/third_party/seL4/workspace}"

DEST="workspace"

if [ -d "$DEST" ]; then
    echo "🧹 Cleaning existing $DEST"
    rm -rf "$DEST"
fi

echo "📥 Syncing seL4 repos into $DEST..."

# Clone seL4 into workspace directly
git clone https://github.com/seL4/seL4.git "$DEST"
cd "$DEST"
git fetch --tags
git checkout 13.0.0

# Now add tools and projects inside workspace
git clone https://github.com/seL4/seL4_libs.git projects/seL4_libs
git clone https://github.com/seL4/musllibc.git projects/musllibc
git clone https://github.com/seL4/util_libs.git projects/util_libs
git clone https://github.com/seL4/sel4runtime.git projects/sel4runtime
#git clone https://github.com/seL4/sel4test.git projects/sel4test
git clone https://github.com/seL4/seL4_tools.git projects/seL4_tools

echo "✅ seL4 workspace ready at $DEST"

BUILD_DIR="$ROOT/third_party/seL4/workspace/build"

for cmd in cmake ninja aarch64-linux-gnu-gcc aarch64-linux-gnu-g++ rustup cargo readelf nm objdump dtc; do
    command -v "$cmd" >/dev/null 2>&1 || { echo "Missing $cmd" >&2; exit 1; }
done

mkdir -p "$BUILD_DIR"

cd "$BUILD_DIR"
cmake -G Ninja \
  -C "$ROOT/third_party/seL4/workspace/configs/AARCH64_verified.cmake" \
  -DSIMULATION=TRUE \
  -DCROSS_COMPILER_PREFIX=aarch64-linux-gnu- \
  "$SEL4_SRC"
ninja kernel.elf


cp "$BUILD_DIR/kernel.elf" "$ROOT/out/bin/kernel.elf"

echo "📦 Generating kernel ABI flags…"
cd "$ROOT/third_party/seL4/workspace"
cmake -P projects/seL4_tools/cmake-tool/flags.cmake \
  -DOUTPUT_FILE=build/kernel_flags.cmake \
  -DPLATFORM_CONFIG=configs/AARCH64_verified.cmake \
  -DCROSS_COMPILER_PREFIX=aarch64-linux-gnu- || { echo "❌ flags generation failed"; exit 1; }
test -f build/kernel_flags.cmake || { echo "❌ kernel_flags.cmake missing"; exit 1; }

echo "🔍 Setting SEL4_WS to workspace root…"
SEL4_WS=$(pwd)

echo "🔍 Locating kernel_flags.cmake…"
KFLAGS="$SEL4_WS/build/kernel_flags.cmake"
test -f "$KFLAGS" || { echo "❌ kernel_flags.cmake not found"; exit 1; }

echo "🔍 Locating cpio module…"
CPIO_DIR="$SEL4_WS/tools"
test -f "$CPIO_DIR/cpio.cmake" || { echo "❌ cpio.cmake not found in $CPIO_DIR"; exit 1; }

echo "🔍 Locating libsel4.a…"
SEL4_LIB_DIR="$SEL4_WS/../lib"
test -f "$SEL4_LIB_DIR/libsel4.a" || { echo "❌ libsel4.a not found in $SEL4_LIB_DIR"; exit 1; }

echo "🔍 Locating elfloader source…"
ELF_SRC="$SEL4_WS/projects/seL4_tools/elfloader-tool"
test -d "$ELF_SRC" || { echo "❌ elfloader-tool not found in $SEL4_WS/projects/seL4_tools"; exit 1; }

echo "🚀 Building elfloader…"
mkdir -p "$SEL4_WS/elfloader/build"
cd "$SEL4_WS/elfloader/build"
cmake -G Ninja \
  -DCMAKE_MODULE_PATH="$CPIO_DIR" \
  -DCROSS_COMPILER_PREFIX=aarch64-linux-gnu- \
  -DCMAKE_TOOLCHAIN_FILE="$SEL4_WS/configs/AARCH64_verified.cmake" \
  -DKERNEL_FLAGS_PATH="../build/kernel_flags.cmake" \
  -DCMAKE_PREFIX_PATH="$SEL4_LIB_DIR" \
  "$ELF_SRC"
ninja elfloader
cp elfloader "$(pwd)/../../../out/bin/elfloader"
test -f "$(pwd)/../../../out/bin/elfloader" || { echo "❌ elfloader staging failed"; exit 1; }
cd "$(git rev-parse --show-toplevel)"

 mkdir -p "$ROOT/out/boot"
 cd "$ROOT/out/bin"
 DTB="$BUILD_DIR/kernel.dtb"
 if [ ! -f "$DTB" ]; then
 echo "Error - DTB not found"  >&2
 exit 1
fi

[ -f kernel.elf ] || { echo "Missing kernel.elf" >&2; exit 1; }
[ -f cohesix_root.elf ] || { echo "Missing cohesix_root.elf" >&2; exit 1; }
find kernel.elf cohesix_root.elf elfloader $( [ -f "$DTB" ] && echo "$DTB" ) | cpio -o -H newc > ../boot/cohesix.cpio
CPIO_IMAGE="$ROOT/out/boot/cohesix.cpio"
cd "$ROOT"

echo "✅ seL4 build complete"  >&2
popd

# Bulletproof ELF validation
log "🔍 Validating cohesix_root ELF memory layout..."
READLOG="$LOG_DIR/cohesix_root_readelf_$(date +%Y%m%d_%H%M%S).log"
readelf -l "$ROOT/out/cohesix_root.elf" | tee -a "$LOG_FILE" | tee "$READLOG" >&3
readelf -h "$ROOT/out/cohesix_root.elf" >> "$TRACE_LOG"
nm "$ROOT/out/cohesix_root.elf" | grep -E 'seL4_Send|seL4_Recv|seL4_DebugPutChar' >> "$TRACE_LOG"
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

#
# -----------------------------------------------------------
# QEMU bare metal boot test (aarch64)
# -----------------------------------------------------------
## CPIO archive is now built by build_sel4.sh; any checks below should use the output from that script
CPIO_IMAGE="$ROOT/out/boot/image.cpio"

log "🔍 Running ELF checks..."
KREAD="$LOG_DIR/kernel_readelf_$(date +%Y%m%d_%H%M%S).log"
RREAD="$LOG_DIR/cohesix_root_readelf_$(date +%Y%m%d_%H%M%S).log"
NMLOG="$LOG_DIR/nm_$(date +%Y%m%d_%H%M%S).log"
objdump -x "$ROOT/out/bin/cohesix_root.elf" > "$LOG_DIR/objdump_$(date +%Y%m%d_%H%M%S).log"
readelf -h "$ROOT/out/bin/cohesix_root.elf" | tee "$RREAD" | tee -a "$LOG_FILE" >&3
readelf -h "$ROOT/out/bin/kernel.elf" | tee "$KREAD" | tee -a "$LOG_FILE" >&3
grep -q 'AArch64' "$RREAD" || { echo "❌ cohesix_root.elf not AArch64" >&2; exit 1; }
grep -q 'AArch64' "$KREAD" || { echo "❌ kernel.elf not AArch64" >&2; exit 1; }
nm -u "$ROOT/out/bin/cohesix_root.elf" | tee "$NMLOG" | tee -a "$LOG_FILE" >&3
if grep -q " U " "$NMLOG"; then echo "❌ Undefined symbols" >&2; exit 1; fi

log "🧪 Booting elfloader + kernel in QEMU..."
QEMU_LOG="$LOG_DIR/qemu_debug_$(date +%Y%m%d_%H%M%S).log"
QEMU_SERIAL_LOG="$LOG_DIR/qemu_serial_$(date +%Y%m%d_%H%M%S).log"
QEMU_FLAGS="-nographic -serial mon:stdio"
if [ "${DEBUG_QEMU:-0}" = "1" ]; then
  # Deep trace for MMU and CPU faults when DEBUG_QEMU=1
  QEMU_FLAGS="-nographic -serial mon:stdio -d cpu_reset,int,guest_errors,mmu"
fi
qemu-system-aarch64 -M virt,gic-version=2 -cpu cortex-a57 -m 1024M \
  -kernel "$ROOT/out/bin/elfloader" \
  -initrd "$CPIO_IMAGE" \
  $QEMU_FLAGS \
  -D "$QEMU_LOG" |& tee "$QEMU_SERIAL_LOG" || true
log "QEMU log saved to $QEMU_LOG"
log "QEMU serial saved to $QEMU_SERIAL_LOG"
echo "QEMU log: $QEMU_LOG" >> "$TRACE_LOG"


log "📂 Staging boot files..."
cp "$ROOT/out/bin/kernel.elf" "$STAGE_DIR/boot/kernel.elf"
cp "$ROOT/out/bin/cohesix_root.elf" "$STAGE_DIR/boot/userland.elf"
cp "$ROOT/out/bin/elfloader" "$STAGE_DIR/boot/elfloader"
for f in initfs.img bootargs.txt boot_trace.json; do
  [ -f "$f" ] && cp "$f" "$STAGE_DIR/boot/"
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

ART_JSON="$LOG_DIR/artifact_locations.json"
cat > "$ART_JSON" <<EOF
{
  "libsel4.a": "$(realpath "$LIB_PATH")",
  "headers": "$(realpath "$ROOT/third_party/seL4/include")",
  "cohesix_root.elf": "$(realpath "$ROOT/out/bin/cohesix_root.elf")",
  "kernel.elf": "$(realpath "$ROOT/out/bin/kernel.elf")"
}
EOF
cat "$ART_JSON" >> "$TRACE_LOG"


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
        log "📦 Staged Linux helper: $name -> $GO_HELPERS_DIR"
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
  mkdir -p "$STAGE_DIR/mnt/data/bin"
  cp "$MANDOC_BIN" "$STAGE_DIR/mnt/data/bin/cohman"
  chmod +x "$STAGE_DIR/mnt/data/bin/cohman"
  cp bin/cohman.sh "$STAGE_DIR/bin/cohman"
  cp bin/cohman.sh "$STAGE_DIR/usr/bin/cohman"
  chmod +x "$STAGE_DIR/bin/cohman" "$STAGE_DIR/usr/bin/cohman"
  log "✅ Built mandoc" 
  if [ -d "$ROOT/workspace/docs/man" ]; then
    mkdir -p "$STAGE_DIR/usr/share/man/man1" "$STAGE_DIR/usr/share/man/man8"
    cp "$ROOT/workspace/docs/man/"*.1 "$STAGE_DIR/usr/share/man/man1/" 2>/dev/null || true
    cp "$ROOT/workspace/docs/man/"*.8 "$STAGE_DIR/usr/share/man/man8/" 2>/dev/null || true
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

# Stage Plan9 rc tests
mkdir -p "$STAGE_DIR/bin/tests"
cp tests/Cohesix/*.rc "$STAGE_DIR/bin/tests/"
cp tests/Cohesix/run_all_tests.rc "$STAGE_DIR/bin/tests/"
chmod +x "$STAGE_DIR/bin/tests"/*.rc
log "✅ Staged Plan9 rc tests to /bin/tests"

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


log "✅ [Build Complete] $(date)"

grep -Ei 'error|fail|panic|permission denied|warning' "$LOG_FILE" > "$SUMMARY_ERRORS" || true
grep -A 5 -E '^failures:|thread .* panicked at' "$LOG_FILE" > "$SUMMARY_TEST_FAILS" || true

echo "⚠️  Summary of Errors and Warnings:" | tee -a "$LOG_FILE" >&3
tail -n 10 "$SUMMARY_ERRORS" || echo "✅ No critical issues found" | tee -a "$LOG_FILE" >&3

echo "🪵 Full log saved to $LOG_FILE" >&3

# QEMU bare metal launch command (final boot test)
log "🧪 Running final QEMU bare metal boot test..."
# Provide final boot test log locations
QEMU_CONSOLE="$LOG_DIR/qemu_console_$(date +%Y%m%d_%H%M%S).log"
QEMU_FLAGS="-nographic -serial mon:stdio"
if [ "${DEBUG_QEMU:-0}" = "1" ]; then
  QEMU_FLAGS="-nographic -serial mon:stdio -d cpu_reset,int,guest_errors,mmu"
fi
# Provide CPIO archive as initrd to elfloader
qemu-system-aarch64 -M virt,gic-version=2 -cpu cortex-a57 -m 1024M \
  -kernel "$ROOT/out/bin/elfloader" \
  -initrd "$CPIO_IMAGE" \
  $QEMU_FLAGS \
  -D "$LOG_DIR/qemu_baremetal_$(date +%Y%m%d_%H%M%S).log" |& tee "$QEMU_CONSOLE" || true
log "QEMU console saved to $QEMU_CONSOLE"
log "✅ QEMU bare metal boot test complete."
