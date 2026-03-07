<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document Cohesix Raspberry Pi 4 hardware bring-up workflows and constraints using the official U-Boot + binary image path. -->
<!-- Author: Lukas Bower -->
# Hardware Bring-up (As-Built)

## Scope
- Cohesix supports two bring-up paths:
- QEMU `aarch64/virt` (development/CI baseline).
- Raspberry Pi 4 (`bcm2711`) via upstream-style boot chain: `Pi firmware -> U-Boot -> seL4 image -> root-task`.
- Milestone 26 defines the strict no-NIC baseline on Pi 4; Milestone 26a adds profile-gated GENETv5 + static IPv4; the current 26b as-built state adds wired DHCP policy handoff while leaving Pi 4 Wi-Fi runtime support incomplete.

## Canonical Pi 4 boot chain
1. Pi boot firmware loads `start4.elf` and `fixup4.dat`.
2. Firmware loads `u-boot.bin` (from FAT boot partition).
3. U-Boot loads the staged Cohesix payload (`cohesix-image-arm-bcm2711`) using `fatload`.
4. U-Boot transfers control with `go`.
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
- Milestone 26a uses static IPv4 in runtime. The current 26b wired baseline adds optional U-Boot-to-DTB policy handoff for `mode`/`interface` while keeping manifest defaults authoritative when the handoff is absent or invalid.
- Typical env setup:
- `setenv autoload no`
- `setenv ipaddr <board-ip>`
- `setenv serverip <host-ip>`
- `setenv ethact <interface>`
- Optional Cohesix net-policy overrides mirrored into DTB `/chosen` by the staged boot script when `fdtcontroladdr` is available:
- `setenv coh_net_mode <off|static|dhcp>`
- `setenv coh_net_interface <wired|wifi|auto>`
- `setenv coh_wifi_ssid <ssid>` (accepted into DTB handoff; current runtime still rejects Wi-Fi policy)
- `setenv coh_wifi_psk <psk>` (accepted into DTB handoff; current runtime still rejects Wi-Fi policy)
- `ping ${serverip}`
- `saveenv`
- The staged `boot.cmd` copies these env vars into `/chosen/cohesix,*` properties before `go` when possible. If `/chosen` cannot be updated, root-task falls back to manifest defaults and logs `source=manifest`.

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

## Milestone 26b wired DHCP checklist (as-built)
1. Build Pi 4 payload with the 26b policy-capable manifest:
- `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml`
2. Optional U-Boot runtime override before boot:
- `setenv coh_net_mode dhcp`
- `setenv coh_net_interface wired`
3. Boot and confirm policy + DHCP evidence:
- `[net-policy] source=manifest ...` or `[net-policy] source=dtb ...`
- `manifest.hw.network.mode=dhcp`
- `manifest.hw.network.interface=wired`
- `manifest.hw.networking=enabled-dhcp-ipv4`
- `[net-console] pending-dhcp ...` followed by `[dhcp] lease bound ...` and then `[net-console] ready ip=<lease-ip> ...`
4. Validate diagnostics surfaces:
- `netstats` includes `mode=dhcp policy=wired active=wired standby=none addr_src=dhcp-lease ...`
- `nettest` targets the active wired address only.
5. Current limitation:
- `wifi` and `auto` policy requests are parsed and logged but rejected at runtime with deterministic `invalid-config` evidence until the HAL-backed CYW43xx path lands.

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
- Pi 4 USB local-seat DMA buffers must remain below `0xC0000000` (first 3 GiB), matching the BCM2711 PCIe `dma-ranges` limit used by VL805/xHCI.
- Boot must fail before ticket registration if:
- attestation is required/enabled and policy cannot be satisfied.
- `hw.local_seat.required=true` and local-seat initialization cannot be satisfied.
- If `hw.local_seat.required=false`, runtime must degrade to serial-only diagnostics with explicit `[local-seat] degraded ...` boot lines.

## Bootloader/HAL ownership boundary
- Bootloader-owned (pre-handoff): Pi firmware + U-Boot setup, media loading, pre-boot env/network commands.
- Cohesix-owned (post-seL4): HAL-backed runtime device access only (UART, local-seat paths, attestation plumbing, network handoff points).
- Root-task runtime code must not call bootloader/firmware services directly after seL4 entry.
