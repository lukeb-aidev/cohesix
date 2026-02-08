#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Launch Cohesix under QEMU from a release bundle.
# Copyright 2026 Lukas Bower
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
IMAGE_DIR="${ROOT_DIR}/image"

QEMU_BIN="${QEMU_BIN:-qemu-system-aarch64}"
HOST_OS="$(uname -s 2>/dev/null || true)"
QEMU_HOST_ADDR="${QEMU_HOST_ADDR:-127.0.0.1}"
TCP_PORT="${TCP_PORT:-31337}"
UDP_PORT="${UDP_PORT:-31338}"
SMOKE_PORT="${SMOKE_PORT:-31339}"
DEFAULT_QEMU_SMP_TOPO="4,cores=4,threads=1,sockets=1"
DEFAULT_QEMU_VIRT="on"
QEMU_SMP_RAW="${COHESIX_QEMU_SMP:-${QEMU_SMP:-}}"
QEMU_SMP_TOPO_RAW="${COHESIX_QEMU_SMP_TOPO:-${QEMU_SMP_TOPO:-}}"
QEMU_VIRT_RAW="${COHESIX_QEMU_VIRT:-${QEMU_VIRT:-}}"
QEMU_MACHINE_EXTRA_RAW="${COHESIX_QEMU_MACHINE_EXTRA:-${QEMU_MACHINE_EXTRA:-}}"
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

resolve_qemu_smp_arg() {
  if [[ -n "$QEMU_SMP_TOPO_RAW" ]]; then
    echo "$QEMU_SMP_TOPO_RAW"
    return
  fi
  if [[ -n "$QEMU_SMP_RAW" ]]; then
    echo "$QEMU_SMP_RAW"
    return
  fi
  echo "$DEFAULT_QEMU_SMP_TOPO"
}

resolve_qemu_virt_arg() {
  if [[ -n "$QEMU_VIRT_RAW" ]]; then
    echo "$QEMU_VIRT_RAW"
    return
  fi
  echo "$DEFAULT_QEMU_VIRT"
}

validate_qemu_smp_arg() {
  local arg="$1"

  if [[ -z "$arg" ]]; then
    echo "[qemu] Invalid QEMU SMP setting: empty value" >&2
    exit 1
  fi

  if [[ "$arg" =~ ^[0-9]+$ ]]; then
    if [[ "$arg" -lt 1 ]]; then
      echo "[qemu] Invalid QEMU_SMP (must be >= 1): $arg" >&2
      exit 1
    fi
    return
  fi

  if [[ "$arg" == *" "* ]]; then
    echo "[qemu] Invalid QEMU SMP topology (contains spaces): $arg" >&2
    exit 1
  fi

  local token
  IFS=',' read -r -a tokens <<< "$arg"
  for token in "${tokens[@]}"; do
    if [[ "$token" =~ ^[0-9]+$ ]]; then
      if [[ "$token" -lt 1 ]]; then
        echo "[qemu] Invalid QEMU SMP topology token: $token" >&2
        exit 1
      fi
      continue
    fi
    if [[ "$token" =~ ^[A-Za-z][A-Za-z0-9_-]*=[0-9]+$ ]]; then
      local value="${token#*=}"
      if [[ "$value" -lt 1 ]]; then
        echo "[qemu] Invalid QEMU SMP topology token: $token" >&2
        exit 1
      fi
      continue
    fi
    echo "[qemu] Invalid QEMU SMP topology token: $token" >&2
    exit 1
  done
}

validate_qemu_virt_arg() {
  local arg="$1"

  case "$arg" in
    on|off)
      return
      ;;
    *)
      echo "[qemu] Invalid QEMU virtualization setting (use on|off): $arg" >&2
      exit 1
      ;;
  esac
}

format_qemu_machine_arg() {
  local virt="$1"
  local machine="virt,gic-version=${GIC_VER},virtualization=${virt}"
  if [[ -n "$QEMU_MACHINE_EXTRA_RAW" ]]; then
    machine="${machine},${QEMU_MACHINE_EXTRA_RAW}"
  fi
  echo "$machine"
}

QEMU_ACCEL="$(resolve_qemu_accel)"
echo "[qemu] Using QEMU accel: ${QEMU_ACCEL}"
QEMU_SMP_ARG="$(resolve_qemu_smp_arg)"
validate_qemu_smp_arg "$QEMU_SMP_ARG"
echo "[qemu] Using QEMU SMP: ${QEMU_SMP_ARG}"
QEMU_VIRT_ARG="$(resolve_qemu_virt_arg)"
validate_qemu_virt_arg "$QEMU_VIRT_ARG"
QEMU_MACHINE_ARG="$(format_qemu_machine_arg "$QEMU_VIRT_ARG")"
echo "[qemu] Using QEMU machine: ${QEMU_MACHINE_ARG}"

"${QEMU_BIN}" \
  -accel "${QEMU_ACCEL}" \
  -machine "${QEMU_MACHINE_ARG}" \
  -cpu cortex-a57 \
  -m 1024 \
  -smp "${QEMU_SMP_ARG}" \
  -serial mon:stdio \
  -display none \
  -kernel "${ELFLOADER}" \
  -initrd "${CPIO}" \
  -device loader,file="${KERNEL}",addr=0x70000000,force-raw=on \
  -device loader,file="${ROOTSERVER}",addr=0x80000000,force-raw=on \
  -global virtio-mmio.force-legacy=off \
  -netdev "user,id=net0,hostfwd=tcp:${QEMU_HOST_ADDR}:${TCP_PORT}-:31337,hostfwd=udp:${QEMU_HOST_ADDR}:${UDP_PORT}-:31338,hostfwd=tcp:${QEMU_HOST_ADDR}:${SMOKE_PORT}-:31339" \
  -device "virtio-net-device,netdev=net0,mac=52:55:00:d1:55:01,bus=virtio-mmio-bus.0"
