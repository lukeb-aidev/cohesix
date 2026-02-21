#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Boot Cohesix UEFI artifacts in QEMU using EDK2 pflash and an ESP tree.
# Copyright 2026 Lukas Bower

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"

QEMU_BIN="${QEMU_BIN:-qemu-system-aarch64}"
ESP_DIR="${ROOT_DIR}/out/uefi/esp"
OVMF_CODE="${OVMF_CODE:-}"
OVMF_VARS_TEMPLATE="${OVMF_VARS_TEMPLATE:-}"
OUT_DIR="${ROOT_DIR}/out/uefi"
CONSOLE="serial"
MEMORY_MB=2048
SMP_TOPO="${COHESIX_QEMU_SMP_TOPO:-${QEMU_SMP_TOPO:-4,cores=4,threads=1,sockets=1}}"
MACHINE_EXTRA="${COHESIX_QEMU_MACHINE_EXTRA:-${QEMU_MACHINE_EXTRA:-}}"
ENABLE_NET=0
declare -a EXTRA_ARGS=()

usage() {
    cat <<'USAGE'
Usage: scripts/uefi/qemu-uefi.sh [options] [-- <extra-qemu-args>]

Boots QEMU using UEFI pflash and a FAT-backed ESP directory.

Options:
  --esp-dir <dir>            ESP directory (default: out/uefi/esp)
  --qemu <bin>               QEMU binary (default: qemu-system-aarch64)
  --ovmf-code <file>         OVMF/EDK2 code pflash image
  --ovmf-vars-template <file>
                             OVMF/EDK2 vars template image (copied to out/uefi/vars.fd)
  --out-dir <dir>            Output/log directory (default: out/uefi)
  --console <serial|graphical>
                             serial = nographic + stdio serial (default)
  --memory-mb <n>            Guest RAM in MiB (default: 2048)
  --smp <value>              QEMU -smp value (default: COHESIX_QEMU_SMP_TOPO or 4,cores=4,threads=1,sockets=1)
  --enable-net               Enable QEMU user-mode networking (Milestone 26a+)
  -h, --help                 Show this help

Examples:
  scripts/uefi/qemu-uefi.sh --console serial
  scripts/uefi/qemu-uefi.sh --ovmf-code /opt/homebrew/share/qemu/edk2-aarch64-code.fd
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --esp-dir)
            ESP_DIR="$2"
            shift 2
            ;;
        --qemu)
            QEMU_BIN="$2"
            shift 2
            ;;
        --ovmf-code)
            OVMF_CODE="$2"
            shift 2
            ;;
        --ovmf-vars-template)
            OVMF_VARS_TEMPLATE="$2"
            shift 2
            ;;
        --out-dir)
            OUT_DIR="$2"
            shift 2
            ;;
        --console)
            CONSOLE="$2"
            shift 2
            ;;
        --memory-mb)
            MEMORY_MB="$2"
            shift 2
            ;;
        --smp)
            SMP_TOPO="$2"
            shift 2
            ;;
        --enable-net)
            ENABLE_NET=1
            shift
            ;;
        --)
            shift
            EXTRA_ARGS+=("$@")
            break
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "[qemu-uefi] unknown argument: $1" >&2
            usage
            exit 1
            ;;
    esac
done

mkdir -p "$OUT_DIR"
LOG_FILE="${OUT_DIR}/qemu-uefi.log"
: > "$LOG_FILE"

log() {
    echo "[qemu-uefi] $*" | tee -a "$LOG_FILE"
}

fail() {
    log "error: $*"
    exit 1
}

require_file() {
    local path="$1"
    [[ -f "$path" ]] || fail "required file missing: ${path}"
}

discover_ovmf_code() {
    local candidates=(
        "/opt/homebrew/share/qemu/edk2-aarch64-code.fd"
        "/usr/local/share/qemu/edk2-aarch64-code.fd"
        "/usr/share/AAVMF/AAVMF_CODE.fd"
        "/usr/share/qemu-efi-aarch64/QEMU_EFI.fd"
        "/usr/share/edk2/aarch64/QEMU_EFI.fd"
    )
    local path
    for path in "${candidates[@]}"; do
        if [[ -f "$path" ]]; then
            echo "$path"
            return 0
        fi
    done
    return 1
}

discover_ovmf_vars_template() {
    local candidates=(
        "/opt/homebrew/share/qemu/edk2-arm-vars.fd"
        "/usr/local/share/qemu/edk2-arm-vars.fd"
        "/usr/share/AAVMF/AAVMF_VARS.fd"
        "/usr/share/qemu-efi-aarch64/vars-template-pflash.raw"
    )
    local path
    for path in "${candidates[@]}"; do
        if [[ -f "$path" ]]; then
            echo "$path"
            return 0
        fi
    done
    return 1
}

if [[ -z "$OVMF_CODE" ]]; then
    if ! OVMF_CODE="$(discover_ovmf_code)"; then
        fail "no OVMF code image found; pass --ovmf-code explicitly"
    fi
fi

if [[ -z "$OVMF_VARS_TEMPLATE" ]]; then
    OVMF_VARS_TEMPLATE="$(discover_ovmf_vars_template || true)"
fi

[[ -d "$ESP_DIR" ]] || fail "esp directory missing: ${ESP_DIR} (run scripts/uefi/esp-build.sh first)"
require_file "$OVMF_CODE"
command -v "$QEMU_BIN" >/dev/null 2>&1 || fail "qemu binary not found: ${QEMU_BIN}"

VARS_WORKING="${OUT_DIR}/vars.fd"
if [[ -n "$OVMF_VARS_TEMPLATE" ]]; then
    require_file "$OVMF_VARS_TEMPLATE"
    cp -f "$OVMF_VARS_TEMPLATE" "$VARS_WORKING"
else
    # Fallback when a template vars image is unavailable.
    dd if=/dev/zero of="$VARS_WORKING" bs=1m count=64 status=none
fi

if [[ "$CONSOLE" != "serial" && "$CONSOLE" != "graphical" ]]; then
    fail "--console must be 'serial' or 'graphical'"
fi

declare -a qemu_args=(
    -machine "virt,gic-version=3${MACHINE_EXTRA:+,${MACHINE_EXTRA}}"
    -cpu cortex-a72
    -m "$MEMORY_MB"
    -smp "$SMP_TOPO"
    -drive "if=pflash,format=raw,readonly=on,file=${OVMF_CODE}"
    -drive "if=pflash,format=raw,file=${VARS_WORKING}"
    -drive "if=none,file=fat:rw:${ESP_DIR},format=raw,id=esp"
    -device "virtio-blk-device,drive=esp"
)

if [[ "$ENABLE_NET" -eq 1 ]]; then
    qemu_args+=(-netdev user,id=net0 -device virtio-net-device,netdev=net0)
else
    qemu_args+=(-net none)
fi

if [[ "$CONSOLE" == "serial" ]]; then
    qemu_args+=(-nographic -serial mon:stdio)
fi

if [[ ${#EXTRA_ARGS[@]} -gt 0 ]]; then
    qemu_args+=("${EXTRA_ARGS[@]}")
fi

log "qemu=${QEMU_BIN}"
log "esp=${ESP_DIR}"
log "ovmf_code=${OVMF_CODE}"
log "ovmf_vars=${VARS_WORKING}"
log "console=${CONSOLE} net_enabled=${ENABLE_NET}"
log "launching UEFI boot"

"$QEMU_BIN" "${qemu_args[@]}" | tee -a "$LOG_FILE"
