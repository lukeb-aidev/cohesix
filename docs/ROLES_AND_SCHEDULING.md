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

See the [Glossary](GLOSSARY.md) for Cohesix-specific role and scheduling terms.

## Generated authority

The selected profile manifest and generated root-task tables are authoritative
for Worker role records, worker count, any enabled endpoint-cap or notification
requirements, affinity, and scheduling metadata. The checked-in
default-profile values are summarized in
[the generated manifest snippet](snippets/root_task_manifest.md). Do not copy
generated badge bases or quotas into client code or hand-maintained prose.

The default QEMU and Pi 4 profiles each admit 256 executable instances: one
WorkerHeartbeat, 127 WorkerGpu, and 128 WorkerLora. All are passive children served by two active,
role-bounded executor lanes. Cap-backed authority and lifecycle records remain
per instance. WorkerBus remains a model/session-only role. A generated
executable record proves configured admission; a QEMU or Pi claim still
requires target evidence for the exact kernel, resolved manifest, root image,
Worker archive, and ABI version.

The Pi manifest retains `max_workers=256` and per-role
`namespace_capacity=256`, and its complete maximum mix declares all 256 as
executable children. Its 16-bit root CNode exposes 65,536 slots; the generated
fixed, per-Worker, and post-construction-reserve inventory consumes 19,507 and
leaves 46,029 slots of deterministic headroom. This static admission does not
prove construction, READY, driver coexistence, scheduling behavior, or
performance on Pi hardware.

## Role support matrix

The checked-in default and Pi 4 profiles currently declare the following:

| Role | Ticket and host policy | Target worker in selected profiles | Authority scope |
| --- | --- | --- | --- |
| Queen | Implemented | Root-task authority, not a separate worker image | Hive-wide access to enabled control and observability providers. |
| WorkerHeartbeat | Implemented | One admitted passive executable slot | Own telemetry and the minimal worker observability view. |
| WorkerGpu | Implemented | 127 passive executable slots per selected profile | Worker view plus its generated GPU lease scope. GPU hardware remains host-side. |
| WorkerBus | Recognized | **Not executable; session/model only** | Host/sidecar policy can describe a bus scope, but the selected target profiles must reject it as target-task authority. |
| WorkerLora | Implemented | 128 passive executable slots per selected profile | Own Worker view plus bounded AI LoRA model receipts. It receives no local GPU authority; PEFT execution remains host-side. |

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

The selected QEMU profile keeps the top six bits, so
`sha256("worker-7")[0] == 0x8a` becomes label `22` and path
`/shard/22/worker/worker-7/telemetry`. The selected Pi profile retains the full
byte and therefore uses label `8a`. When sharding is disabled or
`shard_bits = 0`, the label helper returns `00`; a disabled layout nevertheless
uses `/worker/<id>/telemetry`, not `/shard/00/...`. These vectors are suitable
for checking clients that construct canonical paths locally; clients must
discover the active profile instead of assuming one target's shard width.

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

The target namespace `Attach` preparation is the sole fallible application
authority step. After it succeeds, NineDoor commits its local role/ticket
context before best-effort audit, logger, or tracer observation; root commits
the matching session and `OK ATTACH` only after the bridge succeeds. Logger
attachment enters UART+EP mirroring without a synchronous ping/ack wait. The
optional EP-only self-test may run only from a later explicit promotion request
and cannot veto or roll back the committed namespace authority.

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
   badges do not carry structured data. The Worker's passive endpoint accepts
   an SC only from its compiler-allowlisted role executor, through one
   depth-one `Call` and its instance-owned Reply object.

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

The fixed `worker-task-abi/v2` outcome field has the exact values
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
4. The supervisor suspends the exact TCB generation, resolves its Reply path
   once, regains any donated executor SC, clears mappings, and revokes the
   role's grouped child-untyped derivations before returning the executable
   slot to the pool.
5. Subsequent namespace and console operations fail rather than silently
   continuing; logs and enabled observability providers record the transition.

Generated records prove topology and offline admission. Target acceptance
additionally proves that the selected image created, delivered, handled, and
revoked the capabilities on the named profile.

## Scheduling layers

Cohesix uses three scheduling layers. They share bounds and observability but
do not grant one another authority.

### 1. Kernel and target-task scheduling

The selected QEMU and Pi profiles use four-core, one-domain seL4 SMP+MCS.
Every active temporal record owns one generated scheduling context and declares
its core, budget, period, refill bound, priority, maximum controlled priority,
blocking, release jitter, WCET provenance, response time, and admission result.
NineDoor and every executable Worker are passive. NineDoor runs only on the
root-control donation chain after one bootstrap activation. Workers run only
on two generated active executors: GPU on core 2, LoRA plus Heartbeat on core
3. Each executor selects from a fixed bounded fair queue and donates its SC to
one exact instance at a time.

`SchedControl` remains root-only. Active scheduling contexts bind to TCBs, not
notifications. IRQ/locality-bound drivers, autonomous drains, the
console-network child, supervisors, and Worker executors therefore retain
their declared temporal owner. A passive Worker cannot borrow an undeclared
lane, nest donation, or retain an executor SC after its exact Reply/fault
boundary.

The selected timeout policy for root control and the active console child is
`NaturalPostpone`: exhausting the current refill postpones execution until a
valid replenishment. Their standard fault endpoints remain installed and
terminal; generated timeout capability identities and resources remain
reserved and accounted even though they are not installed as TCB timeout
handlers. This policy does not change client deadlines, retries, console
grammar, or fault authority.

The selected Pi profile applies the same kernel mechanism to the resumable
serial, USB, HDMI, CYW43, and SDIO physical runtimes. Their active SCs remain
independent. Serial, USB, CYW43, and SDIO retain their existing values; the
write-only HDMI damage compositor alone receives 2,000 us per unchanged
10,000 us period. Its 1,800 us candidate WCET and derived 2,100 us response are
static-admission values, not measurements of Pi execution time. The adjacent
GPU executor remains a passive-Worker donor at 5,000 us and its recomputed
static response is 7,100 us. Together with the unchanged 400 us PCIe owner,
core 2 admits 7,400 us against the generated 9,000 us usable bound and retains
the mandatory 1,000 us reserve. The HDMI command envelope is independently
fixed at 1,280 operations, 4,096 bytes, and 80 physical clear rows; a resumable
parser/plane/raster cursor bounds every retained slice by both that immutable
grant and the selected MCS reservation. The 4,096-byte field is an envelope
ceiling for parser-bound proof; the unchanged pointer-free frame transport
still limits a submitted payload to 1,536 bytes.
Ordinary current-refill exhaustion postpones execution instead of converting a
retained multi-turn device lifetime into a terminal driver fault. Standard
faults and explicit device deadlines remain terminal,
and PCIe plus GENET retain their selected terminal timeout policies.
The temporal adjustment changes neither QEMU scheduling nor driver ownership;
the shared HDMI grant values change without changing the pointer-free record
layout.

The serial throughput correction changes transport storage, not scheduling.
The existing four shared pages form two generation-bound two-page SPSC rings
with 8,128 payload bytes per direction. Only these CPU-to-CPU pages use
identically cacheable, execute-never, coherent Normal-memory mappings so their
AArch64 atomic cursors are valid; DMA, MMIO, and other driver payloads retain
their existing uncached mappings. The child retains the combined UART IRQ
lifetime and any pending handler acknowledgement; root consumes and produces
through its already-scheduled cooperative EventPump turns. No direct IRQ wake
into root is claimed, and the selected baud, FIFO, IRQ, core, priority, budget,
period, response, refill, deadline, and timeout policy remain unchanged.

The aligned SDIO/CYW43 bulk copy likewise changes no scheduling contract. It
moves bytes only between the existing shared payload and the SDIO child's
private uncached DMA4 bounce region under the existing bounded command turn.
The owners, retry ceilings, pair-restart sequencing, and device/aggregate
deadlines remain unchanged.

Retained exact-grant admission for the CYW43/SDIO pair may collapse at most
three hardware-free bookkeeping states within one already admitted child turn.
The bound covers source arbitration, one stable grant read, and only after an
`Empty` result one condition-before-sleep recheck. A persistent-source
`Service` decision stops before grant access; `Inactive` fails closed without a
recheck; `Ready` revalidates and acknowledges the immutable grant before
authorizing exactly one existing bounded physical quantum. ACK failure restores
the pre-grant gate and performs no device I/O. This removes unnecessary
`CheckWake`/`CheckGrant` scheduler edges; it does not compose physical actions,
repeat a grant, consume a second owner quantum, move a postphysical boundary,
or alter an SC budget, period, deadline, refill, priority, core, owner, retry,
or Reply/fault contract.

GENET remains on core 3 with its selected `1,000 us / 10,000 us` active SC and
terminal timeout policy. The Pi-only IRQ 189/badge 1024 default-queue DPC may
drain at most 16 frames and 24,576 bytes into its private queue per quantum;
remaining exact IRQ work retains a masked, unacknowledged continuation. QEMU
keeps its existing three-entry driver-runtime IRQ topology with no GENET IRQ
and identical scheduling. After DHCP and exact old-path quiescence, Pi GENET
performs one fail-closed handoff to the console child over the compiler-declared
32-page CPU-only direct link. GENET keeps its independent core-3 SC and sole
MMIO/DMA/IRQ ownership; the console child keeps its core-0 SC and sole
TCP/auth ownership. Their fixed notifications are wake hints, not scheduling
donation or packet authority. A peer fault couples containment only: suspend
the GENET owner and remove both cross-child signal caps before unmapping the
console copies, with no root packet fallback.

For this Pi-only production path, console-network retains priority/MCP 180/200
with its unchanged `3,000 us / 10,000 us` SC. Priority-200 root-control therefore
continues to service unauthenticated serial and local-seat input first, while the
direct packet path and existing bounded authenticated-response selection remove
steady root packet mediation without granting a pre-authentication network
preemption. Compiler response analysis records `8,100 us` for console-network
and `5,100 us` for root-control, retains the mandatory reserve, and admits the
32 additional console mapping-cap slots. These are static bounds, not measured
packet latency. Fresh same-image Pi load evidence must still prove authenticated
response cadence; QEMU priorities and placement remain unchanged.

The Pi CYW43 boot supervisor consumes those two generated response bounds only
after the DHCP-bound console child is finalized and resumed. Each bound is
rounded independently to the millisecond clock, so the current profile admits
one `9 ms + 6 ms = 15 ms` exact-generation Ready publication/observation
window. Invalid generated authority or a missing, late, or wrong-generation
Ready fails closed. This is a distinct post-activation observation bound, not a
budget increase, retry, renewal of the 90-second pre-handoff Gate 8 deadline,
or proof of packet latency.

The driver TCB constructor applies that selection at the actual timeout-handler
installation boundary and reports `timeout_policy` plus `timeout_endpoint` in
its existing bounded MCS construction record. A reserved timeout identity is
not evidence that the handler was installed.

QEMU and Pi use independently generated counter frequencies and response-time
analysis. QEMU/TCG and artificial counter modes are diagnostic comparators, not
alternate accepted scheduler profiles. Exact temporal values belong to the
selected generated tables; client code and prose must not copy them as
repository-wide constants.

### 2. Root-task service turns

Kernel budgets do not replace application scheduling. Root continues to admit
bounded logical units through its event pump, including operator input,
response flushing, timers, network handoff, driver supervision, Worker
supervision, diagnostics, and display work.

Before an authenticated TCP session exists, physical operator input is served
serial first, then local-seat USB, with HDMI as bounded feedback. With an
authenticated session, its response lane receives bounded flush priority
without starving serial/local-seat input, fatal output, timers, containment, or
ordinary service. Large responses are divided into bounded units with an
ordinary service turn between bursts.

On the physical Pi MCS path, one root-control turn may compose one complete
local-operator rotation before its explicit cooperative yield when Network is
not yet admissible or has been quarantined. Each Serial, LocalSeat, Dispatch,
and Display visit retains its existing one-operation bound; the quarantined
Network cursor is a hardware-free transition. The preflight and quarantined
rotors have hard limits of four and five polls respectively and stop early for
reboot, containment, or a completed cycle. This composition changes no SC
budget or period: the generated per-driver period gate still admits at most one
wake for each child runtime inside its period. Active physical Network service
and non-MCS profiles retain their established single-poll outer turn.

The isolated QEMU path instead composes a counter-, completion-, and
probe-bounded root-control quantum. An idle, blocked, rebooting, or faulted
quantum returns to the scheduler. A mechanical quota with durable work may
immediately re-enter the outer root loop, but all such re-entries retain one
continuous counter window. The counter guard is checked before every next leaf
operation and requires a cooperative yield, preserving the generated epilogue
and passive-call reserve before MCS exhaustion. The unchanged MCS scheduling
context remains the hard execution bound, while every inner phase, queue, fault
check, and operator-debt rule remains bounded. This QEMU service composition
does not alter the physical Pi rotor or any Pi hardware owner.

The passive NineDoor service is co-located with `root-control` on core 0 in
every checked-in target manifest. Its compiler-validated `locality_bound`
contract makes the sole permitted donor and service share one core, avoiding a
cross-core synchronous handoff while retaining the same one-Reply-object,
one-in-flight, depth-one donation and fault-containment bounds. This is a
shared QEMU/Pi control-plane invariant, not a platform-specific tuning path.
For active tasks, the same field continues to express physical-resource
locality; for a passive service it constrains every allowlisted donor core.

Notifications are wakeups rather than queues. Durable shared records carry
identity and completion; producers publish a complete record before signalling,
and consumers validate the committed identity before acting. A missing,
duplicate, stale, or uncredited record fails closed instead of creating a
hidden retry or spin loop.

### 3. Namespace schedule queue

`/queen/schedule/ctl` is a bounded namespace control queue for requested work.
It is not a kernel scheduler and cannot mint scheduling contexts, change TCB
budgets, or bypass role, ticket, lifecycle, policy, or quota checks.

The provider validates the complete record before enqueueing it, retains
deterministic FIFO order within the selected bound, and returns a typed refusal
when full. The Queen consumer removes only the exact FIFO head with the bounded
`dequeue` record defined in [External Interfaces](INTERFACES.md); empty, stale,
and out-of-order acknowledgements fail closed. Dequeue transfers responsibility
for the request but is not Worker completion evidence. Target latency claims
still require the appropriate target and benchmark evidence.
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
