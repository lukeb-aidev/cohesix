# CLASSIFICATION: COMMUNITY
# Filename: build_root_elf.sh v0.15
# Author: Lukas Bower
# Date Modified: 2026-12-01
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
source "$ROOT/scripts/load_arch_config.sh"

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

ARCH="$COHESIX_ARCH"
case "$ARCH" in
    aarch64|arm64)
        TARGET="aarch64-unknown-uefi"
        ;;
    x86_64|amd64)
        TARGET="x86_64-unknown-uefi"
        ;;
    *)
        echo "Unsupported architecture: $ARCH" >&2
        exit 1
        ;;
esac


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

echo "Using Rust target: $TARGET"
echo "nvcc path: $(command -v nvcc || echo 'not found')"

mkdir -p "$OUT_DIR"

if [ -n "$CUDA_HOME" ] && command -v nvcc >/dev/null 2>&1; then
    echo "CUDA detected; building with GPU support"
    FEATURES="rapier,cuda"
    CARGO_ARGS=()
else
    echo "⚠️ CUDA toolkit not detected. Building without GPU support." >&2
    FEATURES="rapier,no-cuda"
    CARGO_ARGS=(--no-default-features)
fi

# Using linker from .cargo/config.toml for ld.lld

do_build() {
    cargo build --release "${CARGO_ARGS[@]}" --bin cohesix_root \
        --target "$TARGET" --features "$FEATURES"
}

copy_output() {
    local built="target/$TARGET/release/cohesix_root"
    if [ ! -s "$built" ]; then
        echo "ERROR: expected ELF not found: $built" >&2
        return 1
    fi
    cp "$built" "$OUT_ELF"
}

do_build
copy_output

[ -s "$OUT_ELF" ] && echo "ROOT TASK BUILD OK: $OUT_ELF"
