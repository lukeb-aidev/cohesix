#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: test_boot_efi.sh v0.2
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
    -drive format=raw,file=fat:rw:out/ -net none -M q35 -m 256M -no-reboot)

qemu-system-x86_64 "${QEMU_ARGS[@]}" -nographic -serial file:"${LOGFILE}" || true

grep -q "Booting Cohesix from UEFI" "${LOGFILE}"
grep -q "kernel.elf loaded successfully" "${LOGFILE}"
