#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Capture Raspberry Pi 4 Linux USB, WiFi, SDIO, PCIe, and interrupt diagnostics for Cohesix driver bring-up.
# Copyright 2026 Lukas Bower

set -uo pipefail

SCRIPT_NAME="$(basename "$0")"
DEFAULT_DURATION_SECONDS=90
DEFAULT_COMMAND_TIMEOUT_SECONDS=30
LONG_COMMAND_TIMEOUT_SECONDS=75
BACKGROUND_GRACE_SECONDS=30
TRACE_PIPE_MAX_BYTES=$((64 * 1024 * 1024))
USBMON_MAX_BYTES=$((16 * 1024 * 1024))
SERVICE_NAME="cohesix-pi4-capture.service"
BOOT_SCRIPT_NAME="cohesix-pi4-linux-capture.sh"
MARKER_TOKEN="cohesix_capture=1"

duration_seconds="${DEFAULT_DURATION_SECONDS}"
install_next_boot=0
capture_once=1
service_capture_once=0
reload_wifi=0

usage() {
    cat <<'USAGE'
Usage: sudo bash cohesix-pi4-linux-capture.sh [options]

Captures Raspberry Pi 4 Linux evidence needed to map known-good USB/xHCI and
WiFi/SDIO/brcmfmac behavior back to Cohesix.

Options:
  --duration <seconds>       Live trace duration (default: 90).
  --install-next-boot        Install boot cmdline debug and a one-shot systemd
                             capture service, then exit. Reboot afterwards.
  --capture-once             Capture immediately (default).
  --service-capture-once     Capture immediately, then disable the systemd
                             service. Used by --install-next-boot.
  --reload-wifi              Unload/reload brcmfmac/brcmutil/cfg80211 during
                             capture. Do not use over a WiFi SSH session.
  -h, --help                 Show this help.

Output is written under /boot/firmware/cohesix-traces when available, otherwise
/boot/cohesix-traces or /var/log/cohesix-traces.
USAGE
}

log() {
    printf '[cohesix-capture] %s\n' "$*"
}

warn() {
    printf '[cohesix-capture] warning: %s\n' "$*" >&2
}

die() {
    printf '[cohesix-capture] error: %s\n' "$*" >&2
    exit 1
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --duration)
                [[ $# -ge 2 ]] || die "--duration requires a value"
                duration_seconds="$2"
                shift 2
                ;;
            --install-next-boot)
                install_next_boot=1
                capture_once=0
                shift
                ;;
            --capture-once)
                capture_once=1
                shift
                ;;
            --service-capture-once)
                service_capture_once=1
                capture_once=1
                shift
                ;;
            --reload-wifi)
                reload_wifi=1
                shift
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                die "unknown argument: $1"
                ;;
        esac
    done

    case "${duration_seconds}" in
        ''|*[!0-9]*)
            die "--duration must be an integer number of seconds"
            ;;
    esac
}

require_root() {
    if [[ "$(id -u)" -ne 0 ]]; then
        die "run as root: sudo bash ${SCRIPT_NAME}"
    fi
}

find_boot_dir() {
    local candidate
    for candidate in /boot/firmware /boot; do
        if [[ -d "${candidate}" ]]; then
            printf '%s\n' "${candidate}"
            return 0
        fi
    done
    printf '/var/log\n'
}

find_cmdline_path() {
    local candidate
    for candidate in /boot/firmware/cmdline.txt /boot/cmdline.txt; do
        if [[ -f "${candidate}" ]]; then
            printf '%s\n' "${candidate}"
            return 0
        fi
    done
    return 1
}

find_config_path() {
    local candidate
    for candidate in /boot/firmware/config.txt /boot/config.txt; do
        if [[ -f "${candidate}" ]]; then
            printf '%s\n' "${candidate}"
            return 0
        fi
    done
    return 1
}

trace_root_dir() {
    local boot_dir
    boot_dir="$(find_boot_dir)"
    if [[ -d "${boot_dir}" && -w "${boot_dir}" ]]; then
        printf '%s\n' "${boot_dir}/cohesix-traces"
    else
        printf '%s\n' "/var/log/cohesix-traces"
    fi
}

shell_quote() {
    local value="$1"
    printf "'%s'" "${value//\'/\'\\\'\'}"
}

sanitize_cmdline_debug() {
    local cmdline_path="$1"
    local original

    original="$(tr '\n' ' ' < "${cmdline_path}" | sed 's/[[:space:]]*$//')"
    printf '%s\n' "${original}" | sed -E \
        -e 's/(^|[[:space:]])ignore_loglevel([[:space:]]|$)/ /g' \
        -e 's/(^|[[:space:]])initcall_debug([[:space:]]|$)/ /g' \
        -e 's/(^|[[:space:]])loglevel=[0-9]+([[:space:]]|$)/ /g' \
        -e 's/[[:space:]]+/ /g' \
        -e 's/^ //' \
        -e 's/ $//'
}

append_cmdline_debug() {
    local cmdline_path="$1"
    local timestamp backup
    timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
    backup="${cmdline_path}.cohesix-bak.${timestamp}"

    cp "${cmdline_path}" "${backup}" || die "failed to back up ${cmdline_path}"

    local dyndbg_query
    dyndbg_query='file drivers/net/wireless/broadcom/brcm80211/brcmfmac/* +p; file drivers/net/wireless/broadcom/brcm80211/brcmutil/* +p; file drivers/mmc/core/* +p; file drivers/mmc/host/sdhci* +p; file drivers/usb/core/* +p; file drivers/usb/host/xhci* +p; file drivers/pci/* +p'

    local original
    original="$(sanitize_cmdline_debug "${cmdline_path}")"
    if ! printf '%s\n' "${original}" | grep -q 'console=serial0,115200'; then
        original="${original} console=serial0,115200"
    fi

    if grep -q "${MARKER_TOKEN}" "${cmdline_path}"; then
        printf '%s loglevel=3\n' "${original}" > "${cmdline_path}" || \
            die "failed to update ${cmdline_path}"
        log "Sanitized existing Cohesix kernel cmdline debug; backup saved as ${backup}"
    else
        printf '%s %s loglevel=3 brcmfmac.debug=0x001fffff usbcore.autosuspend=-1 dyndbg="%s"\n' \
            "${original}" "${MARKER_TOKEN}" "${dyndbg_query}" > "${cmdline_path}" || \
            die "failed to update ${cmdline_path}"
        log "Updated ${cmdline_path}; backup saved as ${backup}"
    fi
}

ensure_config_line() {
    local config_path="$1"
    local line="$2"
    local key="${line%%=*}"

    if grep -Eq "^[[:space:]]*${key}=" "${config_path}"; then
        sed -i "s|^[[:space:]]*${key}=.*|${line}|" "${config_path}"
    else
        printf '\n%s\n' "${line}" >> "${config_path}"
    fi
}

enable_serial_logging() {
    local config_path
    config_path="$(find_config_path)" || {
        warn "could not find Raspberry Pi config.txt for serial logging"
        return 0
    }

    ensure_config_line "${config_path}" "enable_uart=1"
    ensure_config_line "${config_path}" "uart_2ndstage=1"
    log "Enabled Pi firmware/kernel serial logging in ${config_path}"
}

run_with_timeout() {
    local timeout_seconds="$1"
    shift

    if command -v timeout >/dev/null 2>&1; then
        timeout --kill-after=5s "${timeout_seconds}" "$@"
    else
        "$@"
    fi
}

install_capture_service() {
    local boot_dir script_dst service_path quoted_script
    boot_dir="$(find_boot_dir)"
    [[ -d "${boot_dir}" ]] || die "boot directory not found"

    script_dst="${boot_dir}/${BOOT_SCRIPT_NAME}"
    if [[ "$(readlink -f "$0" 2>/dev/null || realpath "$0" 2>/dev/null || printf '%s\n' "$0")" != "$(readlink -f "${script_dst}" 2>/dev/null || realpath "${script_dst}" 2>/dev/null || printf '%s\n' "${script_dst}")" ]]; then
        cp "$0" "${script_dst}" || die "failed to copy script to ${script_dst}"
    fi
    chmod 0755 "${script_dst}" 2>/dev/null || true

    service_path="/etc/systemd/system/${SERVICE_NAME}"
    quoted_script="$(shell_quote "${script_dst}")"
    cat > "${service_path}" <<SERVICE
[Unit]
Description=Cohesix Pi 4 USB and WiFi capture
After=local-fs.target systemd-journald.service
Wants=systemd-journald.service

[Service]
Type=oneshot
ExecStart=/bin/bash ${quoted_script} --service-capture-once --duration ${duration_seconds}
TimeoutStartSec=$((duration_seconds + 180))

[Install]
WantedBy=multi-user.target
SERVICE

    systemctl daemon-reload || die "systemctl daemon-reload failed"
    systemctl enable "${SERVICE_NAME}" || die "systemctl enable ${SERVICE_NAME} failed"
    log "Installed and enabled ${SERVICE_NAME}"
}

install_next_boot_capture() {
    local cmdline_path
    require_root
    cmdline_path="$(find_cmdline_path)" || die "could not find Raspberry Pi cmdline.txt"
    append_cmdline_debug "${cmdline_path}"
    enable_serial_logging
    install_capture_service
    log "Next-boot capture is installed. Reboot the Pi, then retrieve /boot/firmware/cohesix-traces."
}

capture_cmd_timeout() {
    local timeout_seconds="$1"
    local out_dir="$2"
    local name="$3"
    shift 3

    {
        printf '$'
        printf ' %q' "$@"
        printf '\n\n'
        run_with_timeout "${timeout_seconds}" "$@"
    } > "${out_dir}/${name}.txt" 2>&1 || true
}

capture_cmd() {
    capture_cmd_timeout "${DEFAULT_COMMAND_TIMEOUT_SECONDS}" "$@"
}

capture_cmd_long() {
    capture_cmd_timeout "${LONG_COMMAND_TIMEOUT_SECONDS}" "$@"
}

capture_shell_timeout() {
    local timeout_seconds="$1"
    local out_dir="$2"
    local name="$3"
    local command="$4"

    {
        printf '$ %s\n\n' "${command}"
        run_with_timeout "${timeout_seconds}" /bin/bash -c "${command}"
    } > "${out_dir}/${name}.txt" 2>&1 || true
}

capture_shell() {
    capture_shell_timeout "${DEFAULT_COMMAND_TIMEOUT_SECONDS}" "$@"
}

capture_shell_long() {
    capture_shell_timeout "${LONG_COMMAND_TIMEOUT_SECONDS}" "$@"
}

mount_debugfs() {
    mkdir -p /sys/kernel/debug 2>/dev/null || true
    if ! mountpoint -q /sys/kernel/debug 2>/dev/null; then
        mount -t debugfs none /sys/kernel/debug 2>/dev/null || true
    fi
}

find_tracefs() {
    local candidate
    for candidate in /sys/kernel/tracing /sys/kernel/debug/tracing; do
        if [[ -d "${candidate}" ]]; then
            printf '%s\n' "${candidate}"
            return 0
        fi
    done
    return 1
}

write_dynamic_debug() {
    local control="/sys/kernel/debug/dynamic_debug/control"
    [[ -w "${control}" ]] || {
        warn "dynamic_debug control is not writable"
        return 0
    }

    local query
    for query in \
        'file drivers/net/wireless/broadcom/brcm80211/brcmfmac/* +p' \
        'file drivers/net/wireless/broadcom/brcm80211/brcmutil/* +p' \
        'file drivers/mmc/core/* +p' \
        'file drivers/mmc/host/sdhci* +p' \
        'file drivers/usb/core/* +p' \
        'file drivers/usb/host/xhci* +p' \
        'file drivers/pci/* +p'
    do
        printf '%s\n' "${query}" > "${control}" 2>/dev/null || true
    done
}

enable_trace_events() {
    local trace_dir="$1"
    [[ -d "${trace_dir}/events" ]] || return 0

    printf '0\n' > "${trace_dir}/tracing_on" 2>/dev/null || true
    printf '\n' > "${trace_dir}/trace" 2>/dev/null || true

    local enable
    while IFS= read -r enable; do
        printf '1\n' > "${enable}" 2>/dev/null || true
    done < <(
        find "${trace_dir}/events" -path '*/enable' -type f 2>/dev/null | \
            grep -E '/(xhci-hcd|usb|mmc|sdio|pci)/[^/]+/enable$' || true
    )

    printf '1\n' > "${trace_dir}/tracing_on" 2>/dev/null || true
}

disable_trace_events() {
    local trace_dir="$1"
    [[ -n "${trace_dir}" && -d "${trace_dir}" ]] || return 0
    printf '0\n' > "${trace_dir}/tracing_on" 2>/dev/null || true
}

start_background_capture() {
    local pid_file="$1"
    local output="$2"
    shift 2

    if command -v timeout >/dev/null 2>&1; then
        timeout --kill-after=5s "$((duration_seconds + BACKGROUND_GRACE_SECONDS))" "$@" > "${output}" 2>&1 &
    else
        "$@" > "${output}" 2>&1 &
    fi
    printf '%s\n' "$!" >> "${pid_file}"
}

start_background_shell_capture() {
    local pid_file="$1"
    local output="$2"
    local command="$3"

    /bin/bash -c "${command}" > "${output}" 2>&1 &
    printf '%s\n' "$!" >> "${pid_file}"
}

stop_background_captures() {
    local pid_file="$1"
    [[ -f "${pid_file}" ]] || return 0

    local pid
    while IFS= read -r pid; do
        [[ -n "${pid}" ]] || continue
        kill "${pid}" 2>/dev/null || true
    done < "${pid_file}"
    sleep 1
    while IFS= read -r pid; do
        [[ -n "${pid}" ]] || continue
        kill -9 "${pid}" 2>/dev/null || true
    done < "${pid_file}"
}

maybe_reload_wifi() {
    local out_dir="$1"
    [[ "${reload_wifi}" -eq 1 ]] || return 0

    {
        echo "Reloading WiFi modules. This can disconnect SSH over wlan0."
        ip link show wlan0 2>/dev/null || true
        modprobe -r brcmfmac brcmutil cfg80211 2>&1 || true
        sleep 2
        modprobe brcmfmac debug=0x001fffff 2>&1 || true
        sleep 8
        ip link show wlan0 2>/dev/null || true
    } > "${out_dir}/wifi-reload.txt" 2>&1 || true
}

copy_file_if_present() {
    local src="$1"
    local dst="$2"
    if [[ -e "${src}" ]]; then
        cp -R "${src}" "${dst}" 2>/dev/null || true
    fi
}

capture_sysfs_state() {
    local out_dir="$1"
    mkdir -p "${out_dir}/sysfs" "${out_dir}/proc" "${out_dir}/debug" "${out_dir}/firmware" 2>/dev/null || true

    copy_file_if_present /sys/firmware/fdt "${out_dir}/firmware/fdt.dtb"

    capture_shell "${out_dir}" "sysfs-pci-summary" \
        'for d in /sys/bus/pci/devices/*; do [ -d "$d" ] || continue; echo "### $d"; for f in vendor device class subsystem_vendor subsystem_device irq resource driver_override enable broken_parity_status local_cpus numa_node; do [ -e "$d/$f" ] && printf "%s=" "$f" && cat "$d/$f"; done; [ -e "$d/config" ] && od -Ax -tx1 -v "$d/config"; done'
    capture_shell "${out_dir}" "sysfs-usb-summary" \
        'for d in /sys/bus/usb/devices/*; do [ -d "$d" ] || continue; echo "### $d"; for f in busnum devnum devpath speed version idVendor idProduct manufacturer product serial bDeviceClass bDeviceSubClass bDeviceProtocol maxchild authorized avoid_reset_quirk quirks urbnum; do [ -e "$d/$f" ] && printf "%s=" "$f" && cat "$d/$f"; done; done'
    capture_shell "${out_dir}" "sysfs-mmc-summary" \
        'for d in /sys/bus/mmc/devices/* /sys/class/mmc_host/*; do [ -d "$d" ] || continue; echo "### $d"; find "$d" -maxdepth 2 -type f 2>/dev/null | sort | while read -r f; do case "$f" in */uevent|*/cid|*/csd|*/scr|*/ocr|*/rca|*/type|*/name|*/serial|*/manfid|*/oemid|*/hwrev|*/fwrev|*/date|*/clock|*/actual_clock|*/ios|*/power/*) echo "--- $f"; cat "$f" 2>/dev/null || true;; esac; done; done'
    capture_shell "${out_dir}" "device-tree-summary" \
        'find /proc/device-tree /sys/firmware/devicetree/base -maxdepth 4 \( -type f -o -type l \) 2>/dev/null | sort | while read -r f; do echo "--- $f"; tr "\000" "\n" < "$f" 2>/dev/null | sed -n "1,24p" || true; done'
}

capture_all() {
    require_root

    local root out_dir pid_file trace_dir boot_dir started_at
    root="$(trace_root_dir)"
    started_at="$(date -u +%Y%m%dT%H%M%SZ)"
    out_dir="${root}/linux-${started_at}"
    pid_file="${out_dir}/background-pids.txt"
    boot_dir="$(find_boot_dir)"

    mkdir -p "${out_dir}" || die "failed to create ${out_dir}"
    log "Writing capture to ${out_dir}"

    {
        echo "started_at_utc=${started_at}"
        echo "duration_seconds=${duration_seconds}"
        echo "script=${SCRIPT_NAME}"
        echo "boot_dir=${boot_dir}"
        echo "reload_wifi=${reload_wifi}"
        echo "service_capture_once=${service_capture_once}"
    } > "${out_dir}/capture-meta.txt"

    mount_debugfs
    dmesg -n 3 2>/dev/null || true
    write_dynamic_debug
    trace_dir="$(find_tracefs || true)"
    if [[ -n "${trace_dir}" ]]; then
        enable_trace_events "${trace_dir}"
        capture_shell "${out_dir}" "trace-events-enabled" \
            "find '${trace_dir}/events' -path '*/enable' -type f -exec sh -c 'for f; do v=\$(cat \"\$f\" 2>/dev/null || true); [ \"\$v\" = 1 ] && echo \"\$f\"; done' sh {} + | sort"
    else
        warn "tracefs not found"
    fi

    capture_cmd "${out_dir}" "uname" uname -a
    capture_cmd "${out_dir}" "os-release" cat /etc/os-release
    capture_cmd "${out_dir}" "proc-cmdline" cat /proc/cmdline
    capture_cmd "${out_dir}" "proc-interrupts-before" cat /proc/interrupts
    capture_cmd "${out_dir}" "proc-iomem" cat /proc/iomem
    capture_cmd "${out_dir}" "proc-modules" cat /proc/modules
    capture_cmd "${out_dir}" "lsmod" lsmod
    capture_cmd "${out_dir}" "ip-link" ip -d link show
    capture_cmd "${out_dir}" "ip-addr" ip addr show
    capture_cmd "${out_dir}" "rfkill" rfkill list
    capture_cmd "${out_dir}" "iw-dev" iw dev
    capture_cmd "${out_dir}" "iw-phy" iw phy
    capture_cmd_long "${out_dir}" "lspci-nnvvxxx" lspci -nnvvxxx
    capture_cmd "${out_dir}" "lsusb-tv" lsusb -tv
    capture_cmd_long "${out_dir}" "lsusb-v" lsusb -v
    capture_cmd "${out_dir}" "usb-devices" usb-devices
    capture_cmd "${out_dir}" "debug-usb-devices" cat /sys/kernel/debug/usb/devices
    capture_cmd "${out_dir}" "modinfo-brcmfmac" modinfo brcmfmac
    capture_cmd "${out_dir}" "modinfo-xhci-hcd" modinfo xhci_hcd
    capture_cmd "${out_dir}" "modinfo-sdhci" modinfo sdhci
    capture_cmd_long "${out_dir}" "dmesg-before" dmesg -T
    capture_cmd_long "${out_dir}" "journal-kernel-before" journalctl -k -b -n 5000 -o short-monotonic --no-pager
    capture_cmd_long "${out_dir}" "journal-wifi-usb-units" journalctl -b --no-pager -n 2000 -o short-monotonic -u "${SERVICE_NAME}" -u systemd-modules-load -u NetworkManager -u wpa_supplicant -u dhcpcd -u ssh
    capture_cmd "${out_dir}" "vcgencmd-version" vcgencmd version
    capture_cmd "${out_dir}" "vcgencmd-config-int" vcgencmd get_config int
    capture_cmd "${out_dir}" "vcgencmd-throttled" vcgencmd get_throttled
    capture_shell "${out_dir}" "firmware-brcm-files" \
        'find /lib/firmware/brcm -maxdepth 1 -type f 2>/dev/null | sort | while read -r f; do echo "### $f"; ls -l "$f"; sha256sum "$f" 2>/dev/null || shasum -a 256 "$f" 2>/dev/null || true; case "$f" in *.txt) sed -n "1,220p" "$f";; esac; done'

    capture_sysfs_state "${out_dir}"

    start_background_capture "${pid_file}" "${out_dir}/dmesg-follow.txt" dmesg -wT
    if [[ -n "${trace_dir}" && -r "${trace_dir}/trace_pipe" ]]; then
        start_background_shell_capture "${pid_file}" "${out_dir}/trace-pipe.txt" \
            "timeout --kill-after=5s $((duration_seconds + BACKGROUND_GRACE_SECONDS)) cat $(shell_quote "${trace_dir}/trace_pipe") | head -c ${TRACE_PIPE_MAX_BYTES}"
    fi
    if [[ -r /sys/kernel/debug/usb/usbmon/0u ]]; then
        start_background_shell_capture "${pid_file}" "${out_dir}/usbmon-0u.txt" \
            "timeout --kill-after=5s $((duration_seconds + BACKGROUND_GRACE_SECONDS)) cat /sys/kernel/debug/usb/usbmon/0u | head -c ${USBMON_MAX_BYTES}"
    fi

    maybe_reload_wifi "${out_dir}"

    log "Collecting live trace for ${duration_seconds}s"
    sleep "${duration_seconds}"
    stop_background_captures "${pid_file}"
    disable_trace_events "${trace_dir}"

    capture_cmd "${out_dir}" "proc-interrupts-after" cat /proc/interrupts
    capture_cmd_long "${out_dir}" "dmesg-after" dmesg -T
    capture_cmd_long "${out_dir}" "journal-kernel-after" journalctl -k -b -n 7000 -o short-monotonic --no-pager
    capture_cmd "${out_dir}" "ip-link-after" ip -d link show
    capture_cmd "${out_dir}" "iw-dev-after" iw dev
    capture_cmd "${out_dir}" "lsusb-tv-after" lsusb -tv
    capture_cmd "${out_dir}" "debug-usb-devices-after" cat /sys/kernel/debug/usb/devices

    capture_shell "${out_dir}" "summary" \
        "printf 'model='; tr '\\000' '\\n' < /proc/device-tree/model 2>/dev/null || true; printf '\\ncmdline='; cat /proc/cmdline; printf '\\nusb modules:\\n'; lsmod | grep -E 'xhci|usb|hid' || true; printf '\\nwifi modules:\\n'; lsmod | grep -E 'brcm|cfg80211|mmc|sdhci' || true; printf '\\ninterrupt deltas captured in proc-interrupts-before/after\\n'"

    if command -v tar >/dev/null 2>&1; then
        tar -C "${root}" -czf "${out_dir}.tar.gz" "$(basename "${out_dir}")" 2>/dev/null || true
    fi
    if command -v sha256sum >/dev/null 2>&1; then
        (cd "${root}" && sha256sum "$(basename "${out_dir}").tar.gz" > "$(basename "${out_dir}").tar.gz.sha256") 2>/dev/null || true
    fi
    sync

    if [[ "${service_capture_once}" -eq 1 ]]; then
        systemctl disable "${SERVICE_NAME}" >/dev/null 2>&1 || true
    fi

    log "Capture complete: ${out_dir}"
}

main() {
    parse_args "$@"

    if [[ "${install_next_boot}" -eq 1 ]]; then
        install_next_boot_capture
    fi

    if [[ "${capture_once}" -eq 1 ]]; then
        capture_all
    fi
}

main "$@"
