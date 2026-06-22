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
readonly service_name="AX88179B"

owner_uid="$(id -u "${owner_user}")"

launchctl bootout "gui/${owner_uid}" "${agent_plist}" >/dev/null 2>&1 || true
launchctl bootout system "${daemon_plist}" >/dev/null 2>&1 || true
pkill -f "/usr/libexec/bootpd -d -D -i en8 -f ${repo_root}/tools/host-bootpd/bootpd.plist" >/dev/null 2>&1 || true
pkill -f "${supervisor}" >/dev/null 2>&1 || true

networksetup -setmanual "${service_name}" 192.168.10.1 255.255.255.0 >/dev/null
networksetup -setv6off "${service_name}" >/dev/null
mkdir -p "${repo_root}/out/host-bootpd"
chmod 755 "${supervisor}"

cat > "${daemon_plist}" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!-- Author: Lukas Bower -->
<!-- Purpose: Launch the root Mac-side en8 bootpd supervisor for Pi 4 wired NIC DHCP bring-up. -->
<!-- Copyright 2026 Lukas Bower -->
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>${label}</string>
	<key>ProgramArguments</key>
	<array>
		<string>${supervisor}</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<true/>
	<key>StandardOutPath</key>
	<string>${repo_root}/out/host-bootpd/root-launchd.out.log</string>
	<key>StandardErrorPath</key>
	<string>${repo_root}/out/host-bootpd/root-launchd.err.log</string>
</dict>
</plist>
PLIST

chown root:wheel "${daemon_plist}"
chmod 644 "${daemon_plist}"
launchctl bootstrap system "${daemon_plist}"
launchctl kickstart -k "system/${label}"
