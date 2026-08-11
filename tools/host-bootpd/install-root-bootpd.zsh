#!/bin/zsh
# Author: Lukas Bower
# Purpose: Install the Pi 4 en8 bootpd supervisor as a root LaunchDaemon.
# Copyright 2026 Lukas Bower

set -eu

readonly label="com.lukasbower.cohesix.en8-bootpd"
readonly owner_user="lukasbower"
readonly script_dir="${0:A:h}"
readonly repo_root="${script_dir:h:h}"
readonly agent_plist="${repo_root}/tools/host-bootpd/${label}.plist"
readonly daemon_plist="/Library/LaunchDaemons/${label}.plist"
readonly supervisor="${repo_root}/tools/host-bootpd/start-en8-bootpd.zsh"
readonly runtime_dir="/Users/lukasbower/cohesix/host-bootpd"
readonly runtime_parent="${runtime_dir:h}"
readonly legacy_runtime_dir="${repo_root}/out/host-bootpd"
readonly service_name="AX88179B"
readonly bootpd_command="/usr/libexec/bootpd -d -D -i en8 -f ${repo_root}/tools/host-bootpd/bootpd.plist"

fail() {
	print -u2 -- "host-bootpd installer: $*"
	exit 78
}

path_present() {
	[[ -e "$1" || -L "$1" ]]
}

supervisor_running() {
	pgrep -f -x "/bin/zsh ${supervisor}" >/dev/null 2>&1 || \
		pgrep -f -x "${supervisor}" >/dev/null 2>&1
}

bootpd_running() {
	pgrep -f -x "${bootpd_command}" >/dev/null 2>&1
}

wait_for_shutdown() {
	local attempt
	for attempt in {1..50}; do
		if ! supervisor_running && ! bootpd_running; then
			return 0
		fi
		sleep 0.2
	done
	fail "could not stop the exact bootpd supervisor and child within 10 seconds"
}

validate_runtime_state() {
	if path_present "${legacy_runtime_dir}" && path_present "${runtime_dir}"; then
		fail "ambiguous runtime state: both ${legacy_runtime_dir} and ${runtime_dir} exist"
	fi
	if [[ -L "${legacy_runtime_dir}" || -L "${runtime_dir}" ]]; then
		fail "runtime directories must not be symlinks"
	fi
	if [[ -e "${legacy_runtime_dir}" && ! -d "${legacy_runtime_dir}" ]]; then
		fail "legacy runtime path is not a directory: ${legacy_runtime_dir}"
	fi
	if [[ -e "${runtime_dir}" && ! -d "${runtime_dir}" ]]; then
		fail "runtime path is not a directory: ${runtime_dir}"
	fi
}

prepare_runtime_dir() {
	mkdir -p "${runtime_parent}"
	if path_present "${legacy_runtime_dir}"; then
		mv "${legacy_runtime_dir}" "${runtime_dir}"
	else
		mkdir -p "${runtime_dir}"
	fi
	chown root:staff "${runtime_dir}"
	chmod 755 "${runtime_dir}"
}

if (( EUID != 0 )); then
	fail "run with sudo"
fi

owner_uid="$(id -u "${owner_user}")"

launchctl bootout "gui/${owner_uid}" "${agent_plist}" >/dev/null 2>&1 || true
launchctl bootout system "${daemon_plist}" >/dev/null 2>&1 || true
pkill -f -x "${bootpd_command}" >/dev/null 2>&1 || true
pkill -f -x "/bin/zsh ${supervisor}" >/dev/null 2>&1 || true
pkill -f -x "${supervisor}" >/dev/null 2>&1 || true
wait_for_shutdown

validate_runtime_state
networksetup -setmanual "${service_name}" 192.168.10.1 255.255.255.0 >/dev/null
networksetup -setv6off "${service_name}" >/dev/null
prepare_runtime_dir
chmod 755 "${supervisor}"

/usr/bin/plutil -lint "${agent_plist}" >/dev/null
/usr/bin/install -o root -g wheel -m 644 "${agent_plist}" "${daemon_plist}"
launchctl bootstrap system "${daemon_plist}"
launchctl kickstart -k "system/${label}"
