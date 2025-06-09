# CLASSIFICATION: COMMUNITY
# Filename: test_cohcc_build_c.sh v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-17

#!/usr/bin/env bash
set -euo pipefail
mkdir -p /mnt/data /log
cat > /mnt/data/hi.c <<'C'
#include <stdio.h>
int main(){ printf("hi\\n"); return 0; }
C

../bin/cohcc build /mnt/data/hi.c -o /mnt/data/hi --backend=zig --trace
if file /mnt/data/hi | grep -vq "statically linked"; then
    echo "binary not static" >&2
    exit 1
fi
/mnt/data/hi > /mnt/data/hi.out
grep -q hi /mnt/data/hi.out

