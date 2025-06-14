# CLASSIFICATION: COMMUNITY
# Filename: check-qemu-deps.sh v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-28
#!/usr/bin/env bash
set -euo pipefail

missing=0

if ! command -v qemu-system-x86_64 >/dev/null 2>&1; then
    echo "ERROR: qemu-system-x86_64 not found." >&2
    echo "Install QEMU via your package manager (e.g., 'sudo apt install qemu-system-x86')." >&2
    missing=1
fi

firmware=""
for f in /usr/share/OVMF/OVMF.fd /usr/share/qemu/OVMF.fd \
         /usr/share/edk2/ovmf/OVMF_CODE.fd /usr/share/OVMF/OVMF_CODE.fd; do
    if [ -f "$f" ]; then
        firmware="$f"
        break
    fi
done

if [ -z "$firmware" ]; then
    echo "ERROR: UEFI firmware OVMF.fd not found." >&2
    echo "Install the 'ovmf' or 'edk2-ovmf' package for your distribution." >&2
    missing=1
fi

if [ ! -f /usr/include/efi/efi.h ]; then
    echo "ERROR: gnu-efi headers not found." >&2
    echo "Install gnu-efi development headers (e.g., 'sudo apt install gnu-efi')." >&2
    missing=1
fi

if [ "$missing" -eq 1 ]; then
    exit 1
else
    echo "QEMU and EFI dependencies are present." >&2
    exit 0
fi
