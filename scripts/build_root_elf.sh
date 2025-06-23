# CLASSIFICATION: COMMUNITY
# Filename: build_root_elf.sh v0.9
# Author: Lukas Bower
# Date Modified: 2026-07-24
#!/usr/bin/env bash
set -euo pipefail

ENV_FILE="$(git rev-parse --show-toplevel 2>/dev/null || pwd)/.cohesix_env"
[ -f "$ENV_FILE" ] && source "$ENV_FILE"
if [ -z "${COHESIX_ARCH:-}" ]; then
    echo "Select target architecture:" >&2
    select a in x86_64 aarch64; do
        case "$a" in
            x86_64|aarch64) COHESIX_ARCH="$a"; break;;
            *) echo "Invalid choice" >&2;;
        esac
    done
    echo "COHESIX_ARCH=$COHESIX_ARCH" > "$ENV_FILE"
fi

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
        TARGET="aarch64-unknown-linux-gnu"
        ;;
    x86_64|amd64)
        TARGET="x86_64-unknown-linux-gnu"
        ;;
    *)
        echo "Unsupported architecture: $ARCH" >&2
        exit 1
        ;;
esac

CUDA_LIB="/usr/lib/${ARCH}-linux-gnu"
export CUDA_HOME=/usr
export PATH=/usr/bin:$PATH
export LD_LIBRARY_PATH="${CUDA_LIB}:${LD_LIBRARY_PATH:-}"
echo "Using Rust target: $TARGET"
echo "nvcc path: $(command -v nvcc || echo 'not found')"

mkdir -p "$OUT_DIR"

if command -v nvcc >/dev/null 2>&1 && [ -d /usr/local/cuda ]; then
    echo "CUDA detected; building with GPU support"
    FEATURES="rapier,cuda"
    CARGO_ARGS=()
else
    echo "⚠️ CUDA toolkit not detected. Building without GPU support." >&2
    FEATURES="rapier,no-cuda"
    CARGO_ARGS=(--no-default-features)
fi

if [[ "$TARGET" == *musl ]]; then
    RUSTFLAGS="-C link-arg=-static" \
        cargo build --release "${CARGO_ARGS[@]}" --bin cohesix_root --target "$TARGET" --features "$FEATURES"
else
    cargo build --release "${CARGO_ARGS[@]}" --bin cohesix_root --target "$TARGET" --features "$FEATURES"
fi
cp "target/$TARGET/release/cohesix_root" "$OUT_ELF"

[ -s "$OUT_ELF" ] && echo "ROOT TASK BUILD OK: $OUT_ELF"
