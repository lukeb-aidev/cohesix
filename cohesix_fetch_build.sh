#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: cohesix_fetch_build.sh v1.34
# Author: Lukas Bower
# Date Modified: 2027-12-31

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
export LIBRARY_PATH="$(pwd)/third_party/seL4/lib:${LIBRARY_PATH:-}"
export LD_LIBRARY_PATH="$(pwd)/third_party/seL4/lib:$CUDA_LIBRARY_PATH:${LD_LIBRARY_PATH:-}"
WORKSPACE="${WORKSPACE:-$ROOT/third_party/seL4}"

cd "$ROOT"

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
trap 'echo "❌ Build failed." >&3; [[ -f "$LOG_FILE" ]] && { echo "Last 40 log lines:" >&3; tail -n 40 "$LOG_FILE" >&3; }' ERR

log(){ echo "[$(date +%H:%M:%S)] $1" | tee -a "$LOG_FILE" >&3; }

log "🛠️ [Build Start] $(date)"
log "🚀 Using existing repository at $ROOT"

STAGE_DIR="$ROOT/out"
GO_HELPERS_DIR="$ROOT/out/go_helpers"
cd "$STAGE_DIR"
mkdir -p bin usr/bin usr/cli usr/share/man/man1 usr/share/man/man8 \
         etc srv mnt/data tmp dev proc roles home/cohesix boot init
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

# --- GUI orchestrator -----------------------------------------------------
GUI_DIR="$ROOT/go/cmd/gui-orchestrator"
if [ -d "$GUI_DIR" ]; then
    log "👁️  Building GUI orchestrator"

    case "$COH_ARCH" in
        aarch64) GOARCH=arm64  ;;
        x86_64)  GOARCH=amd64 ;;
        *)       GOARCH=$COH_ARCH ;;
    esac

    pushd "$GUI_DIR" >/dev/null

    # One tidy is enough; harmless if already tidy
    go mod tidy

    log "  running go test"
    if ! go test ./...; then
        echo "❌ GUI orchestrator tests failed" | tee -a "$SUMMARY_TEST_FAILS" >&3
        exit 1
    fi

    OUT_BIN="$GO_HELPERS_DIR/gui-orchestrator"
    log "  compiling (GOOS=linux GOARCH=$GOARCH)"
    if GOOS=linux GOARCH=$GOARCH go build -tags unix -o "$OUT_BIN" .; then
        chmod +x "$OUT_BIN"
        log "✅ GUI orchestrator built → $OUT_BIN"
    else
        echo "❌ GUI orchestrator build failed" | tee -a "$SUMMARY_ERRORS" >&3
        exit 1
    fi

    popd >/dev/null
else
    log "⚠️  GUI orchestrator source not found – skipping"
fi
# -------------------------------------------------------------------------

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

# Phase 1: Build all host crates except sel4-sys and cohesix_root
log "🔨 Building host crates"
cargo +nightly build --release --workspace \
  --exclude sel4-sys \
  --exclude cohesix_root
cargo +nightly test --release --workspace \
  --exclude sel4-sys \
  --exclude cohesix_root
log "✅ Host crates built and tested"

# Phase 2: Cross-compile sel4-sys (no-std, panic-abort)
log "🔨 Building sel4-sys (no-std, panic-abort)"
RUSTFLAGS="-C panic=abort" \
cargo +nightly build -p sel4-sys --release \
  --target=cohesix_root/sel4-aarch64.json \
  -Z build-std=core,alloc,compiler_builtins \
  -Z build-std-features=compiler-builtins-mem
log "✅ sel4-sys built (tests skipped)"

# Phase 3: Cross-compile cohesix_root
log "🔨 Building cohesix_root (no-std, panic-abort)"
RUSTFLAGS="-C panic=abort" \
cargo +nightly build -p cohesix_root --release \
  --target=cohesix_root/sel4-aarch64.json \
  -Z build-std=core,alloc,compiler_builtins \
  -Z build-std-features=compiler-builtins-mem
RUSTFLAGS="-C panic=abort" \
cargo +nightly test -p cohesix_root --release \
  --target=cohesix_root/sel4-aarch64.json \
  -Z build-std=core,alloc,compiler_builtins \
  -Z build-std-features=compiler-builtins-mem
log "✅ cohesix_root built and tested"

log "✅ Rust components built with proper split targets"

TARGET_DIR="$ROOT/workspace/target/sel4-aarch64/release"
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

log "🔍 Running Rust tests (user‑land target)…"
# We can only run unit tests for crates that build against the musl user‑land
# environment.  The bare‑metal cohesix_root target has no std and therefore
# no runnable tests here.
RUST_BACKTRACE=1 \
cargo test --release --workspace --exclude cohesix_root \
  --target=aarch64-unknown-linux-musl \
  -- --nocapture
TEST_EXIT_CODE=$?

# Capture failures and surface them in the summary
grep -A5 -E '^failures:|thread .* panicked at' "$LOG_FILE" \
    > "$SUMMARY_TEST_FAILS" || true

if [ $TEST_EXIT_CODE -ne 0 ]; then
  echo "❌ Rust tests failed." | tee -a "$SUMMARY_TEST_FAILS" >&3
  exit $TEST_EXIT_CODE
else
  log "✅ Rust tests passed"
fi

log "📖 Building mandoc and staging man pages..."
bash "$ROOT/scripts/build_mandoc.sh"
MANDOC_BIN="$ROOT/prebuilt/mandoc/mandoc.$COH_ARCH"
if [ -f "$MANDOC_BIN" ]; then
  cp "$MANDOC_BIN" "$STAGE_DIR/bin/man"
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
cp -- "$ROOT/tests/Cohesix/"*.rc "$STAGE_DIR/bin/tests/" 2>/dev/null || true
chmod +x "$STAGE_DIR/bin/tests/"*.rc
log "✅ Staged Plan9 rc tests to /bin/tests"

echo "✅ All builds complete."

echo "[🧪] Checking boot prerequisites..."
if [ ! -x "$STAGE_DIR/bin/init" ]; then
  echo "❌ init binary missing in $STAGE_DIR/bin" >&2
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

# Ensure all staged binaries are executable
chmod +x "$STAGE_DIR/bin"/*

# 1) Create boot directory
mkdir -p "$ROOT/boot"


# 2) Stage cohesix_root and elfloader
ROOT_ELF_SRC="$ROOT/workspace/target/sel4-aarch64/release/cohesix_root"
ROOT_ELF_DST="$ROOT/third_party/seL4/artefacts/cohesix_root.elf"
mkdir -p "$ROOT/out/bin"
cp -- "$ROOT_ELF_SRC" "$ROOT/out/bin/cohesix_root.elf"
cp -- "$ROOT_ELF_SRC" "$ROOT_ELF_DST"

cp -- "$ROOT/third_party/seL4/artefacts/elfloader" \
      "$ROOT/boot/elfloader"

# 3) Verify artefacts exist
cd "$ROOT/third_party/seL4/artefacts"
[ -f kernel.elf ] || { echo "❌ Missing kernel.elf" >&2; exit 1; }
[ -f cohesix_root.elf ] || { echo "❌ Missing cohesix_root.elf" >&2; exit 1; }
[ -f kernel.dtb ] || { echo "❌ Missing kernel.dtb" >&2; exit 1; }

# 4) Pack into a newc cpio archive
printf '%s\n' kernel.elf kernel.dtb cohesix_root.elf | \
  cpio -o -H newc > "$ROOT/boot/cohesix.cpio"

# Verify archive order
log "📦 CPIO first entries:"
mapfile -t _cpio_entries < <(cpio -it < "$ROOT/boot/cohesix.cpio" | head -n 3)
printf '%s\n' "${_cpio_entries[@]}" >&3
if [ "${_cpio_entries[0]}" != "kernel.elf" ] || \
   [ "${_cpio_entries[1]}" != "kernel.dtb" ] || \
   [ "${_cpio_entries[2]}" != "cohesix_root.elf" ]; then
  echo "❌ Unexpected CPIO order: ${_cpio_entries[*]}" >&2
  exit 1
fi

# Replace the embedded CPIO archive in the elfloader
aarch64-linux-gnu-objcopy \
  --update-section ._archive_cpio="$ROOT/boot/cohesix.cpio" \
  "$ROOT/third_party/seL4/artefacts/elfloader" "$ROOT/boot/elfloader"

# Sanity check - ensure sel4test-driver is not present
if cpio -it < "$ROOT/boot/cohesix.cpio" | grep -q sel4test-driver; then
  echo "❌ CPIO archive still contains sel4test-driver" >&2
  exit 1
fi

# 5) Export the path
CPIO_IMAGE="$ROOT/boot/cohesix.cpio"
cd "$ROOT"

#
# -----------------------------------------------------------
# QEMU bare metal boot test (aarch64)
# -----------------------------------------------------------

log "🧪 Booting elfloader + kernel in QEMU..."

# Timestamped log files
QEMU_LOG="$LOG_DIR/qemu_debug_$(date +%Y%m%d_%H%M%S).log"
QEMU_SERIAL_LOG="$LOG_DIR/qemu_serial_$(date +%Y%m%d_%H%M%S).log"

# Base QEMU flags
QEMU_FLAGS=(-nographic -serial mon:stdio)

if [ "${DEBUG_QEMU:-0}" = "1" ]; then
  echo "🔍 QEMU debug mode enabled: GDB stub on :1234, tracing CPU and MMU events"
  # Connect using: gdb -ex 'target remote :1234' <vmlinux>
  QEMU_FLAGS+=(-S -s -d cpu_reset,int,mmu,page,unimp)
fi

# Launch QEMU
qemu-system-aarch64 \
  -M virt,gic-version=2 \
  -cpu cortex-a57 \
  -m 1024M \
  -kernel "$ROOT/boot/elfloader" \
  -initrd "$CPIO_IMAGE" \
  -dtb "$ROOT/third_party/seL4/artefacts/kernel.dtb" \
  "${QEMU_FLAGS[@]}" \
  -D "$QEMU_LOG" |& tee "$QEMU_SERIAL_LOG"

# Report where logs went
log "✅ QEMU debug log saved to $QEMU_LOG"
log "✅ QEMU serial log saved to $QEMU_SERIAL_LOG"

# Pre-handoff reserved region dump
echo "Reserved regions:" | tee -a "$TRACE_LOG"
grep -i "reserved" "$QEMU_SERIAL_LOG" | tee -a "$TRACE_LOG"

# Append to trace log for CI
echo "QEMU debug log: $QEMU_LOG" >> "$TRACE_LOG"
echo "QEMU serial log: $QEMU_SERIAL_LOG" >> "$TRACE_LOG"

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

log "✅ [Build Complete] $(date)"

grep -Ei 'error|fail|panic|permission denied|warning' "$LOG_FILE" > "$SUMMARY_ERRORS" || true
grep -A 5 -E '^failures:|thread .* panicked at' "$LOG_FILE" > "$SUMMARY_TEST_FAILS" || true

echo "⚠️  Summary of Errors and Warnings:" | tee -a "$LOG_FILE" >&3
tail -n 10 "$SUMMARY_ERRORS" || echo "✅ No critical issues found" | tee -a "$LOG_FILE" >&3

echo "🪵 Full log saved to $LOG_FILE" >&3
