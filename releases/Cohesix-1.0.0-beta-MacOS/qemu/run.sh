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
RELEASE_PROFILE="$(
  python3 - "$ROOT_DIR/BUILD_PROVENANCE.json" <<'PY_PROFILE'
import json
import sys
record = json.load(open(sys.argv[1], encoding="utf-8"))
profiles = {"macos": ("qemu_smp_production", 24000000),
            "linux": ("qemu_smp_kvm_production", 31250000)}
host = record["host"]
if (record["sel4_profile"], record["timer_clock_hz"]) != profiles[host]:
    raise SystemExit("release profile/timer mismatch")
print({"macos": "Darwin", "linux": "Linux"}[host], record["timer_clock_hz"])
PY_PROFILE
)"
read -r RELEASE_HOST RELEASE_TIMER_CLOCK_HZ <<< "$RELEASE_PROFILE"
[[ "$HOST_OS" == "$RELEASE_HOST" ]] || {
  echo "[qemu] bundle requires $RELEASE_HOST; use the native host bundle" >&2
  exit 1
}
QEMU_HOST_ADDR="${QEMU_HOST_ADDR:-127.0.0.1}"
TCP_PORT="${TCP_PORT:-31337}"
UDP_PORT="${UDP_PORT:-31338}"
SMOKE_PORT="${SMOKE_PORT:-31339}"
DEFAULT_QEMU_SMP_TOPO="4,cores=4,threads=1,sockets=1"
DEFAULT_QEMU_VIRT="off"
DEFAULT_QEMU_ACCEL=""
DEFAULT_QEMU_MACHINE_EXTRA=""
if [[ "$HOST_OS" == "Darwin" ]]; then
  DEFAULT_QEMU_ACCEL="hvf"
  DEFAULT_QEMU_VIRT="off"
  DEFAULT_QEMU_MACHINE_EXTRA="kernel-irqchip=off"
fi
QEMU_SMP_RAW="${COHESIX_QEMU_SMP:-${QEMU_SMP:-}}"
QEMU_SMP_TOPO_RAW="${COHESIX_QEMU_SMP_TOPO:-${QEMU_SMP_TOPO:-}}"
QEMU_VIRT_RAW="${COHESIX_QEMU_VIRT:-${QEMU_VIRT:-}}"
QEMU_MACHINE_EXTRA_RAW="${COHESIX_QEMU_MACHINE_EXTRA:-${QEMU_MACHINE_EXTRA:-}}"
if [[ -z "$QEMU_VIRT_RAW" ]]; then
  QEMU_VIRT_RAW="$DEFAULT_QEMU_VIRT"
fi
if [[ -z "$QEMU_MACHINE_EXTRA_RAW" && -n "$DEFAULT_QEMU_MACHINE_EXTRA" ]]; then
  QEMU_MACHINE_EXTRA_RAW="$DEFAULT_QEMU_MACHINE_EXTRA"
fi
if [[ -z "${COHESIX_QEMU_ACCEL:-}" && -z "${QEMU_ACCEL:-}" && -n "$DEFAULT_QEMU_ACCEL" ]]; then
  QEMU_ACCEL="$DEFAULT_QEMU_ACCEL"
fi
GIC_VER_FILE="${IMAGE_DIR}/gic-version.txt"
if [[ ! -f "${GIC_VER_FILE}" ]]; then
  echo "[qemu] missing compiler-selected GIC version: ${GIC_VER_FILE}" >&2
  exit 1
fi
GIC_VER="$(tr -d '\n' < "${GIC_VER_FILE}")"
if [[ "$GIC_VER" != "3" ]]; then
  echo "[qemu] release requires GICv3; selected GIC${GIC_VER}" >&2
  exit 1
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
    if [[ "$HOST_OS" == "Darwin" && "$accel" == "hvf" ]]; then
      echo "[qemu] canonical Darwin QEMU requires HVF, but ${QEMU_BIN} does not advertise it; set COHESIX_QEMU_ACCEL=tcg only for a claim-ineligible diagnostic run" >&2
      exit 1
    fi
    echo "[qemu] Requested QEMU accelerator '$accel' not supported by ${QEMU_BIN}; falling back to tcg" >&2
    accel="tcg"
  fi
  echo "$accel"
}

resolve_qemu_cpu_arg() {
  local accel="$1"
  local cpu_model="cortex-a57"
  if [[ "$HOST_OS" == "Linux" && "$accel" == "kvm" ]]; then
    cpu_model="host"
  fi
  if [[ "$accel" == "tcg" ]]; then
    cpu_model="${cpu_model},cntfrq=${RELEASE_TIMER_CLOCK_HZ}"
  fi
  echo "$cpu_model"
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

  if [[ "$arg" != "off" ]]; then
    echo "[qemu] selected release profile requires virtualization=off; got $arg" >&2
    exit 1
  fi
}

format_qemu_machine_arg() {
  local virt="$1"
  local machine="virt,gic-version=${GIC_VER},virtualization=${virt}"
  if [[ "$QEMU_MACHINE_EXTRA_RAW" == *"gic-version"* \
      || "$QEMU_MACHINE_EXTRA_RAW" == *"virtualization"* \
      || "$QEMU_MACHINE_EXTRA_RAW" == *"virt,"* \
      || "$QEMU_MACHINE_EXTRA_RAW" == *"machine="* \
      || "$QEMU_MACHINE_EXTRA_RAW" == *"type="* ]]; then
    echo "[qemu] machine extras must not override the profile-owned machine" >&2
    exit 1
  fi
  if [[ -n "$QEMU_MACHINE_EXTRA_RAW" ]]; then
    machine="${machine},${QEMU_MACHINE_EXTRA_RAW}"
  fi
  echo "$machine"
}

QEMU_ACCEL="$(resolve_qemu_accel)"
echo "[qemu] Using QEMU accel: ${QEMU_ACCEL}"
if [[ "$QEMU_ACCEL" == "tcg" ]]; then
  echo "[qemu] TCG is an explicit diagnostic envelope; this run is claim-ineligible"
elif [[ "$HOST_OS" == "Darwin" && "$QEMU_ACCEL" != "hvf" ]]; then
  echo "[qemu] non-HVF Darwin acceleration is outside the release envelope; this run is claim-ineligible"
fi
QEMU_SMP_ARG="$(resolve_qemu_smp_arg)"
validate_qemu_smp_arg "$QEMU_SMP_ARG"
echo "[qemu] Using QEMU SMP: ${QEMU_SMP_ARG}"
QEMU_VIRT_ARG="$(resolve_qemu_virt_arg)"
validate_qemu_virt_arg "$QEMU_VIRT_ARG"
QEMU_MACHINE_ARG="$(format_qemu_machine_arg "$QEMU_VIRT_ARG")"
echo "[qemu] Using QEMU machine: ${QEMU_MACHINE_ARG}"
QEMU_CPU_ARG="$(resolve_qemu_cpu_arg "$QEMU_ACCEL")"
echo "[qemu] Using QEMU CPU: ${QEMU_CPU_ARG}"

"${QEMU_BIN}" \
  -accel "${QEMU_ACCEL}" \
  -machine "${QEMU_MACHINE_ARG}" \
  -cpu "${QEMU_CPU_ARG}" \
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
