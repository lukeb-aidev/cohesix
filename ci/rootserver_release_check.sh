#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: rootserver_release_check.sh v0.1
# Author: Lukas Bower
# Date Modified: 2030-08-09
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
TARGET="${1:-$ROOT/out/bin/cohesix_root.elf}"
MAX_SIZE="${ROOTSERVER_MAX_BYTES:-1048576}"

log() {
  printf '%s\n' "$*"
}

find_strip_tool() {
  for candidate in aarch64-linux-gnu-strip; do
    if command -v "$candidate" >/dev/null 2>&1; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

find_readelf_tool() {
  for candidate in aarch64-linux-gnu-readelf llvm-readelf readelf; do
    if command -v "$candidate" >/dev/null 2>&1; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

if [ ! -f "$TARGET" ]; then
  echo "❌ Rootserver artefact not found at $TARGET" >&2
  exit 1
fi

strip_tool="$(find_strip_tool)"
if [ -z "$strip_tool" ]; then
  echo "❌ Required strip tool aarch64-linux-gnu-strip not found; install aarch64 binutils." >&2
  exit 1
fi

tmp_root="${TMPDIR:-$ROOT/tmp}"
mkdir -p "$tmp_root"
work_copy="$(mktemp "$tmp_root/cohesix_root_check.XXXXXX")"
trap 'rm -f "$work_copy" "$work_copy.stripped"' EXIT
cp "$TARGET" "$work_copy"

if ! "$strip_tool" --strip-debug -o "$work_copy.stripped" "$work_copy" >/dev/null 2>&1; then
  echo "❌ Failed to execute $strip_tool --strip-debug on $TARGET" >&2
  exit 1
fi

readelf_tool="$(find_readelf_tool)"
if [ -z "$readelf_tool" ]; then
  echo "❌ No readelf-compatible tool detected; install aarch64-linux-gnu-readelf or llvm-readelf." >&2
  exit 1
fi

if "$readelf_tool" --section-headers "$TARGET" 2>/dev/null | grep -qE '\\.debug'; then
  echo "❌ Debug sections detected in $TARGET" >&2
  exit 1
fi

size_bytes="$(wc -c <"$TARGET" | tr -d ' ')"
if [ -n "$size_bytes" ] && [ "$size_bytes" -gt "$MAX_SIZE" ]; then
  echo "❌ Rootserver size ${size_bytes} exceeds budget ${MAX_SIZE}" >&2
  exit 1
fi

log "✅ Rootserver release check passed (${size_bytes} bytes, strip tool ${strip_tool})"
