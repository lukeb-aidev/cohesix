# CLASSIFICATION: COMMUNITY
# Filename: build_mandoc.sh v0.2
# Author: Lukas Bower
# Date Modified: 2025-06-22
#!/bin/sh
set -e
VER=1.9.9
ARCH=$(uname -m)
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/prebuilt/mandoc"
mkdir -p "$OUT"
TMP=$(mktemp -d)
cd "$TMP"
curl -L "https://mandoc.bsd.lv/snapshots/mdocml-${VER}.tar.gz" -o mandoc.tar.gz
if [ ! -s mandoc.tar.gz ]; then
    echo "Download failed" >&2
    exit 1
fi
tar xzf mandoc.tar.gz
cd mdocml-${VER}
make LDFLAGS=-static WITHOUT_X11=1 WITHOUT_MANDOCDB=1 >/dev/null
cp mandoc "$OUT/mandoc.$ARCH"
echo "mandoc built for $ARCH at $OUT/mandoc.$ARCH"
