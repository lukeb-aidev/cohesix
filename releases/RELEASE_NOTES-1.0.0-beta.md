<!-- Author: Lukas Bower -->
<!-- Purpose: Describe Cohesix 1.0.0-beta changes, upgrade requirements, and release evidence boundaries. -->
<!-- Copyright 2026 Lukas Bower -->

# Cohesix 1.0.0-beta Release Notes

Date: 2026-09-13

Status: Release preparation; final qualification and bundle assembly are in
progress. This document does not assert a completed release gate.

## Changes since 0.9.0-beta

The comparison baseline is tag `v0.9.0-beta` (`4d1b9f09f`). This release moves
the target to seL4 16 and the Milestone 26e isolation architecture, adds a
Raspberry Pi 4 distribution, and updates the host toolkit for real target
Workers and the current generated contracts.

### Kernel, services, and Workers

- Upstream seL4 16.0.0 replaces the earlier kernel generation. Production QEMU
  and Pi profiles use four cores with mixed-criticality scheduling (MCS).
- Root control, fault handling, emergency service and supervisors have explicit
  scheduling and capability bounds. NineDoor runs as a passive namespace child;
  an isolated console-network child owns the authenticated TCP connection.
- Both production profiles declare 256 passive Workers: one Heartbeat, 127 GPU,
  and 128 LoRA instances, serviced through two bounded executor lanes. Worker
  admission, retirement, replacement and receipts carry generation identities.
  WorkerBus remains a model/session-only role.
- IPC storage ownership, TLS layout, donated scheduling contexts, Reply
  handling and terminal containment were repaired during qualification.
  A partial serial or USB input line no longer excludes network service merely
  because its unfinished text remains buffered.

### Raspberry Pi 4

- The supported boot path is Pi firmware → U-Boot → seL4 → the Rust root task.
  The new Pi archive contains a complete SD image and its boot identity.
- Serial, display, USB, GENET Ethernet, SDIO and CYW43 Wi-Fi use
  manifest-declared isolated driver runtimes admitted through HAL.
- PCIe admission now requires complete link, endpoint and firmware proof.
  Cold-reset VL805 firmware handling, ordered hardware waits, early mapping
  publication, boot memory layout and stack bounds were corrected.
- CYW43 processing retains ownership while shared receive records are being
  published. GENET scheduling and response handling preserve bounded console
  service and pending work across notifications and waits.
- Early integrity and boot audit records survive ordinary log-ring eviction.
  Build, media, RAM boot, ordinary SD boot and repeated-boot measurements remain
  distinct evidence; one does not establish the others.

### Host tools and Python

- The eight host executables remain `cas-tool`, `coh`, `cohsh`,
  `gpu-bridge-host`, `hive-gateway`, `host-sidecar-bridge`, `host-ticket-agent`
  and `swarmui`. CUDA, NVML, model execution and PEFT remain host-side.
- SwarmUI and host discovery follow the generated shard topology and actual
  Worker state. Live host/GPU data requires authenticated snapshots with
  freshness and identity checks; absent providers remain unavailable.
- Shared gateway sessions and REST deadlines now account for bounded broker
  operation. Exact-authority pooled sessions avoid redundant target ATTACH
  churn while changed authority still requires a real target attachment.
- Native FUSE startup and mount options were repaired on macOS and Linux.
  Evidence export handles large records and empty audit streams, retains
  failed-export details, and reports structured AuditFS errors in timelines.
- Maintenance accounts for active leases and terminal Worker containment,
  allowing drain/resume to reflect the work that remains outstanding.
- Host-ticket GPU/PEFT handling preserves real results, model commits and
  generation-bound receipts. Systemd output keeps complete property records.
- The target-neutral Python wheel ships with generated QEMU and Pi contracts,
  typed Worker support, and operator/evidence examples. Its package version is
  independent of the overall release version.

### Distribution and evidence

- Native host bundles carry separate QEMU guests: Mac HVF at 24 MHz and Linux
  AArch64 KVM at 31.25 MHz. They are not interchangeable.
- The release factory uses an exact compiler-owned file inventory, manifest
  hashes, native artifact/result records and `BUILD_PROVENANCE.json`.
- All three archives carry the maintained quickstart. The Pi archive requires
  the matching Mac or Linux archive for CLI, Python and SwarmUI tools.
- Raw framed TCP is available as the direct network performance measurement.
  REST and host-model results remain separately identified.
- Superseded bundles and release notes are removed from the current tree;
  their original Git tags preserve them unchanged. Current firmware, seL4
  build inputs, fixtures and audit records are retained.

## Downloads

| Archive | Contents |
| --- | --- |
| `Cohesix-1.0.0-beta-MacOS.tar.gz` | Apple Silicon host tools, Mac QEMU guest, Python wheel, configuration and guides |
| `Cohesix-1.0.0-beta-linux.tar.gz` | Linux AArch64 host tools, native KVM guest, Python wheel, configuration and guides |
| `Cohesix-1.0.0-beta-Pi4.tar.gz` | Raw SD image, SHA-256 sidecar, layout/boot identity metadata and guides |

Each extracted directory has `VERSION.txt` and `MANIFEST.sha256`. The Pi image
metadata records `minimum_target_bytes`; a larger card retains unused spare
capacity. Linux GPU hosts require a compatible host CUDA/NVML stack. Jetson is
one reference host, not a requirement for the host-tool interface.

## Upgrade from 0.9.0-beta

1. Export evidence you need to retain before replacing a target image.
2. Extract each 1.0.0-beta archive into a new directory and verify its manifest.
3. Run the new host bundle's setup script and use its Python environment.
4. Use the guest, host binaries and generated policies from the same bundle.
   Review deployment-specific credentials, tickets and configuration against
   the new generated contracts; do not overwrite them with the old defaults.
5. For a Pi, install the new SD image using the bundled `QUICKSTART.md`, then
   configure its network and authenticated host connection.

## Known limitations and accepted risk

- This remains a beta research operating system. seL4's verification does not
  constitute formal verification of Cohesix userspace or its host tools.
- One direct authenticated TCP owner is supported per target. Use Hive
  Gateway for concurrent clients. Direct TCP is not encrypted; keep it on
  loopback or carry it through an authenticated tunnel.
- Worker counts describe admitted control-plane instances, not independent
  GPU machines. No GPU execution runs inside Cohesix.
- Cross-Queen replication and automatic in-VM leader election are absent.
  Active/standby operation requires host fencing and controlled replay.
- AWS/UEFI and the later integration roadmap are outside this release's
  supported target scope.
- DD26–29 are closed with their recorded scoped evidence. DD30 remains P1 /
  `ACCEPTED_RISK` under `EX-2026-0030`: Lukas Bower accepts the remaining
  dynamic fault/wake evidence gap specifically for 1.0.0-beta. That test is
  unexecuted. The source-bound waiver expires on 2026-10-13 and does not waive
  other release gates or change their results.

Qualification results remain bound to their original source, image, host and
target. Publication metadata records documentation and packaging changes separately
from the qualified runtime. Bundle assembly and hash verification establish
packaging provenance, not a new target or performance PASS.
