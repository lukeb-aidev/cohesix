<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Audit the seL4 16 API, security, capability, and proof changes that affect the Cohesix Milestone 26d refresh. -->
<!-- Author: Lukas Bower -->

# Milestone 26d seL4 16 Capability Audit

```
Title/ID: m26d-sel4-capability-utilization-audit
Milestone: Milestone 26d — seL4 16 Baseline Refresh + Reference/Performance Realignment / m26d-sel4-capability-utilization-audit
Goal: Classify every seL4 16 change relevant to Cohesix without confusing upstream availability, generated ABI compatibility, current use, or target proof.
Inputs: seL4 Reference Manual 16.0.0, seL4 16.0.0 release notes and caveats, generated v16 headers, Cohesix bindings/runtime/bootstrap code, profile contracts, and v15 audit ledgers.
Changes:
  - docs/audit/M26D_SEL4_16_CAPABILITY_AUDIT.md — record required compatibility work, intentional exclusions, security impact, and proof boundaries.
Commands: scripts/check-generated.sh; cargo check --workspace; cargo test -p sel4-sys; cargo test -p root-task --lib; out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py validate --all
Checks: No ABI, capability, security, scheduling, cache, timer, or proof claim exceeds the selected v16 profile and observed target evidence.
Deliverables: Reviewable v16 impact matrix, blocking compatibility gates, intentional exclusions, and exact evidence routing.
```

## Authority and interpretation

This audit is version-bound to the
[seL4 16.0.0 release notes](https://docs.sel4.systems/releases/sel4/16.0.0.html),
[seL4 Reference Manual 16.0.0](https://sel4.systems/Info/Docs/seL4-manual-16.0.0.pdf),
and tag-pinned [16.0.0 caveats](https://github.com/seL4/seL4/blob/16.0.0/CAVEATS.md).
The exact full-project source identity is recorded in
`M26D_SEL4_16_PROVENANCE.md`.

The release is breaking. Its presence in an upstream header does not mean
Cohesix uses a facility, and compilation alone does not prove invocation
semantics or target behavior. The v15 audit remains the historical description
of the accepted v15 system; this document records the v16 migration delta and
the evidence needed to accept it.

## Security-relevant release impact

| Upstream v16 fix | Cohesix exposure and disposition |
| --- | --- |
| AArch32 cache maintenance could crash the kernel when invoked through a stale unmapped frame capability. | Cohesix operational targets are AArch64, so the affected AArch32 kernel path is not selected. The underlying authority lesson remains applicable: teardown and VSpace reuse must delete or revoke old frame caps and mappings; v16 caveats still warn that stale mapped-frame metadata retains cache-maintenance and unmap authority. Existing HAL cache and child-revocation tests remain required. |
| x86-64 VT-x `restore_vmx()` could permit kernel-mode execution after a crafted VM-entry failure. | No Cohesix operational x86, VCPU, or guest-VM profile exists. Keep the hypervisor/VCPU facility excluded; the fix cannot be cited as a Cohesix target hardening result. |
| `PAR_EL1` could leak or be overwritten across AArch32/AArch64 hypervisor-mode context switches and AArch64 VCPU faults. | Operational QEMU and Pi profiles are AArch64 non-hypervisor and have no VCPU path. The separate BCM2711 proof-eligibility lane uses an upstream hypervisor configuration, so it must be rebuilt on v16 before renewed eligibility is recorded; that still proves neither operational boot nor Cohesix confidentiality. |

The release also adds boot-memory-region checks and diagnostic output. Those
checks are defense in depth, not a replacement for Cohesix BootInfo bounds,
untyped admission, generated CSpace limits, or fail-closed artifact validation.

## API and generated-binding impact

| v16 change | Cohesix requirement | Classification |
| --- | --- | --- |
| `seL4_TCB_ReadRegisters` and `seL4_TCB_WriteRegisters` now respect `count` instead of unconditionally copying a full `seL4_UserContext`. | Regenerate or audit the Rust invocation encoding so request length, copied registers, and return decoding follow the v16 `count` contract. Exercise zero, partial, maximum, and out-of-range counts. The root bootstrap's full AArch64 context write must pass the exact supported count and preserve resume/error behavior. | Blocking direct-API compatibility gate. |
| New inclusive `seL4_UserVSpaceTop`; `seL4_UserTop` is deprecated because its inclusive/exclusive meaning differs by platform. | Expose the generated v16 constant without reinterpreting it. Bounds code must use inclusive comparisons safely and must not manufacture one-past-top addresses. Remove any Cohesix dependency on deprecated cross-platform `seL4_UserTop` semantics. | Blocking bindings and bounds audit. |
| New `seL4_DebugGetThreadAffinity()` syscall for SMP. | If bound, keep it diagnostic-only and generated-header gated. Production profiles have kernel debug facilities disabled; authority and admission continue to come from successful `TCB_SetAffinity`, generated topology, and retained state, not a debug query. | Optional diagnostic facility; not a production prerequisite. |
| TCB size decreases only for MCS + SMP + hypervisor + benchmarking configurations. | Never hard-code an object size. Continue deriving TCB object sizes from selected generated headers. The current 26d profiles are non-MCS, and the future 26e MCS migration must revalidate allocation independently. | No current profile change; future configuration-sensitive gate. |
| Full 32-bit physical address space becomes available through Untyped caps. | Cohesix targets are AArch64. Do not import the 32-bit overflow upgrade note into 64-bit product claims; generic untyped range arithmetic must nevertheless remain checked. | Not selected by current targets. |
| IPC truncation gains a debug warning. | Preserve bounded messages and explicit length validation. A debug warning is not an operator error protocol and must not alter ACK/ERR/END behavior. | Diagnostic only. |

Version-generated `seL4_Error`, object-type, invocation-label, syscall-number,
UserContext, BootInfo, and message-layout values remain authoritative. Manual
Rust declarations must be compared against the v16 generated artifacts for
every selected configuration; a match against v15 or one diagnostic header is
insufficient.

## ARM, cache, timer, and interrupt impact

| v16 change | Cohesix requirement | Classification |
| --- | --- | --- |
| `seL4_ARM_Page_Unify_Instruction` gains a missing `dsb`. | Rebuild against v16 and retain focused cache-maintenance tests. Do not delete Cohesix ordering, DMA ownership, or HAL admission barriers on the assumption that one kernel operation now contains an additional barrier. | Relevant upstream correctness fix; no authority redesign. |
| `KernelArmVtimerUpdateVOffset` now defaults to `OFF`. | Record the effective value explicitly for every profile instead of inheriting a changed default. The operational non-hypervisor Pi timer contract still uses exported read-only `CNTVCT_EL0` and generated `TIMER_CLOCK_HZ`. | Blocking profile-drift check. |
| AArch32/hypervisor `KernelArmHypEnableVCPUCP14SaveAndRestore` now defaults to `OFF`. | No selected Cohesix AArch32/hypervisor target exists. Do not enable it as part of the refresh. | Out of scope. |
| GICv3 enables the system-register interface at EL2 in hypervisor configurations. | Operational QEMU remains GICv3 but non-hypervisor; no VMM/VCPU path is introduced. Continue validating GICv3 independently in generated DTBs and the launcher. | No current system-model change. |
| Read-only `ISR_EL1` is removed from the VCPU context. | Cohesix has no operational VCPU context. Ensure copied/generated `seL4_UserContext` declarations match v16 and do not retain stale fields. | Binding-shape audit; VCPU use excluded. |
| MCS reliably exports timer frequency to userspace and fixes missing cap-fault information. | Current 26d profiles remain non-MCS. These fixes are inputs to the separately gated 26e design, not permission to activate MCS or relax its Reply, timeout, budget, or exact-target acceptance gates. | Deferred to Milestone 26e. |

The Pi 4 counter gate remains exact:
`KernelArmExportVCNTUser=ON`,
`KernelArmExportPCNTUser=OFF`,
`KernelArmExportPTMRUser=OFF`,
`KernelArmExportVTMRUser=OFF`, and
`TIMER_CLOCK_HZ=54000000`.
No release-note change promotes physical counters, EL0 timer controls, dummy
timers, or raw spin loops into valid elapsed-time evidence.

## Capability and scheduling disposition

| Facility | Required v16 disposition |
| --- | --- |
| Untyped, CNodes, guarded CSpaces, retype/copy/mint/delete/revoke | Retain the existing BootInfo- and HAL-admitted authority model. Re-run object-size, slot-window, retype, revoke, and teardown tests against v16 generated values. |
| TCB, VSpace, ASID, IPC buffer, endpoint, notification, IRQ, and badge paths | Preserve manifest-declared isolated physical-driver runtimes. General Worker roles remain model/session state until separately launched target objects and exact caps exist. |
| Debug syscalls | Treat as optional telemetry only. A missing production debug syscall cannot block capability creation or admission, and `DebugCapIdentify`/affinity results cannot replace successful typed operations and generated bounds. |
| AArch64 mapping and cache maintenance | Preserve W^X, XN, root-alias removal, bounded volatile MMIO, HAL-only mapping, and cache/DMA ownership. Re-run exact invocation and ordering tests after the v16 binding refresh. |
| Four-core SMP and affinity | Retain for QEMU and Pi operational profiles. The v16 caveats still state SMP is not formally verified. |
| Runtime domains | Retain one domain and reject `KernelDomainSchedule`. CAmkES 3.13.0 domain-schedule units are not a Cohesix dependency. |
| MCS scheduling contexts, Reply objects, donation, timeout faults | Remain disabled for 26d. v16 adds a functional-correctness proof for 64-bit RISC-V MCS, while AArch64 MCS proof remains in progress and SMP+MCS remains less explored and experimental. The existing 26e acceptance and atomic-reversion decision remains binding. |
| Hypervisor/VCPU | Remain disabled in operational profiles. Presence in the upstream BCM2711 verified configuration does not create a product requirement. |
| SMMU | Remain disabled; Pi 4 isolation claims stay `bounded-no-iommu`. |
| SMC forwarding | Remain disabled operationally; its presence in a proof-eligibility reference configuration does not grant a Cohesix authority path. |
| Printing, hardware debug, PMU, kernel benchmarks | Keep separated between production and diagnostic contracts. No diagnostic facility becomes a shipping dependency. |
| BCM2712/Raspberry Pi 5 and STM32MP2 support | New upstream platform support is outside the Pi 4/QEMU Milestone 26d target set and does not authorize a new Cohesix target. |

## Proof and assurance boundary

The v16 caveats add 64-bit RISC-V MCS functional correctness and list BCM2711
and BCM2712 in the AArch64 verified platform family. For AArch64, the stated
functional-correctness configuration requires hypervisor extensions and FPU.
The listed security result is integrity; confidentiality and non-interference
are still in progress. The caveats also state that:

- proofs are configuration-sensitive;
- SMP and SMP+hypervisor remain unverified;
- SMP+MCS is supported but less explored, less tested, and experimental; and
- the proof does not cover machine code, compiler, linker, boot code, cache or
  TLB management.

Accordingly:

- a pristine v16 `AARCH64_bcm2711_verified.cmake` build is only proof
  eligibility;
- an operational four-core Pi or QEMU build is not a verified configuration;
- neither result verifies the Cohesix Rust root task, bindings, isolated driver
  runtimes, boot chain, DMA behavior, devices, timers, or exact image; and
- live Pi acceptance remains separate from static profile and QEMU evidence.

## CAmkES 3.13.0 classification

[CAmkES 3.13.0](https://docs.sel4.systems/releases/camkes/camkes-3.13.0.html)
uses seL4 16.0.0 and adds GCC 14/Python 3.10-or-newer support, concurrent unit
tests, and domain-schedule units. Cohesix does not use CAmkES to assemble its
system and Milestone 26d explicitly excludes adopting it, Microkit, or the
capDL loader.

Therefore CAmkES 3.13.0 is recorded as the compatible companion release, not a
build input or accepted test surface. A CAmkES source sync or example build
cannot replace Cohesix direct-API compilation, profile validation, linked QEMU
execution, exact Pi image proof, or live Pi gates.

## Blocking v16 acceptance gates

1. Resolve the full official seL4Test project and any authenticated Pi overlay
   exactly as recorded in `M26D_SEL4_16_PROVENANCE.md`.
2. Regenerate or audit Cohesix bindings against the selected v16 headers,
   especially register `count`, `seL4_UserVSpaceTop`, syscall labels/numbers,
   object sizes, UserContext, BootInfo, and mapping invocations.
3. Make changed CMake defaults explicit and reproduce every profile from an
   empty build directory with cache reuse disabled.
4. Run focused binding/runtime/bootstrap, W^X, cache, DMA, affinity, fault,
   revocation, counter, and generated-artifact tests.
5. Produce fresh static profile evidence for production, diagnostic, and
   proof-eligibility lanes without crossing their eligibility classes.
6. Build and boot the exact linked QEMU image through the staged Test Plan.
7. Produce a sealed/read-back-bound Pi image, fresh board boot, and separately
   named Wi-Fi, TCP/`cohsh`, operator-liveness, and benchmark evidence.

No source-only PASS, v15 result, directory rename, or CAmkES result closes any
later gate.
