<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define the as-built Cohesix roles, ticket authority, worker lifecycle, and scheduling layers. -->
<!-- Author: Lukas Bower -->

# Roles, Authority, and Scheduling

This document owns Cohesix role semantics, ticket authority, target-worker
lifecycle, and scheduling layers. It does not redefine namespace schemas,
Secure9P framing, or physical-driver scheduling. See
[INTERFACES.md](INTERFACES.md), [SECURE9P.md](SECURE9P.md), and
[DRIVERS.md](DRIVERS.md) for those contracts.

## Generated authority

The selected profile manifest and generated root-task tables are authoritative
for Worker role records, worker count, any enabled endpoint-cap or notification
requirements, affinity, and scheduling metadata. The checked-in
default-profile values are summarized in
[the generated manifest snippet](snippets/root_task_manifest.md). Do not copy
generated badge bases or quotas into client code or hand-maintained prose.

The default QEMU and Pi 4 profiles admit one executable slot each for
WorkerHeartbeat, WorkerGpu, and WorkerLora. Cap-backed authority and lifecycle
notifications are enabled for those three roles. WorkerBus remains a
model/session-only role. A generated executable record proves configured
admission; a QEMU or Pi claim still requires target evidence for the exact
kernel, resolved manifest, root image, Worker archive, and ABI version.

## Role support matrix

The checked-in default and Pi 4 profiles currently declare the following:

| Role | Ticket and host policy | Target worker in selected profiles | Authority scope |
| --- | --- | --- | --- |
| Queen | Implemented | Root-task authority, not a separate worker image | Hive-wide access to enabled control and observability providers. |
| WorkerHeartbeat | Implemented | One admitted executable slot | Own telemetry and the minimal worker observability view. |
| WorkerGpu | Implemented | One admitted executable slot | Worker view plus its generated GPU lease scope. GPU hardware remains host-side. |
| WorkerBus | Recognized | **Not executable; session/model only** | Host/sidecar policy can describe a bus scope, but the selected target profiles must reject it as target-task authority. |
| WorkerLora | Implemented | One admitted executable slot | Own Worker view plus bounded AI LoRA model receipts. It receives no local GPU authority; PEFT execution remains host-side. |

`implemented = true` means that the role has a compiler-owned task ABI,
executable slot, object budget, temporal record, selected child image, and
bounded supervisor lifecycle. It does not by itself assert that a target run
passed. Operational and release claims require separate exact-image QEMU and Pi
acceptance records. WorkerGpu lease receipts and WorkerLora PEFT receipts stay
on their existing host-ticket paths; CUDA, NVML, and PEFT execution remain
host-side.

## Namespace views

Role views are capability filters over the enabled namespace, not independent
filesystems.

- Queen can access the enabled hive-wide tree and can write only the control
  nodes permitted by its ticket and lifecycle state.
- A heartbeat worker can see its own telemetry path and the bounded boot,
  lifecycle, and log information allowed by the server policy.
- GPU and bus roles add only the resource scope encoded for that role. LoRA
  remains within its own Worker view and generated receipt endpoints. None of
  these roles inherits Queen paths.
- Host-only providers remain host-only even when their paths appear in a role
  policy.

The canonical worker telemetry path is:

`/shard/<label>/worker/<id>/telemetry`

The legacy `/worker/<id>/telemetry` alias exists only when
`sharding.legacy_worker_alias` is enabled. Current checked-in profiles enable
the alias for compatibility, but new documentation and clients should prefer
the canonical sharded path. Exact sharding values are generated in
[root_task_manifest.md](snippets/root_task_manifest.md).

### Shard derivation

Host NineDoor, the target namespace adapter, and worker helpers use the same
deterministic algorithm:

1. Hash the exact UTF-8 bytes of `worker_id` with SHA-256; do not case-fold or
   normalize the identifier.
2. Read the first digest byte. When `shard_bits < 8`, keep its most-significant
   `shard_bits`; when `shard_bits = 8`, keep the full byte.
3. Format the resulting value as two lowercase hexadecimal digits.

For the checked-in `shard_bits = 8` profile, `sha256("worker-7")` begins with
`8a`, so the canonical telemetry path is
`/shard/8a/worker/worker-7/telemetry`. When sharding is disabled or
`shard_bits = 0`, the label helper returns `00`; a disabled layout nevertheless
uses `/worker/<id>/telemetry`, not `/shard/00/...`. This vector is suitable for
checking clients that construct canonical paths locally; clients must still
discover the active profile instead of assuming eight shard bits.

A ticket scope such as `/worker`, `/gpu`, or `/bus` is an authority prefix; it
does not replace the canonical telemetry path template generated for the role.
WorkerLora uses `/worker` because it is an AI worker role, not a device or
radio authority.

## Capability tickets

Tickets are MAC-protected capability claims encoded as:

`cohesix-ticket-<hex-payload>.<hex-mac>`

The current claim structure contains:

- role;
- budget (`ticks`, `ops`, and `ttl_s`);
- optional subject identity;
- optional mount specification;
- issue time;
- optional path/verb/rate scopes; and
- optional bandwidth and cursor quotas.

The implementation lives in
[`crates/cohesix-ticket`](../crates/cohesix-ticket/src/lib.rs). Ticket strings
and secrets are credentials; documentation, logs, and evidence must not print
their secret material.

### Attach rules

- Queen may attach without a ticket. If a Queen ticket is supplied on an
  enforcing path, it must be valid for Queen.
- Every worker role requires a ticket and subject identity.
- The requested role must match the ticket role.
- The requested worker identity must match the ticket subject when both are
  supplied.
- MAC, TTL, scope, quota, role, subject, and selected-manifest bounds are
  checked before authority is granted.
- A successful application attach binds a role-scoped control-plane session; it
  does not start or prove a target Worker task.
- A target-task attach additionally requires the role to be marked implemented
  and live cap-backed endpoint authority to match its role, slot, lease epoch,
  supervisor generation, and cap generation.

Host NineDoor verifies tickets against registered role keys. The target console
performs transport authentication first for TCP, then validates the
application `ATTACH` role/ticket. Transport `AUTH` proves access to the console;
it does not grant Queen or worker namespace authority by itself.

### Budgets and quotas

Budget defaults are role-aware. Queen uses an unbounded default when no ticket
overrides it; GPU and heartbeat-family workers use finite defaults from the
ticket library or their worker record. The server clamps applicable ticket
budgets to the worker record rather than allowing a ticket to expand authority.

Generated ticket limits bound scope count, path length, per-scope rate,
bandwidth, cursor resumes, and cursor advances. The current generated values
are documented in [ticket_quotas.md](snippets/ticket_quotas.md) and
[root_task_manifest.md](snippets/root_task_manifest.md).

Exhaustion or expiry is a refusal, not a successful no-op. The server closes or
revokes the affected authority and returns the protocol-specific error surface.

## Worker authority lifecycle

Worker authority has two distinct layers:

1. **Session authority.** A valid role, subject, ticket, lifecycle gate, and
   namespace scope authorize an operation at the control-plane layer.
2. **seL4 invocation authority.** An executable target role receives a live
   Write-only output endpoint cap whose immutable badge identifies
   `(role, slot, logical lease epoch, supervisor generation, cap generation)`
   and a Read-only lifecycle notification cap whose one-hot badges identify
   control, timeout, shutdown, and revoke. ABI action is a validated message
   label, while sequence and generation data live in bounded shared records;
   badges do not carry structured data. The Worker's active scheduling context
   binds only to its TCB, never to a notification.

The root Worker supervisor owns construction, READY admission, one-in-flight
control publication, bounded shutdown, fault teardown, revocation, and fresh
generation. A bound supervisor-wake notification coalesces the generated
heartbeat/GPU/LoRA completion bits with the critical handoff bit. The
supervisor validates the entire received mask, drains durable child records,
then drains fault records before root-control records when the critical bit is
present. The three fault mailboxes are keyed by the generated temporal Worker
ordinal, not the role-local ABI slot: the current Heartbeat, GPU, and LoRA
identities each use role-local slot zero and therefore cannot safely index a
shared mailbox array by `identity.slot`.

The fixed `worker-task-abi/v1` outcome field has the exact values
`NotApplicable=0`, `Confirmed=1`, `Rejected=2`, and `Stale=8`. The explicit
stale value extends the existing fixed layout without changing its record
sizes, magic values, or ABI version, and both GPU and LoRA receipt/completion
runtimes preserve it without aliasing it to rejection. For host-ticket/v2,
root maps `succeeded` to `Confirmed` and both `failed` and `expired` to
`Rejected`. `Stale` is reserved for an otherwise valid terminal result whose
pinned Worker identity changed or was torn down after admission. Root retains
the admitted result with an internal stale disposition and never sends that old
result to a replacement generation. A stale control or completion that does
exist is accepted only when its complete identity, action, sequence, and receipt
digests match the exact current one-in-flight record.

Revocation is bounded and explicit:

1. Policy denial, ticket expiry, budget exhaustion, lease expiry, or a valid
   Queen lifecycle operation selects the authority to close.
2. The session or worker record is marked closed/revoked with a reason.
3. Root publishes the generated one-hot lifecycle cause and waits only for the
   bounded completion/timeout window.
4. The supervisor suspends the exact TCB generation, unbinds its active SC,
   clears mappings, and revokes the role's grouped child-untyped derivations
   before returning the executable slot to the pool.
5. Subsequent namespace and console operations fail rather than silently
   continuing; logs and enabled observability providers record the transition.

Generated records prove topology and offline admission. Target acceptance
additionally proves that the selected image created, delivered, handled, and
revoked the capabilities on the named profile.

## Scheduling layers

Cohesix exposes three scheduling concepts. They must not be collapsed into one
claim.

### 1. Kernel and target-task scheduling

The default QEMU and Pi 4 profiles select four-core, one-domain seL4 SMP+MCS.
Every active temporal record owns a distinct SC, uses the SchedControl cap for
its declared core, and declares budget, period, deadline, refill bound,
priority, MCP, blocking, release jitter, WCET provenance, response time, and
admission result. `max_refills` is the total refill bound; root passes
`max_refills - 2` as the seL4 `extra_refills` argument. NineDoor is the one
passive service in this inventory and may run only on its generated bounded
donor/Reply chain.

The init TCB and initial SC are the real `root-control` domain because that
thread retains bootstrap and HAL admission authority. There is no duplicate or
idle root-control child. Four restricted root-resident children have separate
TCBs, active SCs, CSpaces, IPC buffers, stacks, timeout caps, and named duties:

- `root-fault` owns the standard and timeout receive lanes and their distinct
  Reply objects;
- `root-emergency` is the terminal fail-stop path;
- `root-worker-supervisor` owns Worker lifecycle and teardown; and
- `root-driver-supervisor` owns driver faulted-call failure and containment.

All four restricted children are constructed and registered while suspended.
Root seals the exact generated fault registry and completes the bounded
synchronous bootstrap IPC trace before any restricted child is resumed onto
its generated SC. Root-control remains on the kernel-provided initial SC
through the rest of bootstrap. Its generated budget, period, fault endpoint,
and timeout policy are applied exactly once at the selected userland event-loop
entry, immediately before steady polling; kernel construction does not arm that
temporal policy mid-bootstrap.

The root-fault CSpace receives compiler-bounded child-local TCB control caps at
the exact critical task-index slots used by its containment loop. Root-relative
registered TCB caps remain root-control records and are never invoked from the
restricted root-fault CSpace.

The QEMU and Pi manifests reserve `3000 us / 10000 us` for `root-fault`, with
a compiler-admitted candidate `2400 us` containment WCET and `2600 us`
response bound. These values supersede the original 500-us candidate: live
four-core GICv3 QEMU proved that candidate could expire while suspending an
already-faulted child. Standard-plus-timeout fault injection must still qualify
the larger candidate against its two bounded receive lanes; the terminal
timeout policy remains enabled throughout that test.

For an isolated service fault, `root-fault` suspends the exact registered TCB.
For passive NineDoor only, it then consumes the dedicated
compiler-selected recovery Reply association: an outstanding `root-control`
Call receives exactly one typed `Closed` failure, while a between-call fault
issues no Reply. The atomic ready-to-replied transition prevents double Reply,
and the active console service is rejected by the recovery-contract validator.
Root-fault next publishes one durable record into that service's generated
temporal-index mailbox. Root-control can take only the named record and perform
scrub, unmap, and retained-anchor revoke. A full or aliased service mailbox is
fail-stop; it is never treated as a dropped notification.

Critical-domain manifest slots retain permanent CNode caps so root can account
for and retain those permanent objects. They are not grouped child-untyped
reclamation anchors and must not be reported as reusable Worker or driver
donors. Worker slot anchors are separately derived from the grouped child
untyped used for that generation.

Claims about applied affinity, priority, SC consumption, or bounded response on
a target still require selected-kernel QEMU or Pi evidence, not just generated
metadata.

#### Offline response-time admission

For active task `i`, the compiler iterates the fixed-priority recurrence

`w(0) = WCET(i) + blocking(i)`

`w(n+1) = WCET(i) + blocking(i) + sum(ceil((w(n) + jitter(j)) / period(j)) * WCET(j))`

over every other same-core task `j` with priority greater than or equal to
`i`. Equal-priority peers interfere because seL4 FIFO ordering may place each
peer ahead of the task being checked. The result is `w + jitter(i)`. Compilation
fails on overflow, non-convergence after 128 iterations, a stale declared
result, or a result beyond the declared deadline.

All active tasks in both selected profiles use a 10,000 us period/deadline,
zero declared blocking/jitter, and a 1,000 us per-core reserve. The generated
QEMU budget demand and largest admitted response by core are:

| Core | Active duties | Budget demand / 10,000 us | Largest response |
| --- | --- | ---: | ---: |
| 0 | emergency, fault, control, console-network | 6,250 us | 5,000 us |
| 1 | driver supervisor, Worker supervisor | 1,750 us | 1,400 us |
| 2 | Worker GPU, Worker LoRA | 800 us | 600 us |
| 3 | Worker heartbeat | 300 us | 200 us |

The Pi profile adds only its seven admitted linked-driver tasks:

| Core | Added Pi duties | Total budget demand / 10,000 us | Largest response |
| --- | --- | ---: | ---: |
| 0 | none | 6,250 us | 5,000 us |
| 1 | serial, USB | 3,250 us | 2,600 us |
| 2 | HDMI, PCIe | 1,600 us | 1,200 us |
| 3 | GENET, CYW43, SDIO | 4,300 us | 3,400 us |

Every total remains below the 9,000 us usable per-core window. The Pi row is
offline admission only until the separate linked-driver MCS and hardware gates
pass.

#### Executable-slot resource arithmetic

Namespace/model capacity is eight identities per executable Worker role, but
the maximum live mix is exactly one heartbeat, one GPU, and one LoRA slot. Each
slot costs one TCB, CNode, VSpace, ASID, notification, standard-fault cap,
timeout-fault cap, and active SC; eight page tables, sixteen frames, 64 CSpace
slots, and 1 MiB of child untyped. No namespace count multiplies these kernel
objects.

The compiler checks `fixed + maximum live role mix + post-construction reserve`
against the selected capacity. The exact admitted totals are:

The selected seL4 16 AArch64 SMP+MCS object-size record is TCB 11 bits,
endpoint 4, notification 6, Reply 5, minimum scheduling context 7, CNode slot
5, and page/page-table/VSpace 12. Compiler admission rejects stale classic
notification or Reply sizes rather than understating MCS object memory.

| Resource | QEMU total | Pi total | Capacity |
| --- | ---: | ---: | ---: |
| TCBs / CNodes / VSpaces / ASIDs | 18 each | 25 each | 64 each |
| Page tables | 344 | 600 | 1,024 |
| Frames | 2,616 | 4,664 | 8,192 |
| Endpoints | 32 | 48 | 128 |
| Notifications | 35 | 51 | 128 |
| Standard / timeout fault caps | 18 each | 25 each | 64 each |
| Reply objects | 15 | 22 | 64 |
| Scheduling contexts | 17 | 24 | 64 |
| CSpace slots | 6,336 | 11,240 | 16,384 |
| Untyped bytes | 103,809,024 | 137,363,456 | 268,435,456 |

Allocation is fail-closed: an invalid maximum mix, aliased retention/Worker
slot, object or byte overflow, missing per-core SchedControl, partial child
construction, duplicate fault registration, or incomplete registry prevents
activation.

### 2. Root-task service turns

The event pump gives the root-owned namespace model, consoles, timers, and
driver clients bounded service opportunities. A service-turn budget limits
cooperative root work performed before yielding. It does not itself create a
Worker TCB or prove CPU isolation or real-time latency; executable
Heartbeat/GPU/LoRA tasks instead use their compiler-declared MCS TCB/SC bundles
and require target-qualified evidence.

Physical operator input and an authenticated TCP console follow the bounded
priority rules in `AGENTS.md`; nonessential mirroring and verbose output may be
reduced under load, but command acknowledgements and emergency diagnostics must
remain live.

### 3. Namespace schedule queue

`/queen/schedule/ctl` is a bounded control-plane queue. Its JSONL entries and
`/proc/schedule/*` snapshots describe requested orchestration state. They do not
directly set seL4 TCB priorities or create scheduling contexts. Exact paths and
record fields are in [INTERFACES.md](INTERFACES.md), with generated capacities
in [observability_interfaces.md](snippets/observability_interfaces.md).

## Lifecycle and scheduling evidence

Documentation must label evidence by layer and profile:

| Evidence | What it proves | What it does not prove |
| --- | --- | --- |
| Generated role table | Role selection and configured bounds | Target worker boot or invocation |
| Endpoint/notification unit test | Encoding and validation logic | On-target capability delivery |
| QEMU worker transcript | QEMU profile behavior | Pi 4 behavior |
| Pi 4 boot log with matching image | Behavior for that image and boot | A newer image or another transport |
| Schedule-queue test | Control-file parsing and bounds | Kernel scheduling or latency |
| Counter-qualified target trace | Timing for the recorded target/profile | A general performance guarantee |

## Source map and verification

- Profile inputs: [`configs/root_task.toml`](../configs/root_task.toml) and
  [`configs/root_task_pi4_uboot_aarch64.toml`](../configs/root_task_pi4_uboot_aarch64.toml)
- Generated worker tables:
  [`apps/root-task/src/generated`](../apps/root-task/src/generated)
- Ticket format: [`crates/cohesix-ticket`](../crates/cohesix-ticket)
- Shared role/ticket parsing:
  [`crates/cohsh-core/src/ticket.rs`](../crates/cohsh-core/src/ticket.rs)
- Target worker authority:
  [`apps/root-task/src/worker_authority.rs`](../apps/root-task/src/worker_authority.rs)
- Host role enforcement:
  [`apps/nine-door/src/host`](../apps/nine-door/src/host)
- Target namespace adapter:
  [`apps/root-task/src/ninedoor.rs`](../apps/root-task/src/ninedoor.rs)
- Validation workflow: [TEST_PLAN.md](TEST_PLAN.md)

Any change to a role, ticket claim, namespace scope, endpoint badge layout,
notification, or scheduling field must update manifest IR, generated artifacts,
tests, and this suite in the same authorized change.
