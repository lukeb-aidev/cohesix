#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: test_boot_efi.sh v0.3
# Author: Lukas Bower
# Date Modified: 2025-07-22
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

if ! command -v qemu-system-x86_64 >/dev/null; then
    echo "ERROR: qemu-system-x86_64 not installed" >&2
    exit 1
fi

make bootloader kernel
objdump -h out/EFI/BOOT/BOOTX64.EFI > out/BOOTX64_sections.txt

LOGFILE="out/qemu_debug.log"
QEMU_ARGS=(-bios /usr/share/qemu/OVMF.fd \
    -drive format=raw,file=fat:rw:out/ -net none -M q35 -m 256M \
    -no-reboot -monitor none)

qemu-system-x86_64 "${QEMU_ARGS[@]}" -nographic -serial file:"${LOGFILE}" || true
tail -n 20 "${LOGFILE}" || true

grep -q "EFI loader" "${LOGFILE}" || exit 1
grep -q "Kernel launched" "${LOGFILE}" || exit 1

