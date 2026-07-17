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

Milestone 26e also owns the atomic transition to the one selected QEMU/Pi
scheduler architecture: four-core SMP+MCS. Task `m26e-mcs-abi-foundation` must
land before the child ABIs below freeze so scheduling-context, Reply, donation,
timeout, core, admission, and revoke semantics are structural rather than a
later retrofit. Task `m26e-mcs-smp-target-acceptance` runs after the real child
topology exists and is a hard closure gate. No classic runtime fallback or
dual-profile scheduler path is planned; an unforeseen failure requires
reverting the atomic scheduler change set.

Task `m26e-driver-runtime-mcs-port-and-cyw43-coexistence` follows the MCS ABI
and resource foundation before service extraction. Current linked-driver MCS
paths are stubs, so the task must produce a newly hashed active-SC MCS driver
archive. It freezes CYW43/SDIO ownership, state machine, timing, retries, rings,
recovery order, and errors rather than making the impossible claim that an MCS
rebuild is byte-identical to the classic artifact; exact-image coexistence is
re-proved as a separate evidence lane.

Task `m26e-worker-resource-admission-critical-tcbs` must also precede child
activation. Root/Queen control, fault supervision, emergency serial, Worker
supervision, and driver containment/command-Reply recovery require distinct
generated TCBs and active SCs; several SC records attached conceptually to the
current single event-pump TCB would not create independent temporal reserves.
Generated root-control and root-fault handoff records, separate Worker/driver
supervisor wake notifications with disjoint one-hot badge ranges, exact cap
rights, Reply-lane cardinality, and an acyclic critical-TCB fault graph are part
of that task. Root-fault failure routes to root-emergency; root-emergency never
handles its own fault.

## Recommended split order

1. **Linked-driver MCS port and containment supervisor**
   - Owner: `m26e-driver-runtime-mcs-port-and-cyw43-coexistence`.
   - Replace MCS stubs, bind active SCs only to driver TCBs, make IRQ work
     notification-woken, and generate exact root command `Write + GrantReply`,
     driver receive/IRQ-wait Read, and software-signal Write rights.
   - Give the independent driver supervisor retained origin/recovery caps and
     per-runtime command state so a driver fault during `Call` returns exactly
     one typed failure before it suspends the driver TCB, verifies the separate
     fault Reply association is clear, and revokes the old runtime generation.
2. **NineDoor parser/provider service**
   - Owner: `m26e-ninedoor-service-isolation`.
   - Move untrusted path, frame, record, and namespace parsing behind a bounded
     generated request/reply ABI.
   - Delegate only provider-specific capability bundles; keep policy ownership
     explicit and avoid a catch-all namespace cap.
3. **TCP console and smoltcp service**
   - Owner: `m26e-console-network-service-isolation`.
   - Isolate packet/frame/authentication parsing after the namespace ABI is
     stable.
   - Preserve the sole-listener exception, bounded response flush, emergency
     serial, and deterministic disconnect/error behavior.
4. **Worker supervisor and live Worker children**
   - Owners, in order:
     `m26e-worker-abi-identity-notifications`,
     `m26e-worker-image-pipeline-loader`,
     `m26e-worker-supervisor-child-isolation`,
     `m26e-host-worker-integration`, and
     `m26e-worker-target-evidence-promotion`.
   - Load separately packaged Heartbeat/GPU/LoRA images; create complete
     TCB/CSpace/VSpace/IPC/stack/frame/endpoint/notification/fault/SC state;
     deliver immutable role/slot/lease-epoch/supervisor-generation/
     cap-generation identity; decode
     one-hot notifications as coalescing bitsets; use durable completion records
     plus the supervisor wake notification for READY/receipts; allow only
     droppable `NBSend` telemetry on the output endpoint; bind terminal fault
     handling, complete teardown, fresh supervisor-generation recreation,
     host-tool correlation for WorkerGpu lease grant/renew/release and
     WorkerLora PEFT export/import/activate/rollback receipts, and exact
     QEMU/Pi evidence. WorkerBus remains model/session-only.
5. **Policy/audit split only if measurement justifies it**
   - Separate decision from evidence storage without creating duplicate
     authority or inconsistent replay state.

HAL admission and emergency serial are not the first split candidates. They
are bootstrap/fatal-recovery authority and require a stronger resource and
recovery model before delegation can reduce rather than duplicate trust.

The common QEMU/Pi containment and no-regression owner is
`m26e-root-tcb-target-proof`; full-system temporal acceptance is owned by
`m26e-mcs-smp-target-acceptance`. The final task is verification-only over
frozen artifacts and promotes release acceptance only from the matching six
QEMU/Pi Worker-component, root-TCB, and full-system records. Every cap or
mapping exercised by a 26e Worker or linked driver is part of its complete 26e
instance bundle and teardown or containment path. The later production Worker ticket/lease binding, projection
of the already complete applicable driver MMIO/DMA/shared-ring inventory,
structured quarantine/evidence, fresh-ticket Worker restart, and fresh driver
runtime-generation recovery remain separate
Milestone 28e tasks: `m28e-production-worker-ticket-driver-inventory` and
`m28e-structured-fault-lifecycle`; they do not complete basic Worker isolation,
driver authority, or either containment path.

## Required gate for each compartment

Every future service split must include, in one atomic milestone:

- compiler-owned task, CSpace, VSpace, endpoint, notification, shared-frame,
  fault, and revoke records plus an MCS scheduling record declaring either a
  bound active SC or an allowlisted passive donation chain. Active records
  contain SC object/size, SchedControl core, budget, period,
  refill/max-refill policy, priority/MCP, timeout/overrun action, consumed-time
  evidence, and WCET/admission provenance; passive records contain allowed
  donors/cores, Reply cardinality, timeout policy, and maximum donation depth;
- an explicit capability matrix proving absence of ambient root authority;
- exact direction/rights: Worker output Write-only without Grant/GrantReply,
  supervisor receive Read-only, Worker lifecycle wait Read-only, supervisor
  signal Write-only, fault caps `Write + GrantReply`, fault receiver Read-only,
  driver command `Write + GrantReply` without Grant, driver command/IRQ wait
  Read-only, driver software signal Write-only, and each active SC bound only
  to its TCB rather than a notification;
- bounded request/reply and pressure behavior with no unbounded queue;
- replay, cancellation, shutdown, late-message, and fault semantics;
- immutable instance/generation identity, disjoint endpoint/notification badge
  domains, one-hot notification bits with coalesced-bitset decoding, durable
  required completions, and a versioned pointer-free internal ABI;
- deletion of all stale mapped-frame caps and every other child-held capability
  before VSpace or generation reuse;
- source tests plus target evidence that the selected image created and ran the
  child and observed both normal IPC and injected fault containment;
- QEMU and fresh Pi evidence kept separate from host/model tests;
- before/after TCB and latency measurements tied to the atomic change-set and
  pre-change baseline; failure reverts the whole scheduler change rather than
  selecting a compiled fallback.

Until those gates pass, documentation must describe the root-owned service as
trusted and must not draw it as an isolated seL4 task.
