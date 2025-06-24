# CLASSIFICATION: COMMUNITY
# Filename: cohesix_fetch_build.sh v0.65
# Author: Lukas Bower
# Date Modified: 2026-08-11
#!/bin/bash

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

ROOT="$HOME/cohesix"
if [ ! -f "$ROOT/scripts/load_arch_config.sh" ]; then
  echo "❌ Missing: $ROOT/scripts/load_arch_config.sh" >&2
  exit 1
fi
source "$ROOT/scripts/load_arch_config.sh"

case "$COHESIX_ARCH" in
  x86_64) COHESIX_TARGET="x86_64-unknown-linux-gnu" ;;
  aarch64) COHESIX_TARGET="aarch64-unknown-linux-gnu" ;;
  *) echo "Unsupported architecture: $COHESIX_ARCH" >&2; exit 1 ;;
esac

export COHESIX_TARGET COHESIX_ARCH
COH_ARCH="$COHESIX_ARCH"
log "Architecture: $COH_ARCH (target $COHESIX_TARGET)"


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

cd "$HOME"
log "🧹 Cleaning workspace..."
rm -rf cohesix

log "📦 Cloning repository..."
git clone git@github.com:lukeb-aidev/cohesix.git
cd cohesix
ROOT="$(pwd)"
STAGE_DIR="$ROOT/out/iso"
mkdir -p "$ROOT/out/bin"
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
command -v python3 >/dev/null || { echo "❌ python3 not found" >&2; exit 1; }
VENV_DIR=".venv_${COHESIX_ARCH}"
python3 -m venv "$VENV_DIR"
source "$VENV_DIR/bin/activate"
# Ensure \$HOME/.local/bin is included for user installs
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

# Install Python linters if missing
for tool in flake8 mypy black; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    python -m pip install "$tool" --break-system-packages
  fi
done

# Validate presence of Python files before linting
if find python tests -name '*.py' | grep -q .; then
  flake8 python tests
  mypy python tests --check-untyped-defs
  black --check python tests
else
  log "ℹ️ No Python files detected; skipping lint checks"
fi

log "🧱 Building Rust components..."
if [[ "$(uname -m)" == "aarch64" ]]; then
  COHESIX_TARGET="aarch64-unknown-linux-gnu"
else
  COHESIX_TARGET="x86_64-unknown-linux-gnu"
fi
export COHESIX_TARGET
log "Using target $COHESIX_TARGET"

# Patch find_cuda_helper crate if not already staged
if [ ! -d "third_party/find_cuda_helper" ]; then
  log "📥 Downloading find_cuda_helper crate..."
  mkdir -p third_party
  curl -L https://crates.io/api/v1/crates/find_cuda_helper/0.2.0/download | tar -xz
  mv find_cuda_helper-0.2.0 third_party/find_cuda_helper
  log "✅ find_cuda_helper crate staged"
fi

# Install the target if rustup is available and it's not already installed
if command -v rustup >/dev/null 2>&1; then
  if ! rustup target list --installed | grep -q "^${COHESIX_TARGET}$"; then
    log "🔧 Installing Rust target ${COHESIX_TARGET}"
    rustup target add "${COHESIX_TARGET}"
  fi
else
  log "⚠️ rustup not found; assuming ${COHESIX_TARGET} toolchain is installed"
fi

log "🧱 Building all Rust binaries in workspace (CLI, tools, validator, shell, etc)..."
cargo build --release --workspace --all-targets --all-features
grep -Ei 'error|fail|panic|permission denied|warning' "$LOG_FILE" > "$SUMMARY_ERRORS" || true

# Ensure output directory exists before copying Rust binaries
mkdir -p "$STAGE_DIR/bin" "$STAGE_DIR/usr/bin" "$STAGE_DIR/usr/cli" "$STAGE_DIR/home/cohesix"

# Copy Rust CLI binaries into out/bin for ISO staging (copy only, skip build)
# The workspace build above already built all binaries, so just copy them if present.
for bin in cohcc cohbuild cohcap cohtrace cohrun_cli validator fs nsbuilder shell; do
  BIN_PATH="target/${COHESIX_TARGET}/release/$bin"
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
log "🧱 Building root ELF..."
log "CUDA_HOME=${CUDA_HOME:-}" 
log "nvcc path: $(command -v nvcc || echo 'not found')"
log "LD_LIBRARY_PATH=${LD_LIBRARY_PATH:-}"
bash scripts/build_root_elf.sh || { echo "❌ root ELF build failed" >&2; exit 1; }
[ -f out/cohesix_root.elf ] || { echo "❌ out/cohesix_root.elf missing" >&2; exit 1; }

# Ensure staging directories exist for config and roles
mkdir -p "$STAGE_DIR/etc" "$STAGE_DIR/roles" "$STAGE_DIR/init" \
         "$STAGE_DIR/usr/bin" "$STAGE_DIR/usr/cli" "$STAGE_DIR/home/cohesix"

# Build or update seL4 kernel from external workspace
SEL4_WORKSPACE="${SEL4_WORKSPACE:-/home/ubuntu/sel4_workspace}"
echo "Using kernel from: $SEL4_WORKSPACE"
if [ ! -d "$SEL4_WORKSPACE" ]; then
  echo "seL4 not found in $SEL4_WORKSPACE. Please build it using the official sel4test-manifest flow before continuing." >&2
  exit 1
fi
case "$COH_ARCH" in
  x86_64) KERNEL_SRC="$SEL4_WORKSPACE/build_pc99/kernel/kernel.elf" ;;
  aarch64) KERNEL_SRC="$SEL4_WORKSPACE/build_qemu_arm/kernel/kernel.elf" ;;
  *) echo "Unknown arch $COH_ARCH" >&2; exit 1 ;;
esac
log "Checking kernel path: $KERNEL_SRC"
if [ ! -f "$KERNEL_SRC" ]; then
  echo "❌ Kernel ELF not found at $KERNEL_SRC. Did you run init-build.sh + ninja?" >&2
  ls -l "$SEL4_WORKSPACE"/build_* 2>/dev/null || true
  exit 1
fi
SEL4_BUILD_DIR="${SEL4_BUILD_DIR:-"$(find "$SEL4_WORKSPACE" -maxdepth 1 -type d -name 'build_*' | head -n1)"}"
if [ -z "$SEL4_BUILD_DIR" ] || [ ! -d "$SEL4_BUILD_DIR" ]; then
  echo "No build_* directory found under $SEL4_WORKSPACE" >&2
  exit 1
fi
SRC_KERNEL="$SEL4_BUILD_DIR/kernel/kernel.elf"
OUT_KERNEL="$ROOT/out/bin/kernel.elf"
if [ ! -f "$OUT_KERNEL" ] || [ "$SRC_KERNEL" -nt "$OUT_KERNEL" ]; then
  log "🏗️ Building seL4 kernel via ninja"
  (cd "$SEL4_BUILD_DIR" && ninja)
fi
[ -f "$SRC_KERNEL" ] || { echo "Kernel build failed: $SRC_KERNEL missing" >&2; exit 1; }
cp "$SRC_KERNEL" "$OUT_KERNEL"
log "kernel.elf staged to $OUT_KERNEL"
log "Building init EFI binary..."
make init-efi >/dev/null
INIT_EFI="$ROOT/out/bin/init.efi"
[ -f "$INIT_EFI" ] || { echo "❌ init.efi build failed" >&2; exit 1; }
cp "$INIT_EFI" "$STAGE_DIR/bin/init.efi"


log "📂 Staging boot files..."
mkdir -p "$STAGE_DIR/boot"
cp "$OUT_KERNEL" "$STAGE_DIR/boot/kernel.elf"
log "kernel build complete"
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
mount -b /dev /dev
mount -b /proc /proc
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

log "📂 Staging configuration..."
mkdir -p "$STAGE_DIR/config"
CONFIG_SRC=""
if [ -f config/config.yaml ]; then
  CONFIG_SRC="config/config.yaml"
elif [ -f setup/config.yaml ]; then
  CONFIG_SRC="setup/config.yaml"
else
  echo "⚠️ config.yaml missing. Generating fallback..."
  mkdir -p config
  cat > config/config.yaml <<EOF
# Auto-generated fallback config
system:
  role: worker
  trace: true
EOF
  CONFIG_SRC="config/config.yaml"
fi
mkdir -p "$STAGE_DIR/config"
cp "$CONFIG_SRC" "$STAGE_DIR/config/config.yaml"
log "config.yaml staged from $CONFIG_SRC"
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
  mkdir -p "$STAGE_DIR/bin"
  for dir in go/cmd/*; do
    if [ -f "$dir/main.go" ]; then
      name="$(basename "$dir")"
      log "  compiling $name for $COH_ARCH"
      if GOOS=linux GOARCH="$COH_ARCH" go build -tags unix -C "$dir" -o "$STAGE_DIR/bin/$name"; then
        log "  built $name"
      else
        log "  cross build failed for $name; trying native"
        (cd "$dir" && go build -tags unix -o "$STAGE_DIR/bin/$name") || log "  build failed for $name"
      fi
    fi
  done
  if (cd go && go test ./...); then
    log "✅ Go tests passed"
  else
    echo "❌ Go tests failed" | tee -a "$SUMMARY_TEST_FAILS" >&3
  fi
else
  log "⚠️ Go not found; skipping Go build"
fi

log "🐍 Running Python tests..."
if command -v pytest &> /dev/null; then
  if pytest -q; then
    log "✅ Python tests passed"
  else
    echo "❌ Python tests failed" | tee -a "$SUMMARY_TEST_FAILS" >&3
  fi
fi
if command -v flake8 &> /dev/null; then
  flake8 python tests
fi

log "🔧 Checking C compiler..."
if ! command -v gcc >/dev/null 2>&1; then
  echo "❌ gcc not found. Install with: sudo apt install build-essential" >&2
  exit 1
fi
CC_TEST_C="$(mktemp --suffix=.c coherix_cc_test.XXXX)"
cat <<'EOF' > "$CC_TEST_C"
#include <stdio.h>
int main(void){ printf("hello\n"); return 0; }
EOF
CC_TEST_BIN="${CC_TEST_C%.c}"
if gcc "$CC_TEST_C" -o "$CC_TEST_BIN" >/dev/null 2>&1 && "$CC_TEST_BIN" >/dev/null; then
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
if [ ! -x "$STAGE_DIR/bin/init" ] && [ ! -x "$STAGE_DIR/bin/init.efi" ]; then
  echo "❌ No init binary found in $STAGE_DIR/bin. Aborting." >&2
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
if [ "${VIRTUAL_ENV:-}" != "$(pwd)/${VENV_DIR}" ]; then
  echo "❌ Python venv not active before ISO build" >&2
  exit 1
fi

bash ./scripts/make_grub_iso.sh
ISO_OUT="out/cohesix_grub.iso"
if [ ! -f "$ISO_OUT" ]; then
  echo "❌ ISO build failed: $ISO_OUT missing" >&2
  exit 1
fi
du -h "$ISO_OUT" 2>/dev/null | tee -a "$LOG_FILE" >&3
find "$STAGE_DIR/bin" -type f -print | tee -a "$LOG_FILE" >&3
if [ ! -d "/srv/cuda" ] || ! command -v nvidia-smi >/dev/null 2>&1 || ! nvidia-smi >/dev/null 2>&1; then
  echo "⚠️ CUDA hardware or /srv/cuda not detected" | tee -a "$LOG_FILE" >&3
fi

# Optional QEMU boot check
QEMU_BIN="qemu-system-${COH_ARCH}"
ISO_IMG="$ISO_OUT"
if [ -x "$(command -v "$QEMU_BIN" 2>/dev/null)" ]; then
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
  OVMF_CODE=""
  OVMF_VARS=""
  if [ "$COH_ARCH" = "x86_64" ]; then
    for p in /usr/share/qemu/OVMF.fd /usr/share/OVMF/OVMF_CODE.fd /usr/share/OVMF/OVMF.fd /usr/share/edk2/ovmf/OVMF_CODE.fd; do
      [ -f "$p" ] && OVMF_CODE="$p" && break
    done
    for p in /usr/share/OVMF/OVMF_VARS.fd /usr/share/edk2/ovmf/OVMF_VARS.fd; do
      [ -f "$p" ] && OVMF_VARS="$p" && break
    done
    [ -f "$OVMF_CODE" ] || { echo "OVMF firmware not found" >&2; exit 1; }
    [ -n "$OVMF_VARS" ] || { echo "OVMF_VARS.fd not found" >&2; exit 1; }
    cp "$OVMF_VARS" "$TMPDIR/OVMF_VARS.fd"
    QEMU_EXTRA=(-bios "$OVMF_CODE" -drive if=pflash,format=raw,file="$TMPDIR/OVMF_VARS.fd" -M q35)
  else
    for p in /usr/share/qemu/QEMU_EFI.fd /usr/share/AAVMF/AAVMF_CODE.fd; do
      [ -f "$p" ] && OVMF_CODE="$p" && break
    done
    [ -n "$OVMF_CODE" ] && QEMU_EXTRA=(-bios "$OVMF_CODE") || QEMU_EXTRA=()
    QEMU_EXTRA+=(-machine virt -cpu cortex-a57)
  fi
  log "🧪 Booting ISO in QEMU..."
  "$QEMU_BIN" \
    "${QEMU_EXTRA[@]}" \
    -cdrom "$ISO_IMG" -net none -m 1024M \
    -no-reboot -nographic -serial file:"$SERIAL_LOG"
  QEMU_EXIT=$?
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
  log "⚠️ $QEMU_BIN not installed; skipping boot test"
fi

BIN_COUNT=$(find "$STAGE_DIR/bin" -type f -perm -111 | wc -l)
ROLE_COUNT=$(find "$STAGE_DIR/roles" -name '*.yaml' | wc -l)
ISO_SIZE_MB=$(du -m "$ISO_OUT" | awk '{print $1}')
echo "ISO BUILD OK: ${BIN_COUNT} binaries, ${ROLE_COUNT} roles, ${ISO_SIZE_MB}MB total" >&3
du -h "$ISO_OUT" | tee -a "$LOG_FILE" >&3
find "$STAGE_DIR/bin" -type f -print | tee -a "$LOG_FILE" >&3

log "✅ [Build Complete] $(date)"

grep -Ei 'error|fail|panic|permission denied|warning' "$LOG_FILE" > "$SUMMARY_ERRORS" || true
grep -A 5 -E '^failures:|thread .* panicked at' "$LOG_FILE" > "$SUMMARY_TEST_FAILS" || true

echo "⚠️  Summary of Errors and Warnings:" | tee -a "$LOG_FILE" >&3
tail -n 10 "$SUMMARY_ERRORS" || echo "✅ No critical issues found" | tee -a "$LOG_FILE" >&3

echo "🪵 Full log saved to $LOG_FILE" >&3
echo "✅ ISO build complete. Run QEMU with:" >&3
echo "qemu-system-${COH_ARCH} -cdrom $ISO_OUT -boot d -m 1024" >&3
