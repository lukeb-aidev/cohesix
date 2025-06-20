// CLASSIFICATION: COMMUNITY
// Filename: build_root_elf.sh v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-20
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
SRC="$ROOT/src/seL4"
OUT_DIR="$ROOT/out"
OUT_ELF="$OUT_DIR/cohesix_root.elf"

ARCH="$(uname -m)"
case "$ARCH" in
    aarch64|arm64)
        CC=aarch64-linux-gnu-gcc
        ;;
    x86_64)
        CC=x86_64-elf-gcc
        ;;
    *)
        echo "Unsupported architecture: $ARCH" >&2
        exit 1
        ;;
esac

mkdir -p "$OUT_DIR"
"$CC" -static -nostdlib -o "$OUT_ELF" "$SRC/sel4_start.S" "$SRC/root_task.c"

[ -s "$OUT_ELF" ] && echo "Built $OUT_ELF"
