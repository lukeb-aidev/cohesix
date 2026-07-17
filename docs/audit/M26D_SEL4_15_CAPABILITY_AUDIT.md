<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Audit Cohesix use of applicable seL4 15 capabilities and route closure work to exact milestone tasks. -->
<!-- Author: Lukas Bower -->

# Milestone 26d seL4 15 Capability Audit

```
Title/ID: m26d-sel4-capability-utilization-audit
Milestone: Milestone 26d — seL4 15 Baseline Refresh + Reference/Performance Realignment / applicable capability closure
Goal: Bind every applicable seL4 15 capability claim to current source, generated profile truth, and target-qualified evidence.
Inputs: seL4 Reference Manual 15.0.0, seL4 15.0.0 release notes and caveats, selected QEMU/Pi build trees, root-task/driver/Worker source, generated manifests, M26c/M26d audit ledgers.
Changes:
  - docs/audit/M26D_SEL4_15_CAPABILITY_AUDIT.md — record used, intentionally excluded, defective, and later-gated facilities.
  - docs/audit/M26D_MCS_DECISION.md — retain non-MCS only for 26d, record SMP+MCS as the selected 26e QEMU/Pi architecture, and define its hard target-acceptance and atomic-reversion gates.
  - docs/audit/M26D_ROOT_TCB_BOUNDARY_AUDIT.md — state the root-task trust boundary and future split order without claiming unbuilt compartments.
Commands: scripts/check-generated.sh; python3 scripts/sel4_profile.py validate --all
Checks: No seL4 task, capability, scheduling, proof, DMA-isolation, or fault-containment claim exceeds the selected profile and observed target evidence.
Deliverables: Reviewable capability matrix, blocking defect list, intentional exclusions, and milestone routing.
```

## Authority and scope

The version-specific authorities are the
[seL4 Reference Manual 15.0.0](https://sel4.systems/Info/Docs/seL4-manual-15.0.0.pdf),
[15.0.0 release notes](https://docs.sel4.systems/releases/sel4/15.0.0.html),
and tag-pinned [15.0.0 caveats](https://github.com/seL4/seL4/blob/15.0.0/CAVEATS.md).
Living documentation is useful for ecosystem status, but it must not silently
expand the proof or platform claims of the 15.0.0 tag.

This audit asks whether Cohesix uses each *applicable, security-positive*
facility. It does not treat every configurable feature as desirable. Several
options are target-dependent, mutually constraining, diagnostic-only, or
outside the verified configuration.

## Capability matrix

| Facility | Current Cohesix state | 26d disposition |
| --- | --- | --- |
| Untyped retype, CNodes, guarded CSpaces, mint/copy/delete/revoke | Used by root bootstrap and isolated physical-driver construction. | Retain; focused tests and target evidence remain authoritative. |
| Separate TCB/VSpace/ASID/IPC-buffer construction | Used for manifest-declared physical-driver runtimes. General Worker roles are currently model/session scaffolding, not launched target tasks. | Preserve driver-task implementation; remove unsupported Worker-task claims and route root-service/Worker isolation to pending Milestone 26e. |
| Endpoint IPC, notifications, IRQ delivery and badges | Used for root/driver paths. Generated Worker badge ranges are reserved metadata until a live Worker task receives and invokes the corresponding caps. | Require target invocation evidence before describing Worker authority as cap-backed. |
| Fault endpoints | Root receives and decodes faults and suspends faulting driver TCBs. General Workers are not yet live, and production Worker ticket/lease binding plus structured Worker/driver quarantine evidence are not built. | Preserve the current handler in 26d. Route complete live Worker bundles, MCS fault/Reply lanes, teardown, and linked-driver MCS containment to Milestone 26e; route only production Worker ledger binding, ticket-free driver-inventory projection, and structured quarantine/restart evidence to Milestone 28e. |
| AArch64 page attributes and cache maintenance | The isolated-runtime loader now rejects W+X segments/pages, maps only validated read-only code executable, maps all non-code aliases XN, and unmaps executable frames' writable root aliases before TCB resume. | Source closure implemented with focused policy/order tests; target-qualified boot evidence remains separate. |
| SMP and TCB affinity | Selected QEMU/Pi operational profiles use four nodes and apply root-authority/driver affinity. General Worker and NineDoor operations remain in-process and do not have separate TCB affinity. seL4 15 SMP is supported but unverified. | Retain as the operational lane; never collapse it into verified-kernel evidence. |
| QEMU GICv3 | The canonical production default is `out/sel4/profile-v2/qemu-smp-production`, rebuilt from the pinned v15 project, and passes source, cache, generated-config, dual-DTB, launcher, toolchain, causal-stamp, archive-capacity, and artifact validation. Build, release, regression, staged-test, and newcomer entrypoints consume and validate it by default. The five-stage target-qualified run at `out/test-plan/m26d-qemu-sel4-15-gap-audit` boots the linked image with `gic-version=3` and passes authenticated TCP and REST regressions. Preserved legacy GICv2 trees are explicit diagnostic inputs only. | Profile, consumer, and current QEMU runtime closure implemented; keep this evidence separate from Pi and formal-proof claims. |
| Runtime domain schedule | Operational SMP profiles use one domain. The canonical wrapper omits the obsolete sel4test `KernelDomainSchedule` input and the validator rejects its reappearance. Preserved legacy trees still contain it. | Keep one domain and use wrapper-built trees for claims; do not normalize legacy caches by hand. |
| MCS scheduling contexts, reply objects, timeout faults and SC donation | Not used in the accepted 26d build. Current service-turn budgets are cooperative non-MCS policy, and linked-driver MCS cfg paths are stubs rather than runnable service loops. | Retain non-MCS only through 26d. Pending tasks `m26e-mcs-abi-foundation`, `m26e-driver-runtime-mcs-port-and-cyw43-coexistence`, and `m26e-mcs-smp-target-acceptance` make four-core SMP+MCS the sole operational QEMU/Pi architecture, add independent Worker/driver supervisors and exact faulted-call Reply recovery, and retain no runtime classic fallback. |
| Verified-configuration alignment | Operational production and diagnostic profiles are distinct SMP contracts; neither is represented as the tag-pinned AArch64 verified configuration. A separate pristine BCM2711 proof-eligibility build passes its static contract. | Retain the proof lane only as upstream configuration compatibility, explicitly not a Cohesix proof or operational boot claim. |
| Hypervisor/VCPU | No guest-VM requirement exists in the active architecture. | Keep disabled in operational profiles. The proof-eligibility reference may use the upstream HYP configuration without making it the product profile. |
| SMMU | No supported BCM2711 SMMU boundary exists in the selected profile. | Keep disabled and retain `bounded-no-iommu` wording. |
| SMC forwarding | No active Cohesix requirement; an external monitor remains outside seL4 control and WCET. | Keep disabled operationally. Do not infer operational need from its presence in an upstream proof configuration. |
| PMU, kernel benchmarks, hardware debug, printing | Development aids that can weaken production channel and assurance posture. Production bootstrap treats successful kernel invocations and generated bootinfo bounds as authority; `DebugCapIdentify` is optional evidence only. | Separate diagnostic and production contracts; never use diagnostic output as a production-hardening claim or a production boot prerequisite. |
| Large/huge pages and per-TCB FPU disable | Available but no measured Cohesix bottleneck currently requires them. | Defer until target measurements justify their complexity. |

## Blocking defects discovered by the audit

1. Isolated-runtime writable mappings were not ExecuteNever and the ELF loader
   did not reject effective W+X pages. **Closed in source:** all non-code
   mappings are XN, ELF W+X is rejected, and executable root write aliases are
   removed before resume.
2. The charter-selected QEMU GICv3 profile was external while the tracked
   default build and provenance remained GICv2. **Closed for profile and
   consumer integration:** the canonical build, release, regression,
   staged-test, and runbook defaults select the isolated production tree and
   run fail-closed source, GICv3, generated-artifact, DTB, launcher, compiler,
   rootserver-archive-capacity, runtime, or release validation as applicable.
   The production wrapper reserves bounded placeholder capacity so the current
   Cohesix root task can replace seL4Test without growing a linked ELF in place.
   Target boot remains a separate evidence class.
3. The old sel4test project unconditionally repopulated the removed
   `KernelDomainSchedule` cache key, so cache deletion alone was not durable.
   **Closed for canonical builds:** the source-controlled wrapper does not
   introduce that input and validation rejects it; preserved old trees remain
   claim-ineligible.
4. Canonical architecture and scheduling prose described general Worker roles
   as separate seL4 tasks with live caps and notifications, while the root-task
   spawn path only maintained in-process Worker records. **Closed for truth:**
   generated profiles, code labels, tests, and canonical prose now identify
   these roles as model-only; actual task launch remains reopened work.
5. Operational, diagnostic, and proof-eligibility kernel intents were not
   represented by separately validated profile contracts. **Closed for
   tooling and consumers:** source-controlled contracts, fixed class-to-runtime
   and class-to-release eligibility, negative schema tests, and release/runtime
   entrypoint validation keep those evidence classes distinct.
6. The operational Pi overlay was authenticated only by a digest of local dirt,
   so a pristine pinned checkout could not reproduce it. **Closed for
   tooling:** the exact raw diff is source-controlled, an explicit idempotent
   preparation command applies it only to a pristine pinned checkout, and
   read-only validation compares the resulting diff byte-for-byte. The
   authenticated patch adds the required author, purpose, and 2026 copyright
   metadata in the target DTS file's native comment header.
7. Profile evidence could previously inherit a rolling compiler, incomplete
   Python dependency resolution, ambient packaging tools, stale outputs, or
   configure-time memoization. **Closed for canonical builds:** the contract now
   binds the official Arm compiler archive and all required executable hashes,
   a hash-locked 38-distribution Python environment, official DENX U-Boot source
   and `mkimage`, empty build directories, disabled memoization, causal
   completion stamps, and structurally valid AArch64 executable artifacts.
8. Root-endpoint admission treated a zero `DebugCapIdentify` result as a hard
   failure even though production kernels intentionally omit that diagnostic
   syscall, so the canonical production QEMU image parked after a successful
   endpoint retype. **Closed for production bootstrap:** successful bounded
   retype, the generated bootinfo CSpace window, and publication state remain
   authoritative; endpoint type identification is checked only when the
   selected kernel exposes it. Early production diagnostics remain buffered
   until the admitted PL011 sink exists instead of being drained into the
   printing-disabled stub.

## Required evidence classes

- **Static source:** proves that a path exists and is bounded; it does not prove
  that a selected image executed it.
- **Generated profile:** proves selected configuration and generated interface
  values; it does not prove boot or live capability delivery.
- **QEMU target:** proves the exact QEMU build reached the observed behavior;
  it is not Pi 4 or formal-proof evidence.
- **Fresh Pi target:** proves the read-back-bound image and named boot evidence;
  it is not interchangeable with QEMU or an older image.
- **Proof eligibility:** proves only that a pristine source/configuration matches
  the upstream proof entry conditions. It is not proof that Cohesix, its boot
  path, Rust userspace, hardware, DMA, or timing behavior is verified.

## Closure routing

- W^X and canonical GICv3/domain cleanup are 26d blockers because they restore
  the reopened 26a/26b driver-task and selected-kernel contracts.
- Worker generated/docs truth is a 26d docs-as-built blocker and reopens the
  affected Milestone 25/26c task-isolation claims. Actual Worker launch must be
  implemented only by pending Milestone 26e after its packaging, boot,
  supervisor, IPC, fault, and target-evidence surfaces are available as one
  atomic change.
- Manifest validation now rejects any `implemented=true` Worker role until IR
  contains the image, TCB, CSpace, VSpace, IPC-buffer, stack, fault, and
  revocation state needed for an executable task-object contract.
- Complete Worker and linked-driver cap bundles, fault/Reply ownership, and
  basic revoke/teardown belong to Milestone 26e. Milestone 28e adds production
  Worker ticket/lease-to-bundle ledger binding, ticket-free driver-inventory
  projection, structured quarantine evidence, fresh-ticket Worker restart, and
  fresh-generation driver recovery.
- MCS activation and root-service/Worker decomposition are routed together to
  the ordered Milestone 26e tasks. `m26e-mcs-abi-foundation` freezes the MCS
  object/IPC/generated contract before child extraction,
  `m26e-driver-runtime-mcs-port-and-cyw43-coexistence` replaces current driver
  MCS stubs while freezing CYW43 behavior, and
  `m26e-mcs-smp-target-acceptance` is a verification-only frozen-artifact gate
  that requires positive exact-image CYW43 closure plus matching QEMU/Pi
  Worker-component, root-TCB, and full-system records before closure or release
  promotion. Neither is silently authorized by the phrase "use
  more seL4 features," and failure requires atomic code/configuration reversion
  rather than a shipped non-MCS fallback.

## Parallel-lane reconciliation

The active CYW43/operator changes in `apps/root-task/src/kernel.rs`,
`docs/TEST_PLAN.md`, and the Stage 02 scripts were preserved. Reconciliation was
limited to non-state-machine truth, canonical QEMU default validation, and
test-registration edits: empty Worker
spawn breadcrumbs and the in-process timer-poll helper now identify model or
cooperative behavior, the affinity text no longer claims an executable general
Worker TCB, and the profile plus existing Pi image tests are registered in the
shared staged test plan. No CYW43 timing, restart, packaging, operator-liveness,
or evidence-classification logic was rewritten by this audit.

## Validation status, 2026-07-17

The current combined worktree passes generated-artifact and test-plan integrity,
workspace formatting/check/clippy, the 91-test profile suite, 14 focused W^X
tests, 5 unit plus 4 integration Worker-authority tests, QEMU production
root-task cross-compilation, Pi root-task and isolated driver-runtime target
checks, all 25 driver-ABI plus 485 driver-runtime tests, `cargo audit`, and
`cargo deny check advisories`. `cargo audit` reports allowed maintenance and
unsoundness warnings but no failing vulnerability advisory under the current
policy. The complete `cargo test --workspace` gate also passes.

All five fresh canonical `out/sel4/profile-v2/*` defaults pass individual
validation with source and artifacts required. The final aggregate evidence at
`out/audit/m26d-profile-v2-all.json` records `valid=true`,
`failed_profiles=[]`, and `PASS profiles=5`. That aggregate is static profile
closure. Separately, the five-stage QEMU run at
`out/test-plan/m26d-qemu-sel4-15-gap-audit` passes current linked-image boot,
18-script authenticated TCP regression, REST parity/client smoke, and full
due diligence. Neither result is Pi exact-image evidence.

The external `/Users/lukasbower/seL4/build_UBOOT` Pi diagnostic tree was
previously rebuilt from an empty path. The evidence-class guard changed the
validator identity, so current validation now fails closed on its causal stamp.
The parallel CYW43 lane owns the required fresh rebuild; this audit did not
rewrite or replace that active exact-image input. Its superseded U-Boot-wrapped
seL4Test profile image SHA-256 was
`b48506c6e91de207924c91978b0ecd61d97238ddbdf6287f6bf66c54f1e78680`.
It is retained only as diagnostic identity, not current static validation, the
Cohesix exact image, or a board boot result.

The earlier intermediate root-task build-script risk-contract mismatch was
resolved by the owning CYW43 lane; the final workspace and explicit risk gates
were rerun successfully. Exact-image Pi boot, Wi-Fi repeatability, Pi
TCP/`cohsh`, operator-liveness, and benchmark evidence remain separate open
Milestone 26d gates.
