# CLASSIFICATION: COMMUNITY
# Filename: test_cohbox.sh v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-18

#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
LOG_DIR="${TMPDIR:-$(mktemp -d)}/cohbox"
BIN="$ROOT_DIR/bin/cohbox"

mkdir -p "$LOG_DIR" /mnt/data

# Log help output
"$BIN" --help > "$LOG_DIR/busybox_build.log"

# Verify static linking
if ldd "$BIN" 2>&1 | grep -q "not a dynamic executable"; then
    true
else
    echo "dynamic linking detected" >&2
    exit 1
fi

# Run approved applets
TMP="${TMPDIR:-$(mktemp -d)}/cohbox_test"
mkdir -p "$TMP"

"$BIN" sh -c 'true'
"$BIN" echo "hello" > "$TMP/hello.txt"
"$BIN" cat "$TMP/hello.txt" >"$LOG_DIR/cat.log"
"$BIN" cp "$TMP/hello.txt" "$TMP/copy.txt"
"$BIN" ls "$TMP" >"$LOG_DIR/ls.log"
"$BIN" mv "$TMP/copy.txt" "$TMP/moved.txt"
"$BIN" grep hello "$TMP/moved.txt" >"$LOG_DIR/grep.log"
"$BIN" head -n 1 "$TMP/moved.txt" >"$LOG_DIR/head.log"
"$BIN" tail -n 1 "$TMP/moved.txt" >"$LOG_DIR/tail.log"
"$BIN" printf "%s\n" "ok" >"$LOG_DIR/printf.log"
"$BIN" test -f "$TMP/moved.txt"
"$BIN" mkdir "$TMP/dir"
"$BIN" rmdir "$TMP/dir"
"$BIN" rm "$TMP/moved.txt"
"$BIN" vi --help >"$LOG_DIR/vi.log"

rm "$TMP/hello.txt"
rmdir "$TMP"
