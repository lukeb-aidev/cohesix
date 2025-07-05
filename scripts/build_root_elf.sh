# CLASSIFICATION: COMMUNITY
# Filename: build_root_elf.sh v0.19
# Author: Lukas Bower
# Date Modified: 2027-01-15
#!/usr/bin/env bash
set -euo pipefail
export MEMCHR_DISABLE_RUNTIME_CPU_FEATURE_DETECTION=1

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
source "$ROOT/scripts/load_arch_config.sh"

ensure_vendor() {
    if [ ! -d "$ROOT/vendor" ]; then
        echo "📦 Populating cargo vendor directory"
        cargo vendor -h >/dev/null 2>&1 || cargo install cargo-vendor
        (cd "$ROOT" && cargo vendor > /dev/null)
    fi
}

ensure_vendor

command -v ld.lld >/dev/null 2>&1 || {
    echo "ERROR: ld.lld not found" >&2
    exit 1
}
ld.lld --version >&2

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

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
OUT_DIR="$ROOT/out"
OUT_ELF="$OUT_DIR/cohesix_root.elf"



# Detect CUDA installation
CUDA_HOME=""
if command -v nvcc >/dev/null 2>&1; then
    NVCC_PATH="$(command -v nvcc)"
    CUDA_HOME="$(dirname "$(dirname "$NVCC_PATH")")"
elif [ -d /usr/local/cuda ]; then
    CUDA_HOME="/usr/local/cuda"
else
    CUDA_HOME="$(ls -d /usr/local/cuda-* 2>/dev/null | head -n1)"
fi

if [ -n "$CUDA_HOME" ] && [ -f "$CUDA_HOME/bin/nvcc" ]; then
    export CUDA_HOME
    export PATH="$CUDA_HOME/bin:$PATH"
    if [ -d "$CUDA_HOME/lib64" ]; then
        export LD_LIBRARY_PATH="$CUDA_HOME/lib64:${LD_LIBRARY_PATH:-}"
    elif [ -d "$CUDA_HOME/lib" ]; then
        export LD_LIBRARY_PATH="$CUDA_HOME/lib:${LD_LIBRARY_PATH:-}"
    fi
    export CUDA_LIBRARY_PATH="$LD_LIBRARY_PATH"
    echo "CUDA detected at $CUDA_HOME"
else
    echo "⚠️ CUDA toolkit not detected." >&2
fi

mkdir -p "$OUT_DIR"

cargo build --release --target=target-sel4.json --bin cohesix_root

local_target="target/target-sel4/release/cohesix_root"
if [ ! -s "$local_target" ]; then
    local_target="${local_target}.elf"
fi
cp "$local_target" "$OUT_ELF"

[ -s "$OUT_ELF" ] && echo "ROOT TASK BUILD OK: $OUT_ELF"
