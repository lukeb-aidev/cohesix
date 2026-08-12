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
steady-state passive service in this inventory and may run only on its generated
bounded donor/Reply chain. Manifest `root_task.schema = "1.11"` also admits
exactly one fixed, root-retained bootstrap SC for that service; it is not a
steady temporal-task SC. The same schema adds the profile-scoped
`virtio_operator_serial_io_bytes_per_turn` temporal field.

The init TCB and initial SC are the real `root-control` domain because that
thread retains bootstrap and HAL admission authority. There is no duplicate or
idle root-control child. Four restricted root-resident children have separate
TCBs, active SCs, CSpaces, IPC buffers, stacks, timeout caps, and named duties:

- `root-fault` owns one compiler-generated blocking fault receive endpoint and
  its single serialized Reply object; the exact badge resolves standard versus
  timeout class only after receive;
- `root-emergency` is the terminal fail-stop path;
- `root-worker-supervisor` owns Worker lifecycle and teardown; and
- `root-driver-supervisor` owns driver faulted-call failure and containment.

All four restricted children are constructed and registered while suspended.
Root seals the exact generated fault registry and completes the bounded
synchronous bootstrap IPC trace before any restricted child is resumed onto
its generated SC. Root-control remains on the kernel-provided initial SC
through the rest of bootstrap. Its generated budget, period, fault endpoint,
and timeout policy are applied exactly once at the selected userland event-loop
entry; kernel construction does not arm that temporal policy mid-bootstrap.
After the steady SC and timeout endpoint are installed, root-control performs
one universal MCS `seL4_Yield` before any containment probe or ordinary phase.
That one-time activation seam sacrifices the partially consumed initial refill
and waits for the next replenishment, so the first Operator receives the same
per-phase accounting as every later phase. It is not an inner phase yield and
does not add an SC, budget, refill, capability, or authority. Pi incurs the same
one startup-period wait as QEMU.

The QEMU and Pi manifests assign `root-control` a `2750 us / 10000 us` active
SC, a compiler-admitted `2500 us` per-phase WCET with provenance
`m26e-qemu-root-exclusive-predispatch-candidate-v23`, and a
`5100 us` per-phase response bound.
They assign the active `console-network-service` child `3000 us / 10000 us`, a
`2400 us` WCET with provenance
`m26e-qemu-console-bounded-stack-steps-candidate-v6`, and a `7500 us`
response bound. Every numeric timing and core-reserve value in the current
v23 root-control, V6 root-fault, and v6 console-child slice is unchanged.
That response-time result is scheduler admission for one runnable phase, not
end-to-end host/TCP latency. The former
v2 whole-turn interpretation was live-falsified: after the uncached cache
operation was removed and the budget was raised, four-core QEMU again consumed
the complete root-control refill at the final console-network control wake.
The compact-page live run at source `00bf02540` then reached the root prompt but
timed out `root-control` at the console-network control poll, disproving the v3
phase that combined Network and runtime-IPC work. Source `4d1a47b89` retained
three exclusive outer calls but then exhausted the root-control refill in
`VirtioTxToken::consume` after queue notify, falsifying v4's multi-unit Network
turn rather than proving a device-completion wait. The subsequent live v5 run
falsified two further whole-turn assumptions before authentication:
`console-network-service` consumed exactly `3000 us` and raised timeout badge
`0x26ee0007` at `Send` after `publish_exchange`, then `root-control` consumed
exactly `2750 us` during containment/quarantine and raised timeout badge
`0x26ee0001` at the sole outer `seL4_Yield`. A later canonical v6 run used root
ELF SHA-256
`0059fd675b476106888d6ca62c8bba21f9b340b9aa607e000fbf96997fd29900`.
The child remained healthy and no Recovery ran, but `root-control` again
consumed exactly `2750 us` and raised `0x26ee0001` at the outer yield. Its
preceding Network turn attempted an empty ObserveChild, no-op StageOutput and
Disconnect, then committed and signalled the first 60-byte ARP ingress as
sequence 1. This falsifies v6's composition of no-op lower units, not the child
or Recovery. The canonical v7 root ELF SHA-256
`d2f69bddbf56deef6919ec6ea802e9d3c44a691c2dbe05aa59428854bbf7a6ae`
then reached the startup command list while the UART-visible
`[mark] root-console.start.ok` record remained queued. The absent wire marker
does not mean source had not crossed the console lifecycle boundary. Before any
ordinary Network or Recovery phase, `root-control` consumed exactly `2750 us`
and raised current-fault `Timeout`, badge `0x26ee0001`, with fault PC `0x43e84`
in the serial queue's `inner_dequeue` and LR `0x77b74` in
`SerialPort::flush_tx_unlocked`. This falsifies unbounded serial-poll/flush
composition in one Operator turn; it does not falsify the v7 Network cursor,
child loop, or Recovery cursor because none had executed.

The canonical v8 root ELF SHA-256
`5052e7a5070987c252d3c1f5cf6f27172bd5ece1836a8f6c2a5c329c789a0a61`
then ran with the generated `64`-byte serial credit but exhausted the full
`2750 us` root-control refill and raised current-fault `Timeout`, badge
`0x26ee0001`, at PC `0xede84` immediately after `emit_prompt_now`. The byte
credit remained necessary, but v8 is falsified because it still allowed more
than one retained output-record attempt to compose inside one Operator turn.

The canonical v9 root ELF SHA-256
`fa488c9367136f0eadef7182a18691664c3ae51c2ac2974e12000ff5d27f38ed`
and CPIO SHA-256
`aca549e99e0d86299e9f98348d896b730259277654544ebd22a74595b61e9bfb`
then retained the one-record rule. The bootstrap SC completed the direct
command list; at the first post-bind Operator, serial was idle and both the
queued `[mark] root-console.start.ok` and retained initial prompt remained
pending. Root consumed the complete `2750 us` refill and raised current-fault
`Timeout`, badge `0x26ee0001`, at PC `0x13a798`, the first instruction of
`compiler_builtins` `memmove`. LR `0x79ccc` was
`heapless::Vec<PendingConsoleOutput, 72>::remove(0)` and `x2 = 0x110` described
the prospective 272-byte move, but no byte was copied and the output-record
cursor remained full. This is aggregate first-post-bind refill exhaustion, not
proof that the move itself caused the timeout. V9 is falsified while its
one-record admission remains intact.

The canonical v10 root ELF SHA-256 was
`022908395c954f73a67136f70fe4404d96e0cf1ff16f4531fa95eae7a6f57cb5`.
Its one-time post-activation yield completed, and UART emitted the retained
startup marker and prompt in separate bounded Operator visits. In the second
fresh Runtime visit, `root-control` consumed the complete `2750 us` refill and
raised current-fault `Timeout`, badge `0x26ee0001`, at PC `0xce98c`, the
`seL4_NBWait`/nonblocking receive on root endpoint `0x0a70`. The Runtime
successor had committed Network, the output FIFO was empty, its record cursor
was inactive, and the response barrier had crossed the prompt. V10 therefore
falsifies composition of every Runtime responsibility in one refill; it does
not falsify the activation, output, Network, or Recovery boundaries.

The same run recorded console timeout sequence 1, badge `0x26ee0007`, with
Terminal policy. The saved child was at `seL4_Wait` with
`service_pending = 1` and `control_pending = 1`, proving one logical unit and
its pending successor composed on one residual SC. Recovery reached Complete
with the TCB suspended, SC unbound, mappings scrubbed, capabilities revoked,
objects deleted, and generation fenced; NineDoor remained healthy. This
falsifies the v3 child replenishment boundary, not containment completeness.

The canonical v11/v4 run used root ELF SHA-256
`44971429e4941d751248c216082256f01e187930d9a6d40028e5c89d8611b597`,
console child ELF SHA-256
`af08f817191cc51c9354b61f09f3eeb50c8cdf875c660c7231987a426886666d`,
and CPIO SHA-256
`9fbb58e1dc6dc508361f37ce0c24219e3e9029dae101e2be789df1bcb1a5b11d`.
There were four TCP connects. The first three completed authentication attempts
each wrote 18 bytes and read zero; the fourth connect had no completed
authentication record. The child exhausted exactly `3000 us`, raised timeout
badge `0x26ee0007`, and stopped at PC `0x213458`, the `seL4_Yield` immediately
after the composite `PollService` completed and cleared; saved retained state
identified `PollService` as that completed unit. Root completed the retained
containment cursor through `Complete(6)`, then exhausted exactly `2750 us` at
PC `0xf5fbc`, the sole recurring outer yield after an empty Operator, and
raised timeout badge `0x26ee0001`. The stored ordinary successor was `Runtime`;
the retained Runtime successor was `ControlEndpoint`, proving the preceding
Runtime selected `Worker`; root output was empty. V11/v4 therefore fail their
live per-unit bounds. The root fault was not inside Recovery, and Stage 03 plus
pressure remained withheld.

The non-claiming v12/v5 convergence run
`out/test-plan-convergence/v12-v5-auth-20260812T010200Z` then bound root ELF
SHA-256 `7cec5bd582d063adc73830af8cc62e0ec8dbbb33d91bd4701db09ca69e32e6ca`,
console child ELF SHA-256
`920883c5e706688a65e7f168a643dbc527d09d7f48584bfb41fbd0c0ae823cb6`,
and CPIO SHA-256
`dc36495a5de0df13bfb853ffa33fdc6e7ccc3bbf3a1a3c8c4cd74c8551160c16`.
Each of four authentication attempts wrote 18 bytes and read zero. Only
`root-control` timed out: badge `0x26ee0001`, exact consumption `2750 us`, and
outer-Yield PC `0xf612c`. Stored ordinary successor `Network(2)` identifies the
completed phase as Runtime; stored Runtime successor `StreamFlush(3)` identifies
the selected unit as `BootstrapDrain`, and its staged `Option` was `None`.
Fault sequence 2 with the child healthy at Yield-then-Wait proves no earlier
child fault or Recovery. The immutable run embedded dirty source commit
`a533290ffe264f0a2bf0af3db4bb4c45d1a4a278`; HEAD later advanced to
`84934dda6`, so it remains diagnostic/failure evidence only. It falsifies the
generic Runtime-without-control prelude composed with an empty selected unit.

The next non-claiming v13/v5 convergence run
`out/test-plan-convergence/v13-v5-auth-20260812T014607Z` bound dirty source
commit `84934dda6fcffbfa536d4e437cc1904c7fdeb0b1`, root ELF SHA-256
`0275cd7d701263cc1731ca3301d9aeab8a0393651745659f192106a0d558d78f`,
the unchanged v5 child SHA-256
`920883c5e706688a65e7f168a643dbc527d09d7f48584bfb41fbd0c0ae823cb6`,
and CPIO SHA-256
`142e2aec64662888a9872ff77ff85d1f5f7c351b7aaa478ded8cf99ba9e64f29`.
All four authentication attempts wrote 18 bytes and read zero, so the first
failed proof layer was `real-target-operation`. The initiating root-control
timeout was badge `0x26ee0001` at `sel4::poll` SVC PC `0xce98c`, with caller
`0x108910` immediately after the child-to-root notification poll inside
`IsolatedVirtioConsole::poll`. Committed ordinary successor `Operator` and
lower successor `StageOutput` identify the selected Network unit as
`ObserveChild`. The child remained healthy at `seL4_Wait`. Root-fault then
timed out with badge `0x26ee0002` at `suspend_tcb` SVC PC `0xce0cc` while
targeting the root-control TCB cap `0x10`; root-emergency fail-stop followed.
V13 therefore falsifies its generic Network prelude and compiler-expanded
all-unit adapter closure. The secondary fault falsifies root-fault v2's
composed receive/classify/suspend handling. Neither failure falsifies child v5.

The following v14 QEMU artifact set exposed a separate cold-activation
admission failure before either core-1 supervisor executed its first user
instruction. It bound root ELF SHA-256
`4265ee26a8a23b38851167aa046f4adce50764715131d044059b4e08211b9361`,
system CPIO
`85a1e211f5cb83ad8ace277d0a4cfe89c22317ffa148e0aeae64611f1bd315d6`,
kernel
`865b5a0614f1633ca636800705f97339e78f47065fdaffd2cb4139e4a25630c0`,
driver archive
`88a3a9f1df93cb560501ac13275efb20b52985db8878b54372e29a397539474d`,
and driver manifest
`ef168c902062ff1c9f08208bc1eadf92773991ee2ce297a28da5ee26e2cfa385`.
`root-worker-supervisor` first consumed its complete `750 us` refill and
timed out with badge `0x26ee0004`, saved entry PC `0x1143dc`; after root-fault
received that record, `root-driver-supervisor` consumed its complete `1000 us`
refill and timed out with badge `0x26ee0005`, saved entry PC `0x113d98`.
The generated core-1 SchedControl is configured before SC-to-TCB binding, and
that MCS bind migrates the TCB to the SC core; direct `TCB.SetAffinity` is not
the MCS mechanism. The v15 QEMU-only admission candidates therefore preserve
core, priority, period, deadline, refill count, caps, and runtime ordering while
assigning each supervisor `3000 us / 10000 us` and `2400 us` WCET.
`root-worker-supervisor` declares a `4800 us` response bound with provenance
`m26e-qemu-root-worker-supervisor-cold-activation-candidate-v15`;
`root-driver-supervisor` declares `2400 us` with provenance
`m26e-qemu-root-driver-supervisor-cold-activation-candidate-v15`. Their steady
active core-1 demand is `6000 us`; adding the root-retained NineDoor bootstrap
envelope (`3000 us`, priority 128 below both supervisors) reaches the exact
`9000 us` usable capacity and preserves the `1000 us` reserve. The Pi manifest
retains its prior supervisor timing and provenance unchanged. The exact v15
QEMU image proved both supervisors reached their healthy blocking waits; Pi
remains unqualified pending its required image-bound checkpoint.

The exact v15 non-claiming run bound dirty source HEAD `84934dda6`, root ELF
SHA-256 `6c145a1d81bd57e791781a052f62dfc6dd5d34c7c7ca0aa4e3311a9b5696018c`,
system CPIO SHA-256
`07b84ff5dc2a40e2b9039d49b1e37bb88824909fe2fd902c9dd0165b4a643529`,
and resolved-manifest SHA-256
`46f3264e862944b84188064941bd581e60a78d80d9a7590dfe4b42fcfa3e7482`.
Root-control exhausted exactly `2750 us` at the outer yield after the retained
prompt was emitted and output state cleared. Successors `Runtime` and
`ControlEndpoint` prove the preceding Runtime selected `Worker`; the stale
entry-time TX predicate admitted the generic Runtime tail after completed
output. Root-fault subsequently exhausted exactly `3000 us` at the
`SignalEmergency` send return, but emergency delivery and fail-stop completed.
Both v15 supervisors were healthy at `seL4_Wait`. V16 therefore made the
isolated-QEMU retained-output Operator exclusive and advanced root provenance
only. The exact v16 image bound root ELF
`4fab7abc8707b9829ba66ac525efdfc7afefa812df4bab9abb8cb67d504a76a6`
and CPIO
`456558cac05e4d136d3cbc18d1290cc48bebf619ba5459cd623b667dbfff3e96`.
The prompt serial/output completed, but root-control consumed `2750 us` and
faulted at outer-Yield PC `0xf61c4`; saved successors `Runtime` and
`ControlEndpoint` identify the completed phase as Operator and the earlier
Runtime unit as Worker. Target disassembly showed an approximately `0x42c0`-
byte generic EventPump frame plus an approximately `0x12a0`-byte generic
Operator frame still preceded the output leaf. Root-fault then consumed
`3000 us` at its first post-classification Yield, PC `0x113938`, before
suspension or emergency signalling. Those failures authorize the v17 compact
dispatcher and v4 Classify boundary; every numeric, Pi behavior, child v5, and
supervisor v15 value remains unchanged.

The exact v17 non-claiming run
`out/test-plan-convergence/v17-v4-auth-20260812T041428Z` bound root ELF
`3d0641bac42d21ce383c47f38628a05db0d2474fab69fc6e14b67ba39a71bd47`,
the unchanged v5 child
`920883c5e706688a65e7f168a643dbc527d09d7f48584bfb41fbd0c0ae823cb6`,
and CPIO
`fa478638d6d2b93b654a2615e4dcd1e1d7f666d0945d4e012adcf28da2292af1`.
All four authentication attempts wrote 18 bytes and read zero. Current fault
`.1` was root-control at outer-Yield PC `0xf6624`; the committed ordinary,
Runtime, and Operator successors were `Runtime`, `Worker`, and
`SerialDispatch`. V17 therefore proved the compact dispatcher but falsified
its composed `SerialIo` leaf. V18 makes the `SerialIo` visit RX-only and retains
`SerialDispatch` for a later Operator, which commits its successor before
bounded consume/echo and TX flush. The raw-UART RX trace is suppressed only
while the admitted ordinary root-control turn is active. No Operator visit performs both the RX probe
and serial dispatch/flush.

The exact v18 artifact bound root ELF
`e7d34f018ff308c575fedb79ca7cef5542a7da8e753c09ddb9d55cf9daa79d4e`
and CPIO
`0dca41cc6fdd9a877144dcd2db610beaeafef95423a81ce6896b01bb9b8f5cf5`.
All four authentication attempts wrote 18 bytes and read zero. Root-control
used exactly `2750 us` and timed out at outer-Yield FaultIP `0xf66e4` after
Network. Ordinary successor `Operator(0)` and lower successor `Disconnect(2)`
identify selected `StageOutput(1)`; pending egress was zero, deferred diagnostic
state was `2`, and there was no child signal. Root-fault timeout `.2` at
`suspend_tcb` SVC PC `0xce1f4`, with cursor `SignalEmergency`, was downstream.
V19 therefore removed only CYW43/HDMI reconciliation from the true split QEMU
Network prelude. Its timer and one selected NIC unit remained; the split Runtime
prelude and generic/Pi paths retained reconciliation. The exact clean v19
artifact bound root ELF
`0737a6f008197fd5b931af104c95164ddcd925fa04a8440439895c1e76b26fca`
and CPIO
`51e7b955b449b42b7a0cad569aa187e19a0f71464ffb81080d29733a589e7ed0`.
Four authentication attempts each wrote 18 bytes and read zero. Root-control
timed out at outer-Yield PC `0xf66dc` after Network; lower successor
`Ingress(3)` proves selected `Disconnect(2)` was a no-op without child signal.
Pending egress was empty, the child was healthy at Wait PC `0x21343c`, and
root `smoltcp_polls` was `250098`. This falsifies the post-leaf counter-refresh,
NETDIAG, and NineDoor aggregate. V20 retains a compact telemetry/time-horizon
observation after the timer-plus-one-NIC visit and dedicates the next Network
visit to taking that observation and running NETDIAG only. The exact compact
Operator and Runtime visits between those Network turns cannot mutate the
diagnostic counters. Immediate accounting remains in the originating visit;
quarantine clears the slot. Generic/Pi behavior is unchanged.
The exact v20 root/CPIO hashes were
`ed5cb9f587d0d63e6121f8b00b083e68f5a0a7dd23dd6d2bbf0c899e1e85e80f`
and `ca2a52038eb0814a17c8609f03bec32ff357fdd524edee3e7080ac69ceb7823b`.
That image reached the root marker and prompt, then root-control timed out at
outer-Yield PC `0xf680c`. Successor Operator plus retained NETDIAG proves the
timer-plus-NIC visit completed while its diagnostic successor had not run;
lower-cursor, egress, and child state remain unconfirmed. V21 makes Timer and
Nic distinct successor-before-work Network visits. A retained diagnostic
preempts without cursor advance, giving
Timer -> Nic -> DeferredDiagnostic -> Timer. Quarantine preserves the cursor,
and generic/Pi paths remain unchanged.

The isolated VirtIO path continues to cycle
Operator/Dispatch -> Runtime/IPC -> Network ->
Operator/Dispatch. Operator owns root policy and command dispatch; Runtime/IPC
owns a persistent Worker -> ControlEndpoint -> BootstrapDrain -> StreamFlush ->
RebootTail cursor with command ingress suppressed; Network owns only VirtIO/NIC
service. The public EventPump entry selects this exact QEMU contract before
allocating the generic EventPump frame and enters a tiny noinline dispatcher;
Pi/non-VirtIO, linked-runtime, and physical-owner paths remain generic. The
compact dispatcher commits the outer successor before work and never calls the
generic Operator or Runtime bodies. Each Operator begins the shared serial and
one-record output credits. A retained SerialDispatch is selected first;
otherwise a bounded RX-only SerialIo probe may retain SerialDispatch for a
later Operator. Each visit admits at most
one eligible material noinline leaf in strict priority: serial RX or retained
dispatch/TX,
local-seat input, ordered physical-response output, one lifecycle event, one
buffered authenticated network line, background/high-impact pending output,
then display/frontier/attach.
Every material leaf commits its recorded successor before work; idle returns
to the sole outer yield. Each isolated QEMU Runtime visit attempts exactly one selected unit,
including an idle/no-op unit, commits its successor before the compact
isolated-VirtIO Runtime prelude, and returns to the sole recurring outer yield.
That prelude reads the HAL timebase and performs one timer poll. An observed
tick updates `now_ms`, increments the timer metric, publishes the HAL timebase,
and runs the existing conditional timer trace; without a tick, `now_ms` takes
the read timebase. It then reconciles CYW43 network-ready HDMI state and does
not execute the generic Runtime-without-control tail. Worker consumes
one pending mailbox operation or checks one retained Heartbeat/GPU/LoRA role
slot; ControlEndpoint performs at most one poll and its immediate forward;
BootstrapDrain takes one staged `Option`; RebootTail owns its visit. The MCS
fault-endpoint poll is absent from the cursor. StreamFlush uses distinct visits for its
terminal sequence: one visit emits one retained final line and returns; the next
no-line visit performs cursor/bandwidth finalization only and returns; the third
visit emits END only and returns. Every earlier line likewise uses its own
visit. Pi/non-VirtIO Runtime retains its existing 48-line/16-KiB bound.
Schema 1.11 gives QEMU `root-control` a generated
`virtio_operator_serial_io_bytes_per_turn = 64`; every non-root temporal task
and every Pi/non-VirtIO root-control record uses zero. One shared credit is
created only at VirtIO Operator entry. Every root-context serial RX poll and TX
flush in that turn consumes it, repeated helpers cannot reset it, and bytes
left after exhaustion remain queued for a later Operator turn. If TX backlog
exists at entry, `32` of the `64` bytes are reserved for TX and RX is capped at
`32`; with no entry backlog, RX may consume the full `64`. This preserves
bounded ACK/ERR/END output under sustained input. When the generated bound is
nonzero, the same Operator may attempt at most one retained output record;
later FIFO and response-tail records remain queued for a later Operator. Pi and
non-VirtIO turns retain the existing two-record attempt limit. These are root
scheduling bounds, not the physical serial driver quota: the existing
`max_bytes=1024` linked-runtime contract and Pi phase behavior do not change;
Pi adds only the universal one-time startup-period activation wait.
After the first bounded serial poll, local-seat priority consume, serial
consume/flush, and one buffered authenticated-line dispatch attempt, the
isolated QEMU VirtIO Operator snapshots only serviceable retained work. If that
snapshot is empty, v12 returns directly to the sole outer yield before the
repeated serial, local-seat, output, display, attach, and
runtime-without-control tail. Quarantine status, timer work, Runtime/Network
work, unattached-seat-only flags, and broad HAL hints cannot make the snapshot
nonempty. This cut does not apply to Pi or non-VirtIO Operator behavior and
does not alter the ordinary or Runtime cursor.

An isolated Network call attempts exactly one internal unit, whether productive
or a no-op. Its persistent lower cursor is ObserveChild -> StageOutput ->
Disconnect -> Ingress -> ServiceTick -> ObserveChild. Every selected lower
attempt returns from the Network visit; a no-op advances the cursor, while a
successful child signal forces the next lower attempt to ObserveChild. A pending
compact normal-success diagnostic has first priority and emits at most one
record on its own non-publish call. Otherwise retained egress preempts the lower
cursor for exactly one TX attempt and returns on publication or backpressure.
Both preemptions preserve the lower cursor and forced-observe state. The
retained-TX unit performs at most two bounded reclaim checks before its one
attempt. The bounded cadence covers successful attempt sequences 0 through 63
and every 64th eligible success thereafter, not every TX; counters remain
continuous. ObserveChild may copy and retain egress but cannot publish it in the
same visit. For a diagnostic-bearing success, the front sequence is Observe ->
TX -> DeferredDiagnostic before lower service resumes from its preserved state;
a second publication cannot overwrite or merge the pending record and anomalies
remain immediate. A TX call initializes the bounded descriptor payload,
atomically publishes the avail entry and any required notify, commits its
in-flight identity, performs no later buffer write, and returns without waiting
for completion.

The active console-network child likewise closes at most one logical unit per
active-MCS replenishment. Its retained-first priority is retained completion,
retained service event, retained egress, retained service-poll continuation,
new ingress, then new control. After its initial Ready signal and after every
later nonterminal unit, including idle or backpressure retention, it executes
exactly one `seL4_Yield` and then one `seL4_Wait`. Existing root service ticks
and the existing notification drive later progress; retained state survives
the Yield-then-Wait boundary. Terminal revoke/shutdown diverts only to the
wait-only park. This adds no capability, ABI/schema field, budget, or refill.
The retained `ChildTurnUnit::PollService` is itself resumable in v6. Its private
cursor commits `ServicePollUnit::StackIngress ->
ServicePollUnit::StackEgress -> ServicePollUnit::Session` before executing the
selected work. `StackIngress` performs one smoltcp ingress attempt;
`StackEgress` performs one egress pass. Each returns
`ServicePollOutcome::Continuation`, so the kernel retains `service_pending`,
crosses the existing Yield-then-Wait boundary, and later executes the
successor. `Session` owns connection/session RX, tick, TX, close, and relisten
work and returns `ServicePollOutcome::Complete`; only that result clears
`service_pending`. Errors never complete the scheduler unit.

Network -> Operator preserves immediate buffered TCP dispatch, and Operator ->
Runtime/IPC promptly services newly published control work. Each outer call
commits its successor before early return and reaches the userland loop's sole
recurring `seL4_Yield` replenishment boundary; no internal phase yield, extra
SC, notification omission, or cache-semantic exception implements the cut.
The separate one-time post-activation yield occurs before this cycle. Those
three calls are the ordinary phase cycle. Before any one begins, root-control
probes console-network's durable fault mailbox first and probes NineDoor only
when the console probe reports no work. A consumed record or attempted
containment claims that entire refill as an exclusive Recovery turn and advances
at most one material containment unit in fixed owner-local order. The
successor is retained across replenishments; the selected ordinary phase and
its retained Runtime- and Network-unit states do not advance, and the turn
returns through the same sole outer `seL4_Yield` without pump fallthrough.
Console-network is probed
first on every turn and NineDoor advances only when no console work remains, so
simultaneous faults complete the console sequence before NineDoor. Recovery is
not another temporal task or authority domain and adds no SC, budget, refill,
Reply authority, or internal yield.
Mailbox contention returns `Retry` without fencing authority. The first latched
console turn performs only the value/resource latch plus a lock-free scalar
authority fence. Later turns execute fourteen material units: suspend; unbind;
separate scrub/clean and unmap units for shared-frame indices 0 through 3; two
indexed fault-cap deletes; anchor revoke/reset; and `Finalize`. `Finalize`
commits `Complete`; the next idempotent `Complete` turn alone publishes the
proof and quarantines the terminal generation.

Ordinary retained-output turns then advance the quiet cleanup cursor one state
at a time: `RootSessionTicket -> RootTicketUsage -> NineDoorSessionTicket ->
NineDoorSessionScope -> NineDoorSessionBinds -> PendingStreamCursor ->
PendingStream -> Finalize -> Complete`. Successors are stored before work and
heap owners move into reboot-lifetime tombstones without drop, allocation, or
logging. Cleanup precedes its diagnostics. Service fault/failure/teardown
diagnostics remain first, console remains ahead of NineDoor, admission commits
only after queueing, backpressure retains the record, and flushing requires a
later ordinary turn.

The three-phase state is QEMU-VirtIO-only; non-VirtIO and Pi service ordering is
unchanged. Quarantining the console-network service does not collapse or bypass
the QEMU phase state: Network observes quarantine, preserves its retained unit
state, and fences NIC work rather than polling or falling back to combined
Operator/runtime service. Operator and Runtime/IPC remain separate admitted
turns. The attached VirtIO
contract suppresses the Pi/GENET-only synchronous
raw-UART idle-input trace in both live and quarantined states; it has no QEMU
consumer and cannot consume an admitted Operator phase. Fresh canonical QEMU
boot, console, regression, pressure, and fault-injection evidence remain
required for the v23 root-control, V6 root-fault, and v6 child candidates. The
focused direct base `.coh` batch, Hive Gateway REST core/parity plus Python
smoke, Conditional D performance matrix, and complete host-tool validation
remain blocked until fresh QEMU authentication and standard/timeout injection
pass; compiler admission alone is not target qualification. V23 preserves
v21's one-NIC-per-three-featured-Network-visits cadence. Successful
physical-tail reconciliation or prompt queueing returns before phase
selection; a still-pending bounded attempt runs exactly one compact Operator
unit and returns without phase advance. Ready reboot remains exclusive. The
REST performance result must therefore be measured.

During NineDoor construction, root configures and binds the schema-1.11
bootstrap candidate (8 object bits, `3000 us / 10000 us`, `max_refills = 2`)
while the child remains suspended and before registry seal. Only after the
registry is sealed and the independent `root-fault` receiver is active does
root perform exactly: resume; a validated empty `Log` prepare; the child's
atomic `seL4_ReplyRecv` reply-and-next-receive; and root-side SC unbind. Only
after that unbind may ordinary `root-control` Calls donate the caller's SC to
the queued passive receiver. The child CSpace receives neither an SC nor
SchedControl cap, and this path performs no `TCB.SetAffinity`. Activation,
probe, or unbind failure revokes the namespace boundary and fails boot; probe
and unbind failure also suspend the child where possible. The candidate is
frozen compiler truth but remains unqualified until the selected four-core
GICv3 QEMU run proves the transition and repeated steady calls.

The root-fault CSpace receives compiler-bounded child-local TCB control caps at
the exact critical task-index slots used by its containment loop. Root-relative
registered TCB caps remain root-control records and are never invoked from the
restricted root-fault CSpace.

The QEMU and Pi manifests reserve `3000 us / 10000 us` for `root-fault`, with
a compiler-admitted `2400 us` per-unit containment WCET, `2600 us` per-unit
response bound, and
`m26e-qemu-root-fault-service-units-candidate-v6` provenance. These
values supersede the original 500-us candidate: live
four-core GICv3 QEMU proved that candidate could expire while suspending an
already-faulted child. A later live four-core GICv3 boot proved that the
replacement reserve could also be consumed by the former `seL4_NBRecv`/yield
polling loop while no fault was available. V13 then proved root-fault v2 could
consume the replacement reserve at `TCB.Suspend` after receive and
classification. The v3 repair split terminal-critical receive/classify,
suspend, and emergency signal, but exact v16 evidence then consumed `3000 us`
at the first post-classification Yield (`0x113938`) before suspension or signal.
V4 therefore retains the blocking shared receive and makes Classify its own
admitted unit: Receive commits Classify before the blocking receive, copies
only label/badge, and yields. A Released classification yields before another
Receive. RetainedByDriver waits for and validates the exact release badge and
cleared busy state, then yields. Critical commits SuspendCritical and yields;
SuspendCritical commits SignalEmergency, suspends, and yields; SignalEmergency
commits Receive, signals, and yields. The `2600 us` result therefore applies to
one unit, not the complete terminal-fault sequence.
The exact V22 snapshot then found root-fault timeout badge `0x26ee0002` at
FaultIP/NextIP `0x113e70`, immediately after the first Receive unit's terminal
Yield SVC at `0x113e6c`; LR `0x113e5c` followed publication of root-control
timeout label `5` and badge `0x26ee0001`, with Classify already committed. V5
adds only the initial `PrimeReceive`: after construction, registry seal, and
resume make root-fault runnable, it commits Receive and yields before receiving
or creating a copied-value/Reply association. A concurrent sender can remain
queued on the already constructed endpoint until the replenished Receive runs.
All released, driver-release, critical, suspend, and signal successors retain
the v4 recurring cursor and commit Receive rather than PrimeReceive. V6 adds a
service-only route after Classify:
`ResolveService -> SuspendService -> RecoverPassiveService -> PublishService ->
Receive`; active console service records skip `RecoverPassiveService`.
Resolution performs one fixed generated lookup and a nonblocking
registry-lock/scalar-snapshot attempt, retrying without loss on contention.
Suspension performs one quiet bounded syscall. Passive recovery may issue at
most one Reply; active console recovery issues zero. Publication performs one
mailbox action and retains the snapshot on backpressure.
Standard-plus-timeout fault injection must still qualify this candidate with
terminal timeout policy enabled; generated admission and a successful boot are
not that evidence.

Standard and timeout send caps target that same endpoint and retain disjoint
exact-identity badges. Root-fault supplies the sole Reply object to blocking
`Recv`, resolves the sealed-registry record from the nonzero badge, and does not
reuse the Reply while it remains associated. For a terminal critical-domain
fault, Receive commits Classify before receiving, Classify commits the suspend
successor before yielding, SuspendCritical commits the emergency-signal
successor before suspending the exact registered TCB, then SignalEmergency
signals root-emergency on the following admitted turn and yields before another
Receive. Released classifications and validated RetainedByDriver releases also
cross a post-classification yield before the next receive. The association remains retained
in root-fault's CSpace throughout, and this terminal path cannot return to
`Recv`. Other Worker, driver, and service paths retain their existing handling.
Ordinary noncritical terminal containment clears the association before the next
receive. For a linked-driver fault,
root-fault publishes the reserved containment record and blocks on the existing
root-fault wake notification; only the driver supervisor's generated
release-badge signal, after command-failure and containment work has released
the association, permits root-fault to receive again. No second fault Reply,
poller, or notification-carried fault identity exists.

For an isolated service fault, `root-fault` suspends the exact registered TCB.
For steady-state passive NineDoor only, it then consumes the dedicated
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
| 0 | emergency, fault, control, console-network | 9,000 us | 7,500 us |
| 1 | driver supervisor, Worker supervisor | 6,000 us | 4,800 us |
| 2 | Worker GPU, Worker LoRA | 800 us | 600 us |
| 3 | Worker heartbeat | 300 us | 200 us |

The Pi profile retains its own unchanged supervisor base and includes its seven
admitted linked-driver tasks:

| Core | Added Pi duties | Total budget demand / 10,000 us | Largest response |
| --- | --- | ---: | ---: |
| 0 | none | 9,000 us | 7,500 us |
| 1 | serial, USB | 3,250 us | 2,600 us |
| 2 | HDMI, PCIe | 1,600 us | 1,200 us |
| 3 | GENET, CYW43, SDIO | 4,300 us | 3,400 us |

Every total remains at or below the 9,000 us usable per-core window. Core 0
uses that complete window while preserving the separately declared 1,000 us
reserve. The Pi row is offline admission only until the separate linked-driver
MCS and hardware gates pass.

#### Executable-slot resource arithmetic

Namespace/model capacity is eight identities per executable Worker role, but
the maximum live mix is exactly one heartbeat, one GPU, and one LoRA slot. Each
slot costs one TCB, CNode, VSpace, ASID, notification, standard-fault cap,
timeout-fault cap, and active SC; eight page tables, sixteen frames, 64 CSpace
slots, and 1 MiB of child untyped. No namespace count multiplies these kernel
objects.

The compiler checks `fixed + maximum live role mix + post-construction reserve`
against the selected capacity. The 59-page console-network image contributes
97 fixed frames and 121 fixed CSpace slots after its 32-page stack, IPC/init
pages, and four shared pages are included. The exact admitted totals are:

The selected seL4 16 AArch64 SMP+MCS object-size record is TCB 11 bits,
endpoint 4, notification 6, Reply 5, minimum scheduling context 7, CNode slot
5, and page/page-table/VSpace 12. Compiler admission rejects stale classic
notification or Reply sizes rather than understating MCS object memory.

| Resource | QEMU total | Pi total | Capacity |
| --- | ---: | ---: | ---: |
| TCBs / CNodes / VSpaces / ASIDs | 18 each | 25 each | 64 each |
| Page tables | 344 | 600 | 1,024 |
| Frames | 2,577 | 4,625 | 8,192 |
| Endpoints | 31 | 47 | 128 |
| Notifications | 35 | 51 | 128 |
| Standard / timeout fault caps | 18 each | 25 each | 64 each |
| Reply objects | 14 | 21 | 64 |
| Scheduling contexts | 18 | 25 | 64 |
| CSpace slots | 6,296 | 11,200 | 16,384 |
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
