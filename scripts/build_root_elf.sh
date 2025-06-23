# CLASSIFICATION: COMMUNITY
# Filename: build_root_elf.sh v0.5
# Author: Lukas Bower
# Date Modified: 2026-07-22
#!/usr/bin/env bash
set -euo pipefail

ARCH="$(uname -m)"
if [[ "$ARCH" = "aarch64" ]] && ! command -v aarch64-linux-musl-gcc >/dev/null 2>&1; then
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

ARCH="${ARCH:-$(uname -m)}"
case "$ARCH" in
    aarch64|arm64)
        TARGET="aarch64-unknown-linux-musl"
        ;;
    x86_64)
        if command -v x86_64-linux-musl-gcc >/dev/null 2>&1; then
            TARGET="x86_64-unknown-linux-musl"
        else
            TARGET="x86_64-unknown-linux-gnu"
        fi
        ;;
    *)
        echo "Unsupported architecture: $ARCH" >&2
        exit 1
        ;;
esac

mkdir -p "$OUT_DIR"

FEATURES="rapier"
cuda_available() {
    command -v nvcc >/dev/null 2>&1 && return 0
    [ -n "${CUDA_HOME:-}" ] && [ -d "$CUDA_HOME" ] && return 0
    [ -d /usr/local/cuda ] && return 0
    return 1
}

if [ "${COH_GPU:-0}" = "1" ]; then
    if cuda_available; then
        FEATURES="${FEATURES},cuda"
    else
        echo "⚠️ CUDA toolkit not detected. Building without GPU support." >&2
        echo "Install with: sudo apt install nvidia-cuda-toolkit" >&2
        echo "Or visit: https://developer.nvidia.com/cuda-downloads" >&2
        FEATURES="${FEATURES},no-cuda"
        COH_GPU=0
    fi
else
    FEATURES="${FEATURES},no-cuda"
fi

if [[ "$TARGET" == *musl ]]; then
    RUSTFLAGS="-C link-arg=-static" \
        cargo build --release --bin cohesix_root --target "$TARGET" --features "$FEATURES"
else
    cargo build --release --bin cohesix_root --target "$TARGET" --features "$FEATURES"
fi
cp "target/$TARGET/release/cohesix_root" "$OUT_ELF"

[ -s "$OUT_ELF" ] && echo "ROOT TASK BUILD OK: $OUT_ELF"
