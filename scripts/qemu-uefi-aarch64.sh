#!/usr/bin/env bash
# Author: Lukas Bower
set -euo pipefail

QEMU_BIN="${QEMU_BIN:-qemu-system-aarch64}"

for tool in "${QEMU_BIN}" qemu-img; do
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

CONSOLE_PORT="${CONSOLE_PORT:-31337}"
UDP_ECHO_PORT="${UDP_ECHO_PORT:-31338}"
TCP_SMOKE_PORT="${TCP_SMOKE_PORT:-31339}"
NETDEV_OPTS="${NETDEV_OPTS:-user,id=net0,hostfwd=tcp::${CONSOLE_PORT}-:${CONSOLE_PORT},hostfwd=udp::${UDP_ECHO_PORT}-:${UDP_ECHO_PORT},hostfwd=tcp::${TCP_SMOKE_PORT}-:${TCP_SMOKE_PORT}}"

detect_qemu_accel() {
    local accel="${COHESIX_QEMU_ACCEL:-${QEMU_ACCEL:-}}"
    if [[ -n "$accel" ]]; then
        echo "$accel"
        return
    fi

    local host_os
    host_os="$(uname -s 2>/dev/null || true)"
    case "$host_os" in
        Darwin)
            echo "hvf"
            ;;
        Linux)
            if [[ -c /dev/kvm && -r /dev/kvm && -w /dev/kvm ]]; then
                echo "kvm"
            else
                echo "tcg"
            fi
            ;;
        *)
            echo "tcg"
            ;;
    esac
}

qemu_accel_supported() {
    local accel="$1"
    local help
    help="$("$QEMU_BIN" -accel help 2>/dev/null || true)"
    if [[ -z "$help" ]]; then
        return 0
    fi
    echo "$help" | grep -Eiq "(^|[ ,])${accel}([ ,]|$)"
}

resolve_qemu_accel() {
    local accel
    accel="$(detect_qemu_accel)"
    if [[ -z "$accel" ]]; then
        accel="tcg"
    fi
    if ! qemu_accel_supported "$accel"; then
        echo "[qemu-uefi] Requested QEMU accelerator '$accel' not supported by ${QEMU_BIN}; falling back to tcg" >&2
        accel="tcg"
    fi
    echo "$accel"
}

QEMU_ACCEL="$(resolve_qemu_accel)"
echo "[qemu-uefi] Using QEMU accel: ${QEMU_ACCEL}"

exec "$QEMU_BIN" \
    -accel "${QEMU_ACCEL}" \
    -machine virt,gic-version=2 \
    -cpu cortex-a57 -m 1024 -smp 1 \
    -serial mon:stdio -display none \
    -bios "${QEMU_FIRM}" \
    -drive if=none,id=esp,format=raw,file="${ESP_IMG}" \
    -device virtio-blk-pci,drive=esp \
    -device rtl8139,netdev=net0 \
    -netdev "${NETDEV_OPTS}"
