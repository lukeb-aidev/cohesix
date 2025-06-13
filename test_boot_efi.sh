#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: test_boot_efi.sh v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-22
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
make bootloader
objdump -h out/EFI/BOOT/BOOTX64.EFI > out/BOOTX64_sections.txt
qemu-system-x86_64 -bios /usr/share/ovmf/OVMF.fd -drive format=raw,file=fat:rw:out/ -serial file:out/boot.log -nographic -net none -M q35 -m 256M -no-reboot || true
grep -q "Booting Cohesix" out/boot.log
