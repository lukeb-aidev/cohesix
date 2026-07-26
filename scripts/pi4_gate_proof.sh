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
DEFAULT_LOG_PATH="/Users/lukasbower/pi4-serial-$(date +%Y%m%d-%H%M%S).log"
LOG_PATH="${COHESIX_PI4_SERIAL_LOG:-${DEFAULT_LOG_PATH}}"
RUNTIME_DMA_PROOF_PATH="${COHESIX_PI4_RUNTIME_DMA_PROOF:-}"
BOOT_WAIT_SECONDS=12
CONSOLE_READY_TIMEOUT_SECONDS=60
CAPTURE_SECONDS=10
COMMAND_DELAY_SECONDS=2
COMMAND_CHAR_DELAY_SECONDS="${COHESIX_PI4_COMMAND_CHAR_DELAY_SECONDS:-0.06}"
COMMAND_PROMPT_TIMEOUT_SECONDS=30
SKIP_BUILD=0
NO_CAPTURE=0
NORMALIZE_ONLY=0
ALLOW_SUMMARY_ONLY=0
REQUIRE_USB_READY=0
REQUIRE_WIFI_READY=0
REQUIRE_WIRED_READY=0
REQUIRE_DRIVER_TASK_PROOF=0
REQUIRE_INPUT_RESPONSIVE=0

DEFAULT_COMMANDS=(
    "smp activity"
    "wifi diag"
    "wifi probe-ht"
    "nettest"
    "netstats"
    "usb status"
    "usb probe-kbd"
    "usb diag"
    "usb status"
    "netstats"
    "smp activity"
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
                             (default: /Users/lukasbower/pi4-serial-<timestamp>.log)
                             Active capture refuses existing paths; use
                             --normalize-only or --no-capture for existing logs.
  --runtime-dma-proof-out <path>
                             Write an env-style Pi runtime/DMA proof artifact
                             after successful normalization. Defaults to a
                             sibling file next to --log when driver-task proof
                             is required.
  --boot-wait <seconds>      Delay before issuing console commands
                             (default: 12)
  --console-ready-timeout <seconds>
                             Maximum extra time to wait for the Cohesix prompt,
                             advancing the top-level Cohesix boot menu
                             with its default choice when needed
                             (default: 60)
  --capture-seconds <n>      Delay after the final command before normalization
                             (default: 10)
  --command-delay <seconds>  Delay between console commands
                             (default: 2)
  --command-char-delay <seconds>
                             Delay between characters while sending commands
                             (default: 0.02)
  --command-prompt-timeout <seconds>
                             Maximum time to wait for the prompt after each
                             command before sending the next command
                             (default: 30)
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
  --require-usb-ready        Require USB gate 10, USB_BLOCKER=none, and the
                             linked old-good USB replay contract.
  --require-wifi-ready       Require WiFi gate 10, DHCP, nettest, authenticated
                             TCP bytes, healthy DPC, ordered Gate 7a-7e proof,
                             and the linked old-good CYW43 replay contract.
  --require-wired-ready      Require netstats to report active=wired.
  --require-driver-task-proof
                             Require driver-task substrate, capset, fault,
                             revoke, scheduling, per-driver affinity,
                             VSpace, role, latency, zero bootstrap failure,
                             and zero budget-overrun proof.
  --require-input-responsive Require serial echo, USB burst, and HDMI proof
                             breadcrumbs with zero USB burst drops.
  --require-ready            Require both USB and WiFi ready gates plus their
                             linked old-good replay contracts.
  -h, --help                 Show this help

Default proof commands:
  smp activity
  wifi diag
  wifi probe-ht
  nettest
  netstats
  usb status
  usb probe-kbd
  usb diag
  usb status
  netstats
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

ensure_capture_log_is_fresh() {
    local parent

    parent="$(dirname "${LOG_PATH}")"
    mkdir -p "${parent}"
    if [[ -e "${LOG_PATH}" ]]; then
        fail "refusing to capture to existing log without truncating: ${LOG_PATH}; pass a fresh --log path, or use --normalize-only/--no-capture for existing logs"
    fi
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

console_prompt_count() {
    "${PYTHON}" - "${LOG_PATH}" <<'PY'
import pathlib
import sys

data = pathlib.Path(sys.argv[1]).read_bytes()
count = 0
for line in data.replace(b"\r", b"\n").split(b"\n"):
    if line.startswith(b"cohesix>"):
        count += 1
print(count)
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
            && grep -q '\[cohesix\] Cohesix boot menu' "${LOG_PATH}" \
            && grep -q 'Select option \[1\]:' "${LOG_PATH}"; then
            log "advancing Cohesix boot menu with its displayed default selection"
            printf '1\r' > "${SERIAL_DEVICE}"
            boot_options_advanced=1
        fi
        sleep 1
    done
    fail "Cohesix console prompt did not appear within ${CONSOLE_READY_TIMEOUT_SECONDS}s"
}

wait_for_prompt_after_command() {
    local previous_count="$1"
    local command="$2"
    local deadline
    local current_count

    deadline=$((SECONDS + COMMAND_PROMPT_TIMEOUT_SECONDS))
    while ((SECONDS <= deadline)); do
        current_count="$(console_prompt_count)"
        if ((current_count > previous_count)); then
            return
        fi
        sleep 1
    done
    fail "Cohesix prompt did not return within ${COMMAND_PROMPT_TIMEOUT_SECONDS}s after command: ${command}"
}

send_console_line() {
    local line="$1"
    local index
    local char

    for ((index = 0; index < ${#line}; index++)); do
        char="${line:index:1}"
        printf '%s' "${char}" > "${SERIAL_DEVICE}"
        sleep "${COMMAND_CHAR_DELAY_SECONDS}"
    done
    printf '\r' > "${SERIAL_DEVICE}"
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
    ensure_capture_log_is_fresh
    : >> "${LOG_PATH}"
    stty -f "${SERIAL_DEVICE}" 115200 cs8 -cstopb -parenb -ixon -ixoff -crtscts raw

    log "capturing ${SERIAL_DEVICE} to ${LOG_PATH}"
    cat "${SERIAL_DEVICE}" >> "${LOG_PATH}" &
    CAPTURE_PID="$!"
    trap cleanup_capture EXIT

    sleep "${BOOT_WAIT_SECONDS}"
    wait_for_console_ready
    for command in "${commands[@]}"; do
        local prompt_count_before
        prompt_count_before="$(console_prompt_count)"
        log "console command: ${command}"
        send_console_line "${command}"
        wait_for_prompt_after_command "${prompt_count_before}" "${command}"
        sleep "${COMMAND_DELAY_SECONDS}"
    done
    sleep "${CAPTURE_SECONDS}"
    cleanup_capture
    trap - EXIT
}

run_normalizer() {
    local -a args=("${PYTHON}" "${TRACE_NORMALIZER}" "${LOG_PATH}" "--gate-summary")
    local index
    local output
    local status
    local require_usb_frontier=1

    require_file "${TRACE_NORMALIZER}"
    require_file "${LOG_PATH}"
    if [[ "${REQUIRE_DRIVER_TASK_PROOF}" -eq 1 && "${REQUIRE_USB_READY}" -eq 0 && "${REQUIRE_INPUT_RESPONSIVE}" -eq 0 ]]; then
        require_usb_frontier=0
    fi
    if [[ "${ALLOW_SUMMARY_ONLY}" -eq 0 ]]; then
        if [[ "${require_usb_frontier}" -eq 1 ]]; then
            args+=("--expect-min" "USB_GATE=3")
        fi
        if [[ "${REQUIRE_WIRED_READY}" -eq 1 && "${REQUIRE_WIFI_READY}" -eq 0 ]]; then
            args+=("--expect" "WIFI_BLOCKER=not-selected")
        else
            args+=("--expect-min" "WIFI_GATE=1")
        fi
        args+=("--expect" "SERIAL_CLEAN=yes")
        args+=("--expect" "BOOT_HALTED=no")
        args+=("--expect" "PANIC_SEEN=no")
        args+=("--expect" "PANIC_REASON=none")
        args+=("--expect" "TIMER_IRQ27_SEEN=no")
        args+=("--expect" "USB_BOOTLOADER_HANDOFF_SEEN=no")
        args+=("--expect" "USB_COLD_BOOT_SEEN=yes")
        args+=("--expect" "USB_STALE_UEFI_HINT_SEEN=no")
        args+=("--expect" "ROOT_CONSOLE_READY=yes")
        args+=("--expect" "ROOT_PROMPT_SEEN=yes")
        if [[ "${require_usb_frontier}" -eq 1 ]]; then
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
            args+=("--expect-not" "USB_BLOCKER=command-event-rings")
            args+=("--expect-not" "USB_BLOCKER=command-event-ring-not-proven")
            args+=("--expect-not" "USB_BLOCKER=enable-slot-completion-pending")
            args+=("--expect-not" "USB_BLOCKER=command-ring-ready")
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
            args+=("--expect-not" "USB_BLOCKER=root-port-connected")
            args+=("--expect-not" "USB_BLOCKER=no-connected-ports")
            args+=("--expect-not" "USB_BLOCKER=root-port-reset-no-reply")
            args+=("--expect-not" "USB_BLOCKER=root-port-connect-no-reply")
            args+=("--expect-not" "USB_BLOCKER=root-port-connect-timeout")
            args+=("--expect-not" "USB_BLOCKER=root-port-reset-completion-no-reply")
            args+=("--expect-not" "USB_BLOCKER=root-port-enable-no-reply")
            args+=("--expect-not" "USB_BLOCKER=root-port-reset-retry")
            args+=("--expect-not" "USB_BLOCKER=root-port-reset-failed")
            args+=("--expect-not" "USB_BLOCKER=root-port-stale-cleanup-no-reply")
            args+=("--expect-not" "USB_BLOCKER=root-port-stale-cleanup-failed")
            args+=("--expect-not" "USB_BLOCKER=port-reset-timeout")
            args+=("--expect-not" "USB_BLOCKER=port-enable-timeout")
            args+=("--expect-not" "USB_BLOCKER=root-port-reset-timeout")
            args+=("--expect-not" "USB_BLOCKER=root-port-enable-timeout")
            args+=("--expect-not" "USB_BLOCKER=root-port-device-not-found")
            args+=("--expect-not" "USB_BLOCKER=address-enable-slot-no-reply")
            args+=("--expect-not" "USB_BLOCKER=address-device-context-publish-no-reply")
            args+=("--expect-not" "USB_BLOCKER=address-device-command-submit-no-reply")
            args+=("--expect-not" "USB_BLOCKER=address-device-command-completion-no-reply")
            args+=("--expect-not" "USB_BLOCKER=address-device-publish-no-reply")
            args+=("--expect-not" "USB_BLOCKER=address-device-timeout")
            args+=("--expect-not" "USB_BLOCKER=address-device-pending")
            args+=("--expect-not" "USB_BLOCKER=address-device-failed")
            args+=("--expect-not" "USB_BLOCKER=address-failed")
            args+=("--expect-not" "USB_BLOCKER=device-addressed")
            args+=("--expect-not" "USB_BLOCKER=device-descriptor-no-reply")
            args+=("--expect-not" "USB_BLOCKER=device-descriptor")
            args+=("--expect-not" "USB_BLOCKER=config-descriptor")
            args+=("--expect-not" "USB_BLOCKER=config-parse")
            args+=("--expect-not" "USB_BLOCKER=set-config")
            args+=("--expect-not" "USB_BLOCKER=invalid-config-value")
            args+=("--expect-not" "USB_BLOCKER=hid-init-failed")
            args+=("--expect-not" "USB_BLOCKER=hid-interrupt-in")
            args+=("--expect-not" "USB_BLOCKER=hid-queue-read-failed")
            args+=("--expect-not" "USB_BLOCKER=hid-first-report")
            args+=("--expect-not" "USB_BLOCKER=hid-first-byte")
            args+=("--expect-not" "USB_BLOCKER=keyboard-first-byte")
            args+=("--expect-not" "USB_BLOCKER=no-keyboard-found")
            args+=("--expect-not" "USB_BLOCKER=keyboard-not-ready")
            args+=("--expect-not" "USB_BLOCKER=pcie-xhci-device-coverage-missing")
            args+=("--expect-not" "USB_BLOCKER=pcie-owner-ring-unavailable")
            args+=("--expect-not" "USB_BLOCKER=pcie-vl805-config-contract-missing")
            args+=("--expect-not" "USB_BLOCKER=unavailable")
            args+=("--expect-not" "USB_BLOCKER=safe-port-event-required")
            args+=("--expect-not" "USB_BLOCKER=safe-port-state")
        fi
        args+=("--expect-not" "WIFI_BLOCKER=ht-recover-cmd5-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=unknown")
        args+=("--expect-not" "WIFI_BLOCKER=deferred")
        args+=("--expect-not" "WIFI_BLOCKER=boot-deferred-local-seat-usb")
        args+=("--expect-not" "WIFI_BLOCKER=boot-deferred-root-console")
        args+=("--expect-not" "WIFI_BLOCKER=boot-waiting-for-wifi")
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
        args+=("--expect-not" "WIFI_BLOCKER=wifi-driver-task-runtime-unproved")
    fi
    if [[ "${REQUIRE_USB_READY}" -eq 1 ]]; then
        args+=("--expect-min" "USB_GATE=10" "--expect" "USB_BLOCKER=none")
        args+=("--expect" "USB_LOCAL_SEAT_STATE=ready")
        args+=("--expect" "USB_COMMAND_READY=yes")
        args+=("--expect" "USB_FIRST_REPORT_READY=yes")
        args+=("--expect" "USB_BUSY_AFTER_READY=no")
        args+=("--expect" "USB_OLDGOOD_REPLAY=yes")
        args+=("--expect" "USB_OLDGOOD_MISSING=none")
    fi
    if [[ "${REQUIRE_WIFI_READY}" -eq 1 ]]; then
        args+=("--expect" "CYW43_BOOTSTRAP_SUPERVISOR_SEEN=yes")
        args+=("--expect" "CYW43_BOOTSTRAP_SUPERVISOR_READY=yes")
        args+=("--expect" "CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS=ready")
        args+=("--expect" "CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER=none")
        args+=("--expect-min" "WIFI_GATE=10" "--expect" "WIFI_BLOCKER=none")
        args+=("--expect" "NET_ACTIVE=wifi")
        args+=("--expect" "NET_ADDR_SRC=dhcp-lease")
        args+=("--expect" "NET_DHCP=bound")
        args+=("--expect" "NET_TCP_READY=yes")
        args+=("--expect" "NETTEST_PROOF=yes")
        args+=("--expect" "COHSH_TCP_AUTH_PROOF=yes")
        args+=("--expect-min" "TCP_ACCEPTS=1")
        args+=("--expect-min" "TCP_AUTH_SESSIONS=1")
        args+=("--expect-min" "TCP_RX_BYTES=1")
        args+=("--expect" "WIFI_DPC_PROOF=yes")
        args+=("--expect" "WIFI_DPC_REASON=none")
        args+=("--expect" "WIFI_GATE7_COMPLETE=yes")
        args+=("--expect" "WIFI_GATE7_SEEN=7a>7b>7c>7d>7e")
        args+=("--expect" "WIFI_GATE7_LAST=7e")
        args+=("--expect" "WIFI_GATE7_MISSING=none")
        args+=("--expect" "WIFI_OLDGOOD_REPLAY=yes")
        args+=("--expect" "WIFI_OLDGOOD_MISSING=none")
        args+=("--expect" "WIFI_FIRMWARE_IDENTITY_PROOF=yes")
        args+=("--expect" "WIFI_FIRMWARE_IDENTITY_BLOCKER=none")
        args+=("--expect" "WIFI_CLM_READY_PROOF=yes")
        args+=("--expect" "WIFI_FIRMWARE_VERSION_PROOF=yes")
        args+=("--expect" "WIFI_CLM_VERSION_PROOF=yes")
        args+=("--expect" "SDIO_IRQ158_INBAND_PROOF=yes")
    fi
    if [[ "${REQUIRE_WIRED_READY}" -eq 1 ]]; then
        args+=("--expect" "NET_ACTIVE=wired")
    fi
    if [[ "${REQUIRE_DRIVER_TASK_PROOF}" -eq 1 ]]; then
        local require_sdio_proof=1
        if [[ "${REQUIRE_WIRED_READY}" -eq 1 && "${REQUIRE_WIFI_READY}" -eq 0 ]]; then
            require_sdio_proof=0
        fi
        args+=("--expect" "DRIVER_TASK_DEFAULT_REQUESTED=yes")
        args+=("--expect" "DRIVER_TASK_LIVE_HOT_PATHS=yes")
        args+=("--expect-min" "DRIVER_TASK_CONTRACTS=5")
        args+=("--expect-min" "DRIVER_TASK_DEDICATED=5")
        args+=("--expect" "DRIVER_TASK_COMPATIBILITY=0")
        args+=("--expect" "DRIVER_TASK_DEDICATED_READY=yes")
        args+=("--expect" "DRIVER_TASK_SERIAL_DEDICATED=yes")
        args+=("--expect" "DRIVER_TASK_USB_DEDICATED=yes")
        args+=("--expect" "DRIVER_TASK_DISPLAY_DEDICATED=yes")
        args+=("--expect" "DRIVER_TASK_NET_DEDICATED=yes")
        if [[ "${require_sdio_proof}" -eq 1 ]]; then
            args+=("--expect" "DRIVER_TASK_SDIO_DEDICATED=yes")
        fi
        args+=("--expect" "DRIVER_TASK_PCIE_DEDICATED=yes")
        args+=("--expect" "DRIVER_TASK_SUBSTRATE_READY=yes")
        args+=("--expect" "DRIVER_TASK_FAILED_COUNT=0")
        args+=("--expect" "DRIVER_TASK_CAPSET_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_FAULT_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_REVOKE_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_SCHED_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_AFFINITY_PROOF=yes")
        args+=("--expect-min" "DRIVER_TASK_AFFINITY_CONFIGURED=5")
        args+=("--expect-min" "DRIVER_TASK_AFFINITY_APPLIED=5")
        args+=("--expect" "DRIVER_TASK_AFFINITY_MANIFEST_PROOF=yes")
        args+=("--expect-min" "DRIVER_TASK_AFFINITY_MANIFEST_MATCHES=5")
        args+=("--expect" "DRIVER_TASK_AFFINITY_MANIFEST_MISSING=0")
        args+=("--expect" "DRIVER_TASK_AFFINITY_MANIFEST_MISMATCHES=0")
        args+=("--expect" "DRIVER_TASK_VSPACE_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_POINTER_FREE_IPC_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_OWNER_STATE_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_BUDGET_OVERRUNS=0")
        args+=("--expect" "TIMER_BACKEND=arch-counter")
        args+=("--expect" "TIMER_CLOCK_HZ=54000000")
        args+=("--expect" "TIMER_EL0_COUNTER=vct")
        args+=("--expect" "DUMMY_TIMER_SEEN=no")
        args+=("--expect-min" "DRIVER_TASK_LATENCY_PROOFS=5")
        args+=("--expect" "DRIVER_TASK_RING_CALL_OUTSTANDING=0")
        args+=("--expect" "DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT=0")
        args+=("--expect" "DRIVER_TASK_BOOTSTRAP_DEFERRED=0")
        if [[ "${require_sdio_proof}" -eq 1 ]]; then
            args+=("--expect-min" "DRIVER_TASK_DMA_PROOFS=6")
        else
            args+=("--expect-min" "DRIVER_TASK_DMA_PROOFS=5")
        fi
        args+=("--expect" "DRIVER_TASK_DMA_BLOCKER=none")
        args+=("--expect" "PI4_RUNTIME_DMA_PROOF=fresh-pi")
        args+=("--expect" "PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified")
    fi
    if [[ "${REQUIRE_INPUT_RESPONSIVE}" -eq 1 ]]; then
        args+=("--expect" "SERIAL_RESPONSIVE_PROOF=yes")
        args+=("--expect" "USB_POST_FIRST_BYTE_BLOCKER=none")
        args+=("--expect" "USB_BURST_PROOF=yes")
        args+=("--expect" "USB_BURST_DROPS=0")
        args+=("--expect" "HDMI_RESPONSIVE_PROOF=yes")
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

    set +e
    output="$("${args[@]}")"
    status=$?
    set -e
    printf '%s\n' "${output}"
    if [[ "${status}" -ne 0 ]]; then
        return "${status}"
    fi
    if [[ "${REQUIRE_DRIVER_TASK_PROOF}" -eq 1 ]]; then
        write_runtime_dma_proof "${output}"
    fi
}

runtime_dma_proof_path() {
    if [[ -n "${RUNTIME_DMA_PROOF_PATH}" ]]; then
        printf '%s\n' "${RUNTIME_DMA_PROOF_PATH}"
        return 0
    fi
    printf '%s.runtime-dma-proof.env\n' "${LOG_PATH%.*}"
}

write_runtime_dma_proof() {
    local summary="$1"
    local proof_path
    local build_proof="${ROOT_DIR}/out/pi4-sd/pi4-runtime-dma-proof.env"
    proof_path="$(runtime_dma_proof_path)"
    mkdir -p "$(dirname "${proof_path}")"
    {
        printf 'PI4_RUNTIME_DMA_PROOF_ARTIFACT_VERSION=1\n'
        printf 'PI4_RUNTIME_DMA_SERIAL_LOG=%s\n' "${LOG_PATH}"
        printf 'PI4_RUNTIME_DMA_MANIFEST_SOURCE=%s\n' "${MANIFEST_PATH}"
        if [[ -n "${TEST_PLAN_STATE_DIR:-}" ]]; then
            printf 'PI4_RUNTIME_DMA_TEST_PLAN_STATE_DIR=%s\n' "${TEST_PLAN_STATE_DIR}"
        fi
        if [[ -f "${build_proof}" ]]; then
            printf 'PI4_RUNTIME_DMA_STAGE_BUILD_PROOF=%s\n' "${build_proof}"
            printf 'PI4_RUNTIME_DMA_STAGE_BUILD_PROOF_SHA256=%s\n' "$(shasum -a 256 "${build_proof}" | awk '{print $1}')"
        fi
        while IFS= read -r line; do
            case "${line}" in
                PI4_RUNTIME_DMA_*|DRIVER_TASK_DMA_*|DRIVER_TASK_COUNTER_*|DRIVER_TASK_RESOURCE_*|DRIVER_TASK_RING_CALL_*|DRIVER_TASK_BOOTSTRAP_DEFERRED=*|DRIVER_TASK_ACTIVE_NET=*|DRIVER_TASK_OWNER_STATE_PROOF=*|DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_*|DRIVER_TASK_POINTER_FREE_IPC_PROOF=*|DRIVER_TASK_VSPACE_PROOF=*|TIMER_BACKEND=*|TIMER_CLOCK_HZ=*|TIMER_EL0_COUNTER=*|DUMMY_TIMER_SEEN=*)
                    printf '%s\n' "${line}"
                    ;;
            esac
        done <<<"${summary}"
    } >"${proof_path}"
    log "runtime/DMA proof artifact: ${proof_path}"
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
        --runtime-dma-proof-out)
            require_arg "$1" "$#"
            RUNTIME_DMA_PROOF_PATH="$2"
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
        --command-char-delay)
            require_arg "$1" "$#"
            COMMAND_CHAR_DELAY_SECONDS="$2"
            shift 2
            ;;
        --command-prompt-timeout)
            require_arg "$1" "$#"
            COMMAND_PROMPT_TIMEOUT_SECONDS="$2"
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
        --require-wired-ready)
            REQUIRE_WIRED_READY=1
            shift
            ;;
        --require-driver-task-proof)
            REQUIRE_DRIVER_TASK_PROOF=1
            shift
            ;;
        --require-input-responsive)
            REQUIRE_INPUT_RESPONSIVE=1
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
require_nonnegative_integer "--command-prompt-timeout" "${COMMAND_PROMPT_TIMEOUT_SECONDS}"
require_file "${PYTHON}"

if [[ "${ALLOW_SUMMARY_ONLY}" -eq 1 ]] \
    && { [[ "${REQUIRE_USB_READY}" -eq 1 ]] \
        || [[ "${REQUIRE_WIFI_READY}" -eq 1 ]] \
        || [[ "${REQUIRE_WIRED_READY}" -eq 1 ]] \
        || [[ "${REQUIRE_DRIVER_TASK_PROOF}" -eq 1 ]] \
        || [[ "${REQUIRE_INPUT_RESPONSIVE}" -eq 1 ]]; }; then
    echo "[pi4-gate] error: --allow-summary-only cannot be combined with ready-gate requirements" >&2
    exit 2
fi

if [[ "${NORMALIZE_ONLY}" -eq 0 ]]; then
    if [[ "${NO_CAPTURE}" -eq 0 ]]; then
        ensure_capture_log_is_fresh
    fi
    run_image_build
    if [[ "${NO_CAPTURE}" -eq 0 ]]; then
        run_capture
    fi
fi

run_normalizer
