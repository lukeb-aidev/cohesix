#!/bin/zsh
# Author: Lukas Bower
# Purpose: Keep the Mac-side bootpd DHCP server ready for Pi 4 wired NIC bring-up on en8.
# Copyright 2026 Lukas Bower

set -u

readonly iface="en8"
readonly service_name="AX88179B"
readonly script_dir="${0:A:h}"
readonly repo_root="${script_dir:h:h}"
readonly config="${repo_root}/tools/host-bootpd/bootpd.plist"
readonly runtime_dir="${repo_root}/out/host-bootpd"
readonly log_file="${runtime_dir}/bootpd-supervisor.log"
readonly pid_file="${runtime_dir}/bootpd-supervisor.pid"

last_state=""

ensure_runtime_dir() {
	mkdir -p "${runtime_dir}"
	print "$$" > "${pid_file}"
}

log() {
	local now
	now="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
	print -- "${now} $*" >> "${log_file}"
}

configure_service() {
	networksetup -setmanual "${service_name}" 192.168.10.1 255.255.255.0 >/dev/null 2>&1 || true
	networksetup -setv6off "${service_name}" >/dev/null 2>&1 || true
}

interface_ready() {
	ifconfig "${iface}" | grep -q "status: active" && \
		ifconfig "${iface}" | grep -q "inet 192.168.10.1"
}

while true; do
	ensure_runtime_dir
	configure_service
	if interface_ready; then
		if [[ "${last_state}" != "ready" ]]; then
			log "interface=${iface} state=ready action=start-bootpd"
			last_state="ready"
		fi
		/usr/libexec/bootpd -d -D -i "${iface}" -f "${config}" >> "${log_file}" 2>&1
		bootpd_status=$?
		log "interface=${iface} bootpd_exited status=${bootpd_status} action=retry"
		sleep 2
	else
		if [[ "${last_state}" != "waiting" ]]; then
			log "interface=${iface} state=waiting reason=no-carrier-or-no-192.168.10.1 action=wait"
			last_state="waiting"
		fi
		sleep 2
	fi
done
