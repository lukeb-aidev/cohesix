#!/usr/bin/env bash
# Author: Lukas Bower
set -euo pipefail
BIN="${1:-out/cohesix/staging/rootserver}"
LLVM="${LLVM:-/opt/homebrew/opt/llvm@17/bin}"
MAX_LOAD=$((32*1024*1024))
MAX_BSS=$((8*1024*1024))
FAIL=0

if [ ! -f "$BIN" ]; then
  echo "[ELF][FAIL] missing rootserver: $BIN" >&2
  exit 66
fi

if ! "$LLVM/llvm-readelf" -l "$BIN" | awk '
/LOAD/ {inl=1}
inl && /MemSiz:/ {
  match($0,/0x([0-9a-fA-F]+)/,m);
  if (m[1]!="") {
    v=strtonum("0x" m[1]);
    if (v>'"$MAX_LOAD"') {
      print "[ELF][FAIL] PT_LOAD MemSiz:",v;
      exit 42;
    }
  }
}
'; then
  status=${PIPESTATUS[1]:-1}
  if [ "$status" -eq 42 ]; then
    FAIL=1
  else
    exit 1
  fi
fi

BSS=$("$LLVM/llvm-size" -A "$BIN" | awk '$1==".bss"{print $2}')
if [ -z "$BSS" ] || [ "$BSS" -gt "$MAX_BSS" ]; then
  echo "[ELF][FAIL] .bss=${BSS:-0}"
  FAIL=1
fi

if [ $FAIL -ne 0 ]; then
  exit 1
fi

echo "[ELF][OK]"
