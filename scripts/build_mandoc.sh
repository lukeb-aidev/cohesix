# CLASSIFICATION: COMMUNITY
# Filename: build_mandoc.sh v0.2
# Author: Lukas Bower
# Date Modified: 2025-10-06
#!/bin/sh
set -e
VER=1.9.9
ARCH=$(uname -m)
case "$ARCH" in
    arm64) ARCH="aarch64" ;;
    amd64) ARCH="x86_64" ;;
esac
# Build mandoc from the vendored snapshot under third_party/mandoc.
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/prebuilt/mandoc"
MANDOC_DIR="$ROOT/third_party/mandoc"

pick_tarball() {
    for candidate in \
        "$MANDOC_DIR/mdocml-${VER}.tar.gz" \
        "$MANDOC_DIR/mandoc-${VER}.tar.gz" \
        "$MANDOC_DIR/mandoc.tar.gz" \
        "$MANDOC_DIR/mandoc.tar"; do
        if [ -f "$candidate" ]; then
            printf '%s\n' "$candidate"
            return
        fi
    done
    # Fallback: first tarball found in directory (lexicographically sorted)
    find "$MANDOC_DIR" -maxdepth 1 -type f \( -name '*.tar.gz' -o -name '*.tgz' -o -name '*.tar' \) | sort | head -n 1
}

VENDORED_TARBALL=$(pick_tarball)

if [ -z "$VENDORED_TARBALL" ]; then
    PREBUILT_SOURCE=""
    PREBUILT_SOURCE=""
    for candidate in \
        "$MANDOC_DIR/mandoc.$ARCH" \
        "$MANDOC_DIR/bin/mandoc.$ARCH" \
        "$MANDOC_DIR/prebuilt/mandoc.$ARCH"; do
        if [ -f "$candidate" ]; then
            PREBUILT_SOURCE="$candidate"
            break
        fi
    done
    if [ -n "$PREBUILT_SOURCE" ]; then
        mkdir -p "$OUT"
        cp "$PREBUILT_SOURCE" "$OUT/mandoc.$ARCH"
        echo "mandoc staged from vendored binary: $PREBUILT_SOURCE" >&2
        exit 0
    fi
    echo "Missing vendored mandoc archive under $MANDOC_DIR" >&2
    echo "Populate third_party/mandoc with mandoc tarball or mandoc.$ARCH to build offline." >&2
    exit 1
fi

mkdir -p "$OUT"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT INT HUP TERM
cd "$TMP"
case "$VENDORED_TARBALL" in
    *.tar.gz|*.tgz)
        tar xzf "$VENDORED_TARBALL"
        ;;
    *.tar)
        tar xf "$VENDORED_TARBALL"
        ;;
    *)
        echo "Unsupported mandoc archive format: $VENDORED_TARBALL" >&2
        exit 1
        ;;
esac

SRC_DIR=$(find . -maxdepth 1 -type d -name 'mdocml-*' -o -name 'mandoc-*' | head -n 1)
if [ -z "$SRC_DIR" ]; then
    SRC_DIR="."
fi

cd "$SRC_DIR"
make LDFLAGS=-static WITHOUT_X11=1 WITHOUT_MANDOCDB=1 >/dev/null
cp mandoc "$OUT/mandoc.$ARCH"
echo "mandoc built for $ARCH at $OUT/mandoc.$ARCH"
