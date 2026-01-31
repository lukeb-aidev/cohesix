#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
IMAGE_DIR="${ROOT_DIR}/image"

QEMU_BIN="${QEMU_BIN:-qemu-system-aarch64}"
HOST_OS="$(uname -s 2>/dev/null || true)"
TCP_PORT="${TCP_PORT:-31337}"
UDP_PORT="${UDP_PORT:-31338}"
SMOKE_PORT="${SMOKE_PORT:-31339}"
GIC_VER_FILE="${IMAGE_DIR}/gic-version.txt"
GIC_VER="2"
if [[ -f "${GIC_VER_FILE}" ]]; then
  GIC_VER="$(tr -d '\n' < "${GIC_VER_FILE}")"
fi

ELFLOADER="${IMAGE_DIR}/elfloader"
KERNEL="${IMAGE_DIR}/kernel.elf"
ROOTSERVER="${IMAGE_DIR}/rootserver"
CPIO="${IMAGE_DIR}/cohesix-system.cpio"

for path in "${ELFLOADER}" "${KERNEL}" "${ROOTSERVER}" "${CPIO}"; do
  if [[ ! -f "${path}" ]]; then
    echo "[qemu] missing: ${path}" >&2
    exit 1
  fi
done

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

has_kvm_device() {
  [[ -c /dev/kvm && -r /dev/kvm && -w /dev/kvm ]]
}

qemu_accel_supported() {
  local accel="$1"
  local help
  help="$("${QEMU_BIN}" -accel help 2>/dev/null || true)"
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
  if [[ "$accel" == "kvm" && "$HOST_OS" == "Linux" ]]; then
    if ! has_kvm_device; then
      echo "[qemu] Requested QEMU accelerator 'kvm' but /dev/kvm is unavailable; falling back to tcg" >&2
      accel="tcg"
    fi
  fi
  if ! qemu_accel_supported "$accel"; then
    echo "[qemu] Requested QEMU accelerator '$accel' not supported by ${QEMU_BIN}; falling back to tcg" >&2
    accel="tcg"
  fi
  echo "$accel"
}

QEMU_ACCEL="$(resolve_qemu_accel)"
echo "[qemu] Using QEMU accel: ${QEMU_ACCEL}"

"${QEMU_BIN}" \
  -accel "${QEMU_ACCEL}" \
  -machine "virt,gic-version=${GIC_VER}" \
  -cpu cortex-a57 \
  -m 1024 \
  -smp 1 \
  -serial mon:stdio \
  -display none \
  -kernel "${ELFLOADER}" \
  -initrd "${CPIO}" \
  -device loader,file="${KERNEL}",addr=0x70000000,force-raw=on \
  -device loader,file="${ROOTSERVER}",addr=0x80000000,force-raw=on \
  -global virtio-mmio.force-legacy=off \
  -netdev "user,id=net0,hostfwd=tcp:127.0.0.1:${TCP_PORT}-:31337,hostfwd=udp:127.0.0.1:${UDP_PORT}-:31338,hostfwd=tcp:127.0.0.1:${SMOKE_PORT}-:31339" \
  -device "virtio-net-device,netdev=net0,mac=52:55:00:d1:55:01,bus=virtio-mmio-bus.0"
