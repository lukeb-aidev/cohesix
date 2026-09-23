<!-- Author: Lukas Bower -->
<!-- Purpose: Explain the verified Release A operator workflows, installation paths and proof boundaries. -->
<!-- Copyright 2026 Lukas Bower -->

# Cohesix 1.1.0-beta

Release A makes the Cohesix control plane useful for a complete edge AI job:
admit native work, follow its Worker and host receipts, inspect the result, and
recover the same operation when a reply is lost. The Queen still runs on seL4;
CUDA, model training and serving stay in the host's native environment. This
build connects those boundaries through a CLI, Python, CI recipes and the native
SwarmUI workbench.

## What you can do

**Run accountable CUDA work.** A scoped ticket and lease admit a native job;
the resulting evidence ties the chosen Worker, host execution, input and output
to one operation. The recipes check the outcome independently. If a response is
lost, recovery reconciles that identity before deciding whether work should be
retried. A refused native job cannot be reported as successful. See
[GPU nodes](../docs/GPU_NODES.md) and [CI workflows](../docs/CI_WORKFLOWS.md).

**Prepare a private LoRA release.** Native adapter import and training now feed
evaluation, a canary decision, promotion and verified rollback. A failed canary
leaves the requested candidate unreleased; CI remains failed even when the
previous serving state has been restored. The [private LoRA guide](../docs/PRIVATE_LORA_RELEASE.md)
shows the supported path and the evidence needed at each step.

**Operate from a desktop.** SwarmUI presents the hive, scoped namespace,
tickets, approvals, recovery and evidence in a native workbench. Live operations
use the authenticated Hive Gateway. Offline replay is deliberately read-only
and visibly distinct from current state. The [SwarmUI guide](../docs/SWARMUI.md)
and [gallery](../docs/SWARMUI_GALLERY.md) introduce the views.

**Install and automate with matching packages.** Signed component packages,
the target-neutral Python wheel, CLI tools, examples and operator guides travel
with generated QEMU and Pi contracts. QEMU and Pi Release A profiles select the
same 512-entry, non-evicting Queen intent capacity. Both also enable the
existing Mac launchd, native network discovery and read-only endpoint-compliance
provider paths by default. Service actions remain bound to exactly enrolled
services and delegated authority; absent helpers or endpoints remain explicit
unavailable states. See [adoption](../docs/ADOPTION.md),
[host tools](../docs/HOST_TOOLS.md) and the [operator walkthrough](../docs/OPERATOR_WALKTHROUGH.md).

## Qualification and performance

One measured 120-minute Pi 4 GENET, Mac and Linux AArch64 NVIDIA operator run
completed 24 scheduled CUDA outputs and four LoRA journeys, including failed
canary and verified rollback. Failed and resumed segments remain visible in the
[M27g record](../docs/audit/M27G_IMPLEMENTATION_RECORD.md); this was an
experienced maintainer session, not a novice usability study.

The exact Release A QEMU pressure run observed 256/256 READY Workers. Medium
pressure completed 43,099 operations without error. High pressure completed
61,166 successful operations and recorded 19 bounded buffer-full refusals among
61,185 attempts, within the unchanged one-percent error budget. Physical Pi
GENET first-connection measurements, USB input and HDMI observations have
separate provenance; QEMU and host-only timings are never presented as Pi or
GPU speedups. The [Test Plan](../docs/TEST_PLAN.md) owns target, media, package
and release acceptance, and the [M27g record](../docs/audit/M27G_IMPLEMENTATION_RECORD.md)
links the exact evidence and limitations.

## Get started

| Archive | Includes |
| --- | --- |
| [Cohesix-1.1.0-beta-MacOS.tar.gz](Cohesix-1.1.0-beta-MacOS.tar.gz) | Apple Silicon host tools, an HVF QEMU guest, Python and desktop assets |
| [Cohesix-1.1.0-beta-linux.tar.gz](Cohesix-1.1.0-beta-linux.tar.gz) | Linux AArch64 host tools, a KVM QEMU guest, Python and desktop assets |
| [Cohesix-1.1.0-beta-Pi4.tar.gz](Cohesix-1.1.0-beta-Pi4.tar.gz) | Complete Raspberry Pi 4 SD image, checksums and first-boot guidance |

Follow the [quickstart](../docs/QUICKSTART.md) for a clean extraction and
manifest verification. The Mac HVF and Linux KVM guests use different native
timer profiles; keep each guest with its matching host bundle. Back up any Pi
network settings and evidence you need before writing the raw SD image, which
replaces the whole card. Configure fresh credentials and tickets against the
packaged contracts rather than copying an old deployment's authority state.

Linux AArch64 NVIDIA is the native CUDA-host contract; Jetson is its current
reference host. A Mac without NVIDIA hardware has no native NVIDIA execution
claim. Cohesix Workers govern work but do not run CUDA, NVML or PEFT inside
the seL4 VM. AWS/UEFI and the later provider and agent-protocol milestones are
outside this release. The immutable [1.0.0-beta release](RELEASE_NOTES-1.0.0-beta.md)
remains available for comparison.
