<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: State the as-built root-task trusted boundary and gate future seL4 service compartmentalization. -->
<!-- Author: Lukas Bower -->

# Milestone 26d Root-Task TCB Boundary Audit

## As-built boundary

The selected upstream seL4 kernel enforces objects, address spaces,
capabilities, IPC, notifications, scheduling, and interrupts. Above it, one
trusted Rust root-task currently owns:

- BootInfo/untyped allocation and initial CSpace/VSpace construction;
- HAL device admission and physical-driver child construction;
- the event pump and operator scheduling/degradation policy;
- serial, local-seat, HDMI feedback, and authenticated TCP console handling;
- smoltcp state and network packet parsing;
- console grammar and `NineDoorBridge` namespace parsing/projection;
- ticket, role, lifecycle, lease, schedule, and namespace policy;
- audit, logs, telemetry, evidence state, and fault supervision.

Manifest-declared physical-driver runtimes are genuine separate seL4 tasks with
restricted CSpaces/VSpaces and bounded shared mappings. General Worker crates,
roles, badges, and lifecycle records are currently model/build scaffolding;
they are not live child TCBs. Queen is root-task authority, not a separate task.
Milestone 25 is therefore reopened for the broader NineDoor, console/network,
provider, policy, and Worker task-isolation claims; SMP and physical-driver
child evidence do not close those surfaces.

Consequently a memory-safety or logic defect in a root-owned network/parser,
namespace, lifecycle, or provider path executes inside the principal userspace
authority domain. seL4 still protects the kernel and separate driver children,
but it cannot create a compartment that userspace has not constructed.

## Milestone 26d decision

Milestone 26d does not split the root task. A partial split during the live Pi
driver/reliability lane would change IPC ordering, fault behavior, pressure
handling, boot packaging, and target evidence while the selected kernel
baseline is still being closed. The audit instead removes unsupported prose
claims and establishes the following future order. Pending Milestone 26e is the
exact implementation owner; it remains inactive until 26d and the parallel
CYW43/current-image lane close.

## Recommended split order

1. **NineDoor parser/provider service**
   - Owner: `m26e-ninedoor-service-isolation`.
   - Move untrusted path, frame, record, and namespace parsing behind a bounded
     generated request/reply ABI.
   - Delegate only provider-specific capability bundles; keep policy ownership
     explicit and avoid a catch-all namespace cap.
2. **TCP console and smoltcp service**
   - Owner: `m26e-console-network-service-isolation`.
   - Isolate packet/frame/authentication parsing after the namespace ABI is
     stable.
   - Preserve the sole-listener exception, bounded response flush, emergency
     serial, and deterministic disconnect/error behavior.
3. **Worker supervisor and live Worker children**
   - Owner: `m26e-worker-supervisor-child-isolation`.
   - Load separately packaged images; create TCB/CSpace/VSpace/IPC/stack state;
     deliver role/epoch-badged endpoint and lifecycle notification caps; bind
     fault handling and deterministic revoke/restart evidence.
4. **Policy/audit split only if measurement justifies it**
   - Separate decision from evidence storage without creating duplicate
     authority or inconsistent replay state.

HAL admission and emergency serial are not the first split candidates. They
are bootstrap/fatal-recovery authority and require a stronger resource and
recovery model before delegation can reduce rather than duplicate trust.

The common QEMU/Pi containment and no-regression owner is
`m26e-root-tcb-target-proof`. The later production-wide role/lease/epoch cap
bundle and structured fault/restart lifecycle remain separate Milestone 28e
tasks: `m28e-full-cap-bundle-ticket-authority` and
`m28e-structured-fault-lifecycle`.

## Required gate for each compartment

Every future service split must include, in one atomic milestone:

- compiler-owned task, CSpace, VSpace, endpoint, notification, shared-frame,
  scheduling, fault, and revoke records;
- an explicit capability matrix proving absence of ambient root authority;
- bounded request/reply and pressure behavior with no unbounded queue;
- replay, cancellation, shutdown, late-message, and fault semantics;
- deletion of all stale mapped-frame caps before VSpace reuse;
- source tests plus target evidence that the selected image created and ran the
  child and observed both normal IPC and injected fault containment;
- QEMU and fresh Pi evidence kept separate from host/model tests;
- before/after TCB and latency measurements, with a rollback profile.

Until those gates pass, documentation must describe the root-owned service as
trusted and must not draw it as an isolated seL4 task.
