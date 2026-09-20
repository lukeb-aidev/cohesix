<!-- Author: Lukas Bower -->
<!-- Purpose: Describe the Release A candidate and distinguish implemented workflows from pending assembled qualification. -->
<!-- Copyright 2026 Lukas Bower -->

# Cohesix 1.1.0-beta release candidate

**Qualification pending.** This is the reserved Release A candidate under
[Milestone 27g](../docs/BUILD_PLAN.md#27g), not a published or accepted release.
The physical operator burn-in, complete subsequent staged Test Plan, native
package and desktop checks, human review and release promotion must all close
before publication. Current acceptance status is recorded in
[Release Status](../docs/STATUS.md).

Cohesix coordinates native host work through its seL4 Queen and narrowly scoped
Workers. This candidate adds complete operator journeys around the authority,
driver isolation, telemetry, FUSE and lifecycle capabilities of
[1.0.0-beta](RELEASE_NOTES-1.0.0-beta.md). CUDA, training, model evaluation and
serving continue to execute on the host.

## Implemented workflows

- **Admitted CUDA work and recovery.** CLI, Python and CI recipes bind exact
  inputs, native provider execution, target Worker receipts and independently
  checked outcomes. Recovery reconciles the same operation after a lost reply;
  it cannot turn a known native refusal into successful execution. See
  [GPU nodes](../docs/GPU_NODES.md) and [CI workflows](../docs/CI_WORKFLOWS.md).
- **Verified private LoRA releases.** Native adapter import and training use
  evaluation, canary, promotion and rollback checks. A recovered baseline does
  not make CI pass when the requested candidate was refused or not released.
  See [Private LoRA release](../docs/PRIVATE_LORA_RELEASE.md).
- **Installation and operator evidence.** Signed component packages bind exact
  files and independently enrolled trust. Installed CLI and Python workflows
  retain operation identities, verification results and evidence for handoff.
  See [Adoption](../docs/ADOPTION.md) and
  [Operator evidence](../docs/OPERATOR_EVIDENCE.md).
- **Native desktop workflows.** SwarmUI connects through the authenticated
  gateway for reviewed operations, recovery and evidence inspection. Offline
  replay remains read-only and cannot establish a current live outcome. See
  [SwarmUI](../docs/SWARMUI.md).
- **Consistent host defaults.** QEMU and Pi select the existing Mac launchd,
  native network discovery and read-only endpoint-compliance paths. Service
  actions still require exact compiler enrollment and delegated authority;
  missing hardware, helpers or mapped endpoints remain typed unavailable
  conditions. See [Host tools](../docs/HOST_TOOLS.md).

## Targets and qualification boundaries

The target paths remain QEMU AArch64/GICv3 and Raspberry Pi 4 through
Pi firmware, U-Boot and seL4. Mac/HVF and native Linux AArch64/KVM artifacts
have distinct generated timer and profile contracts. Linux AArch64 NVIDIA is
the supported native CUDA-host contract; Jetson is its current reference host.
A Mac without NVIDIA hardware does not acquire a native NVIDIA execution claim.

Earlier component records establish only their named source, profiles and
checks. They do not qualify this assembled candidate. Full pressure, independent
boots, physical media readback, native UI and release evidence retain their
separate requirements in the [Test Plan](../docs/TEST_PLAN.md). Deferred provider
and agent-protocol milestones are outside this release's scope.

The immutable 1.0.0-beta release and its documented exceptions are unchanged.
Release B (`1.2.0-beta`) and Release C (`1.3.0-beta`) remain reserved.
