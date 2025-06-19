# CLASSIFICATION: COMMUNITY
# Filename: test_cohcc_build_c.sh v0.2
# Author: Lukas Bower
# Date Modified: 2025-12-09

#!/usr/bin/env bash
set -euo pipefail
tmpdir=$(mktemp -d)
mkdir -p "$tmpdir" /log
cat > "$tmpdir/hi.c" <<'C'
#include <stdio.h>
int main(){ printf("hi\\n"); return 0; }
C

../bin/cohcc build "$tmpdir/hi.c" -o "$tmpdir/hi" --backend=zig --trace
if file "$tmpdir/hi" | grep -vq "statically linked"; then
    echo "binary not static" >&2
    exit 1
fi
"$tmpdir/hi" > "$tmpdir/hi.out"
grep -q hi "$tmpdir/hi.out"
rm -rf "$tmpdir"

