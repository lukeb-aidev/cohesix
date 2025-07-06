# CLASSIFICATION: COMMUNITY
# Filename: manual_efi_link.sh v0.5
# Author: Lukas Bower
# Date Modified: 2026-09-08
#!/bin/bash
[ -n "$BASH_VERSION" ] && set -euo pipefail
ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"
mkdir -p obj/init_efi out/init
LOG_FILE="init_efi_link.log"
{
  aarch64-linux-gnu-gcc -fno-stack-protector -nostdlib \
    -I"$HOME/gnu-efi/inc" -I"$HOME/gnu-efi/inc/aarch64" \
    -c src/init_efi/main.c -o obj/init_efi/main.o
  aarch64-linux-gnu-ld -nostdlib -znocombreloc -shared -Bsymbolic \
    -T src/init_efi/elf_aarch64_efi.lds \
    "$HOME/gnu-efi/gnuefi/crt0-efi-aarch64.o" \
    obj/init_efi/main.o \
    -L"$HOME/gnu-efi/aarch64/lib" -lefi -lgnuefi \
    -o out/init/init.efi
} &> "$LOG_FILE"

if grep -qi "rwx" "$LOG_FILE"; then
  aarch64-linux-gnu-ld -nostdlib -znocombreloc --no-warn-rwx-segment -shared -Bsymbolic \
    -T src/init_efi/elf_aarch64_efi.lds \
    "$HOME/gnu-efi/gnuefi/crt0-efi-aarch64.o" \
    obj/init_efi/main.o \
    -L"$HOME/gnu-efi/aarch64/lib" -lefi -lgnuefi \
    -o out/init/init.efi >> "$LOG_FILE" 2>&1
fi

file out/init/init.efi >> "$LOG_FILE"
