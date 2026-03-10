<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document Cohesix Raspberry Pi 4 hardware bring-up workflows and constraints using the official U-Boot + binary image path. -->
<!-- Author: Lukas Bower -->
# Hardware Bring-up (As-Built)

## Scope
- Cohesix supports two bring-up paths:
- QEMU `aarch64/virt` (development/CI baseline).
- Raspberry Pi 4 (`bcm2711`) via upstream-style boot chain: `Pi firmware -> U-Boot -> seL4 image -> root-task`.
- Milestone 26 defines the strict no-NIC baseline on Pi 4; Milestone 26a adds profile-gated GENETv5 + static IPv4; the current 26b as-built state adds the interactive U-Boot policy wizard, DTB `/chosen/cohesix,*` handoff, wired DHCP, and the staged Pi 4 Wi-Fi runtime path.

## Canonical Pi 4 boot chain
1. Pi boot firmware loads `start4.elf` and `fixup4.dat`.
2. Firmware loads `u-boot.bin` (from FAT boot partition).
3. U-Boot loads the staged Cohesix payload (`cohesix-image-arm-bcm2711`) and the padded staged Pi 4 DTB (`bcm2711-rpi-4-b.dtb`) using `fatload`.
4. U-Boot applies any saved/runtime Cohesix policy into DTB `/chosen/cohesix,*`, quiesces the USB host stack, and transfers control with `bootm`.
5. seL4 enters root-task; Cohesix reaches `cohesix>` prompt.

## Manifest profiles
- Development profile: `configs/root_task.toml` (`profile.name = "virt-aarch64"`).
- Pi 4 baseline profile (Milestone 26 no-NIC): `configs/root_task_uefi_aarch64.toml` (`profile.name = "uefi-aarch64"` migration alias).
- Pi 4 networking profile (Milestone 26a/26b policy baseline): `configs/root_task_pi4_uboot_aarch64.toml` (`profile.name = "pi4-uboot-aarch64"`).
- `coh-rtc` enforces profile gates:
- Milestone 26 baseline: `hw.no_nic=true`, `features.net_console=false`.
- Milestone 26a/26b networking: `hw.network.enabled=true`, `hw.network.backend=bcmgenet-v5`, bounded `hw.network.mode` (`off|static|dhcp`), bounded `hw.network.interface` (`wired|wifi|auto`), DHCP retry/timeout bounds, required `net` device declaration, and bounded non-zero static IPv4 (`prefix_len=1..32`) when `mode=static`.
- declared `uart` + `rtc` devices
- local-seat declarations when `hw.local_seat.enabled=true`
- attestation policy/device requirements when `hw.attestation.enabled=true`

## Pi 4 build + boot commands
1. Build seL4 Pi 4 image:
`cmake --build seL4/build_UEFI --target images/sel4test-driver-image-arm-bcm2711`
2. Build U-Boot (Pi 4):
`make -C third_party/u-boot rpi_4_defconfig`
`make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j$(sysctl -n hw.ncpu)`
3. Prepare FAT boot partition with:
- Pi firmware files (`start4.elf`, `fixup4.dat`, required DTB/overlay files),
- `u-boot.bin`,
- `cohesix-image-arm-bcm2711` (copied from seL4 `images/sel4test-driver-image-arm-bcm2711`),
- Cohesix manifest artifacts (`manifest.json`, `manifest.sha256`, and related packaged assets).
4. Ensure `config.txt` includes:
- `arm_64bit=1`
- `enable_uart=1`
- `kernel=u-boot.bin`
- `dtoverlay=upstream-pi4`
- `core_freq=250` (keeps UART handoff deterministic during firmware -> U-Boot transition)
5. Boot from U-Boot shell:
- `fatls mmc 0:1`
- `fatload mmc 0:1 ${loadaddr} cohesix-image-arm-bcm2711`
- `go ${loadaddr}`

## U-Boot networking setup (for 26a/26b prep)
- Milestone 26a uses static IPv4 in runtime. The current 26b Pi 4 path adds an interactive U-Boot wizard plus persisted `cohesix.env` policy handoff through a staged padded DTB and U-Boot `bootm`, while keeping manifest defaults authoritative when the handoff is absent or invalid.
- Typical env setup:
- `setenv autoload no`
- `setenv ipaddr <board-ip>`
- `setenv serverip <host-ip>`
- `setenv ethact <interface>`
- The staged `boot.cmd` now presents a Linux-style numbered wizard over the existing U-Boot consoles:
- `Continue with existing config` is the default action when saved Cohesix policy already exists in `cohesix.env`.
- `Boot with manifest defaults` is the default action when no saved Cohesix overrides exist.
- `Configure networking` walks DHCP `ON|OFF`, `wired|wifi`, Wi-Fi credentials when needed, and static IPv4 prompts only when DHCP is off.
- `Save current settings and reboot` persists only the user-facing Cohesix policy fields to `cohesix.env` on the FAT boot partition; it does not rewrite the manifest or generic U-Boot environment.
- The wizard intentionally uses `askenv`-based numbered prompts instead of U-Boot `bootmenu`, because upstream U-Boot documents `bootmenu` as an ANSI-terminal path and the simpler prompt flow is more reliable on the Pi 4 HDMI + USB-keyboard bring-up path while still preserving serial control via `minicom`.
- The script reloads only `cohesix.env` from the FAT partition, reuses the Pi 4 preboot USB session, and switches `stdin` to `usbkbd,serial` so the HDMI wizard prefers the USB keyboard while retaining serial fallback; on first menu entry it emits one bounded USB diagnostic snapshot (`printenv stdin/stdout/stderr`, `coninfo`, `usb tree`, `usb info`) and exposes a `USB keyboard diagnostics` menu action that runs the same snapshot, enters `conitrace`, then performs one cold `usb stop`/`usb start` re-enumeration with a temporary `usb_pgood_delay=8000` before capturing the snapshot again. When `usb_pgood_delay` is otherwise unset, both the preboot and menu diagnostics print `usb_pgood_delay=<unset>` instead of an env error.
- The Pi 4 U-Boot build stays on the seL4-aligned `usbkbd` interrupt-polling baseline (`CONFIG_SYS_USB_EVENT_POLL=y`) so the HDMI wizard matches the previously working Pi 4 input path.
- Optional Cohesix net-policy overrides mirrored into DTB `/chosen` by the staged boot script:
- `setenv coh_net_mode <off|static|dhcp>`
- `setenv coh_net_interface <wired|wifi|auto>`
- `setenv coh_static_ip <ipv4>` (mirrored into DTB `/chosen/cohesix,static-ipv4`; only applied when the effective mode is `static`)
- `setenv coh_static_prefix_len <1..32>` (mirrored into DTB `/chosen/cohesix,static-prefix-len`; only applied when the effective mode is `static`)
- `setenv coh_static_gateway <ipv4>` (mirrored into DTB `/chosen/cohesix,static-gateway`; optional and only applied when the effective mode is `static`)
- `setenv coh_wifi_ssid <ssid>` (mirrored into DTB `/chosen/cohesix,wifi-ssid`; used by the CYW43455 path when `coh_net_interface=wifi` or when `auto` prefers Wi-Fi)
- `setenv coh_wifi_psk <psk>` (mirrored into DTB `/chosen/cohesix,wifi-psk`; used by the CYW43455 path for open/WPA2-PSK join)
- The staged boot script now hands the saved `coh_wifi_*` variables directly to `fdt set` without mutating or persisting escaped shadow copies, so repeated boots and policy-file writes do not grow backslashes or corrupt WPA2 credentials. If an older card reports `Wi-Fi credential handoff overflow`, clear the saved `coh_wifi_ssid` / `coh_wifi_psk` values once and re-enter them through the wizard.
- The staged `bcm2711-rpi-4-b.dtb` is padded to 128 KiB before flashing, so U-Boot can add `/chosen/cohesix,*` properties in place without `fdt resize`.
- `setenv coh_show_logo <0|1>` (controls whether the staged `boot.bmp` splash is displayed on HDMI before the menu)
- The staged Pi 4 U-Boot build enables 24bpp BMP drawing, so the centered `boot.bmp` splash uses the same HDMI framebuffer path as `bmp display`.
- Pi 4 U-Boot keeps the seL4-aligned preboot USB-start path, but now also dumps an early USB/console snapshot during `CONFIG_PREBOOT` (`pci enum; usb start; usb tree; usb info; ...; coninfo; printenv stdin; ...`) and rebinds the live console in both `CONFIG_PREBOOT` and the default board env (`stdin=usbkbd,serial`). The default Pi 4 path still avoids a global scripted USB startup delay, but it now keeps a targeted Apple `05ac:1006` keyboard-hub quirk in the U-Boot hub driver: a 250 ms post-config settle and a 5 s hub debounce timeout before downstream child probing. Only the explicit `USB keyboard diagnostics` action temporarily raises `usb_pgood_delay` during its cold re-enumeration path.
- On first entry to the root menu, the staged `boot.bmp` copy of the Cohesix logo is shown centered for a short splash delay and the interactive menu is then drawn on a cleared console, so the splash is visible without being left behind the menu text.
- `ping ${serverip}`
- `fatwrite mmc 0:1 ${coh_policy_addr} cohesix.env ${filesize}` (only when explicitly persisting Cohesix policy)
- The staged `boot.cmd` copies these env vars into `/chosen/cohesix,*` on the staged DTB before `bootm`. If that DTB cannot be loaded or updated, the script aborts before handoff instead of silently booting with stale policy. Saved Cohesix policy persists across reboots via `cohesix.env`, but the manifest remains the build-time default and is never rewritten on the SD card.

## Milestone 26a Pi 4 network checklist
1. Build/validate the QEMU U-Boot harness:
- `scripts/uboot/qemu-uboot-smoke.sh --net user`
2. Build Pi 4 payload with 26a manifest defaults:
- `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml`
3. Boot on Pi 4 and verify runtime lines include:
- `manifest.hw.network.enabled=true`
- `manifest.hw.network.backend=bcmgenet-v5`
- `manifest.hw.network.static_ipv4.ip=<configured-ip>`
- `manifest.hw.networking=enabled-static-ipv4`
4. Validate TCP console reachability from host:
- `cargo run -p cohsh --features tcp -- --transport tcp --host <STATIC_IP> --port 31337 --script scripts/cohsh/boot_v0.coh`

## Milestone 26b Pi 4 network wizard checklist (as-built)
1. Build Pi 4 payload with the 26b policy-capable manifest:
- `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml`
2. At the U-Boot wizard, either:
- wait for `Continue with existing config` or `Boot with manifest defaults`, or
- choose `Configure networking` and walk the prompts:
  - `DHCP ON (automatic address)` or `DHCP OFF (static IPv4)`
  - `Wired Ethernet (GENET)` or `Wi-Fi (CYW43455)`
  - when `DHCP OFF`, enter `Static IPv4 address`, `Prefix length`, and optional `Gateway IPv4`
  - when `Wi-Fi`, enter `Wi-Fi SSID` and optional `Wi-Fi PSK`
- choose `USB keyboard diagnostics` when isolating pre-kernel input faults; the action captures `stdin/stdout/stderr`, `coninfo`, `usb tree`, `usb info`, runs `conitrace`, then forces one cold `usb stop`/`usb start` re-enumeration with `usb_pgood_delay=8000` and captures the snapshot again
3. Optional persistence:
- choose `Save current settings and reboot` or `Save settings and reboot` to persist only the Cohesix policy fields into `cohesix.env`.
4. Boot and confirm policy evidence:
- `[net-policy] source=manifest ...` or `[net-policy] source=dtb ...`
- `manifest.hw.network.mode=static`
- `manifest.hw.network.interface=wired`
- `manifest.hw.networking=enabled-static-ipv4`
- for the manifest-default boot, `[net-console] init: bringing up backend=... mode=static interface=wired ip=192.168.10.42/24 ...`
- for wizard-selected `mode=dhcp`, `[net-console] pending-dhcp ...` followed by `[dhcp] lease bound ...` and then `[net-console] ready ip=<lease-ip> ...`
- for wizard-selected `mode=static`, `[net-console] init: bringing up backend=... mode=static interface=... ip=<static-ip>/<prefix> ...`
- when saved wizard settings are used after reboot, root-task must not print `[boot] dtb locate skipped/failed: bootinfo extra truncated`; that message indicates the DTB handoff was lost before policy resolution and is a regression.
- when Wi-Fi bring-up fails, the serial log must now identify the failing stage with `[cyw43] step: ...` and `[pi4-wifi] ...` breadcrumbs before the final `mailbox-*`, `sdio-*`, or `cyw43-*` error detail.
5. Validate diagnostics surfaces:
- on the manifest-default boot, `netstats` includes `mode=static policy=wired active=wired standby=none addr_src=manifest-static ip=192.168.10.42 gateway=192.168.10.1 dhcp=disabled`
- on the manifest-default boot, `netstatus` prints `ip=192.168.10.42 gateway=192.168.10.1 src=manifest-static dhcp=disabled`
- on DHCP boots, `netstatus` prints the compact lease state (`ip=<lease-ip> gateway=<gw> src=dhcp-lease dhcp=bound`) so wrapped serial consoles still show the active address.
- `nettest` targets the active wired address only.
6. Current limitation:
- explicit `policy=wifi` now supports both `dhcp` and `static` when credentials are present.
- `auto` remains DHCP-only and still tries Wi-Fi first when credentials are present, but Milestone 26b is not complete until on-device Pi 4 serial logs prove join + DHCP and `auto` fallback behavior.

## macOS U-Boot debug harness (fast iteration)
- Purpose: validate U-Boot scripts/env/network setup behavior quickly before hardware retest.
- Build QEMU U-Boot:
`make -C third_party/u-boot qemu_arm64_defconfig`
`make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j$(sysctl -n hw.ncpu)`
- Run:
`qemu-system-aarch64 -machine virt -cpu cortex-a57 -m 2048 -nographic -bios third_party/u-boot/u-boot.bin`
- In harness, test env/network primitives (`printenv`, `setenv`, `dhcp`, `ping`, `tftpboot`) and script logic.
- Limitation: this harness does not prove Pi 4 USB keyboard, HDMI output, or GENET fidelity; those are hardware-only checks.

## Milestone 26 boot evidence requirements
- `/proc/boot` must include:
- `manifest.sha256=...`
- Milestone 26 baseline: `manifest.hw.no_nic=true` and `manifest.hw.networking=disabled-m26-baseline`
- Milestone 26a networking: `manifest.hw.network.enabled=true`, `manifest.hw.network.backend=bcmgenet-v5`, `manifest.hw.networking=enabled-static-ipv4`
- `attestation.bound_manifest_sha256=...`
- `attestation.evidence_sha256=...` (when attestation enabled)
- Keep before/after proof for the same profile family:
- baseline no-NIC transcript (Milestone 26),
- NIC-enabled transcript (Milestone 26a).
- Pi 4 local-seat proof must show:
- HDMI displays `cohesix>` prompt.
- USB keyboard input reaches the existing root-console parser.
- Typed commands produce visible responses on HDMI with deterministic ordering relative to serial.
- A stale firmware DT handoff with `xhci` marked `status = "disabled"` does not strand local-seat: root-task retains any valid xHCI `reg` hint, recovers the VL805 controller BAR, and either brings up the USB keyboard or emits an explicit degraded/unavailable reason.
- If runtime already pinned a legacy xHCI alias but firmware also supplies a different valid xHCI `reg` hint, local-seat must probe the firmware hint instead of discarding it as stale so the HDMI keyboard path does not get trapped on an invalid `0xfe980000` alias.
- Pi 4 firmware DT `xhci` `reg` values authored in the BCM2711 `0x7e...` SoC bus window must be translated through the DT `ranges` mapping into CPU physical addresses before local-seat runtime candidate selection. Boot breadcrumbs must make that translation explicit.
- Pi 4 U-Boot USB proof for common keyboards must show that downstream hubs do not get stuck in repeated EP0 halts during early hub-class port control: per-port `PORT_POWER` is deferred for unsafe downstream hubs, Apple `05ac:1006` still gets its 250 ms post-config settle, and delayed child `status-error` retries do not issue extra `PORT_POWER` writes.
- Pi 4 USB local-seat DMA buffers must remain below `0xC0000000` (first 3 GiB), matching the BCM2711 PCIe `dma-ranges` limit used by VL805/xHCI.
- Boot must fail before ticket registration if:
- attestation is required/enabled and policy cannot be satisfied.
- `hw.local_seat.required=true` and local-seat initialization cannot be satisfied.
- If `hw.local_seat.required=false`, runtime must degrade to serial-only diagnostics with explicit `[local-seat] degraded ...` boot lines.

## Bootloader/HAL ownership boundary
- Bootloader-owned (pre-handoff): Pi firmware + U-Boot setup, media loading, pre-boot env/network commands.
- Cohesix-owned (post-seL4): HAL-backed runtime device access only (UART, local-seat paths, attestation plumbing, network handoff points).
- Root-task runtime code must not call bootloader/firmware services directly after seL4 entry.
