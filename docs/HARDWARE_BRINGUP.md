<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document Cohesix Raspberry Pi 4 hardware bring-up workflows and constraints using the official U-Boot + binary image path. -->
<!-- Author: Lukas Bower -->
# Hardware Bring-up (As-Built)

## Scope
- Cohesix supports two bring-up paths:
- QEMU `aarch64/virt` (development/CI baseline).
- Raspberry Pi 4 (`bcm2711`) via upstream-style boot chain: `Pi firmware -> U-Boot -> seL4 image -> root-task`.
- Milestone 26 defines the strict no-NIC baseline on Pi 4; Milestone 26a adds profile-gated GENETv5 + static IPv4; the current 26b as-built state adds the interactive U-Boot policy wizard, DTB `/chosen/cohesix,*` handoff, bootloader-exported xHCI BAR handoff for local-seat, wired DHCP, and the staged Pi 4 Wi-Fi runtime path.

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
- The script reloads only `cohesix.env` from the FAT partition, bootstraps menu/input USB with `pci enum; usb start` when needed, and switches `stdin` to `usbkbd,serial` so the HDMI wizard prefers the USB keyboard while retaining serial fallback. The menu no longer emits the old bring-up-only USB diagnostic snapshots or the extra USB diagnostics submenu; that U-Boot chatter was removed once the runtime handoff path became the primary evidence source.
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
- Pi 4 U-Boot keeps the seL4-aligned preboot USB-start path (`pci enum; usb start`) and rebinds the live console in both `CONFIG_PREBOOT` and the default board env (`stdin=usbkbd,serial`). The default Pi 4 path still avoids a global scripted USB startup delay, but it keeps a targeted Apple `05ac:1006` keyboard-hub quirk in the U-Boot hub driver: a 250 ms post-config settle and a 5 s hub debounce timeout before downstream child probing.
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
- rely on the runtime `[local-seat] ...` xHCI handoff breadcrumbs for current Pi 4 keyboard evidence; the old U-Boot USB diagnostic submenu and snapshot spam were removed once the cold-start handoff path became the active debug surface
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
- when Wi-Fi bring-up fails, the serial log must now identify the failing stage with `[cyw43] step: ...` and `[pi4-wifi] ...` breadcrumbs before the final `mailbox-*`, `sdio-*`, or `cyw43-*` error detail. In particular, firmware load now emits `firmware core-ctrl mode=cmd53-windowed-read32-cmd52-current-window-write8 fallback=cmd52-byte-rewindow split-window` before the ARMCR4/SOCRAM AI-core sequence so bring-up logs prove the primary mixed CMD53 backplane control path and the staged CMD52 recovery path that root-task is using. The same path now also emits `backplane window program ... low=... mid=... high=...` plus bounded `sdio xfer chunk fn=1 ...` breadcrumbs so window-byte programming and per-chunk CMD53 address translation are explicit in the serial log, and same-window accesses now emit `backplane window reuse ...` instead of replaying the SBADDRLOW/MID/HIGH CMD52 triplet. Small CMD53 byte-mode transfers now drive SDHCI as a one-block transfer with block-count enable asserted, so a stalled `data-wait` on these logs indicates a transport failure rather than a half-programmed host byte-mode transfer. AI core-control writes no longer do a 32-bit read-modify-write before programming `AI_IOCTRL` or `AI_RESETCTRL`; the general `AI_IOCTRL` path still uses a direct byte-sized CMD52 write on the already-windowed unflagged Function 1 address, SOCRAM `assert-reset` emits `firmware core-ctrl reset-write mode=cmd53-word-windowed fallback=cmd52-byte-current-window-rewindow ...`, SOCRAM `clear-reset` now emits `firmware core-ctrl reset-write mode=cmd53-word-windowed fallback=cmd52-byte-current-window ...`, and the surrounding reset path now splits the first release edge into `stage=clear-reset-primary` and `stage=clear-reset-retry`. If that primary reset-release CMD53 write fails, root-task now recovers with `sdhci recover stage=core-ctrl-reset-clear-cmd52-current-window mask=cmd+data cache=preserved restored_window=... shadow_window=... fn=...` and retries only the current-window CMD52 path before leaving any second attempt to the outer `clear-reset-write-retry` stage; a successful preserved retry now emits `firmware core-ctrl reset-clear stage=cmd52-current-window-ok ...`. The first SOCRAM post-release `AI_IOCTRL` write still emits `firmware core-ctrl postreset-write mode=cmd52-byte-current-window fallback=cmd52-byte-rewindow ...`, and the outer reset sequence now splits that edge into `stage=postreset-clock-en-write`, `stage=postreset-clock-en-write-ok`, and `stage=postreset-clock-en-readback` so logs distinguish the first post-reset write from the first post-reset readback while keeping the SOCRAM `AI_IOCTRL` edge on the direct byte path. Once a core is already held in reset the `in-reset-configure` `AI_IOCTRL` path now first checks whether the asserted-reset write would be redundant for SOCRAM, emitting `in-reset-configure-skip ... reason=redundant-after-assert` and reusing the pre-reset `FGC|CLK` state when the requested hold value matches the value already written before reset. When that SOCRAM skip path is taken immediately after a fresh reset assert, the next `AI_RESETCTRL` readback is also deferred and root-task emits `in-reset-ready-read-deferred ... reason=redundant-after-assert` instead of forcing another post-assert probe while Function 1 is still fragile. The SOCRAM `core_reset` path now also skips its redundant re-entry through `core_disable`, emitting `core-reset ... stage=skip-disable reason=held-reset-from-prior-disable`; if that path already established the same held-reset `FGC|CLK` value, root-task now emits `core-reset ... stage=pre-clear-in-reset-configure-skip ... reason=redundant-held-reset-from-prior-disable` and avoids replaying the same SOCRAM `AI_IOCTRL` write before `clear-reset`, so the next remaining failure can move to reset release itself instead of a redundant held-reset replay. ARMCR4 non-redundant in-reset `AI_IOCTRL` writes still emit `firmware core-ctrl in-reset-write mode=cmd53-word-windowed-in-reset ...` and first use the flagged word-sized backplane write path before falling back to CMD52 recovery, while SOCRAM held-reset `AI_IOCTRL` replays now emit `firmware core-ctrl in-reset-write mode=cmd52-byte-current-window fallback=cmd52-byte-rewindow ...` so the fragile SOCRAM prepare stage stays on the already-windowed byte path. The general access breadcrumbs still print both `bus=...` and `trace_bus=...` so the serial log shows the exact unflagged byte address used for live CMD52 writes and the flagged address used for window tracing. The AI core-control sequence otherwise follows the upstream Broadcom order: write `FGC|CLK` before asserting reset, re-apply `FGC|CLK` while the core is held in reset, then clear reset and leave `CLK` enabled; on Pi 4 SOCRAM, that explicit re-apply step is now skipped only when the immediately preceding `skip-disable` path already left the identical held-reset value in place. ARMCR4 uses the upstream `CPUHALT` bit during disable/release so the firmware CPU stays parked until the final release. If the primary word read path times out, root-task now first retries recovery against the current backplane window with CMD52 and only then falls back to a rewindowed CMD52 pass; reset-register writes now log stage-specific reset breadcrumbs before trying current-window CMD52 recovery, and the SOCRAM `clear-reset` path preserves the cached backplane window across that recovery instead of taking an immediate rewindow detour, while in-reset `AI_IOCTRL` attempts still log `write8-in-reset` fallback breadcrumbs before dropping back to CMD52 rewindow recovery. The serial log also emits `sdhci recover stage=cmd-wait|cmd-error|data-wait|data-error|finish-wait|finish-error mask=cmd+data` when host recovery is forced. The same recovery path now emits paired `sdio shadow ...` breadcrumbs that preserve the last programmed backplane window, Function 1 bus address, and cached chipclk/wake/sleep/cardcap control values without issuing extra probe traffic after a failure.
- Wi-Fi SDIO transfer failures now emit a decoded CMD53 metadata line (`op/fn/addr/inc/blk/count/blksz/blkcnt/flagged/trn`) immediately before the failing `sdhci xfer error ...` breadcrumb, and the AI core-control path emits `firmware core-ctrl access ...` plus `firmware core-ctrl fallback ...` breadcrumbs that name the exact backplane window, flagged bus address, byte shift, value, and fallback direction used for each latched write/readback. Those lines are the primary evidence for distinguishing a CMD52 control-path failure from a CMD53 fallback failure.
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
- A stale firmware DT handoff with `xhci` marked `status = "disabled"` does not strand local-seat: root-task ignores that stale node as an active runtime source, rejects a leaked `0xc0000000..0xffffffff` DMA-alias value as non-MMIO, pins the trusted handed-off xHCI BAR before the higher VL805 ECAM page so the lower handoff aperture stays mappable under seL4’s monotonic device retype rules, keeps VL805 ECAM preseed on a map-only path because live ECAM reads are still provoking fatal irq 27 entries on Pi 4, avoids broader live runtime VL805 config-space discovery while that path remains unsafe, and emits concise candidate-decision breadcrumbs (`kind/cfg/cov/pwin/pin/fh/fs/vh`) plus a targeted firmware-handoff trust-gate line (`cmd_safe/token/irq/trusted/reason`) before rejecting an xHCI MMIO source. On the standard Pi 4 U-Boot handoff path, runtime now treats the bootloader-exported high BAR as the only preferred Milestone 26 local-seat runtime source and trusts it only when U-Boot exports `/chosen/cohesix,xhci-mmio`, `/chosen/cohesix,xhci-pci-cmd`, `/chosen/cohesix,xhci-handoff-ready=1`, `/chosen/cohesix,xhci-irq-quiesced=1`, `/chosen/cohesix,xhci-handoff-halted=1`, `/chosen/cohesix,xhci-handoff-safe=1`, and the matching `/chosen/cohesix,xhci-cap-*` capability snapshot after the boot script has successfully quiesced USB with `usb stop` and masked legacy/MSI/MSI-X delivery. If the controller does not halt cleanly or the post-stop state is not safe, U-Boot rejects the handoff contract instead of leaving a half-trusted runtime path behind. Root-task no longer treats the high BAR as a cold-probe fallback once that contract is rejected; when the candidate set would otherwise be empty it now emits `xhci runtime blocked action=reject-untrusted-high-bar ...` instead of touching xHCI MMIO after an untrusted handoff. Root-task no longer treats the low `0xfe980000` / `0x7e980000` BCM2711 USB aliases as runtime sources on Pi 4; the live DT still identifies those aliases as the SoC `brcm,bcm2835-usb` block, so they remain diagnostic breadcrumbs only. The VL805 reset is treated as bootloader-owned on that path; local-seat no longer routes a cold-start reset request through the runtime mailbox before the first xHCI MMIO read, safe-mode runtime now logs `xhci handoff window mmio=... coverage=... pinned=... cmd_safe=...` without touching ECAM, prefers the bootloader-exported xHCI capability snapshot over a live first-read capability probe on the trusted handoff BAR, passes that validated snapshot into `usb-oxide`, and keeps the trusted runtime in explicit polling mode instead of requesting seL4 xHCI IRQ handlers. Root-task now emits `xhci irq policy mmio=... mode=poll-only reason=fw-handoff-cold-start` before controller bring-up, so any remaining failure comes from xHCI init or enumeration rather than IRQ 27 ownership reconstruction. The trusted handoff path still skips only the redundant pre-quiesce work exported by firmware, but it no longer treats the handoff token as proof of a live halted controller: runtime still applies the local write-only `USBCMD` / `USBSTS` / `IMOD` / `IMAN` scrub before ownership transfer, re-reads `USBCMD` / `USBSTS`, clears `CMD_RUN` if `USBSTS.HCH` is not yet set, waits for halt, and then issues an xHCI HCRST before `CONFIG` / `DCBAAP` / `CRCR` programming. After reset it zeros `DNCTRL` before the first command can observe stale device notifications, preserves the controller-reserved bits U-Boot preserves when programming `CRCR` / `ERSTSZ` / `ERSTBA`, primes the initial `ERDP` without setting `EHB`, and then resumes normal polling-mode `IMOD` / `IMAN` bring-up on the reset controller. Root-task now also mirrors the missing handoff diagnostics from DT into boot logs with `xhci-handoff-halted=...`, `xhci-handoff-safe=...`, `xhci-stop-state usbcmd=... usbsts=... iman0=...`, `xhci-handoff-dtb-ext source=... prestop=ready/irq/halted/safe poststop=ready/irq/halted/safe`, and `xhci-handoff-reject source=... reason=...` so a `reprobe-partial` result can be tied directly to the missing post-stop bit. To cover reset- and read-side stalls, the xHCI probe diag stream now names the reset path itself (`reset-write`, `reset-complete`, `reset-hcrst-timeout`, `reset-cnr-timeout`) and still emits explicit pre-read markers before each risky MMIO readback (`config-readback-begin`, `dcbaap-readback-begin`, `crcr-readback-begin`, `runtime-ring-readback-begin`, `usbsts-run-readback-begin`, `usbcmd-run-readback-begin`, `controller-ready-poll-begin`) alongside the corresponding read/result markers (`config-readback`, `dcbaap-readback`, `crcr-readback`, `runtime-ring-readback`, `usbsts-run-readback`, `usbcmd-run-readback`, `controller-ready`, `controller-ready-timeout`) so the last emitted breadcrumb still identifies the exact read that blocked even when the read never returns. If the first connected-port mask is empty after controller bring-up, local-seat now dumps per-port `xhci root-port stage=detect-zero|detect-slow-zero|detect-slow-hit ... portsc=...` breadcrumbs and performs one bounded 100 ms slow recheck before concluding that no root-port device is present. The first 64-bit ownership writes are now split into explicit low/high breadcrumbs (`dcbaap-write-low`, `dcbaap-write-high`, `crcr-write-low`, `crcr-write-high`, `erstba-write-low`, `erstba-write-high`, `erdp-write-low`, `erdp-write-high`), and `usb-oxide` now programs those register pairs in strict low-then-high order to match the xHCI-defined sequence during trusted Pi 4 handoff. It also now matches the known-good U-Boot ownership setup more closely by zeroing `DNCTRL`, preserving the controller-reserved bits on `CRCR` / `ERSTSZ` / `ERSTBA`, and priming the initial `ERDP` without `EHB` set, so the next remaining failure is less likely to be caused by stale notification or ring-register state inherited from firmware. To cover command-side stalls after a trusted handoff, the diag stream now also names the command path itself (`cmd-submit`, `cmd-ring-enqueue`, `cmd-completion`, `cmd-ccs-expected-ptr`, `cmd-ccs-mismatch`, `cmd-fail`, `cmd-fail-state`, `cmd-timeout`, `cmd-timeout-state`, `cmd-timeout-last-event`, `cmd-wait-other-event`) so the next failure can be attributed to command-ring progress rather than the earlier handoff gate.
- USB handoff diagnostics now include a root-task `xhci-handoff-contract state=... ready=... irq=... action=...` breadcrumb plus a companion `xhci-handoff-dtb source=... prestop=.../... poststop=.../...` line, and the staged U-Boot script emits a matching post-`usb stop` contract verdict (`safe|partial|absent`) after logging token state before reprobe, after forced reprobe, and in the pre-stop snapshot. The staged U-Boot serial path now also emits a single `xhci-reprobe-result reset=<success|failed> ready=<0|1|absent> irq=<0|1|absent> halted=<0|1|absent> safe=<0|1|absent> input=<0|1>` breadcrumb immediately after the forced `usb reset`, and the actual `xhci-pci` driver now emits `[cohesix:xhci-pci] stage=probe-entry|init|probe-ready|remove-entry|remove-ready ...` breadcrumbs with BDF, exported BARs, PCI command, and the live `coh_xhci_*` token state including `usbcmd`, `usbsts`, `iman0`, `halted`, and `safe`; on stop failures it emits `stage=remove-deregister ... ret=<err>` or `stage=remove-handoff-unsafe ... ret=<err>` instead of rebooting the board. The U-Boot `usb` command now propagates `usb stop` / `usb reset` failures back to the script, and the staged boot script explicitly clears the live `coh_xhci_*` handoff tokens before `usb stop` so stale probe-time `ready/irq` bits cannot masquerade as a post-stop contract. That makes the reprobe outcome visible in serial logs even before `usb stop` or `bootm`, and it shows whether the controller driver itself ever exported a safe handoff contract. The staged boot script no longer assumes `CONFIG_PREBOOT` already exercised the same xHCI path that local-seat needs for handoff; it uses `pci enum; usb start` only for menu/input bootstrap, then forces a real `usb reset` immediately before handoff capture so the active controller path is reprobed and any exported `coh_xhci_*` contract reflects that fresh session. Final DTB handoff sources now record whether the contract was `*-post-stop-safe`, `*-partial`, `*-absent`, or `reprobe-usb-stop-failed`, so a missing or rejected cold-start handoff stays diagnosable even when the controller is never trusted at runtime.
- If firmware DT hands off an `xhci` node with `status = "disabled"`, local-seat must treat that `reg` as stale for runtime safety, ignore it as an active xHCI source, and either use a verified runtime source or emit explicit degraded/unavailable diagnostics instead of cycling between blind legacy aliases.
- Pi 4 firmware DT `xhci` `reg` values authored in the BCM2711 `0x7e...` SoC bus window must be translated through the DT `ranges` mapping into CPU physical addresses before local-seat runtime candidate selection. Boot breadcrumbs must make that translation explicit.
- Pi 4 U-Boot USB proof for common keyboards must show that downstream hubs do not get stuck in repeated EP0 halts during early hub-class port control: per-port `PORT_POWER` is deferred for unsafe downstream hubs, Apple `05ac:1006` still gets its 250 ms post-config settle, and delayed child `status-error` retries do not issue extra `PORT_POWER` writes.
- Pi 4 USB local-seat DMA buffers must remain below `0xC0000000` (first 3 GiB), matching the BCM2711 PCIe `dma-ranges` limit used by VL805/xHCI.
- Pi 4 Wi-Fi AI core-control diagnostics must surface both sides of the staged recovery path: flagged CMD53 windowed reads and byte-sized CMD52 writes against the current window, with rewindowed CMD52 fallback retained for read-side recovery and selected reset/in-reset writes while SOCRAM `clear-reset` first preserves the cached backplane window and retries only the current-window CMD52 path. Serial logs must keep the exact `firmware core-ctrl fallback ...`, `firmware core-reset ... stage=clear-reset-primary|clear-reset-retry|postreset-clock-en-write|postreset-clock-en-readback ...`, `backplane window program ...`, `sdio xfer chunk fn=1 ...`, and `sdhci recover stage=core-ctrl-reset-clear-cmd52-current-window|core-ctrl-cmd52-current-window|core-ctrl-cmd52-rewindow|... mask=cmd+data` breadcrumbs, plus `cache=preserved restored_window=... shadow_window=... fn=...` on the SOCRAM `clear-reset` recovery path, so remaining failures stay attributable to either the transport path or the recovery path.
- The first SOCRAM reset-release path now adds `pre-clear-in-reset-configure ... reason=required-before-clear-reset`, but when `skip-disable` already established the same held-reset value it emits `pre-clear-in-reset-configure-skip ... reason=redundant-held-reset-from-prior-disable` and avoids replaying that identical write before `clear-reset-prewrite-delay ... reason=socram-fragile-first-write`. On failure it still makes a single `clear-reset-write-retry ... reason=socram-fragile-first-write` recovery attempt before the existing deferred `clear-reset-read-deferred ...` readback path, and that first clear-reset recovery now preserves the cached backplane window while skipping an immediate `cmd52-byte-rewindow` detour, so logs distinguish a redundant held-reset replay from a transient reset-release edge stall instead of a self-inflicted SBADDR replay.
- Pi 4 Wi-Fi firmware load now emits `firmware stage=pre-reset-ht-assist` before the ARMCR4/SOCRAM reset-heavy path whenever the cached chip-clock state does not already request or force HT. That pre-reset assist keeps `wake`, `cardcap`, `chipclk`, and `sleep` primed before `stage=armcr4-disable` and `stage=socram-disable`, so a remaining fault after the earlier backplane-window fix identifies a true reset-edge transport problem rather than a missing HT assist request.
- Boot must fail before ticket registration if:
- attestation is required/enabled and policy cannot be satisfied.
- `hw.local_seat.required=true` and local-seat initialization cannot be satisfied.
- If `hw.local_seat.required=false`, runtime must degrade to serial-only diagnostics with explicit `[local-seat] degraded ...` boot lines.

## Bootloader/HAL ownership boundary
- Bootloader-owned (pre-handoff): Pi firmware + U-Boot setup, media loading, pre-boot env/network commands.
- Cohesix-owned (post-seL4): HAL-backed runtime device access only (UART, local-seat paths, attestation plumbing, network handoff points).
- Root-task runtime code must not call bootloader/firmware services directly after seL4 entry.
