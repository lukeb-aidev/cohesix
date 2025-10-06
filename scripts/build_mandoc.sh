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

# Prefer a vendored mandoc binary if one has been provided; this avoids
# rebuilding (and any host toolchain dependency) when the prebuilt artefact
# is already under version control.
for candidate in \
    "$MANDOC_DIR/mandoc.$ARCH" \
    "$MANDOC_DIR/bin/mandoc.$ARCH" \
    "$OUT/mandoc.$ARCH"; do
    if [ -f "$candidate" ]; then
        mkdir -p "$OUT"
        cp "$candidate" "$OUT/mandoc.$ARCH"
        echo "mandoc staged from vendored binary: $candidate"
        exit 0
    fi
done

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
# Prefer cross toolchains when available so we produce Linux-compatible binaries.
if [ -n "${MANDOC_CC:-}" ]; then
    CC_CHOOSEN="$MANDOC_CC"
else
    case "$ARCH" in
        aarch64)
            if command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
                CC_CHOOSEN="aarch64-linux-gnu-gcc"
            else
                CC_CHOOSEN="cc"
            fi
            ;;
        x86_64)
            if command -v x86_64-linux-gnu-gcc >/dev/null 2>&1; then
                CC_CHOOSEN="x86_64-linux-gnu-gcc"
            else
                CC_CHOOSEN="cc"
            fi
            ;;
        *)
            CC_CHOOSEN="cc"
            ;;
    esac
fi

HOST_TRIPLE=""
case "$ARCH" in
    aarch64) HOST_TRIPLE="aarch64-linux-gnu" ;;
    x86_64) HOST_TRIPLE="x86_64-linux-gnu" ;;
esac

CONFIG_FLAGS="--disable-mandocdb"
if [ -n "$HOST_TRIPLE" ]; then
    CONFIG_FLAGS="$CONFIG_FLAGS --host=$HOST_TRIPLE"
fi

if [ -x ./configure ]; then
    CC="$CC_CHOOSEN" ./configure $CONFIG_FLAGS >/dev/null
fi

# Override config.h entries that were detected for the Darwin build host but are
# not available on the Linux/aarch64 target we ship in the root filesystem.
if [ -f config.h ]; then
    python3 - "$PWD/config.h" <<'PY'
import sys
path = sys.argv[1]
text = open(path, 'r', encoding='utf-8').read()
replacements = {
    '#define HAVE_SYS_ENDIAN 1': '#define HAVE_SYS_ENDIAN 0',
    '#define HAVE_STRLCAT 1': '#define HAVE_STRLCAT 0',
    '#define HAVE_STRLCPY 1': '#define HAVE_STRLCPY 0',
    '#define HAVE_EFTYPE 1': '#define HAVE_EFTYPE 0',
}
for old, new in replacements.items():
    text = text.replace(old, new)
open(path, 'w', encoding='utf-8').write(text)
PY
fi

CPPFLAGS_EXTRA="-DHAVE_SYS_ENDIAN=0 -DHAVE_STRLCAT=0 -DHAVE_STRLCPY=0 -DHAVE_EFTYPE=0 -DEFTYPE=EIO"

make CC="$CC_CHOOSEN" CPPFLAGS="$CPPFLAGS_EXTRA" \
  LDFLAGS=-static WITHOUT_X11=1 WITHOUT_MANDOCDB=1 >/dev/null
cp mandoc "$OUT/mandoc.$ARCH"
echo "mandoc built for $ARCH at $OUT/mandoc.$ARCH"
