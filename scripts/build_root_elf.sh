# CLASSIFICATION: COMMUNITY
# Filename: build_root_elf.sh v0.4
# Author: Lukas Bower
# Date Modified: 2025-06-23
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
if [ "${COH_GPU:-0}" = "1" ]; then
    FEATURES="${FEATURES},cuda"
fi

if [[ "$TARGET" == *musl ]]; then
    RUSTFLAGS="-C link-arg=-static" \
        cargo build --release --bin cohesix_root --target "$TARGET" --features "$FEATURES"
else
    cargo build --release --bin cohesix_root --target "$TARGET" --features "$FEATURES"
fi
cp "target/$TARGET/release/cohesix_root" "$OUT_ELF"

[ -s "$OUT_ELF" ] && echo "ROOT TASK BUILD OK: $OUT_ELF"
