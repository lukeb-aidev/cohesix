# CLASSIFICATION: COMMUNITY
# Filename: test_cohcc_basic.sh v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-16

#!/usr/bin/env bash
set -euo pipefail
mkdir -p /mnt/data /log
cat > /mnt/data/hello.c <<'C'
#include <stdio.h>
int main(){ printf("hello\\n"); return 0; }
C

../bin/cohcc /mnt/data/hello.c -o /mnt/data/hello
if readelf -d /mnt/data/hello | grep -q NEEDED; then
    echo "dynamic sections found" >&2
    exit 1
fi
/mnt/data/hello > /mnt/data/hello.out
grep -q hello /mnt/data/hello.out

echo "ok" > /log/cohcc_test_pass.log
