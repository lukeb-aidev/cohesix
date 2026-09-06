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
fixed, per-Worker, and post-construction-reserve inventory consumes 19,513 and
leaves 46,023 slots of deterministic headroom. This static admission does not
prove construction, READY, driver coexistence, scheduling behavior, or
performance on Pi hardware.

The root CSpace allocator excludes every enabled compiler-declared service,
Worker and critical-retention anchor from ordinary allocation before any child
constructor claims it. A constructor claims its exact reserved slot once, even
after ordinary allocation has passed that address. Duplicate claims, occupied
slots, initial kernel capabilities and addresses outside the BootInfo empty
window fail closed. Image-size changes must not let an earlier constructor
consume a later service's anchor; constructor order does not confer ownership
of another service's reserved slot.

Ordinary Worker service drains its existing bounded immediate fault/policy
work, then snapshots the claimed-slot bitmap under the existing projection
lock. It checks every claimed slot in manifest order using that call's time
sample, without holding the lock across enforcement or seL4 operations.
Claims and their bitmap change in the same admission/checkpoint transaction;
terminal claims remain covered. An empty population needs no per-slot
projection lookup. Empty pending-operation selection also skips the slot scan
only after validating the generated population. Fault-mailbox inspection,
deadline cadence, nonempty pending fairness and QEMU's service quantum retain
their existing bounds.

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

The Pi root-control task also retains one notification used only as a wake
fan-in for exact durable child publications. Least-authority Write-only,
badge-1 aliases are fixed at console-network child slot 6, Pi driver child slot
12, and task-local slot 5 in both the root-fault and Worker-supervisor critical
children. Executor children reuse slot 5 only in their separate CSpaces. The
critical runtime signals its alias only after committing the durable fault,
service, Worker completion-bit, or validated Worker-control publication;
Worker control records remain authoritative in their existing bounded FIFO.
Console and driver children likewise signal their existing component-specific
notification first and the fan-in second. The badge is a coalescing hint,
never publication, identity, work credit, or scheduling authority. At an
already-fenced ordinary or no-successor exit, root may poll it once. A nonzero
badge returns to outer operator/recovery-first arbitration and a fresh durable
read; ordinary zero takes the existing bounded Yield. One exact causal cut is
different: after durable revalidation proves either an exact signal-bound
finite one-way CYW43 operation whose sequence-last terminal is still absent, or
an exact staged console response whose child-consumption or `OutputDrained`
publication is still owed, root polls once and waits only if that poll is
empty. Persistent and steady CYW43 parents retain root-polled deadlines and
cannot wait. A stable terminal or durable child publication visible at the cut
returns to the rotor instead of waiting. Attached WiFi spends its existing
one-shot outer recheck on that durable level even when the corresponding
notification has already coalesced or been consumed; a stagnant level cannot
renew the allowance. A fully fenced direct-GENET global-idle receive closes
the finished software transaction cursor and its work count before fresh
outer arbitration. A causal child wait does not close that cursor. Neither
operation resets or expands the kernel scheduling context.

Attached WiFi may also retain its existing activation after one exact NetData
op8 admission frontier advances. The same immutable request, descriptor,
generation and physical lifetime must survive preparation, command commit,
grant publication, notification and terminal retirement. Repeated readiness or
a changed identity earns no continuation. The frontier relation follows actual
observable call boundaries, including initialization that performs its first
boost or commit in the same call, and an already-visible terminal consumed
directly from grant-required into restoration. A new terminal cannot excuse an
unrelated phase jump. Consumer rank never decreases within a grant; replacement
is exactly the next grant after initial admission or confirmed consumption.
This take-once receipt charges the existing logical 64-turn bound and passes
the full operator/recovery fence;
it is not material ingress credit and creates no new service episode. Waiting
still requires the existing owed-signal proof. The ordinary ingress selector
may observe a stable current nonempty runtime DATA queue or an exact runnable
op8 continuation, with EAPOL, capacity, maintenance and ownership fences intact.
Only the authorized leaf imports/dequeues data, and only successful child
staging counts as material ingress. A resumed valid batch can deliver its first
eligible copied frame in that same receive operation without duplicating it.

After accepting the control watermark,
root may wait again only while the root-local response batch retains the same
generation, authenticated connection, and nonzero control sequence and records
control-complete without output-drained. Bare physical idle, deadline,
recovery, containment, quarantine, reboot, and operator-owned cuts grant no
causal wait authority.
The selected timeout policy for root control and the active console child is
`NaturalPostpone`: exhausting the current refill postpones execution until a
valid replenishment. Their standard fault endpoints remain installed and
terminal; generated timeout capability identities and resources remain
reserved and accounted even though they are not installed as TCB timeout
handlers. This policy does not change client deadlines, retries, console
grammar, or fault authority.

The selected Pi profile applies the same kernel mechanism to the resumable
serial, USB, HDMI, GENET, CYW43, and SDIO physical runtimes. Their active SCs remain
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
faults and explicit device deadlines remain terminal, and PCIe retains its
selected terminal timeout policy. Its completed C3 IRQ duty clears/rearms,
signals root and ACKs before blocking on its combined endpoint/notification.
Its four-refill terminal context retains the two timer-duty fragments and
enable/call carry-in; it does not Yield after the IRQ or inherit the
natural-postpone drivers' indefinite continuation. The compare remains 5,000 us, while IRQ
dispatch remains subject to the unchanged 400/10,000-us reservation.
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

The adjacent nonforeground CYW43-to-SDIO bus-link copy validates the complete
parent-payload and owner-aperture ranges before mutation. Matching actual
virtual-address alignment permits naturally aligned parent words and paired
aligned owner words; other alignment uses the existing physical byte
primitives without consulting foreground transaction state per byte. No copy
may cross the runtime ring/shared-buffer seam. Invalidate, store-barrier, and
clean ordering stays unchanged, as does the foreground sealed-parent
trace/overlay authority. This removes bookkeeping cost without adding an SC
turn, payload authority, retry, fallback issuer, or physical SDIO operation.

Retained exact-grant admission for the CYW43/SDIO pair may collapse at most
three hardware-free bookkeeping states within one already admitted child turn.
The bound covers source arbitration, one stable grant read, and only after an
`Empty` result one condition-before-sleep recheck. A persistent-source
`Service` decision stops before grant access; `Inactive` fails closed without a
recheck; `Ready` revalidates and acknowledges the immutable grant before
authorizing exactly one existing bounded physical quantum. Root-owned and
delegated CYW43-to-SDIO grants both publish a distinct high-bit admitted state
before the action, then the exact low-domain ID and their declared fan-in only
after that bounded action ends. The producer waits while the admitted state is
visible and cannot publish the next grant until the exact completion state is
durable. ACK failure restores
the pre-grant gate and performs no device I/O. This removes unnecessary
`CheckWake`/`CheckGrant` scheduler edges. It also supersedes only the former
source-level postphysical `seL4_Yield` for these generation-bound one-way lanes:
selected MCS charges Yield's complete remaining head refill, so the child
immediately re-enters the same bounded admission and blocks on its existing
local notification when no fresh exact producer/root grant exists. This does
not compose physical actions, replay a consumed grant, consume a second owner
quantum without a fresh completed grant, or alter an SC budget, period, deadline, refill,
priority, core, owner, retry, or Reply/fault contract. Ordinary synchronous
work and every genuine external-condition wait retain their prior scheduler
handoff; rejected, ambiguous, and Reply-bearing generation inputs fail closed.

After one persistent-source `Service` quantum, a healthy CYW43 DPC with an
exact outstanding steady-lease SDIO child may use the existing semantic peer
wait instead of Yield. The retained command and `CheckGrant` state remain
intact: the next admission still validates and acknowledges a fresh grant.
This exception requires no owned Reply, in-flight root/delegated grant,
foreground transaction or watermark fault, and excludes childless, RXBOUND,
capacity and recovery work. The existing final durable-condition check closes
completion/coalesced-wake races before blocking. It admits no further physical
action, resets no local continuation allowance and changes no hardware wait,
deadline, IRQ, ring, recovery or SC contract. Cold-terminal successor admission
remains a separate existing bounded rule.

The exact-743e discriminator proved one remaining retained root-causal cut:
the sequence-last CYW43-to-SDIO command was durable and its outer grant was
action-admitted, but the equal-priority SDIO owner never observed it. At the
final foreground or DPC cut, CYW43 therefore re-observes the durable peer
frontier and may use exactly one MCS `seL4_NBSendWait` only for a nonzero exact
child sequence still in `Waiting` on the `Cyw43Client` route. The syscall sends
the existing send-only slot-8 SDIO doorbell and atomically waits on CYW43's
bound read-only slot-3 local notification. The command and grant record remain
authority; a badge is only a coalescing hint, and re-prompting the same durable
command after an unrelated wake is neither a retry nor a second physical
operation. `Returned`, `Recovery`, `Invalid`, a wrong route, a zero sequence,
or identity drift performs no combined wait and returns to the existing
fail-closed path. The atomic peer prompt is MCS-only. A selected classic
non-MCS profile retains its prior local-notification wait-only behavior and
emits no additional SDIO peer signal. This changes no cap right, slot, owner,
SC numeric, refill, priority, period, deadline, retry, or Reply rule.

The foreground publication cut is earlier and separately exact. Before
committing one nonzero sequence-last fresh SDIO child, including ordinary cold
HOST_CONFIG, CYW43 captures the executing turn, parent, child, generation,
captured healthy publication environment, exact `Cyw43Client` route,
stable `Waiting` completion, issued-unknown, pair-restart, and live-recovery
fences. After the commit and required cache clean, selected MCS performs no
passive re-read, diagnostic publication, or semantic bookkeeping before one
`seL4_NBSendWait` prompts slot
8 and atomically parks on slot 3. The durable command remains the final
authority, so a recovery race after the captured decision can cause only a
redundant coalesced hint and cannot authorize physical work.
On return, CYW43 retains that same activation as a condition-driven publication
episode. While the exact sequence-last child remains `Waiting` and its exact
no-grant first-action receipt is absent, CYW43 blocks again on slot 3 without
another slot-8 send. Ordinary delegated children require strict DRSP1 because
slice two depends on root's later grant. Exact steady or persistent autonomous
children may accept any committed monotonic slice above zero because they have
no grant phase and can advance before CYW43 observes owner intake. Each
returned badge only triggers durable reclassification: that exact receipt or a
terminal returns, exact recovery unwinds, and a mixed marker, aliased grant,
identity, or malformed progress fails closed. The unchanged absolute child
timeout is sampled after a naturally returned wake; expiry takes the existing
late-terminal-first, issued-unknown recovery route and never derives time from
badge count. This
closes any number of already-active root or stale-child badges that satisfy a
wait before the equal-priority SDIO owner runs, without creating a resend,
retry, polling cadence, spin, second operation, or scheduling-number change.
Classic and every inexact, stale, or recovery-fenced publication retain the
existing one slot-8 signal and do not park at this cut. On an SDIO peer wake,
the owner stable-reads and seals the fresh one-way command before wake
telemetry, physical-source arbitration, or another outer-loop boundary; a
coalesced IRQ still wins the physical action and the sealed command remains
pending. The badge is only a hint; the durable ring and completion remain
authority, and SDIO alone validates and issues the physical operation. A
suppression-only atomic latch closes recovery that begins after the turn
snapshot; only the canonical
pair-generation scrub may clear it after private recovery authority resets.

The ordinary root-causal bridge preserves two generation domains: the sealed
root command and its root grant use the root-owned `parent.aux1`, while the
reciprocal child and its action receipt use the independently validated live
CYW43/SDIO link epoch. Cold root generation zero must not be equated with that
nonzero peer epoch. Full parent equality and live peer-epoch checks still fence
replacement; only the invalid cross-domain equality is removed.

A childless retained deadline observation also completes one ordinary root
action. Only the current turn's exact timer-slot witness qualifies; cached
deadlines and empty frontiers are not work receipts. The original deadline and
its terminal-replay boundary remain unchanged. This permits the mandatory CMD0
guard to return through the existing root fan-in without fabricating a child.

The direct-GENET root idle fence reads the linked Pi rotor's actual HDMI work.
It must not import the ordinary VirtIO rotor's post-prompt attach schedule,
which the linked Pi path does not consume. USB input/recovery, HDMI queued or
retained frames, boot milestones, retained diagnostic output, timer-enable
validation, and the complete before/after-enable race check remain fenced.
QEMU's ordinary attachment schedule and selectors are unchanged.

The reciprocal terminal boundary is equally atomic. On selected MCS, every
reply-bearing isolated-driver terminal retires its local sequence and advertises
receive-ready before `seL4_ReplyRecv` both releases the current root caller and
waits for the next command or bound notification. A committed one-way SDIO
child instead retires locally before slot-10-to-CYW43 `seL4_NBSendWait` parks
the sole owner on slot 3. Returned IPC and notification state is retained in
that activation; it is never discarded merely to enter the outer loop. The
SDIO peer prompt precedes passive terminal diagnostics and generic root fan-in,
and those secondary hints may run only after a returned wake has been
classified and any sequence-last successor sealed. A failed completion commit
enters generated standard-fault containment before retirement or any wake. An
exact-command-badged IPC whose MR0 or sequence fails admission is contained the
same way because only the independent supervisor can safely disambiguate a
possibly associated Reply object. A pure peer baton completes the scheduling
round trip without a yield; a coalesced CYW43/SDIO physical source preserves the
existing hard boundary: terminal time and pending state are captured before the
atomic wait, published after successor sealing, and followed by one source
quantum and a yield before foreground I/O. This adds no poll, retry, second
owner, or change to any budget, period, refill, priority, core, deadline, or
Reply association.

The selected physical Pi MCS root-control task in the exact direct-GENET
topology no longer forfeits a refill at the proved ordinary global-idle cut.
Activation binds its existing compiler-
owned fan-in notification to the init TCB, and the root endpoint uses its
existing explicit Reply object for both nonblocking and blocking receive.
Descriptor replay leaves the PCIe-owned timer disarmed in every network mode.
After a completed empty quantum, root consumes exactly one nonblocking
multiplexed receive. If that receive is empty and timer, endpoint, durable
producer, serial/local-seat/display, network, fault, recovery, containment,
quarantine, and reboot predicates remain idle across the complete pre-enable
condition-before-block fence, root issues one synchronous Reply-bearing typed
enable Call to the PCIe owner. Root accepts only the matching completion and
durable `Enabled` publication, repeats the complete fence, and only then blocks
on the endpoint. Later idle entries use
that lifetime-bound `Enabled` record and never reprogram the timer. The record
must also have a currently published endpoint/capability generation, open MCS
command admission and idle or associated Call phase; terminal containment
phases are rejected. The boot owner-state mask is not live
execution proof. Root-fault forwards the driver's validated containment-release
notification through its existing root fan-in cap, so an earlier fault hint
racing the final idle snapshot cannot strand root behind a dead timer. An
endpoint Call and a bound-notification signal are multiplexed by the kernel;
the latter is only a hint, while the former is copied to staged root storage
before returning. Every wake restarts operator/recovery-first arbitration.
Post-response, reserve, operator, recovery, and fault exits retain their
explicit handoff, so the receive cannot speculate about a future request or
weaken physical-console priority. Ready WiFi may use the same global-idle
receive only when its physical one-way owner reports Idle, with no retained
command, root-polled deadline, recovery, or child publication. Its full
operator/output/network fences and lifetime-bound timer proof are identical.
An in-flight WiFi operation continues to use its separate causal wait.

The global-idle operator predicate requires no remaining USB readiness or
recovery service debt. It is intentionally stricter than the compact network
response fence, which may cross that debt after giving the operator rotor its
bounded turn. Healthy command-ready empty polling contributes no readiness
debt, so this correction does not permanently suppress the timer-backed wait.

Deadline completeness comes from the existing isolated `driver-pcie` task,
without another task or SC. Its unchanged `400/10000 us`, `wcet_us=300`,
priority/MCP, core, and terminal timeout contract owns one additional exact BCM
system-timer C3 duty: discontiguous page `0xFE003000`, level IRQ 99, badge 2048,
handler slot 4, local notification slot 3, and 5,000-us interval. Descriptor
adoption publishes `Disarmed` without programming C3. The first exact
direct-GENET enable programs it once; each bounded IRQ turn clears and
self-rearms C3, signals the existing root fan-in, then acknowledges. After that
IRQ service, the owner immediately re-enters its existing combined command-
endpoint/local-notification receive, so a Reply-bearing command or the next
real timer edge reaches the owner without a generic post-IRQ scheduler
traversal. It performs no PCIe operation, retry, catch-up, poll, or root work.
The ten-page PCIe aperture and one-page timer resource remain separately tagged
and fail closed before MMIO on any identity drift. This secondary duty consumes
only the existing isolated task's declared execution budget. The Pi profile
declares four refills: one active head, the two unexpired service fragments
from 5-ms duties in a 10-ms period, and enable/call carry-in. The selected seL4
kernel delays the existing tail when a refill queue is full, so two refills
cannot preserve this repeated blocking pattern. The completed IRQ duty blocks
without a full-head `Yield`; that syscall's entry budget check can raise the
terminal timeout before its nonfaulting retirement body executes. Additional
PCIe command/recovery pressure still requires exact-image hardware evidence;
refill capacity does not increase CPU authority or waive timeout containment.

The deferred physical WiFi supervisor may retain root-control across productive
logical turns only when generated root truth is active, admitted,
consumed-time capable, and selects `NaturalPostpone`. It alternates exactly one
Operator and one Driver, checks the profile and caps before both, and retires
each one-operation Driver finalizer before re-entry. Kernel exhaustion
postpones and resumes that exact instruction cursor; userland no longer
forfeits the remaining refill at a wall-time `budget_us - wcet_us` estimate.
Every full Operator, Driver, and attached Network service turn spends one of
the unchanged 64 logical material-work units, while productive Driver or
attached Network progress also remains capped at 64. Missing or incompatible
generated policy permits one legacy logical turn before yielding, preserving
liveness. After service readiness, an attached Network turn may retain the
same window only
when EventPump proves actual network activity and exactly one CYW43
service-unit advance. The outer 25 ms quantum cap accumulates only admitted
Network-service intervals; replenishment gaps, exact-child waits between
turns, and operator phases do not consume it. A separate 25 ms real-wall
physical-operator checkpoint continues across those gaps, and the generated
192-turn cap remains the absolute work bound. The ordinary continuation
requires an immediate Network successor and durable schedulable physical work.
An active response cursor may
instead complete exactly `Network -> Serial -> LocalSeat -> Dispatch ->
Network`: the nonzero active/authenticated connection, one-step cursor
decrement, service/flush deltas, unchanged accepted-command count, and
generation/pair/lifetime rotation token must all agree. The rotation admits
LocalSeat exactly once, performs one backend poll only when USB service debt
exists, clears the token at Dispatch, performs no second Network operation in
that wrapper, and selects only the next separately charged bounded Network turn
after the caller rechecks the NaturalPostpone profile and 64-unit caps. Real
physical
operator state remains typed across this post-Dispatch cut: `UsbServiceDebt`
requests that one bounded `LocalSeat` opportunity and then releases the Network
fence, while runnable decoded or buffered `Input` retains it. Queued USB bytes
behind the unchanged parser-readiness/recovery gate are `UsbServiceDebt`, not
runnable input: they remain retained, get the bounded LocalSeat opportunity,
and cannot indefinitely exclude independent HDMI and Network after Dispatch.
Serial input, an active parser chunk, and a partial command retain their existing
precedence. The transient-publication
probe preserves that type and may carry passive USB debt only because every
minted credit installs the complete mandatory operator rotor before Network
re-entry; real Input still fences. A terminal-return shortcut remains strict.
If EventPump consumes exactly one existing CYW43 child wake after the first
Network unit, a stack-local token may survive only the complete
Serial/LocalSeat?/Dispatch rotor. Exact physical lifetime, authenticated
response identity, accepted-command epoch, durable work level, and every
operator/recovery/fault fence are revalidated at return to Network. The wrapper
does not execute a second Network unit, and Yield destroys the token. Prior
progress followed only by an unchanged empty snapshot yields; it cannot retain
an eventless polling cadence. The schema-stable `idle_admitted` field remains
zero for this path.
Physical input or response, recovery, quarantine,
containment, reboot, stale identity, idle, wait, nonprogress, handoff, pressure,
fault, or any failed token yields and resets. The selected SC and
natural-postpone policy remain the hard execution boundary. This replaces the
former pair-restart Driver burst and does not apply to QEMU or generic root
control.

During CYW43 bootstrap, an admitted and ready USB controller with
`controller_ready && !command_ready` is itself bounded `LocalSeat` service debt,
including before ordinary keyboard polling is enabled. That bootstrap debt
stops at command readiness and changes no CYW43 prerequisite, budget, retry, or
physical-owner authority.

After command readiness, a physical Pi keyboard that is keyboard-ready, has a
valid first report, and has no buffered input, recovery/no-reply debt, or exact
retained request keeps the ordinary `LocalSeat` rotor topology but issues its
healthy empty USB child operation at most once per shared 25-ms real-wall
counter cadence. Enumeration, readiness, recovery, buffered input, and retained
work remain immediate. First-byte evidence is not a readiness fence: an
untouched keyboard may legitimately have none, and the next cadence poll still
discovers a key within 25 ms plus the existing bounded MCS scheduling delay.
Buffered input then bypasses the cadence, preserving physical operator priority
while releasing healthy empty child turns to serial, HDMI, dispatch, and
network progress. This changes no SC budget, period, priority, owner, queue, or
retry bound.

The Pi direct-GENET-feature authenticated console's isolated TCP socket,
shared by the selected WiFi and wired modes, retains the pinned smoltcp stack's
10 ms ACK timer and disables Nagle. A prompt response carries the receive ACK
without a separate packet/publication round trip; a missing response still
leaves a due ACK obligation. Response data waits for neither that timer nor an
earlier unacknowledged segment. This changes no listener, queue, transport owner,
or scheduling authority. QEMU retains its already-qualified TCP policy.
While direct GENET awaits the root command control, its command latch still
suppresses background service. A due child-owned protocol timer, or its queued
egress once the peer can accept it, admits at most three existing service
units. Both pre-wait and post-wait gates use that same decision; the command
latch remains closed until a newly sequenced applied root control arrives.
This preserves ACK/retransmit/close obligations without reopening the ordinary
64-unit quantum. A blocked TX frame remains retained for a peer wake.

QEMU direct-VirtIO retains the strict lower rotor: `ObserveChild`,
`StageOutput`, `Disconnect`, then `ServiceTick`. Direct GENET alone selects an
exact ready `StageOutput`, then an exact ready `Disconnect`; when neither is
ready it alternates `ObserveChild` and `ServiceTick`, exactly one unit per
Network visit. On Pi, root-control remains on core 0 and console-network runs
independently on core 2 at equal priority. Both Pi backends use only the
post-commit Release plus one-hot Signal handoff; neither pre-drains the child SC
nor calls `SchedContext_YieldTo`. Direct GENET may retain the same causal
activation after an exact stage only by binding and revalidating the sealed
child control's exact nonzero sequence while it still owes its consumption
watermark, then performing the condition-before-block Poll/Wait cut described
above.

Copied CYW43 console-network visits retain deferred diagnostic and already
retained egress priority, then select exact ready output, a drained disconnect,
child publication or owed publication ACK, and current-generation root-copied
RX with available ingress and response capacity. Each visit still executes and
charges one existing unit. When no exact work is ready, observation, ingress,
and timer probes remain bounded; an empty output or disconnect slot does not
consume a visit. The receive leaf revalidates all driver admission gates; this
ordering cannot issue CYW43/SDIO source work, retain a paired RX/TX permit across
actors, or mint a new continuation. The existing child-signal successor,
operator rotation, NaturalPostpone profile, and 64-unit cap remain in force.

Exact durable cross-core direct-GENET transaction identity may retain
root-control under the generated NaturalPostpone profile and unchanged
64-complete-quantum cap only for the current authenticated request. A
stage-bearing continuation binds the exact generation, connection, and
nonzero one-slot child-control sequence; generation and connection alone
cannot match a later sequential publication. The condition-before-block fan-in
may wait while that exact control owes its child-consumption watermark and,
after the watermark is accepted, while the same exact response batch still
owes `OutputDrained`; no durable child publication may already be visible. If
the child wins either race, root returns to outer recovery/operator-first
arbitration without Yield. Every quantum rechecks passive admission, physical operator input or
response, display debt, recovery, fault, containment, quarantine, reboot,
handoff, identity, final Serial phase, and the shared cap; the root SC remains
the hard CPU bound. Fused stage-and-drain or `OutputDrained` retires the current
request. A fresh unconsumed child publication may admit one Serial-first
recheck after the same lane and operator/recovery fences pass. The old token
is removed while clock, causal-wait count and 64-quantum cap remain intact;
an executed empty recheck consumes one quantum and stops. Only fresh
productive progress creates another continuation. An absent publication
retains the ordinary idle/Yield route and cannot mint a post-response Network
baton, empty hot tail, broad wait or future-request authority. A queued next
command remains behind the ordinary operator rotor. CYW43 keeps its distinct
NaturalPostpone/64-unit contract and cannot acquire direct-GENET continuation
authority.

Cold, attached, and steady Pi WiFi root-control use the fan-in only at an
already-fenced current finite-operation or causal-child exit. One nonzero poll result returns to outer
operator/recovery-first arbitration, where the authoritative CYW43, GENET,
Worker, serial, USB, console, and fault state is re-read. Ordinary zero takes
the existing bounded Yield. An exact signal-bound finite one-way CYW43
operation awaiting completion, or an exact staged console control whose child
watermark remains owed, may instead wait at most 64 times inside the unchanged
activation bound. Persistent/steady CYW43 parents cannot wait. Root revalidates
that no stable terminal or child-frontier publication is visible, polls once,
waits only on an empty result, then
returns through the full outer durable-state rotor. WiFi has no generic
blocking idle cut, software work latch, poll loop, retry, fallback owner, or
SDIO deadline-arm side effect. Its transaction waits and the distinct exact
productive direct-GENET path use the generated NaturalPostpone profile and
unchanged 64-unit caps.
Direct GENET has no broad idle wait or post-response cross-core tail. Every SC
numeric, Reply rule, isolation boundary, queue, and owner remains unchanged.

Runtime ABI v13 keeps reply-consuming bootstrap work on MCS Call/Reply, so the
root activation is donated directly through the first descriptor or QEMU smoke
completion. A genuinely asynchronous generic one-way Pi command instead uses
its child notification once. If one bounded action remains pending, the child
commits the exact `DROW` request, action, runtime identity, and next-slice
record, atomically signals the shared root-control fan-in and parks on its bound
local notification. Root may publish `DROA` and signal that same runtime only
after revalidating the current ring and capability generation. The badge is
never continuation authority; the exact durable acknowledgement is. This
closes the useful root-to-child-to-root scheduling seam without a spin, poll
loop, retry, fallback owner, larger work cap, or any SC numeric change.

A parsed passive-service command may survive one expired strict reserve lease
only by crossing a completely new explicit Yield/refill. The new attempt starts
from `AwaitingYield`, drains fresh Consumed evidence, rechecks the exact command,
session, connection, recovery, containment, quarantine, reboot, and fault
identity, and still requires elapsed wall strictly below 3,000 us. A second
expiry emits the existing single typed refusal. No sliding window, same-refill
resample, or unbounded retry is authorized.

The handoff-to-Ready response bound is evaluated in the isolated child's
absolute CNTVCT millisecond domain, not the pump-driven HAL policy clock. Root
samples immediately before and after resume, requires a nonzero unchanged
frequency equal to generated child truth and a nondecreasing cursor, admits
publication from the inclusive pre-resume sample through strictly before the
post-resume-plus-bound deadline, and fails closed on zero, pre-resume,
at-boundary, late, drifted, backwards, or overflowed evidence. The pre-resume
lower bound deliberately includes a child that publishes after resume but
before root returns.

Pi GENET uses core 1 with a selected `3,000 us / 10,000 us` active SC, eight
refill records, priority 160, and natural-postpone timeout policy. This isolates
the production wired path from the CYW43/SDIO pair on core 3 without changing
either Wi-Fi SC.
The selected Pi topology admits exact active demand
`8,750/8,250/8,400/8,000 us` on cores 0--3 against each 9,000-us usable
capacity, leaving `250/750/600/1,000 us` beyond the mandatory reserve. GENET's
existing exact 800 us WCET yields a 3,400 us computed response bound; that is
static admission truth, not measured packet latency. Before direct handoff, the
Pi-only IRQ 189/badge 1024 legacy/default-queue DPC may drain at most 16 frames
and 24,576 bytes into its private queue per quantum;
remaining exact IRQ work retains a masked, unacknowledged continuation. QEMU
keeps its existing three-entry driver-runtime IRQ topology with no GENET IRQ
and identical scheduling. After DHCP and exact old-path quiescence, Pi GENET
performs one fail-closed handoff to the console child over the compiler-declared
32-page CPU-only direct link. GENET keeps its independent core-1 SC and sole
MMIO/DMA/IRQ ownership; the console child keeps its independent core-2 SC and
sole TCP/auth ownership. Their fixed notifications are wake hints, not
scheduling donation or packet authority. A peer fault couples containment only: suspend
the GENET owner and remove both cross-child signal caps before unmapping the
console copies, with no root packet fallback.

The direct GENET owner treats continuously durable packet slices as one MCS
software episode, not as fresh activations. Each guard sample admits at most
one material
TX or RX operation; successive slices alternate their first choice, an empty
side donates its slice, and continuous bidirectional pressure receives an exact
8/8 split inside the 16-slice cap. Retained ambiguous cursor reconciliation
consumes the same bound. The owner may re-enter only below the half-budget
elapsed guard and 16-attempt cap, with one bounded no-progress retry. Re-entry
stays inside the current notification or final-prewait handler; it cannot
escape through generic command-ring arbitration between packet slices. Only
an owned TX/RX state advance is productive; unresolved cursor reconciliation
still consumes its slice but cannot reset the no-progress retry. A successor
that crosses the guard yields and returns to outer arbitration. A final slice
with no durable successor blocks before guard/cap/stalled Yield, so quiescence
cannot charge the remaining refill. That exact empty-ring/rearmed-source cut
closes only the userspace episode start, attempt count, and stalled-retry bit;
a later IRQ or reciprocal peer edge begins a new episode on the unchanged
kernel SC. Any endpoint command forces a later `seL4_Yield` freshness boundary,
and quiescent closure cannot clear it; continuous durable work retains the
existing guard, cap, and stalled decisions. The compiler and generated profile
require exact
`wcet_us=800`; the handoff and runtime validate exact max-eight-refill
`3,000/10,000 us` truth. Sustained legal work that consumes that reservation is
postponed by the kernel until replenishment rather than classified as a device
fault. Counter failure, cursor drift, or contract drift
contains the direct generation. The packet-slice duration high-water and
dense-window reason fields observe these decisions but do not supply
scheduling authority or target acceptance.

CYW43 and SDIO remain separate active core-3 runtimes at priority 184 with
unchanged `1,500 us / 10,000 us` budgets, natural-postpone behavior, WCETs,
Reply/fault paths, and sole physical-owner boundaries. Their 8-bit scheduling
contexts now retain eight refill records rather than two. The selected seL4-16
AArch64 MCS ABI holds ten records at that object size, and compiler validation
binds the calculation to the exact selected kernel/profile/build identity and
rejects eleven. This preserves fragmented wake eligibility; it does not enlarge
CPU budget or authorize another operation, signal, poll, retry, or owner.

For both Pi network modes, root-control selects core 0 and console-network
selects core 2 at priority/MCP 200/200. Root retains
`5,500 us / 10,000 us`, max-two-refill scheduling, and exact 2,500-us WCET;
console retains its unchanged `3,000 us / 10,000 us` budget/period, 3,000-us
WCET, and eight refill records in its existing 8-bit SC. Root-fault returns to
core 0 without any budget, period, priority, fault, Reply, or ownership change.
Direct GENET, mediated WiFi, and QEMU direct-VirtIO all retain signal-only child
handoff; only the exact Pi causal wait described above may bridge a finite
post-commit publication boundary.

The compiler records exact root/console response bounds `5,100/3,000 us`,
HDMI/PCIe/GPU-executor bounds `5,200/3,300/8,300 us`, root-fault at 2,600 us,
the unchanged complete per-core demand `8,750/8,250/8,400/8,000 us`, and
mirrored service/task affinity. Adjacent drift in budget, WCET, response, core,
sched-control core, priority, refill count, or mirrored configuration fails
closed. QEMU retains its selected core-0 9,000-us root, core-2 lower-priority
console child, max-two-refill contracts, affinities, and non-YieldTo
direct-VirtIO behavior. These are static admission bounds, not measured packet
latency. Fresh same-image Pi load evidence must still prove authenticated
response cadence.

Exact durable cross-core direct-GENET productive identity may retain root under
generated NaturalPostpone up to the unchanged 64-quantum cap only while the
current authenticated request has exact productive work. A stage-bearing
token additionally binds the nonzero one-slot child-control sequence and may
use one condition-before-block wait only while that exact control still owes
its child watermark. The root SC remains the hard CPU bound, and every admitted
quantum rechecks generation, connection, final Serial phase, passive admission,
physical operator/response priority, display debt, local fault, recovery,
containment, quarantine, reboot, handoff, and the shared cap. The current fused
stage-and-drain or `OutputDrained` terminal retires its request. A fresh durable
child frontier may justify one separately charged ordinary recheck with the
old token removed and shared cap retained, as specified above. No
post-response baton, cross-core empty tail, broad wait, second packet
operation, retry, refill, or owner authority survives for a future request.
Mediated WiFi cannot acquire direct-GENET continuation authority.

The Pi CYW43 boot supervisor consumes the root and console generated response bounds only
after the DHCP-bound console child is finalized and resumed. Each bound is
rounded independently to the millisecond clock, so the current profile admits
one `6 ms + 3 ms = 9 ms` exact-generation Ready publication window. Root may
take one final shared-page-only observation at or after the boundary and accept
it only when the retained ABI-validated publication time is strictly earlier.
Invalid generated authority or a missing, at-boundary, late, replayed, or
wrong-generation Ready fails closed. This is a distinct post-activation bound, not a
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

On the deferred physical WiFi path, accepting a partial USB command line routes
one bounded `Dispatch -> Display -> Serial` presentation successor before
Network, retaining the exact CYW43 parent and operator fence. A pending reboot
acknowledgement or physical response tail keeps immediate Serial priority and
leaves the HDMI echo queued. This is presentation ordering, not a new USB,
display, child, or scheduling budget.

On the physical Pi MCS path, one root-control turn may compose one complete
local-operator rotation before its explicit cooperative yield when Network is
not yet admissible or has been quarantined. Each Serial, LocalSeat, Dispatch,
and Display visit retains its existing one-operation bound; the quarantined
Network cursor is a hardware-free transition. The preflight and quarantined
rotors have hard limits of four and five polls respectively and stop early for
reboot, containment, or a completed cycle. This composition changes no SC
budget or period: the generated per-driver period gate still admits at most one
wake for each child runtime inside its period. Active physical Network service
and non-MCS profiles retain their established single-poll outer turn; the
separately guarded deferred WiFi bootstrap window above retains logical
one-operation/finalizer turns without requiring a kernel Yield between each
productive pair.

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

On the selected physical-Pi cross-core signal-only console topology, observed
TCP Connected/Disconnected edges bracket kernel Consumed evidence for
root-control, console-network and GENET, or root-control, console-network,
CYW43 and SDIO, plus serial, USB, HDMI and PCIe on either lane. Root invokes only its retained root/console SC caps. A bounded
request in the existing shared root critical-domain state precedes one signal
through the generated driver-supervisor capability. Driver-supervisor drains
fault work first and samples at most one selected, admitted driver SC from its
own CSpace per wake. The finite physical-driver remainder uses its existing self-signal;
there is no timer, retry of a claimed sample, new capability, device action or
fault Reply use. A fault turn suppresses diagnostic work. Missing or contended
samples remain incomplete, and capability-generation drift invalidates a pair.
Existing root Consumed drains contribute to a cumulative evidence sum without
changing their returned values or their passive-admission order. Each sampling
syscall has entry/return virtual-counter timestamps, since the selected kernel
may stall a remote SC while reading its accounting. Owner intervals are
asynchronous and do not identify an individual packet or prove refill
exhaustion. Consumed clears evidence, not budget, and the kernel's current
per-core accounting can enter a later receipt. QEMU and retired YieldTo
comparison topologies do not start these samples.

Physical-Pi serial, local-seat, and TCP ingress always admits bounded raw input,
echo, parsing, and root-owned diagnostics while NineDoor is attached. Only a
parsed command that can enter passive NineDoor validates the generated active
root-control budget, WCET, period, admission, and consumed-time-evidence
contract. `seL4_SchedContext_Consumed` resets evidence but does not replenish
the SC. More precisely, the selected syscall clears the scheduling context's
stored `scConsumed` evidence but cannot clear the live per-core `ksConsumed`
accounting. The exact parsed command and its authority identity are retained
after one baseline sample that validates this interface. Root then performs
the sole selected periodic MCS Yield. Immediately when that Yield returns, the
first userland operation captures `CNTVCT_EL0`; the following Consumed call
drains the preserved pre-Yield accounting once, and its value is deliberately
not used as resumed-activation evidence. One cheap side-effect-free recovery
check preempts and cancels an already-faulted reservation. A healthy retained
command runs before ordinary material containment probes, refreshes the policy
timebase, and prepares authority, environment, and recovery truth before the
final admission sample. That sample closes the strict wall interval from the
immediate post-Yield counter capture to the final admission/WCET cut.
`CallArm` retains the final fault frontier. Passive dispatch is admitted only
when that wall lease is strictly below the checked
`budget_us - wcet_us` limit, currently 3,000 us. This bounds root preparation
against the configured envelope; it does not prove that the current head
refill contains the declared 2,500 us WCET. Yield relinquishes only one head,
and Consumed cannot reveal the remaining refill queue. The strict comparison, direct dispatch,
and bounded response/epilogue are all inside that declared WCET; no mutable
validity or policy work may be inserted between the cut and dispatch. Equality
or excess ends that lease without dispatch. The first expired lease retains the
exact command for one completely new `AwaitingYield` attempt; a second expiry
emits the existing single typed refusal. No within-refill resampling or sliding
wait is permitted. Reboot, containment,
quarantine, recovery, or authority drift cancels before dispatch and projects
a refusal only when the original response authority is still valid. Invalid
generated truth, zero or drifted timer frequency, backwards/missing counter
evidence, invalid period conversion, or failure of the validation/drain
accounting samples latches admission closed and emits one bounded operator
marker. Exact cross multiplication preserves the strict microsecond boundary
without a rounded-down admission; preemption only makes the wall lease more
conservative.
The Pi schema-1.17 `resume-once-return-error` policy covers this donation
boundary. Root-fault may send one zero-label, zero-length kernel Timeout Reply
for each monotonically numbered, currently armed NineDoor Call. It requires
the exact service fault registration, timeout label, two-word payload, sole
root-control SC badge and ready recovery lane. It resumes the same instruction,
retaining the original request, service Reply object, SC and generation; it
never restarts a request or reconfigures budget. The kernel postpones execution
when the donated SC is not ready. A second timeout for that Call, standard
fault, malformed payload, wrong donor or missing/stale Call follows the existing
typed caller failure and terminal containment. Timeout MR1 is retained evidence,
not a per-Call CPU limit. The resume has a finite per-Call bound and retains
kernel reservation enforcement; it does not establish the candidate WCET or
promise immunity from arbitrary service defects. QEMU retains `return-error`.

Unrelated raw input and root-owned diagnostics remain live before a passive
command is retained. The QEMU direct-VirtIO branch exits before this Pi-only
boundary and keeps its existing counter guard.

The recovery samples on this hot boundary are complete-service questions, not
task-discovery operations. Root samples the already-published raw fault badge,
the intermediate service-fault flag, and the two-slot final handoff once with
Acquire ordering. It never scans the generated Worker population to rediscover
the two fixed services and never caches a no-fault answer across turns.
Ambiguous or contended handoff state remains recovery-pending. This keeps the
recovery-first and post-sample `CallArm` fences exact while preventing
population-scaled task discovery from consuming the 3,000-us passive reserve or
multiplying every deferred WiFi supervisor turn.

The passive NineDoor service is co-located with `root-control` on core 0 in
every checked-in target manifest. Its compiler-validated `locality_bound`
contract makes the sole permitted donor and service share one core, avoiding a
cross-core synchronous handoff while retaining the same one-Reply-object,
one-in-flight, depth-one donation and fault-containment bounds. This is a
shared QEMU/Pi control-plane invariant, not a platform-specific tuning path.
Failure to arm or complete a passive Call is terminal generation evidence with
exact `CallArm` or `Call` stage. The generation is revoked and fenced; a
root-fault `REPLIED` state is never reset or retried as a normal service call.
For active tasks, the same field continues to express physical-resource
locality; for a passive service it constrains every allowlisted donor core.

Notifications are wakeups rather than queues. Durable shared records carry
identity and completion; producers publish a complete record before signalling,
and consumers validate the committed identity before acting. A missing,
duplicate, stale, or uncredited record fails closed instead of creating a
hidden retry or spin loop.

The copied Pi console child may use one publication credit for one ready
Command/CommandBatch event plus one already-prepared egress packet. Packet
commit precedes the event commit, which is the barrier for both pages; only
the event notification and root fan-in are signalled. Root validates and
retains both before its single ACK. Watermark-only wakes cannot admit a partial
bundle. This is bounded publication work, not another service cycle or a new
SC grant; direct GENET, QEMU, lifecycle events, owner locality and all generated
budgets retain their existing paths.

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
