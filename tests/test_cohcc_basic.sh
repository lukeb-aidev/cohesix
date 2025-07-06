# CLASSIFICATION: COMMUNITY
# Filename: test_cohcc_basic.sh v0.2
# Author: Lukas Bower
# Date Modified: 2025-12-09

#!/usr/bin/env bash
set -euo pipefail
tmpdir=$(mktemp -d)
mkdir -p "$tmpdir" /log
cat > "$tmpdir/hello.c" <<'C'
#include <stdio.h>
int main(){ printf("hello\\n"); return 0; }
C

../bin/cohcc "$tmpdir/hello.c" -o "$tmpdir/hello"
if readelf -d "$tmpdir/hello" | grep -q NEEDED; then
    echo "dynamic sections found" >&2
    exit 1
fi
"$tmpdir/hello" > "$tmpdir/hello.out"
grep -q hello "$tmpdir/hello.out"

rm -rf "$tmpdir"

echo "ok" > /log/cohcc_test_pass.log
