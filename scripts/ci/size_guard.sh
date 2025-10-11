#!/usr/bin/env bash
# Author: Lukas Bower

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "Usage: scripts/ci/size_guard.sh <cpio-path>" >&2
    exit 1
fi

CPIO_PATH="$1"
LIMIT=$((4 * 1024 * 1024))

if [[ ! -f "$CPIO_PATH" ]]; then
    echo "Rootfs archive not found: $CPIO_PATH" >&2
    exit 1
fi

SIZE=$(stat -f%z "$CPIO_PATH" 2>/dev/null || stat --format=%s "$CPIO_PATH")

if [[ "$SIZE" -gt "$LIMIT" ]]; then
    echo "Archive exceeds 4 MiB: $SIZE bytes" >&2
    exit 2
fi

echo "Archive size OK: $SIZE bytes"
