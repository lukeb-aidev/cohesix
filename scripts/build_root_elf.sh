// CLASSIFICATION: COMMUNITY
// Filename: build_root_elf.sh v0.2
// Author: Lukas Bower
// Date Modified: 2025-12-27
#!/usr/bin/env bash
set -euo pipefail

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
if [[ "$TARGET" == *musl ]]; then
    RUSTFLAGS="-C link-arg=-static" \
        cargo build --release --bin cohesix_root --target "$TARGET"
else
    cargo build --release --bin cohesix_root --target "$TARGET"
fi
cp "target/$TARGET/release/cohesix_root" "$OUT_ELF"

[ -s "$OUT_ELF" ] && echo "ROOT TASK BUILD OK: $OUT_ELF"
