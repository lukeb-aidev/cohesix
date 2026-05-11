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
VENV_DIR="${COHESIX_PI4_VENV:-${ROOT_DIR}/.venv}"
PYTHON="${VENV_DIR}/bin/python"
FLASH_DISK=""
DISK_LABEL="COHESIX"
SERIAL_DEVICE="${COHESIX_PI4_SERIAL_DEVICE:-/dev/cu.usbserial-0001}"
LOG_PATH="${COHESIX_PI4_SERIAL_LOG:-/Users/lukasbower/pi4-serial.log}"
BOOT_WAIT_SECONDS=12
CONSOLE_READY_TIMEOUT_SECONDS=60
CAPTURE_SECONDS=10
COMMAND_DELAY_SECONDS=2
SKIP_BUILD=0
NO_CAPTURE=0
NORMALIZE_ONLY=0
ALLOW_SUMMARY_ONLY=0
REQUIRE_USB_READY=0
REQUIRE_WIFI_READY=0

DEFAULT_COMMANDS=(
    "wifi diag"
    "nettest"
    "usb status"
    "usb probe-kbd"
    "usb diag"
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
                             (default: <repo>/.venv)
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
  --console-ready-timeout <seconds>
                             Maximum extra time to wait for the Cohesix prompt,
                             advancing the top-level U-Boot boot-options prompt
                             with its default choice when needed
                             (default: 60)
  --capture-seconds <n>      Delay after the final command before normalization
                             (default: 10)
  --command-delay <seconds>  Delay between console commands
                             (default: 2)
  --skip-build               Reuse existing seL4 image while staging/flashing
  --no-capture               Do not open serial; normalize the existing log
  --normalize-only           Skip build, flash, and capture; normalize only
  --no-default-commands      Do not send the default proof commands
  --probe-usb-keyboard       Append an extra live USB keyboard probe. The
                             default proof already probes once so command/event
                             ring gates are exercised.
  --command <line>           Append a console command to send during capture
  --expect <KEY=VALUE>       Require a gate summary value from the normalizer.
                             Examples: USB_GATE=3, WIFI_BLOCKER=ht-clock-timeout
  --expect-min <KEY=VALUE>   Require a numeric gate to be at least VALUE.
                             Example: USB_GATE=3 accepts USB_GATE=4.
  --expect-not <KEY=VALUE>   Fail if a gate summary value still equals VALUE.
                             Example: USB_BLOCKER=cmd-poll-only-timeout.
  --allow-summary-only       Do not require USB/WiFi evidence gates. This is
                             for exploratory summaries only, not proof output.
  --require-usb-ready        Require USB gate 10 with USB_BLOCKER=none.
  --require-wifi-ready       Require WiFi gate 10 with WIFI_BLOCKER=none.
  --require-ready            Require both USB and WiFi gate 10 with no blocker.
  -h, --help                 Show this help

Default proof commands:
  wifi diag
  nettest
  usb status
  usb probe-kbd
  usb diag
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

console_prompt_seen() {
    "${PYTHON}" - "${LOG_PATH}" <<'PY'
import pathlib
import sys

data = pathlib.Path(sys.argv[1]).read_bytes()
for line in data.replace(b"\r", b"\n").split(b"\n"):
    if line.startswith(b"cohesix>"):
        sys.exit(0)
sys.exit(1)
PY
}

wait_for_console_ready() {
    local deadline
    local boot_options_advanced=0

    deadline=$((SECONDS + CONSOLE_READY_TIMEOUT_SECONDS))
    while ((SECONDS <= deadline)); do
        if console_prompt_seen; then
            return
        fi
        if [[ "${boot_options_advanced}" -eq 0 ]] \
            && grep -q '\[cohesix\] Cohesix boot options' "${LOG_PATH}" \
            && grep -q 'Select option \[1\]:' "${LOG_PATH}"; then
            log "advancing Cohesix boot-options prompt with default selection"
            printf '1\r' > "${SERIAL_DEVICE}"
            boot_options_advanced=1
        fi
        sleep 1
    done
    fail "Cohesix console prompt did not appear within ${CONSOLE_READY_TIMEOUT_SECONDS}s"
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
    wait_for_console_ready
    for command in "${commands[@]}"; do
        log "console command: ${command}"
        printf '%s\r' "${command}" > "${SERIAL_DEVICE}"
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
        args+=("--expect-min" "USB_GATE=3" "--expect-min" "WIFI_GATE=1")
        args+=("--expect" "SERIAL_CLEAN=yes")
        args+=("--expect" "BOOT_HALTED=no")
        args+=("--expect" "TIMER_IRQ27_SEEN=no")
        args+=("--expect" "USB_BOOTLOADER_HANDOFF_SEEN=no")
        args+=("--expect" "USB_COLD_BOOT_SEEN=yes")
        args+=("--expect" "USB_STALE_UEFI_HINT_SEEN=no")
        args+=("--expect" "ROOT_CONSOLE_READY=yes")
        args+=("--expect" "ROOT_PROMPT_SEEN=yes")
        args+=("--expect-not" "USB_BLOCKER=unknown")
        args+=("--expect-not" "USB_BLOCKER=no-controller-edge-yet")
        args+=("--expect-not" "USB_BLOCKER=policy-skip-before-run")
        args+=("--expect-not" "USB_BLOCKER=pcie-config-replay")
        args+=("--expect-not" "USB_BLOCKER=pcie-irq-quiesce-failed")
        args+=("--expect-not" "USB_BLOCKER=pcie-irq-quiesce-missing")
        args+=("--expect-not" "USB_BLOCKER=cmd-controller-not-running")
        args+=("--expect-not" "USB_BLOCKER=cmd-controller-halted")
        args+=("--expect-not" "USB_BLOCKER=cmd-submit-proof-timer-preempted")
        args+=("--expect-not" "USB_BLOCKER=cmd-pre-doorbell-proof-timer-preempted")
        args+=("--expect-not" "USB_BLOCKER=cmd-doorbell-proof-timer-preempted")
        args+=("--expect-not" "USB_BLOCKER=pcie-window-cmd-doorbell-proof-timer-preempted")
        args+=("--expect-not" "USB_BLOCKER=raw-phys-cmd-doorbell-proof-timer-preempted")
        args+=("--expect-not" "USB_BLOCKER=cmd-poll-pending")
        args+=("--expect-not" "USB_BLOCKER=cmd-doorbell-write-halt")
        args+=("--expect-not" "USB_BLOCKER=cmd-fetch-timeout")
        args+=("--expect-not" "USB_BLOCKER=cmd-event-ring-timeout")
        args+=("--expect-not" "USB_BLOCKER=cmd-poll-only-timeout")
        args+=("--expect-not" "USB_BLOCKER=cmd-live-timeout-snapshot-missing")
        args+=("--expect-not" "USB_BLOCKER=cmd-timeout")
        args+=("--expect-not" "USB_BLOCKER=usbcmd-run-preserved-reset-bit")
        args+=("--expect-not" "USB_BLOCKER=usbcmd-run-posted-flush-halt")
        args+=("--expect-not" "USB_BLOCKER=pcie-window-no-op-timeout")
        args+=("--expect-not" "USB_BLOCKER=raw-phys-cmd-poll-only-timeout")
        args+=("--expect-not" "USB_BLOCKER=brcm-axi-setup-read")
        args+=("--expect-not" "USB_BLOCKER=enumeration-disabled-bootloader-owned")
        args+=("--expect-not" "USB_BLOCKER=reset-pre-usbcmd-source")
        args+=("--expect-not" "USB_BLOCKER=reset-pre-usbcmd-source-timer-preempted")
        args+=("--expect-not" "USB_BLOCKER=port-register-access-disabled")
        args+=("--expect-not" "USB_BLOCKER=root-port-read-begin")
        args+=("--expect-not" "USB_BLOCKER=root-port-read-timer-preempted")
        args+=("--expect-not" "USB_BLOCKER=root-port-sample-deferred")
        args+=("--expect-not" "USB_BLOCKER=no-connected-ports")
        args+=("--expect-not" "USB_BLOCKER=port-reset-timeout")
        args+=("--expect-not" "USB_BLOCKER=port-enable-timeout")
        args+=("--expect-not" "USB_BLOCKER=root-port-device-not-found")
        args+=("--expect-not" "USB_BLOCKER=address-device-timeout")
        args+=("--expect-not" "USB_BLOCKER=address-device-pending")
        args+=("--expect-not" "USB_BLOCKER=address-failed")
        args+=("--expect-not" "USB_BLOCKER=device-descriptor")
        args+=("--expect-not" "USB_BLOCKER=config-descriptor")
        args+=("--expect-not" "USB_BLOCKER=config-parse")
        args+=("--expect-not" "USB_BLOCKER=set-config")
        args+=("--expect-not" "USB_BLOCKER=invalid-config-value")
        args+=("--expect-not" "USB_BLOCKER=hid-init-failed")
        args+=("--expect-not" "USB_BLOCKER=hid-interrupt-in")
        args+=("--expect-not" "USB_BLOCKER=hid-queue-read-failed")
        args+=("--expect-not" "USB_BLOCKER=hid-first-report")
        args+=("--expect-not" "USB_BLOCKER=keyboard-first-byte")
        args+=("--expect-not" "USB_BLOCKER=no-keyboard-found")
        args+=("--expect-not" "USB_BLOCKER=unavailable")
        args+=("--expect-not" "USB_BLOCKER=safe-port-event-required")
        args+=("--expect-not" "USB_BLOCKER=safe-port-state")
        args+=("--expect-not" "WIFI_BLOCKER=ht-recover-cmd5-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=unknown")
        args+=("--expect-not" "WIFI_BLOCKER=deferred")
        args+=("--expect-not" "WIFI_BLOCKER=boot-deferred-local-seat-usb")
        args+=("--expect-not" "WIFI_BLOCKER=ht-clock-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=devon-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=function2-disabled")
        args+=("--expect-not" "WIFI_BLOCKER=ht-backplane-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=ht-backplane-cmd53-data-wait")
        args+=("--expect-not" "WIFI_BLOCKER=ht-backplane-cmd52-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=ht-backplane-cmd52-unreadable")
        args+=("--expect-not" "WIFI_BLOCKER=chipclkcsr-cmd52-pre-f2")
        args+=("--expect-not" "WIFI_BLOCKER=linux-probe-pmu-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=linux-probe-pmu-write-skip")
        args+=("--expect-not" "WIFI_BLOCKER=chipcommon-socram-remap-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=armcr4-prereset-fgc-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=armcr4-reset-assert-cmd52-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=armcr4-reset-assert-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=firmware-core-control")
        args+=("--expect-not" "WIFI_BLOCKER=pre-f2-core-control")
        args+=("--expect-not" "WIFI_BLOCKER=armcr4-release-readback-unavailable")
        args+=("--expect-not" "WIFI_BLOCKER=socram-prereset-zero-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=socram-prereset-fgc-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=socram-assert-reset-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=socram-clear-reset-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=socram-postreset-clock-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=sdio-cmd52-write")
        args+=("--expect-not" "WIFI_BLOCKER=sdio-cmd52-read")
        args+=("--expect-not" "WIFI_BLOCKER=sdio-cmd53-r5-error")
        args+=("--expect-not" "WIFI_BLOCKER=sdhci-byte-mode-count")
        args+=("--expect-not" "WIFI_BLOCKER=firmware-channel-f2")
        args+=("--expect-not" "WIFI_BLOCKER=firmware-ready-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=mailbox-ready-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=sdpcm-credit-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=ioctl-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-bdc-event")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-interrupt-programming-drift")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-interrupts-deferred")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-no-reply")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-partial-hint-visibility")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-rearm-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-reply-idle-loop")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-sideband-unreadable")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-startup-link-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=join-pending")
        args+=("--expect-not" "WIFI_BLOCKER=join-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=wifi-association-failed")
        args+=("--expect-not" "WIFI_BLOCKER=dhcp-pending")
        args+=("--expect-not" "WIFI_BLOCKER=dhcp-failed")
        args+=("--expect-not" "WIFI_BLOCKER=dhcp-invalid-packet")
        args+=("--expect-not" "WIFI_BLOCKER=net-not-ready-ipc-buffer")
        args+=("--expect-not" "WIFI_BLOCKER=nettest-policy-disabled")
        args+=("--expect-not" "WIFI_BLOCKER=nettest-selftest-disabled")
        args+=("--expect-not" "WIFI_BLOCKER=nettest-unsupported")
        args+=("--expect-not" "WIFI_BLOCKER=nettest-failed")
    fi
    if [[ "${REQUIRE_USB_READY}" -eq 1 ]]; then
        args+=("--expect-min" "USB_GATE=10" "--expect" "USB_BLOCKER=none")
    fi
    if [[ "${REQUIRE_WIFI_READY}" -eq 1 ]]; then
        args+=("--expect-min" "WIFI_GATE=10" "--expect" "WIFI_BLOCKER=none")
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
        --console-ready-timeout)
            require_arg "$1" "$#"
            CONSOLE_READY_TIMEOUT_SECONDS="$2"
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
        --probe-usb-keyboard)
            EXTRA_COMMANDS+=("usb probe-kbd")
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
        --require-usb-ready)
            REQUIRE_USB_READY=1
            shift
            ;;
        --require-wifi-ready)
            REQUIRE_WIFI_READY=1
            shift
            ;;
        --require-ready)
            REQUIRE_USB_READY=1
            REQUIRE_WIFI_READY=1
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
require_nonnegative_integer "--console-ready-timeout" "${CONSOLE_READY_TIMEOUT_SECONDS}"
require_nonnegative_integer "--capture-seconds" "${CAPTURE_SECONDS}"
require_nonnegative_integer "--command-delay" "${COMMAND_DELAY_SECONDS}"
require_file "${PYTHON}"

if [[ "${ALLOW_SUMMARY_ONLY}" -eq 1 ]] \
    && { [[ "${REQUIRE_USB_READY}" -eq 1 ]] || [[ "${REQUIRE_WIFI_READY}" -eq 1 ]]; }; then
    echo "[pi4-gate] error: --allow-summary-only cannot be combined with ready-gate requirements" >&2
    exit 2
fi

if [[ "${NORMALIZE_ONLY}" -eq 0 ]]; then
    run_image_build
    if [[ "${NO_CAPTURE}" -eq 0 ]]; then
        run_capture
    fi
fi

run_normalizer
