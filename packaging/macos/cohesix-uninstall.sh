#!/bin/sh
# Author: Lukas Bower
# Purpose: Remove only receipt-owned and byte-matching Mac host code while retaining operator state and credentials.
# Copyright 2026 Lukas Bower
set -eu

root='/Library/Application Support/Cohesix'
receipt='com.cohesix.host'
manifest="$root/INSTALLED.sha256"

if [ "$(id -u)" -ne 0 ]; then
  printf '%s\n' 'Run this helper with administrator privileges.' >&2
  exit 1
fi
if ! /usr/sbin/pkgutil --pkg-info "$receipt" >/dev/null 2>&1; then
  printf '%s\n' 'Cohesix package receipt is missing.' >&2
  exit 1
fi
if [ ! -f "$manifest" ] || [ -L "$manifest" ]; then
  printf '%s\n' 'Installed file manifest is missing or linked.' >&2
  exit 1
fi

receipt_files=$(/usr/sbin/pkgutil --files "$receipt")
while IFS= read -r line; do
  hash=${line%%  *}
  file=${line#*  }
  case "$hash" in
    *[!0123456789abcdef]*|'') printf '%s\n' 'Invalid installed hash.' >&2; exit 1 ;;
  esac
  if [ "${#hash}" -ne 64 ]; then
    printf '%s\n' 'Invalid installed hash length.' >&2
    exit 1
  fi
  case "$file" in
    '/Applications/SwarmUI.app/'*|'/Library/Application Support/Cohesix/'*) ;;
    *) printf '%s\n' 'Refusing path outside Cohesix install roots.' >&2; exit 1 ;;
  esac
  case "$file" in
    *'/../'*|*'/./'*|*'\\'*)
      printf '%s\n' 'Refusing noncanonical installed path.' >&2
      exit 1 ;;
  esac
  relative=${file#/}
  if ! printf '%s\n' "$receipt_files" | /usr/bin/grep -Fqx -- "$relative"; then
    printf '%s\n' 'Installed path is not in the package receipt.' >&2
    exit 1
  fi
done < "$manifest"

if ! /usr/bin/shasum -a 256 --check "$manifest"; then
  printf '%s\n' 'Installed bytes changed; retain the installation for manual review.' >&2
  exit 1
fi

while IFS= read -r line; do
  file=${line#*  }
  /bin/rm -- "$file"
done < "$manifest"

/bin/rm -- "$root/INSTALLED.sha256" "$root/installed-payload.json"
/usr/bin/find '/Applications/SwarmUI.app' -depth -type d -empty -delete 2>/dev/null || true
/usr/bin/find "$root" -depth -type d -empty -delete 2>/dev/null || true
/usr/sbin/pkgutil --forget "$receipt"
printf '%s\n' 'Cohesix package code removed. User data, credentials, models, and evidence were retained.'
