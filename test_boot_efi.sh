#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: test_boot_efi.sh v0.6
# Author: Lukas Bower
# Date Modified: 2025-07-22
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

if ! command -v qemu-system-x86_64 >/dev/null; then
    echo "ERROR: qemu-system-x86_64 not installed" >&2
    exit 1
fi

TOOLCHAIN="${CC:-}"
if [[ -z "$TOOLCHAIN" ]]; then
    if command -v clang >/dev/null; then
        TOOLCHAIN=clang
    else
        TOOLCHAIN=gcc
    fi
fi
echo "Using $TOOLCHAIN toolchain for UEFI build..."
if [[ ! -f /usr/include/efi/efi.h ]]; then
    echo "ERROR: gnu-efi headers not found" >&2
    exit 1
fi
if [[ ! -f /usr/include/efi/x86_64/efibind.h && ! -f /usr/include/efi/$(uname -m)/efibind.h ]]; then
    echo "WARNING: architecture headers missing; build may fail" >&2
fi

"$TOOLCHAIN" --version | head -n 1
"$TOOLCHAIN" -E -x c - -v </dev/null 2>&1 | sed -n '/search starts here:/,/End of search list/p'

make print-env CC="$TOOLCHAIN"
make -n bootloader kernel CC="$TOOLCHAIN" > out/make_debug.log
if ! make bootloader kernel CC="$TOOLCHAIN"; then
    echo "Build failed" >&2
    exit 1
fi
objdump -h out/EFI/BOOT/BOOTX64.EFI > out/BOOTX64_sections.txt

LOGFILE="out/qemu_debug.log"
QEMU_ARGS=(-bios /usr/share/qemu/OVMF.fd \
    -drive format=raw,file=fat:rw:out/ -net none -M q35 -m 256M \
    -no-reboot -monitor none)

qemu-system-x86_64 "${QEMU_ARGS[@]}" -nographic -serial file:"${LOGFILE}" || true
tail -n 20 "${LOGFILE}" || true

grep -q "EFI loader" "${LOGFILE}" || exit 1
grep -q "Kernel launched" "${LOGFILE}" || exit 1

