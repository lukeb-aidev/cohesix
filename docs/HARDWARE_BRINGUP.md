<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document Cohesix Raspberry Pi 4 hardware bring-up workflows and constraints using the official U-Boot + binary image path. -->
<!-- Author: Lukas Bower -->
# Hardware Bring-up (As-Built)

## Scope
- Cohesix supports two bring-up paths:
- QEMU `aarch64/virt` (development/CI baseline).
- Raspberry Pi 4 (`bcm2711`) via upstream-style boot chain: `Pi firmware -> U-Boot -> seL4 image -> root-task`.
- Milestone 26 keeps a strict no-NIC runtime baseline on Pi 4.

## Canonical Pi 4 boot chain
1. Pi boot firmware loads `start4.elf` and `fixup4.dat`.
2. Firmware loads `u-boot.bin` (from FAT boot partition).
3. U-Boot loads the staged Cohesix payload (`cohesix-image-arm-bcm2711`) using `fatload`.
4. U-Boot transfers control with `go`.
5. seL4 enters root-task; Cohesix reaches `cohesix>` prompt.

## Manifest profiles
- Development profile: `configs/root_task.toml` (`profile.name = "virt-aarch64"`).
- Pi 4 bare-metal profile: `profile.name = "pi4-uboot-aarch64"` (legacy `uefi-aarch64` accepted only as transition alias while migration completes).
- `coh-rtc` enforces Milestone 26 gates:
- `hw.no_nic=true`
- `features.net_console=false`
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
5. Boot from U-Boot shell:
- `fatls mmc 0:1`
- `fatload mmc 0:1 ${loadaddr} cohesix-image-arm-bcm2711`
- `go ${loadaddr}`

## U-Boot networking setup (for 26a/26b prep)
- These settings are pre-boot controls; Milestone 26 runtime remains no-NIC.
- Typical env setup:
- `setenv autoload no`
- `setenv ipaddr <board-ip>`
- `setenv serverip <host-ip>`
- `setenv ethact <interface>`
- `dhcp`
- `ping ${serverip}`
- `saveenv`

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
- `manifest.hw.no_nic=true`
- `manifest.hw.networking=disabled-m26-baseline`
- `attestation.bound_manifest_sha256=...`
- `attestation.evidence_sha256=...` (when attestation enabled)
- Pi 4 local-seat proof must show:
- HDMI displays `cohesix>` prompt.
- USB keyboard input reaches the existing root-console parser.
- Typed commands produce visible responses on HDMI with deterministic ordering relative to serial.
- Boot must fail before ticket registration if:
- attestation is required/enabled and policy cannot be satisfied.
- `hw.local_seat.required=true` and local-seat initialization cannot be satisfied.
- If `hw.local_seat.required=false`, runtime must degrade to serial-only diagnostics with explicit `[local-seat] degraded ...` boot lines.

## Bootloader/HAL ownership boundary
- Bootloader-owned (pre-handoff): Pi firmware + U-Boot setup, media loading, pre-boot env/network commands.
- Cohesix-owned (post-seL4): HAL-backed runtime device access only (UART, local-seat paths, attestation plumbing, network handoff points).
- Root-task runtime code must not call bootloader/firmware services directly after seL4 entry.
