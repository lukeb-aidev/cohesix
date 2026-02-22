<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document Cohesix UEFI hardware bring-up workflows and constraints. -->
<!-- Author: Lukas Bower -->
# Hardware Bring-up (As-Built)

## Scope
- Cohesix supports two bring-up paths:
- QEMU `aarch64/virt` (development/CI baseline).
- UEFI aarch64 boot packaging with `elfloader.efi` handoff semantics (`UEFI -> elfloader.efi -> seL4 -> root-task`).
- Milestone 26 keeps a strict no-NIC baseline for `uefi-aarch64`.

## UEFI tooling
- `scripts/uefi/esp-build.sh` builds a deterministic ESP tree with:
- `EFI/BOOT/BOOTAA64.EFI`
- `cohesix/kernel.elf`
- `cohesix/rootserver`
- `cohesix/gic-version.txt`
- optional `cohesix/initrd.cpio`
- `cohesix/manifest.json` + `cohesix/manifest.sha256`
- optional `dtb/*`
- `scripts/uefi/esp-build.sh` syncs `out/cohesix/staging/rootserver` into `seL4/build_UEFI/elfloader/rootserver`, rebuilds `elfloader.efi`, and verifies the embedded rootserver payload before packaging.
- `scripts/uefi/esp-build.sh` validates the `bcm2711` memory profile against generated seL4 headers and fails fast when `RPI4_MEMORY` does not match (`--rpi4-memory-mb`, default `8192` MiB).
- `scripts/uefi/qemu-uefi.sh` boots UEFI on QEMU using EDK2 pflash + FAT-backed ESP.
- `scripts/uefi/qemu-uefi.sh` auto-detects GIC version from ESP/seL4 config and defaults to `-machine virt,gic-version=<detected>,virtualization=on` (`kernel-irqchip=off` is included by default on macOS to match release `run.sh` behavior).
- For `qemu-arm-virt` SMP builds, the DTB consumed by upstream seL4/elfloader should report `psci.method = "smc"` (for example by dumping DTB from `qemu-system-aarch64 -machine virt,virtualization=on,...`).
- Both scripts write auditable logs/artifacts under `out/uefi/`.

## Manifest profiles
- Development profile: `configs/root_task.toml` (`profile.name = "virt-aarch64"`).
- UEFI profile: `configs/root_task_uefi_aarch64.toml` (`profile.name = "uefi-aarch64"`).
- `coh-rtc` enforces Milestone 26 UEFI gates:
- `hw.no_nic=true`
- `features.net_console=false`
- declared `uart` + `rtc` devices
- local-seat declarations when `hw.local_seat.enabled=true`
- attestation policy/device requirements when `hw.attestation.enabled=true`

## UEFI build/boot commands
1. Build EFI elfloader (requires a configured upstream seL4 CMake tree with `ElfloaderImage=efi`, typically `seL4/build_UEFI`):
`cmake --build seL4/build_UEFI`
2. Resolve UEFI manifest:
`cargo run -p coh-rtc -- configs/root_task_uefi_aarch64.toml --out out/uefi/generated --manifest out/uefi/root_task_resolved_uefi.json --cas-manifest-template out/uefi/cas_manifest_template_uefi.json --cli-script out/uefi/boot_v0_uefi.coh --doc-snippet out/uefi/root_task_manifest_uefi.md --gpu-breadcrumbs-snippet out/uefi/gpu_breadcrumbs_uefi.md --observability-interfaces-snippet out/uefi/observability_interfaces_uefi.md --observability-security-snippet out/uefi/observability_security_uefi.md --ticket-quotas-snippet out/uefi/ticket_quotas_uefi.md --trace-policy-snippet out/uefi/trace_policy_uefi.md --cas-interfaces-snippet out/uefi/cas_interfaces_uefi.md --cas-security-snippet out/uefi/cas_security_uefi.md --cohesix-py-defaults out/uefi/cohesix_py_defaults_uefi.py --cohesix-py-doc out/uefi/cohesix_py_defaults_uefi.md --coh-doctor-doc out/uefi/coh_doctor_checks_uefi.md --cohsh-policy out/uefi/cohsh_policy_uefi.toml --cohsh-policy-rust out/uefi/cohsh_policy_uefi.rs --cohsh-policy-doc out/uefi/cohsh_policy_uefi.md --cohsh-client-rust out/uefi/cohsh_client_uefi.rs --cohsh-client-doc out/uefi/cohsh_client_uefi.md --cohsh-grammar-doc out/uefi/cohsh_grammar_uefi.md --cohsh-ticket-policy-doc out/uefi/cohsh_ticket_policy_uefi.md --coh-policy out/uefi/coh_policy_uefi.toml --coh-policy-rust out/uefi/coh_policy_uefi.rs --coh-policy-doc out/uefi/coh_policy_uefi.md --swarmui-defaults out/uefi/swarmui_defaults_uefi.toml --swarmui-defaults-rust out/uefi/swarmui_defaults_uefi.rs --swarmui-defaults-doc out/uefi/swarmui_defaults_uefi.md`
3. Build deterministic ESP:
`scripts/uefi/esp-build.sh --manifest out/uefi/root_task_resolved_uefi.json --out-dir out/uefi/m26`
4. Boot in QEMU UEFI mode:
`scripts/uefi/qemu-uefi.sh --esp-dir out/uefi/m26/esp --console serial`

## Raspberry Pi 4 UEFI Settings (Firmware 1.50)
- For local-seat HDMI + USB keyboard bring-up, use:
- `setvar XhciPci -guid CD7CC258-31DB-22E6-9F22-63B0B8EED6B5 -bs -rt -nv =0x00000000`
- `setvar XhciReload -guid CD7CC258-31DB-22E6-9F22-63B0B8EED6B5 -bs -rt -nv =0x00000001`
- `setvar SystemTableMode -guid CD7CC258-31DB-22E6-9F22-63B0B8EED6B5 -bs -rt -nv =0x00000001`
- `setvar RamLimitTo3GB -guid CD7CC258-31DB-22E6-9F22-63B0B8EED6B5 -bs -rt -nv =0x00000001`
- Reboot firmware after applying values.
- NIC can be re-enabled later by restoring firmware defaults for USB/PCIe networking variables; this does not change Cohesix serial/local-seat protocol semantics.

## Milestone 26 boot evidence requirements
- `/proc/boot` must include:
- `manifest.sha256=...`
- `manifest.hw.no_nic=true`
- `manifest.hw.networking=disabled-m26-baseline`
- `attestation.bound_manifest_sha256=...`
- `attestation.evidence_sha256=...` (when attestation enabled)
- Boot must fail before ticket registration if:
- attestation is required/enabled and policy cannot be satisfied.
- `hw.local_seat.required=true` and local-seat initialisation cannot be satisfied.
- If `hw.local_seat.required=false`, runtime must degrade to serial-only diagnostics with explicit `[local-seat] degraded ...` boot lines.

## UEFI ownership boundary
- Firmware-owned (pre-handoff only): UEFI boot manager/UI, Boot Services, Runtime Services.
- Cohesix-owned (post-handoff): HAL-backed device access only (UART, local seat paths, attestation device plumbing, network handoff points).
- Root-task runtime code must not call UEFI services directly after seL4 entry.
