<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Record the Milestone 26d decision and admission gate for possible seL4 MCS use. -->
<!-- Author: Lukas Bower -->

# Milestone 26d MCS Decision

## Decision

Cohesix retains the classic non-MCS kernel for all accepted Milestone 26d
operational profiles. MCS is neither enabled nor represented as target-ready
by generated scheduling metadata.

This is an assurance and readiness decision, not a conclusion that temporal
isolation is unimportant. The [seL4 MCS tutorial](https://docs.sel4.systems/Tutorials/mcs.html)
shows the facilities Cohesix would need to use: scheduling contexts, budget and
period configuration, reply objects, passive-server donation, timeout
endpoints, consumed-time observation, and per-core scheduling control. The
tag-pinned [seL4 15 caveats](https://github.com/seL4/seL4/blob/15.0.0/CAVEATS.md)
state that AArch64 MCS verification is incomplete and SMP+MCS remains less
explored and experimental.

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
| Classic four-core SMP, non-MCS | Priority, domain, affinity, and bounded cooperative service turns; no kernel CPU-time budget | Current generated bindings, runtime paths, QEMU/Pi profiles, and operator-liveness evidence use this model | Accepted operational baseline |
| Single-core MCS experiment | Kernel scheduling contexts, budget/period enforcement, timeout faults, reply objects, and controlled donation without SMP interaction | Missing Cohesix bindings, generated scheduling contract, measured WCET/admission calculation, timeout-fault lifecycle, and target evidence; AArch64 MCS verification remains incomplete | Eligible only for a non-production admission experiment |
| SMP+MCS experiment | MCS temporal enforcement while retaining multicore placement | Includes every single-core gap plus cross-core scheduling, donation, affinity, locality, and mixed-load evidence; the tag-pinned caveats describe this combination as less explored/experimental | Not selectable unless both the single-core and SMP-specific gates pass |

The design study therefore rejects an immediate MCS switch. Classic SMP remains
the rollback and accepted profile even if an experimental MCS build boots or
wins a throughput test; admission depends on temporal-isolation evidence and
operator/fault behavior, not feature availability.

## Admission work required before activation

1. **Object and binding support**
   - Add version-pinned SchedContext, SchedControl, Reply-object, timeout-fault,
     `Consumed`, and `YieldTo` bindings and negative ABI tests.
   - Allocate each object through the existing BootInfo/untyped discipline;
     no ad-hoc object or capability path is permitted.
2. **Scheduling model**
   - Define generated budget, period, refill, maximum-refill, core, priority,
     passive-server, donation, timeout endpoint, and overrun policy for every
     live target task.
   - Prove that every blocking Call/Recv/Reply path owns or receives the correct
     scheduling context and cannot strand donated authority.
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
   - Build a separate non-production MCS profile first.
   - Pass host tests, QEMU boot/fault/load tests, and fresh Pi target tests with
     no ACK/ERR/END, driver, or operator-liveness regression.
   - Compare classic SMP, single-core MCS, and SMP+MCS only as separate evidence
     classes; an experimental combination cannot replace the accepted profile
     merely because throughput is higher.

## Acceptance and rollback

The exact successor is pending Milestone 27c task
`m27c-mcs-admission-experiment` in
[`docs/BUILD_PLAN.md`](../BUILD_PLAN.md#27c). It must update compiler IR and
generated artifacts and carry its own target-qualified rollback profile. A
negative experiment is an acceptable completed result: failure to meet the
admission calculation, timeout-fault lifecycle, operator-liveness gate, or
target evidence records `retain-non-mcs` and keeps the classic profile
authoritative. It does not block non-MCS Milestone 27c service-bucket work.
