<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Record the Milestone 26d fit decision and Milestone 26e acceptance gate for the selected SMP+MCS architecture. -->
<!-- Author: Lukas Bower -->

# Milestone 26d MCS Decision

## Decision

Cohesix retains the classic non-MCS kernel for all accepted Milestone 26d
operational profiles because 26d is a baseline-refresh milestone, not a
scheduler migration. MCS is neither enabled nor represented as target-ready by
current generated scheduling metadata.

The architectural fit verdict is nevertheless decisive: **SMP+MCS is the
selected Cohesix scheduler architecture for both QEMU and Pi 4 beginning in
Milestone 26e.** CPU time becomes explicit capability-controlled authority for
the same reason memory, device, namespace, and IPC authority are explicit.
Kernel-enforced budget/period ceilings, timeout faults, Reply ownership,
controlled donation, and consumed-time evidence directly fit Cohesix's need to
keep root/Queen policy, emergency serial, and fault handling live beside
untrusted parsers, network ingress, Workers, drivers, telemetry, persistence,
and display work.

This is a staged implementation boundary, not architectural indecision. The
[seL4 MCS tutorial](https://docs.sel4.systems/Tutorials/mcs.html)
shows the facilities Cohesix would need to use: scheduling contexts, budget and
period configuration, reply objects, passive-server donation, timeout
endpoints, consumed-time observation, and per-core scheduling control. The
tag-pinned [seL4 15 caveats](https://github.com/seL4/seL4/blob/15.0.0/CAVEATS.md)
state that AArch64 MCS verification is incomplete and SMP+MCS remains less
explored and experimental. Those facts make exact QEMU/Pi acceptance and honest
proof wording mandatory; they do not justify maintaining two production
schedulers. If the atomic 26e transition encounters an unforeseen blocker, the
change set is reverted in source/configuration rather than shipping a classic
runtime fallback, compatibility mode, or selectable scheduler profile.

## Current as-built state

- Selected kernels set `KERNEL_MCS=false`.
- Worker scheduling records select `non-mcs`; MCS budget, period, timeout badge,
  and consumed-budget evidence are zero or false.
- Root and driver paths use classic reply capabilities and do not allocate or
  configure SchedContext or Reply objects.
- Cooperative service-turn limits bound root-task work but do not provide
  kernel-enforced CPU-time isolation.
- General Worker roles are not currently launched target TCBs, so assigning
  them theoretical MCS values would be generated evidence without a consumer.

## Design comparison

MCS and SMP solve different problems. SMP supplies parallel execution across
cores; MCS adds kernel-enforced temporal budgets, scheduling-context ownership,
timeout faults, and donation rules. Enabling MCS does not by itself make an SMP
system faster or make an unmeasured service schedulable.

| Candidate | Temporal-isolation value | seL4 15 / Cohesix assurance state | Decision |
| --- | --- | --- | --- |
| Classic four-core SMP, non-MCS | Priority, domain, affinity, and bounded cooperative service turns; no kernel CPU-time budget | Current 26d generated bindings, runtime paths, QEMU/Pi profiles, and operator-liveness evidence use this model | Accepted only through 26d; retired by the atomic 26e transition |
| Single-core MCS alternative | Would isolate scheduling-context, budget/period, timeout, Reply, and donation semantics from SMP effects | Useful in the design comparison, but it would add a target configuration that is not the selected Cohesix architecture | Not implemented as an operational or fallback profile |
| Four-core SMP+MCS | Kernel-enforced temporal authority with multicore placement | Requires new Cohesix bindings, generated admission, Reply/timeout lifecycle, cross-core/locality rules, and exact QEMU/Pi evidence; tag-pinned caveats describe the combination as less explored/experimental | Selected 26e operational architecture; both targets must pass before closure |

The design study rejects only an implicit switch inside 26d. Milestone 26e is
the explicit atomic transition. A boot or throughput win is insufficient:
acceptance depends on offline admission, temporal-isolation behavior,
Reply/donation correctness, timeout/fault recovery, operator liveness, and
exact-target evidence. No classic scheduler code or profile is retained as a
runtime rollback after acceptance.

## Required implementation and acceptance work

1. **Object and binding support**
   - Add version-pinned SchedContext, SchedControl, Reply-object, timeout-fault,
     `Consumed`, and `YieldTo` bindings and negative ABI tests.
   - Allocate each object through the existing BootInfo/untyped discipline;
     no ad-hoc object or capability path is permitted.
2. **Scheduling model**
   - Require every live target task to declare either a bound active SC or an
     allowlisted passive donation chain. Active records define budget, period,
     refill/maximum-refill, core, priority/MCP, timeout/overrun, consumed-time,
     and WCET/admission provenance. Passive records define allowed donor
     SCs/cores, Reply-object cardinality, donation depth, and timeout/recovery.
   - Prove that every blocking Call/Recv/Reply path owns or receives the correct
     scheduling context and cannot strand donated authority.
   - Reserve independent active scheduling contexts and admitted CPU time for
     root/Queen authority, emergency serial, and fault/fatal supervision.
   - Default asynchronous network, IRQ/DPC, periodic Worker, and locality-bound
     work to dedicated active contexts. Permit passive donation only for
     compiler-allowlisted short synchronous paths with bounded call depth and
     complete Reply/timeout/revoke proof. Never delegate SchedControl.
3. **WCET and admission**
   - Measure the selected QEMU and Pi kernels rather than accepting platform
     default WCET constants as a Cohesix bound.
   - Provide a deterministic admission calculation for operator, driver,
     fault-supervisor, and future Worker workloads.
4. **Fault and lifecycle behavior**
   - Decode timeout faults, attribute consumed time, suspend/quarantine or
     replenish according to generated policy, and preserve emergency serial
     and fatal-status liveness.
5. **Evidence**
   - Exercise the minimal ABI probes on the same four-core SMP+MCS contracts
     selected for QEMU and Pi 4; do not add a single-core or classic runtime
     profile as a diagnostic shortcut.
   - Pass host tests, QEMU boot/fault/load tests, and fresh Pi target tests with
     no ACK/ERR/END, driver, or operator-liveness regression.
   - Keep the retired 26d classic baseline, four-core QEMU, and fresh Pi results
     as separate evidence classes.
   - Validate cold/warm boot, overload, timeout, fault, revoke/restart, SC leak,
     CYW43 coexistence, per-core admission/reserves, and performance before 26e
     closes.

## Acceptance and atomic reversion

Pending Milestone 26e tasks `m26e-mcs-abi-foundation` and
`m26e-mcs-smp-target-acceptance` in
[`docs/BUILD_PLAN.md`](../BUILD_PLAN.md#26e) own the transition. They update the
version-pinned syscall/runtime layer, compiler IR, generated artifacts, kernel
profile contracts, real service/Worker/driver consumers, admission evidence,
and target tests as one architecture.

Milestone 26e completes only when both QEMU and fresh Pi 4 accept the exact
four-core SMP+MCS topology and no operational classic scheduler path remains.
A failure in admission, Reply/donation lifecycle, timeout/fault handling,
operator liveness, CYW43 coexistence, target stability, or bounded performance
blocks the milestone. Recovery is an atomic source/configuration reversion of
the 26e scheduler change set, followed by a new recorded architecture decision;
it is not a dual-profile or runtime fallback mechanism.
