<!-- Author: Lukas Bower -->
<!-- Purpose: Explain the verified improvements, installation choices and limits of Cohesix 1.1.0-beta. -->
<!-- Copyright 2026 Lukas Bower -->

# Cohesix 1.1.0-beta

Cohesix 1.1.0-beta turns the seL4 Queen and host tools into a more complete
edge-AI operating workflow. Admit native work, follow its Worker and host
evidence, inspect the outcome, and recover an interrupted operation under the
same identity. CUDA, training and serving still run in the host's native
environment; the Pi remains the control plane.

## What improved since 1.0.0-beta

- **SwarmUI has been completely redesigned as a native workbench.** Guided
  connections, reviewed operation forms, namespace browsing, tickets and policy,
  recovery, artifacts and evidence now sit alongside the live Hive view. Its
  run story and evidence ribbon distinguish declared, admitted, executed and
  verified work. Offline signed replay is visibly read-only; profiles store no
  credentials. See the [workbench guide](../docs/SWARMUI.md).
- **Native CUDA jobs have accountable outcomes.** Scoped tickets and leases
  bind a Worker, host executor, exact inputs and outputs. Recipes can reconcile
  lost replies without blindly dispatching a second effect, and a native
  refusal cannot become a success. See [GPU nodes](../docs/GPU_NODES.md).
- **Private LoRA journeys reach a release decision.** Native adapter import and
  training feed evaluation, canary, promotion and verified rollback. A failed
  canary leaves the requested candidate unreleased and CI failed, even when
  rollback restores the previous service. See the [LoRA guide](../docs/PRIVATE_LORA_RELEASE.md).
- **CLI, Python, CI and packaging share the selected contracts.** The portable
  wheel, host tools, examples and generated QEMU/Pi profiles support the same
  bounded authority and evidence path. QEMU and Pi select matching 512-entry,
  non-evicting Queen intent capacity. Existing Mac launchd, native network
  discovery and read-only endpoint-compliance providers are enabled by default
  where their enrolled native dependencies are available. See
  [adoption](../docs/ADOPTION.md) and [CI workflows](../docs/CI_WORKFLOWS.md).

## Measured qualification

One measured 120-minute Pi 4 GENET, Mac and Linux AArch64 NVIDIA operator run
completed 24 CUDA outputs and four LoRA journeys, including failed-canary and
rollback cases. Exact-source QEMU pressure observed 256/256 READY Workers:
43,099 successful operations without error at medium load, then 61,166
successes and 19 bounded buffer-full refusals among 61,185 high-load attempts
(0.00031053, within the unchanged 0.01 budget). These are control-plane
measurements, not GPU speedups. The [M27g record](../docs/audit/M27G_IMPLEMENTATION_RECORD.md)
retains the attempts, source/image identities, failures and proof limits.

## Install and upgrade

| Archive | Contents |
| --- | --- |
| [MacOS](Cohesix-1.1.0-beta-MacOS.tar.gz) | Apple Silicon tools, HVF QEMU guest, Python and desktop assets |
| [Linux](Cohesix-1.1.0-beta-linux.tar.gz) | Linux AArch64 tools, KVM QEMU guest, Python and desktop assets |
| [Pi 4](Cohesix-1.1.0-beta-Pi4.tar.gz) | Complete SD image, checksum and first-boot guidance |

Extract each archive fresh and verify its manifest using the
[quickstart](../docs/QUICKSTART.md). Keep the Mac HVF and Linux KVM guests with
their matching bundles. On macOS 26.6.2, Homebrew QEMU 11.0.3 aborts this
guest before boot; select the validated QEMU 10.1.0 HVF/GIC build with
`QEMU_BIN` as described in the [Mac toolchain guide](../docs/TOOLCHAIN_MAC_ARM64.md).
Back up Pi settings and evidence before writing the raw SD image: installation
replaces the whole card. Configure fresh credentials and tickets against the
packaged contracts rather than copying old authority state.

Jetson is the current reference for the Linux AArch64 NVIDIA host contract;
Macs without NVIDIA hardware have no native CUDA claim. The target Workers
govern jobs but do not run CUDA, NVML or PEFT inside seL4. AWS/UEFI and later
provider and agent-protocol work are outside 1.1.0-beta. The
[1.0.0-beta notes](RELEASE_NOTES-1.0.0-beta.md) remain available for comparison.
