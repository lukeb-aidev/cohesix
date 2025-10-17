#!/usr/bin/env bash
# Author: Lukas Bower
set -euo pipefail

for tool in qemu-system-aarch64 qemu-img; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
        echo "${tool} not found; install QEMU for AArch64." >&2
        exit 1
    fi
done

ESP_IMG="${ESP_IMG:-out/cohesix/esp.img}"

CANDIDATES=(
  "/opt/homebrew/share/qemu/edk2-aarch64-code.fd"
  "/opt/homebrew/share/edk2/aarch64/QEMU_EFI.fd"
  "/usr/local/share/qemu/edk2-aarch64-code.fd"
)
QEMU_FIRM="${QEMU_FIRM:-}"
if [ -z "${QEMU_FIRM}" ]; then
    for candidate in "${CANDIDATES[@]}"; do
        if [ -f "${candidate}" ]; then
            QEMU_FIRM="${candidate}"
            break
        fi
    done
fi

if [ -z "${QEMU_FIRM}" ]; then
    echo "UEFI firmware not found. Set QEMU_FIRM to the path of QEMU_EFI.fd." >&2
    exit 1
fi

if [ ! -f "${ESP_IMG}" ]; then
    echo "ESP image not found: ${ESP_IMG}. Run scripts/esp-build.sh first." >&2
    exit 1
fi

VARSTORE="${VARSTORE:-out/cohesix/edk2_vars.fd}"
mkdir -p "$(dirname "${VARSTORE}")"
if [ ! -f "${VARSTORE}" ]; then
    qemu-img create -f raw "${VARSTORE}" 64M >/dev/null
fi

exec qemu-system-aarch64 \
    -machine virt,gic-version=2 \
    -cpu cortex-a57 -m 1024 -smp 1 \
    -serial mon:stdio -display none \
    -bios "${QEMU_FIRM}" \
    -drive if=none,id=esp,format=raw,file="${ESP_IMG}" \
    -device virtio-blk-pci,drive=esp \
    -device virtio-net-pci,netdev=net0 \
    -netdev user,id=net0
