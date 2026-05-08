#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Build, optionally flash, capture, and normalize Raspberry Pi 4 USB/WiFi gate proofs.
# Copyright 2026 Lukas Bower

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

IMAGE_BUILD_SCRIPT="${SCRIPT_DIR}/pi4-image-build.sh"
TRACE_NORMALIZER="${SCRIPT_DIR}/pi4_trace_normalize.py"
MANIFEST_PATH="${ROOT_DIR}/configs/root_task_pi4_uboot_aarch64.toml"
VENV_DIR="${COHESIX_PI4_VENV:-${HOME}/seL4/.venv_aarch64}"
PYTHON="${VENV_DIR}/bin/python"
FLASH_DISK=""
DISK_LABEL="COHESIX"
SERIAL_DEVICE="${COHESIX_PI4_SERIAL_DEVICE:-/dev/cu.usbserial-0001}"
LOG_PATH="${COHESIX_PI4_SERIAL_LOG:-/Users/lukasbower/pi4-serial.log}"
BOOT_WAIT_SECONDS=12
CAPTURE_SECONDS=10
COMMAND_DELAY_SECONDS=2
SKIP_BUILD=0
NO_CAPTURE=0
NORMALIZE_ONLY=0
ALLOW_SUMMARY_ONLY=0

DEFAULT_COMMANDS=(
    "help"
    "wifi diag"
    "nettest"
    "usb status"
    "usb probe-kbd"
    "usb status"
)
EXTRA_COMMANDS=()
EXPECTATIONS=()
MIN_EXPECTATIONS=()
NOT_EXPECTATIONS=()
CAPTURE_PID=""

usage() {
    cat <<'USAGE'
Usage: scripts/pi4_gate_proof.sh [options]

Builds and stages the Pi 4 payload, optionally flashes an SD card, captures the
Cohesix serial proof commands, and summarizes the current USB/WiFi gates.

Options:
  --manifest <path>          Root-task Pi 4 manifest
                             (default: configs/root_task_pi4_uboot_aarch64.toml)
  --venv <dir>               Python virtualenv for local scripts
                             (default: ~/seL4/.venv_aarch64)
  --flash-disk <device|auto> Flash SD card via scripts/pi4-image-build.sh.
                             "auto" requires exactly one external disk carrying
                             the configured --disk-label.
  --disk-label <name>        FAT32 label used when flashing or auto-detecting
                             (default: COHESIX)
  --serial-device <path>     Serial device for Cohesix console
                             (default: /dev/cu.usbserial-0001)
  --log <path>               Serial log output/input path
                             (default: /Users/lukasbower/pi4-serial.log)
  --boot-wait <seconds>      Delay before issuing console commands
                             (default: 12)
  --capture-seconds <n>      Delay after the final command before normalization
                             (default: 10)
  --command-delay <seconds>  Delay between console commands
                             (default: 2)
  --skip-build               Reuse existing seL4 image while staging/flashing
  --no-capture               Do not open serial; normalize the existing log
  --normalize-only           Skip build, flash, and capture; normalize only
  --no-default-commands      Do not send the default proof commands
  --command <line>           Append a console command to send during capture
  --expect <KEY=VALUE>       Require a gate summary value from the normalizer.
                             Examples: USB_GATE=3, WIFI_BLOCKER=ht-clock-timeout
  --expect-min <KEY=VALUE>   Require a numeric gate to be at least VALUE.
                             Example: USB_GATE=3 accepts USB_GATE=4.
  --expect-not <KEY=VALUE>   Fail if a gate summary value still equals VALUE.
                             Example: USB_BLOCKER=cmd-poll-only-timeout.
  --allow-summary-only       Do not require USB/WiFi evidence gates. This is
                             for exploratory summaries only, not proof output.
  -h, --help                 Show this help

Default proof commands:
  help
  wifi diag
  nettest
  usb status
  usb probe-kbd
  usb status
USAGE
}

log() {
    echo "[pi4-gate] $*"
}

fail() {
    echo "[pi4-gate] error: $*" >&2
    exit 1
}

require_arg() {
    local option="$1"
    local argc="$2"
    [[ "${argc}" -ge 2 ]] || fail "${option} requires a value"
}

require_file() {
    local path="$1"
    [[ -f "${path}" ]] || fail "required file missing: ${path}"
}

require_nonnegative_integer() {
    local name="$1"
    local value="$2"
    [[ "${value}" =~ ^[0-9]+$ ]] || fail "${name} must be a non-negative integer: ${value}"
}

detect_flash_disk() {
    local label="$1"
    local python_bin="$2"

    "${python_bin}" - "${label}" <<'PY'
import plistlib
import subprocess
import sys

label = sys.argv[1]
plist = plistlib.loads(subprocess.check_output(["diskutil", "list", "-plist"]))
candidates: list[str] = []
for disk in plist.get("AllDisksAndPartitions", []):
    parent = disk.get("DeviceIdentifier")
    for partition in disk.get("Partitions", []):
        if partition.get("VolumeName") != label:
            continue
        info = plistlib.loads(
            subprocess.check_output(["diskutil", "info", "-plist", f"/dev/{parent}"])
        )
        removable = (
            info.get("RemovableMediaOrExternalDevice", False)
            or info.get("Removable", False)
            or info.get("Ejectable", False)
        )
        system_image = info.get("SystemImage", False) or info.get("OSInternalMedia", False)
        if not removable or system_image:
            continue
        candidates.append(f"/dev/{parent}")

unique = sorted(set(candidates))
if len(unique) != 1:
    print(
        f"expected exactly one external disk with volume label {label!r}, got {unique}",
        file=sys.stderr,
    )
    sys.exit(2)
print(unique[0])
PY
}

run_image_build() {
    local resolved_flash_disk="${FLASH_DISK}"
    local -a args=(
        "${IMAGE_BUILD_SCRIPT}"
        "--manifest"
        "${MANIFEST_PATH}"
        "--venv"
        "${VENV_DIR}"
    )

    require_file "${IMAGE_BUILD_SCRIPT}"
    if [[ "${SKIP_BUILD}" -eq 1 ]]; then
        args+=("--skip-build")
    fi
    if [[ -n "${resolved_flash_disk}" ]]; then
        if [[ "${resolved_flash_disk}" == "auto" ]]; then
            resolved_flash_disk="$(detect_flash_disk "${DISK_LABEL}" "${PYTHON}")"
            log "auto-selected flash disk ${resolved_flash_disk}"
        fi
        args+=("--flash-disk" "${resolved_flash_disk}" "--disk-label" "${DISK_LABEL}")
    fi

    log "running image stage${resolved_flash_disk:+ and flash}"
    "${args[@]}"
}

cleanup_capture() {
    if [[ -n "${CAPTURE_PID}" ]]; then
        kill "${CAPTURE_PID}" 2>/dev/null || true
        wait "${CAPTURE_PID}" 2>/dev/null || true
        CAPTURE_PID=""
    fi
}

run_capture() {
    local -a commands=()
    local command
    local index

    for ((index = 0; index < ${#DEFAULT_COMMANDS[@]}; index++)); do
        commands+=("${DEFAULT_COMMANDS[$index]}")
    done
    for ((index = 0; index < ${#EXTRA_COMMANDS[@]}; index++)); do
        commands+=("${EXTRA_COMMANDS[$index]}")
    done

    [[ -e "${SERIAL_DEVICE}" ]] || fail "serial device missing: ${SERIAL_DEVICE}"
    : > "${LOG_PATH}"
    stty -f "${SERIAL_DEVICE}" 115200 cs8 -cstopb -parenb -ixon -ixoff -crtscts raw

    log "capturing ${SERIAL_DEVICE} to ${LOG_PATH}"
    cat "${SERIAL_DEVICE}" >> "${LOG_PATH}" &
    CAPTURE_PID="$!"
    trap cleanup_capture EXIT

    sleep "${BOOT_WAIT_SECONDS}"
    for command in "${commands[@]}"; do
        log "console command: ${command}"
        printf '\r%s\r' "${command}" > "${SERIAL_DEVICE}"
        sleep "${COMMAND_DELAY_SECONDS}"
    done
    sleep "${CAPTURE_SECONDS}"
    cleanup_capture
    trap - EXIT
}

run_normalizer() {
    local -a args=("${PYTHON}" "${TRACE_NORMALIZER}" "${LOG_PATH}" "--gate-summary")
    local index

    require_file "${TRACE_NORMALIZER}"
    require_file "${LOG_PATH}"
    if [[ "${ALLOW_SUMMARY_ONLY}" -eq 0 ]]; then
        args+=("--expect-min" "USB_GATE=1" "--expect-min" "WIFI_GATE=1")
    fi
    for ((index = 0; index < ${#EXPECTATIONS[@]}; index++)); do
        args+=("--expect" "${EXPECTATIONS[$index]}")
    done
    for ((index = 0; index < ${#MIN_EXPECTATIONS[@]}; index++)); do
        args+=("--expect-min" "${MIN_EXPECTATIONS[$index]}")
    done
    for ((index = 0; index < ${#NOT_EXPECTATIONS[@]}; index++)); do
        args+=("--expect-not" "${NOT_EXPECTATIONS[$index]}")
    done

    "${args[@]}"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --manifest)
            require_arg "$1" "$#"
            MANIFEST_PATH="$2"
            shift 2
            ;;
        --venv)
            require_arg "$1" "$#"
            VENV_DIR="$2"
            PYTHON="${VENV_DIR}/bin/python"
            shift 2
            ;;
        --flash-disk)
            require_arg "$1" "$#"
            FLASH_DISK="$2"
            shift 2
            ;;
        --disk-label)
            require_arg "$1" "$#"
            DISK_LABEL="$2"
            shift 2
            ;;
        --serial-device)
            require_arg "$1" "$#"
            SERIAL_DEVICE="$2"
            shift 2
            ;;
        --log)
            require_arg "$1" "$#"
            LOG_PATH="$2"
            shift 2
            ;;
        --boot-wait)
            require_arg "$1" "$#"
            BOOT_WAIT_SECONDS="$2"
            shift 2
            ;;
        --capture-seconds)
            require_arg "$1" "$#"
            CAPTURE_SECONDS="$2"
            shift 2
            ;;
        --command-delay)
            require_arg "$1" "$#"
            COMMAND_DELAY_SECONDS="$2"
            shift 2
            ;;
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        --no-capture)
            NO_CAPTURE=1
            shift
            ;;
        --normalize-only)
            NORMALIZE_ONLY=1
            shift
            ;;
        --no-default-commands)
            DEFAULT_COMMANDS=()
            shift
            ;;
        --command)
            require_arg "$1" "$#"
            EXTRA_COMMANDS+=("$2")
            shift 2
            ;;
        --expect)
            require_arg "$1" "$#"
            EXPECTATIONS+=("$2")
            shift 2
            ;;
        --expect-min)
            require_arg "$1" "$#"
            MIN_EXPECTATIONS+=("$2")
            shift 2
            ;;
        --expect-not)
            require_arg "$1" "$#"
            NOT_EXPECTATIONS+=("$2")
            shift 2
            ;;
        --allow-summary-only)
            ALLOW_SUMMARY_ONLY=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            fail "unknown option: $1"
            ;;
    esac
done

require_nonnegative_integer "--boot-wait" "${BOOT_WAIT_SECONDS}"
require_nonnegative_integer "--capture-seconds" "${CAPTURE_SECONDS}"
require_nonnegative_integer "--command-delay" "${COMMAND_DELAY_SECONDS}"
require_file "${PYTHON}"

if [[ "${NORMALIZE_ONLY}" -eq 0 ]]; then
    run_image_build
    if [[ "${NO_CAPTURE}" -eq 0 ]]; then
        run_capture
    fi
fi

run_normalizer
