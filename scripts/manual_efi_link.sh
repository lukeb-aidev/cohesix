# CLASSIFICATION: COMMUNITY
# Filename: manual_efi_link.sh v0.2
# Author: Lukas Bower
# Date Modified: 2026-09-03
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"
mkdir -p obj/init_efi out/iso/init
LOG_FILE="init_efi_link.log"
{
  aarch64-linux-gnu-gcc -fno-stack-protector -nostdlib \
    -I"$HOME/gnu-efi/inc" -I"$HOME/gnu-efi/inc/aarch64" \
    -c src/init_efi/main.c -o obj/init_efi/main.o
  aarch64-linux-gnu-ld -nostdlib -znocombreloc -T src/init_efi/linker.ld \
    "$HOME/gnu-efi/gnuefi/crt0-efi-aarch64.o" \
    obj/init_efi/main.o \
    "$HOME/gnu-efi/aarch64/lib/libefi.a" \
    "$HOME/gnu-efi/gnuefi/libgnuefi.a" \
    -o out/iso/init/init.efi
} &> "$LOG_FILE"
