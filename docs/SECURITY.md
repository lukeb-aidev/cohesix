<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define Cohesix vulnerability reporting, security boundaries, implemented controls, and generated profile limits. -->
<!-- Author: Lukas Bower -->

# Security

Cohesix is a pre-production research operating system. Its design reduces and
makes authority visible; it does not make the complete system formally verified
or suitable for unattended production use. seL4's machine-checked proofs apply
to the kernel under their stated assumptions, not automatically to Cohesix
userspace, host tools, firmware, drivers, or deployment policy.

## Reporting a vulnerability

Do not disclose a suspected vulnerability, exploit, secret, or sensitive log in
a public issue or discussion.

1. Open this repository's **Security** tab and use GitHub private vulnerability
   reporting when it is available.
2. Include the affected commit or release, target/profile, minimal reproduction,
   expected security boundary, and impact. Attach only redacted evidence.
3. If private reporting is unavailable, open a non-sensitive issue asking the
   maintainer for a private reporting channel. Do not include vulnerability
   details in that issue.

Cohesix is maintained as a research project and does not promise a fixed
response or remediation SLA. The maintainer will acknowledge a usable private
report, validate scope, coordinate disclosure when practical, and record a fix
and verification evidence if the issue is accepted.

Current source receives security fixes first. Versioned release directories are
immutable research snapshots; a backport exists only when the corresponding
release notes explicitly identify it. Never assume that an older bundle has the
security posture of the current tree.

## Operator diagnostic authority

`bi`, `caps mcs`, and `smp mcs` are bounded projections on an already admitted
operator surface. Printed slots, badges, and generations are identifiers, not
transferred authority. The live registry is copied with a non-blocking lock and
released before output. Inspection performs no debug dump, capability or
scheduling mutation, or `seL4_SchedContext_Consumed` call; generated state is
never labelled live and early console never claims activation.

## Security objectives and non-goals

Cohesix aims to:

- keep the privileged kernel and in-VM trusted computing base small;
- make resource and namespace authority explicit through seL4 capabilities,
  generated manifests, roles, tickets, and lifecycle state;
- validate hostile input before side effects;
- bound memory, work, retries, queues, and retained evidence;
- compartmentalize physical drivers and keep GPU ecosystems host-side;
- leave deterministic receipts for accepted and denied control-plane actions.

Cohesix does not currently provide:

- a formally verified whole system;
- a POSIX security boundary, multi-user desktop, or general-purpose server;
- in-VM TLS, HTTP, SSH, CUDA, NVML, model execution, or package-management
  services;
- encryption for the direct TCP console or `hive-gateway`;
- a Pi 4 IOMMU/SMMU boundary on BCM2711;
- a guarantee that a compromised operator host, boot firmware, selected driver
  firmware, or privileged build environment cannot compromise a deployment.

## Trust and authority boundaries

The seL4 kernel controls memory objects, execution, notifications, interrupts,
and IPC through capabilities. Cohesix root-task remains trusted for bootstrap,
HAL admission, manifest enforcement, namespace authority, tickets, lifecycle,
and audit. Queen and Worker-role sessions receive only their generated
namespace view; Worker tickets are mandatory and Queen ticket requirements are
profile-controlled. Current profiles mark every target Worker role
non-executable and disable Worker endpoint-cap and lifecycle-notification
authority. Reserved generated badges are not installed capabilities.

Physical devices run in manifest-declared, single-threaded Rust driver
runtimes. HAL owns physical-address discovery, device-untyped admission, MMIO,
IRQ, DMA, PCI, SDIO, and board-level resource assignment. Driver runtimes may
touch only the resources delivered through their generated fixed ABI. The root
task may admit resources, submit bounded service turns, and retain diagnostics,
but must not own steady-state physical drivers.

Host tools are outside the target TCB. They may use operating-system services,
CUDA/NVML, model runtimes, REST, and UI frameworks, but may only project the
documented console and Secure9P semantics. A compromised host tool with a valid
Queen secret can exercise that secret's authority; least-privilege host process
and secret handling remain deployment responsibilities.

See [Architecture](ARCHITECTURE.md) for component boundaries,
[Roles and scheduling](ROLES_AND_SCHEDULING.md) for role authority, and
[Drivers](DRIVERS.md) for the physical-device proof contract.

## Network and console exposure

The authenticated root-task TCP console is the only in-VM TCP listener. It is a
line-oriented console using `AUTH`, `ATTACH`, bounded commands, `OK`/`ERR`
responses, and `END` stream terminators; it is not a 9P-over-TCP server.
Worker-role session attachments require a valid role ticket before namespace
access. Attachment does not start a target Worker task; executable
Heartbeat/GPU/LoRA tasks are admitted separately by the generated Worker
supervisor, while WorkerBus remains model-only.

Authentication is not encryption. Bind direct console forwarding to loopback
or carry it through an authenticated encrypted tunnel. `hive-gateway` also
defaults to loopback, refuses non-loopback binding without explicit opt-in, and
does not terminate TLS. A non-loopback gateway requires an external secure
boundary such as a VPN, authenticated tunnel, or TLS reverse proxy. Use
different secrets for target console authentication and REST request
authentication.

Only one direct owner may hold the target console session. Concurrent clients
must share one `hive-gateway` owner rather than racing `cohsh`, SwarmUI, or
bridges against it.

On the generated QEMU MCS target, that sole listener is implemented by the
separately linked `console-network-runtime` child. The child owns smoltcp,
Ethernet/IP/TCP state, the length-prefixed console transport, constant-time
`AUTH` token comparison, and pre-authentication timeouts. Root owns no TCP
parser: it receives only a copied, authenticated bounded command and remains
the sole authority for Queen policy, tickets, namespace operations, and
command execution. Root sends already-authorized response bytes back through
one bounded control page, so existing `OK`/`ERR`/`END` ordering is unchanged.

Application attachment is one authority transaction, not a logger health
check. After the target namespace service validates and prepares `Attach`,
NineDoor commits its local role/ticket context before best-effort audit,
logger, or boot-tracer observation; root commits its matching session and emits
`OK ATTACH` only after that bridge transaction succeeds. The bridge remains
UART+EP mirrored at attachment. Any optional EP-only ping/ack self-test runs
only from a later explicit promotion request and cannot veto or roll back the
namespace grant.

The compiler fixes the child image path and hash-bound ELF identity, 16-slot
CSpace, one active MCS scheduling context, standard/timeout fault badges, and
four pointer-free sequence-last pages. Those pages preserve the fixed 4096-byte
console-network ABI v3 page layout while each live transfer reads or writes only its
compact scalar header (40 bytes for packets or 64 bytes for control/events) and
the validated active payload. The producer clears commit, release-fences,
writes the scalar fields and active bytes, release-fences again, publishes the
final sequence commit, and signals only afterward. The consumer bounds the
length before copying and accepts the record only when the surrounding commit
observations agree. Scalar reserved header fields must validate as zero.
Inactive payload suffixes and reserved page-tail bytes convey no authority and
are not scanned or copied per turn; construction zeroing and containment scrub
remain the confidentiality boundary for those tails. The child maps both root-to-child
pages (packet ingress and control) read-only, and only its packet-egress and
event pages read-write. ABI v3 retains the internal publication-ACK contract;
field offsets and page layouts remain stable, and authentication plus external
ACK/ERR/END behavior is unchanged. Its new binary `SendBatch = 3` control is
accepted only when encoding version 1, reserved zero, exact used length, one
through eight records, each `1..=256`-byte valid UTF-8 line, and the no-CR/LF
rule all validate before mutation. Malformed, trailing, partial, stale, or
overlapping batches fail closed without granting output authority.
After one complete frame commit, the child retains exactly one following
three-unit service cycle, including for the final record; the next no-progress
Session quiesces. Pending state and capacity/sendability failure cannot retain
work, preventing both missing progress and an uncredited local spin.

The child owns an active `3000 us / 10000 us` SC with honest full-budget
`3000 us` WCET. QEMU retains core-2 placement and derives a `3000 us` response;
Pi retains core 0 and derives `8100 us`. Both use
`m26e-qemu-console-received-progress-retention-candidate-v18` provenance. One
wake may coalesce work, but the child rechecks its badge and publication-credit
gates between logical units. The
retained-first priority is completion publication, service-event publication,
egress publication, service-poll continuation, new ingress, then new control.
It may use `seL4_Poll` for exact eligible internal `PollService`, `IngestPacket`,
or `ApplyControl` work because those units mutate only private state. Idle or
publication-uncredited work calls `seL4_Wait` directly, with no ordinary
`seL4_Yield`. Publication requires one explicit credit granted only by
root badge 64 after ObserveChild has validated and copied all indicated records
and the adapter has durably handled the event and retained any egress. Root
tracks one exact ACK debt, clears it before signalling, and forces ObserveChild
as the next lower successor. Internal work preserves a credit; one Publish arm
consumes it before any queue or page mutation. Ordinary wakes never grant
credit, duplicate or late ACKs fail closed, and one coalesced event-plus-egress
observation earns only one credit. A stable empty packet/control hint retires
before retaining one separate service cycle, preventing a local Poll loop.
Within that cycle, each nonzero bounded socket receive is durable private
progress and retains a fresh cycle; only a zero-receive/no-wire-commit Session
unit quiesces, so already-buffered authenticated input cannot be stranded
behind a missing notification.
Revoke parks without publication. Graceful shutdown waits for a credit,
publishes `ShutdownComplete`, retires terminal debt without ACK, and enters
bounded containment; proof-complete teardown alone establishes terminal state.
Schema 1.14 selects `NaturalPostpone` for this active child: root leaves its TCB
timeout-handler slot empty, so seL4 postpones an exhausted SC until
replenishment. The standard fault endpoint remains terminal. The timeout cap,
badge, resource, and registry identity remain reserved and accounted but are
not installed as the child's timeout handler. ABI/READY identity v3 and the
fifth root Write-only mint on the existing notification remain unchanged; no
notification object, child cap/slot, budget, or refill is added.
The selected QEMU V35 and Pi V24 root-control records use the same
`NaturalPostpone` security disposition: their generated timeout caps, badges,
resources, and registry identities remain reserved, but their TCB timeout
endpoint slots are empty. Exhaustion postpones to a valid adjacent refill;
their standard fault endpoints remain installed and terminal. This changes no
fault authority, badge allocation, budget, refill, client timeout, or retry.
Root V35 preserves V34/V33/V32/V31/V30/V29's capture of synchronous HELP, NETSTATS, SMP, and
CACHELOG body plus the
exact terminal for one authenticated generation and connection, seals it before
publication, and drains it through V27's active response lane. The selected
`DefaultNetStack` must delegate terminal publication, bounded identity, lane
inspection, response-budget polling, and pending console-event state to its
concrete backend. At most eight
useful response units run before one ordinary debt turn, so serial/local-seat
input, fatal output, timers, and retained diagnostics cannot be starved by a
large TCP response. A batch sequence is not terminal until exact
`ControlCompleted` and `OutputDrained` evidence arrives; stale completion or
drain identity faults the isolated service rather than releasing another
connection's output. The retained V28 fence also denies Disconnect publication
until that root-owned lane has retired
the exact terminal records, publication-ACK debt, copied egress, and queued
output. Child-side drain is insufficient authority. This closes the V27 path
that correctly fail-closed at `response-completion-sequence` after QUIT when a
Disconnect completion collided with the still-owned terminal batch sequence.

Routine command, session, TAIL, and NineDoor diagnostic lines are best-effort
debug records, not durable security-audit or evidence authority. On the
selected QEMU VirtIO composition they enter a private four-record EventPump
FIFO and never public `/log/queen.log`; this preserves the namespace's bounded
operator payload against diagnostic contamination. A full FIFO drops only the
new record and retains older FIFO order without retry or fallback. Only the
final-idle Operator unit may attempt one nonblocking serial admission, and it
retains the head if serial refuses it. V34 permits at most one physical audit
byte per eligible visit and retains the complete record plus audit-only tag
across later visits. Any response, stream, flush, physical or
network input, retained output, containment, or display work preempts the
attempt. Pi, linked-runtime, legacy/non-VirtIO, critical/fatal, ordinary
console-failure, and fail-stop sites retain their existing raw diagnostic
route.

Producer-side backpressure remains a security and availability gate until live
proof. Fixed responses use bounded immutable capture. CACHELOG reserves a
bounded snapshot allocation before taking the cache-log lock, copies at most
the existing 1920-record ring under one lock hold, and renders only after the
lock is released; allocation failure emits a typed bounded error without
partial authority. Conflicting physical synchronous producers receive bounded
BUSY while the network response owns shared state. The fixed one-socket matrix
must pass. The 1920-record ring capacity is not a separate five-second
promotion gate; neither host fixtures nor the retained V28 canary can waive
the fixed-matrix availability condition.
Once the endpoint is ready, validated, unlocked, and post-commit unlocked,
steady syscall wrappers take an inline four-read gate before bootstrap trace
counters, tracer snapshots, formatting, locks, or UART diagnostics. Any false
readiness bit retains the cold diagnostic/refusal path. This removes shared
diagnostic work from restricted scheduling contexts without removing a
readiness check or weakening fail-closed IPC admission.
The retained `ChildTurnUnit::PollService` is internally split by the private
`ServicePollUnit::StackIngress -> ServicePollUnit::StackEgress ->
ServicePollUnit::Session` cursor. The successor commits before work.
`StackIngress` performs one interface ingress attempt and `StackEgress`
performs one interface egress pass. Each returns
`ServicePollOutcome::Continuation`, retaining `service_pending` across the
gate recheck and eligible local-Poll path; `Session` owns connection/session RX, tick, TX,
close, and relisten work and returns `Complete`. Only `Complete` clears the
scheduler unit, and an error cannot do so.
When peer FIN advances the socket to `CloseWait`, V12 sets the existing
idempotent graceful-disconnect intent. Exact-generation output remains
authorized and retained until `close_ready`; the existing close/end/listen
path then completes the server half-close, publishes one `Disconnected`,
clears the ended generation, and restores the sole listener. This grants no
new root authority and adds no wake, retry, timeout, scheduler unit, or second
listener.
V13 retains that V12 peer-close path and handles the distinct server-active
close completion. Once the sole socket reports `TimeWait`, Session ends the old
authenticated generation before aborting the completed TCP control block and
restoring LISTEN. That ordering publishes exactly one old-generation
`Disconnected` and clears its authentication/output state before a replacement
connection can own the socket. The existing `Closed` path remains unchanged;
no root authority, second listener, retry, timeout, wake, or scheduler unit is
added.
V26 changes only the QEMU root's authority bookkeeping for that existing
Disconnect control. One successful publication latches the transaction for the
exact active connection; `ControlCompleted` and `OutputDrained` release their
existing page/drain obligations but cannot authorize another Disconnect.
Backpressure does not set the latch, and the requested bit remains until
`Disconnected` so Quit attribution is preserved. Existing connection,
terminal, and fail-closed boundaries clear both bits. This removes duplicate
authority exercise without adding a right, cap, page, wake, retry policy, or
timeout.

Historical V7 retains those v6 turn boundaries and narrows only Session TX admission.
Free TCP-buffer capacity is not send authority: the child calls `send_slice`
only when the socket also reports `can_send()`. A FIN-WAIT or other closing
socket therefore retains any late session output without committing it; after
the peer completes teardown, `end` clears the closed generation, publishes one
`Disconnected` event, and restores the sole listener. The child cannot publish
`OutputDrained` for that output because the queue is nonempty before teardown
and the connection identity is absent afterward. No other
`RuntimeError::Backpressure` source is reclassified or swallowed.

V8 additionally fences root controls by the existing committed connection
identity. The child validates nonzero identity, kind, and bounded payload before
classifying a well-formed control for an ended or different connection as
`StaleConnection`. It consumes that exact sequence and publishes
`ControlCompleted`, but enqueues no bytes, publishes no `OutputDrained`, and
raises no fault. Malformed input and matching-current authentication or queue
violations remain terminal. Thus a delayed old-generation control cannot gain
authority over a replacement session. Root retains its in-flight slot across
`Disconnected` until exact completion and, before staging, returns backpressure
when no authenticated connection exists so pending stream state cannot be
silently discarded.

The exact V24 HVF diagnostic reached `OK AUTH` and `OK ATTACH`, then timed out
waiting for `TAIL /log/queen.log`. Its child standard fault mapped to retained
`ApplyControl(SendLine)` after the owning connection had ended and `AuthState`
was inactive: V7 discarded the control-page connection identity and treated
the old record as a current unauthenticated error. V24 is failure evidence for
that generation fence, not V8 qualification. A fresh same-boot two-session
canary must prove the delayed first-session control completes without output or
fault and cannot alter the authenticated replacement session.

The exact V8 HVF artifact
`out/cohesix-v8-stale-control-hvf-qemu10-20260813T090943Z` preserved that
generation fence and reached `root-console.start.ok` without a target fault.
Its live run `out/m26e-qemu/v8-stale-control-live-20260813T092000Z` made two
sequential authenticated-transport attempts; each wrote the complete 18-byte
AUTH frame and received zero bytes before its bounded timeout. Source audit then
found that every locally retained completion or service continuation still
returned to blocking Wait and therefore depended on a later unrelated root
tick. V9 attempted a local-Poll/publication-fence repair, but its exact artifact
`out/cohesix-v9-retained-work-hvf-qemu10-20260813T095338Z` again wrote 18 AUTH
bytes and read zero before timeout. An unrelated or pre-observation wake cannot
be publication credit; V11 replaces that unsafe inference with explicit ACK.
Both V8 and V9 are failure evidence, not V11 qualification.

The immutable V25/V11 artifact
`out/m26e-qemu/temporal-v25-20260813T125130Z/artifact` completed one raw AUTH,
but replacement connections were reset at both `+1 s` and `+10 s`. A live
read-only snapshot, whose transcript was not retained, showed root current
fault `NullFault`, fully replenished root-control budget `5500 us`, and the
healthy child blocked in core-2 Wait. Source audit found the socket parked in
`CloseWait` without a server half-close/relisten transition; M26b Complete
`72288c7d` had explicitly entered its disconnect state machine there. V12
restores only that internal lifecycle transition and remains pending target
qualification.

The immutable V12 evidence set
`out/m26e-qemu/peer-close-v12-20260813T133000Z`, source digest
`sha256:c047b0886ba42ba1dfe0004009a8e9377d4d2cbd98e997e8dfd463e4bc80eaa0`,
passed raw AUTH 1, host close, same-boot raw AUTH 2, then one `cohsh`
AUTH/ATTACH/four-line-TAIL/END/QUIT session. Replacement authentication timed
out at `+5 s` and raw authentication still timed out at `+30 s`; UART recorded
no fault, and the read-only GDB reproduction found all CPUs kernel-idle.
Smoltcp 0.13.1 keeps a locally closed connection in `TimeWait` for `10 s` and
re-arms that timer when a replacement SYN reaches the same sole socket, so
probing prevented passive expiry. M26b Complete `72288c7d` already classified
`TimeWait` as terminal. V13 restores that classification without changing any
authentication, transport-confidentiality, capability, ABI/READY, schema,
timing, placement, grammar, retry, or timeout contract. V12 is diagnostic
evidence for V13, not V13 qualification.

The immutable V13 target evidence at
`out/m26e-qemu/peer-close-timewait-v13-20260813T140319Z/same-boot-two-complete-sessions-20260813T141000Z`
completed session A through AUTH, ATTACH, four-line TAIL, END, and QUIT. Same-boot
session B connected twice but each AUTH wrote 18 bytes and read zero; QEMU stayed
alive and UART recorded no runtime fault. The exact child retained V13's
`TimeWait` terminal path. Root instead reissued the already-successful
Disconnect after each completion/drain, and every signal reset its lower cursor
before Ingress or ServiceTick. The peer ACK/FIN therefore never crossed the
root-to-child packet authority boundary. V26's single-issue latch restored that
boundary, and the later immutable V26 boot completed two direct sessions. The
selected child is now V18; V13 remains immutable lifecycle chronology, while
V14's 59-page image-binding failure remains build evidence only.

The child receives no root CSpace, device capability, `SchedControl`, policy
object, namespace memory, or second listener. All child objects and translation
tables descend from one retained one-MiB untyped anchor. Standard/protocol fault
handling first closes admission, then advances suspend, SC unbind, bounded
shared-frame scrub/unmap, recovery-cap removal, anchor revoke, and terminal
quarantine through a fixed retained cursor. Each exclusive root Recovery turn
performs at most one material unit and ends at the sole outer yield without
ordinary-pump fallthrough while preserving the selected ordinary phase plus
both retained Runtime- and Network-unit cursors. Console-network has precedence on every turn;
NineDoor containment advances only when no console work remains. This resumable
cut retains the existing root-control authority. QEMU V35 keeps root-control on
core 0 but assigns `5500 us / 10000 us`, `5000 us` WCET, and a `7600 us`
response bound with `m26e-qemu-root-adjacent-refill-natural-postpone-candidate-v35`
provenance. It retains every V25 timing and placement value. Only the QEMU
console child is on core 2, where its unchanged
`3000 us / 10000 us` budget and honest `3000 us` WCET derive a `3000 us`
response. Same-priority GPU/LoRA responses are `3600 us`, while core-2 demand
remains `3800/9000 us`.
Pi root-control selects
`m26e-pi4-root-adjacent-refill-natural-postpone-candidate-v24` while retaining
V23 placement, timing, response, and admission truth; its child advances to
V18 common-child provenance under schema 1.14 with response `8100 us` and
core-0 demand `9000/9000 us`.
After the steady SC and manifest-selected timeout policy are applied, one universal MCS
`seL4_Yield` sacrifices the partially consumed initial refill and waits for the
next replenishment before either containment mailbox or the first ordinary
phase is touched. This one-time activation seam uses no additional authority,
SC, budget, refill, or capability and does not drain retained output under
bootstrap authority. It is distinct from the sole recurring outer yield after
each Recovery or ordinary phase. Pi receives the same truthful first-phase
accounting at the cost of one startup-period wait.
On isolated QEMU VirtIO, Runtime owns a persistent
Worker -> ControlEndpoint -> BootstrapDrain -> StreamFlush -> RebootTail cursor.
Each Runtime visit attempts exactly one selected unit, including idle/no-op,
commits the successor before the compact isolated-VirtIO Runtime prelude, and
returns to the sole recurring outer yield. The compact prelude reads the HAL
timebase and polls the timer once. An observed tick updates `now_ms`, increments
the timer metric, publishes the HAL timebase, and runs the existing conditional
timer trace; without a tick, `now_ms` takes the read timebase. It then reconciles
CYW43 network-ready HDMI state and does not enter the generic
Runtime-without-control tail. Worker consumes one pending mailbox operation
or checks one retained Heartbeat/GPU/LoRA role slot; ControlEndpoint performs at
most one poll and its immediate forward; BootstrapDrain takes one staged
`Option`; RebootTail owns its visit. MCS fault polling is absent from the cursor. StreamFlush separates
its terminal sequence across visits: one visit emits one retained final line,
the next no-line visit performs cursor/bandwidth finalization only, and the
third emits END only. Every earlier line likewise uses its own visit. This cut
does not apply to legacy Pi/non-VirtIO Runtime, which retains its existing
48-line/16-KiB bound.
Serial, local-seat input, emergency diagnostics, and fatal output remain
independent root-owned paths and retain priority when TCP work is absent or
overloaded.
For the isolated VirtIO Operator only, selected schema 1.14 retains the
schema-1.12 compiler-owned
`64`-byte serial-I/O credit shared by every root-context RX poll and TX flush in
that turn. Exhaustion preserves pending bytes for a later Operator; helper
re-entry cannot manufacture more credit. Entry-time TX backlog reserves `32`
of `64` bytes for output; otherwise RX may consume the full bound. All non-root
temporal tasks and the Pi/non-VirtIO root-control record select zero. This bound
does not weaken or replace the physical serial driver's existing
`max_bytes=1024` contract.
When that generated credit is nonzero, the Operator may attempt at most one
retained output record; the remaining FIFO and response-tail records retain
their order for later Operators. Pi and non-VirtIO turns preserve their
existing two-record attempt limit. This record bound changes no external
grammar or authority.
On the isolated QEMU VirtIO path, v18 retains the v17 exact selection at the public
EventPump entry before allocating the generic EventPump frame. A tiny noinline
dispatcher commits the outer successor and cannot call the generic Operator or
Runtime bodies. Operator starts the shared serial and one-record output credits.
A retained SerialDispatch runs first; otherwise a bounded RX-only SerialIo
probe may retain SerialDispatch for a later Operator. Neither unit can call the
other or select another material leaf in the same visit. Operator admits at
most one eligible material
noinline leaf in strict priority: serial RX or retained dispatch/TX, local-seat input, ordered
physical-response output, one network lifecycle event, one buffered
authenticated line, background/high-impact pending output, then
display/frontier/attach. Every material leaf commits its
recorded successor before work. An idle compact Operator returns to the sole
outer yield. Pi, linked-runtime, physical-owner, and non-VirtIO behavior remains
on the generic path.
The v19 split QEMU Network prelude calls only `poll_runtime_timer_prelude`
before exactly one retained NIC unit. It does not reconcile CYW43/HDMI state;
the distinct Runtime prelude and generic/Pi paths retain that reconciliation.
V20 retains one compact diagnostic observation after that visit and makes the
next Network visit take it, sample the stable counters, run NETDIAG only, and
return before timer or NIC work. Immediate flush, connection-id, and NineDoor
ingest accounting remain in the originating visit, and quarantine clears the
retained observation. This is temporal decomposition only and changes no capability, authority,
external grammar, or physical-driver ownership.
The exact v20 root/CPIO hashes were
`ed5cb9f587d0d63e6121f8b00b083e68f5a0a7dd23dd6d2bbf0c899e1e85e80f`
and `ca2a52038eb0814a17c8609f03bec32ff357fdd524edee3e7080ac69ceb7823b`.
The image reached the root marker and prompt, then root-control timed out at
outer-Yield PC `0xf680c`. Successor Operator and retained NETDIAG prove only
that timer plus NIC completed and the diagnostic had not run; lower-cursor,
egress, and child state remain unconfirmed. V21 splits Timer and Nic through a
QEMU-only successor-before-work cursor. The diagnostic preempts without
advancing it, yielding Timer -> Nic -> DeferredDiagnostic -> Timer. Quarantine
clears diagnostic state but preserves the cursor; generic/Pi paths remain
unchanged. This further temporal cut also adds no capability or authority.
The first successfully latched console turn performs only the value/resource
latch and a lock-free scalar authority fence; mailbox `Retry` performs neither.
Later Recovery turns advance exactly one of fourteen material units: suspend;
unbind; separate scrub/clean and unmap units for each of four shared frames; two
indexed fault-cap deletes; anchor revoke/reset; and `Finalize`. `Finalize`
commits `Complete`; the following idempotent `Complete` turn alone publishes
the exact proof and quarantines the generation. The four shared frames are
scrubbed, cleaned, and unmapped; remaining generation mappings are revoked, not
claimed as data-scrubbed.

Heap-owned session state is then retired without allocator or logger entry by
one ordinary retained-output unit at a time:
`RootSessionTicket -> RootTicketUsage -> NineDoorSessionTicket ->
NineDoorSessionScope -> NineDoorSessionBinds -> PendingStreamCursor ->
PendingStream -> Finalize -> Complete`. Only cleanup `Complete` exposes
conditional reboot/parser/serial/local-seat/tail/detach diagnostics. Existing
service fault, failure, and teardown records retain priority; diagnostics are
committed only after queue admission, never evict older output, survive
backpressure, and are flushed on a later distinct turn.
QEMU acceptance establishes this virtual transport boundary only; it is not Pi
4 NIC or CYW43 evidence, and the current Pi network adapter remains outside
this QEMU-first construction and activation path until the separate hardware
phase.

The live v5 candidate is failure evidence: the active child consumed exactly
`3000 us` and raised timeout badge `0x26ee0007` at `Send` after
`publish_exchange`; root then consumed exactly `2750 us` during
containment/quarantine and raised timeout badge `0x26ee0001` at the sole outer
yield. The live v6 root ELF
`0059fd675b476106888d6ca62c8bba21f9b340b9aa607e000fbf96997fd29900`
then raised root timeout badge `0x26ee0001` at the same outer yield after one
Network visit composed an empty ObserveChild, no-op StageOutput and Disconnect,
and a committed/signalled 60-byte ARP ingress sequence 1. The child remained
healthy and no Recovery ran. The v7 security boundary therefore retains a
persistent lower cursor in the order ObserveChild, StageOutput, Disconnect,
Ingress, ServiceTick and permits exactly one attempted unit per Network visit,
including a no-op. No-op attempts advance; a successful child signal forces the
next lower attempt to ObserveChild. Deferred diagnostics and retained TX
preemptions preserve both the cursor and forced-observe state. This changes no
authority, sole-yield rule, Recovery contract, Pi path, or non-VirtIO path.
The next v7 run used root ELF
`d2f69bddbf56deef6919ec6ea802e9d3c44a691c2dbe05aa59428854bbf7a6ae`
and timed out while the UART-visible `[mark] root-console.start.ok` remained
queued, before Network or Recovery. That absent wire marker is not a source
lifecycle boundary. Root consumed exactly `2750 us` and raised current-fault
`Timeout`, badge `0x26ee0001`, at serial queue `inner_dequeue` (PC `0x43e84`)
called from `SerialPort::flush_tx_unlocked` (LR `0x77b74`). The v8 boundary
therefore adds the one shared Operator serial-I/O credit while retaining the v7
Network and Recovery cuts.
The canonical v8 root ELF
`5052e7a5070987c252d3c1f5cf6f27172bd5ece1836a8f6c2a5c329c789a0a61`
then exhausted the complete `2750 us` root-control refill and raised
current-fault `Timeout`, badge `0x26ee0001`, at PC `0xede84` immediately after
`emit_prompt_now`, despite the active `64`-byte serial credit. This falsifies
v8's remaining multi-record Operator composition. The v9 security boundary
therefore admits at most one retained output-record attempt when the generated
VirtIO serial limit is nonzero and retains the rest for later Operators.
The canonical v9 root ELF
`fa488c9367136f0eadef7182a18691664c3ae51c2ac2974e12000ff5d27f38ed`
and CPIO
`aca549e99e0d86299e9f98348d896b730259277654544ebd22a74595b61e9bfb`
then exhausted the complete first post-bind `2750 us` refill and raised
current-fault `Timeout`, badge `0x26ee0001`, at PC `0x13a798`, the first
instruction of `compiler_builtins` `memmove`; LR `0x79ccc` was
`heapless::Vec<PendingConsoleOutput, 72>::remove(0)` with prospective
`x2 = 0x110`. Zero bytes were copied, serial was idle, the one-record cursor
was full, and the queued marker plus prompt were unchanged. This falsifies v9
through aggregate first-post-bind refill exhaustion. It does not justify a
copy-specific repair and does not falsify the one-record security bound. V10
adds only the post-activation replenishment boundary described above.

The canonical v10 root ELF SHA-256 was
`022908395c954f73a67136f70fe4404d96e0cf1ff16f4531fa95eae7a6f57cb5`.
The one-time activation boundary completed, and UART emitted the retained
startup marker and prompt in separate bounded Operator visits. The second fresh
Runtime then consumed the complete `2750 us` and raised root timeout badge
`0x26ee0001` at PC `0xce98c`, the `seL4_NBWait`/nonblocking receive on root
endpoint `0x0a70`. Its successor was Network, the output FIFO and record cursor
were empty/inactive, and the response barrier had crossed the prompt. This
falsifies v10's composed Runtime work without weakening the retained activation,
serial/output, Network, or Recovery boundary.

The same run recorded console timeout sequence 1, badge `0x26ee0007`, with
Terminal policy. The saved child was at `seL4_Wait` with
`service_pending = 1` and `control_pending = 1`, proving a completed logical
unit composed with its pending successor on residual SC. Recovery reached
Complete with the TCB suspended, SC unbound, mappings scrubbed, capabilities
revoked, objects deleted, and generation fenced; NineDoor remained healthy.
The canonical v11/v4 run then used root ELF
`44971429e4941d751248c216082256f01e187930d9a6d40028e5c89d8611b597`,
console child ELF
`af08f817191cc51c9354b61f09f3eeb50c8cdf875c660c7231987a426886666d`,
and CPIO
`9fbb58e1dc6dc508361f37ce0c24219e3e9029dae101e2be789df1bcb1a5b11d`.
There were four TCP connects. The first three completed authentication attempts
each wrote 18 bytes and read zero; the fourth connect had no completed
authentication record. The child exhausted `3000 us` with timeout badge
`0x26ee0007` at PC `0x213458`, the `seL4_Yield` immediately after the composite
`PollService` completed and cleared; saved retained state identified
`PollService` as that completed unit. After containment reached `Complete(6)`, root exhausted
`2750 us` at outer-Yield PC `0xf5fbc` after an empty Operator, with timeout
badge `0x26ee0001`, ordinary successor `Runtime`, retained Runtime successor
`ControlEndpoint`, and empty output. This is direct failure evidence for v11/v4,
not an authentication denial, incomplete Recovery, or qualification result.

The next non-claiming convergence run,
`out/test-plan-convergence/v12-v5-auth-20260812T010200Z`, bound root ELF
`7cec5bd582d063adc73830af8cc62e0ec8dbbb33d91bd4701db09ca69e32e6ca`,
console child ELF
`920883c5e706688a65e7f168a643dbc527d09d7f48584bfb41fbd0c0ae823cb6`,
and CPIO
`dc36495a5de0df13bfb853ffa33fdc6e7ccc3bbf3a1a3c8c4cd74c8551160c16`.
All four authentication attempts wrote 18 bytes and read zero. The only timeout
was root badge `0x26ee0001` after exactly `2750 us` at outer-Yield PC
`0xf612c`. Stored ordinary successor `Network(2)`, Runtime successor
`StreamFlush(3)`, and empty staged bootstrap `Option` prove that the completed
phase was Runtime and its selected unit was empty `BootstrapDrain`. Fault
sequence 2 and the console child healthy at Yield-then-Wait exclude an earlier
child fault or Recovery. The result embedded dirty source commit
`a533290ffe264f0a2bf0af3db4bb4c45d1a4a278`, while HEAD later advanced to
`84934dda6`; it is diagnostic/failure evidence only and falsifies the generic
Runtime-without-control prelude composed with that no-op unit.

The next v13/v5 non-claiming convergence run bound dirty source commit
`84934dda6fcffbfa536d4e437cc1904c7fdeb0b1`, root ELF
`0275cd7d701263cc1731ca3301d9aeab8a0393651745659f192106a0d558d78f`,
the unchanged v5 child
`920883c5e706688a65e7f168a643dbc527d09d7f48584bfb41fbd0c0ae823cb6`,
and CPIO
`142e2aec64662888a9872ff77ff85d1f5f7c351b7aaa478ded8cf99ba9e64f29`.
All four authentication attempts wrote 18 bytes and read zero. Root-control
initiated the failure at child-notification `sel4::poll` SVC PC `0xce98c`,
badge `0x26ee0001`, while the child remained healthy at `seL4_Wait`.
Root-fault then timed out at `suspend_tcb` SVC PC `0xce0cc`, badge
`0x26ee0002`, targeting root-control cap `0x10`; emergency fail-stop was
downstream. This falsifies v13's composed Network adapter path and v2's
receive/classify/suspend terminal-critical path, not child v5.

V14 gives the isolated Network phase a compact timer/timebase plus
network-ready-HDMI prelude followed by exactly one budgeted NIC unit. The lower
successor commits before dispatch to one distinct noinline adapter helper; no
all-unit closure, generic Runtime tail, event drain, or command dispatch runs in
that phase. The following Operator admits at most one retained connection event
before a buffered command. Root-fault v3 separately retains private
`Receive -> SuspendCritical -> SignalEmergency` terminal-critical units across
replenishments. Receive commits SuspendCritical before yielding; the fresh
suspend unit commits SignalEmergency before resolving and suspending the exact
child-local TCB cap, then yields; the fresh signal unit commits Receive before
signalling root-emergency and yields before another blocking receive. This
keeps the sole Reply association serialized through the emergency signal and
leaves Worker, driver, service, and recoverable handling unchanged. The v14
root-control, v3 root-fault, and v5 child candidates change
only those temporal unit boundaries; schema 1.11, numeric resource/timing
fields, capability, ABI, grammar, serial, output, and authority contracts remain
unchanged. All three remain pending fresh canonical QEMU authentication and
fault injection.

The exact v16 image later bound root ELF
`4fab7abc8707b9829ba66ac525efdfc7afefa812df4bab9abb8cb67d504a76a6`
and CPIO
`456558cac05e4d136d3cbc18d1290cc48bebf619ba5459cd623b667dbfff3e96`.
The prompt serial/output completed, but root-control consumed the full
`2750 us` and faulted at outer-Yield PC `0xf61c4`; saved successors `Runtime`
and `ControlEndpoint` identify the completed phase as Operator. Target
disassembly showed that the selected route still paid approximately `0x42c0`
bytes of generic EventPump frame and `0x12a0` bytes of generic Operator frame
before its bounded output leaf. Root-fault then consumed the full `3000 us` at
the first post-classification Yield, PC `0x113938`, before suspension or
emergency signal. These are failure evidence, not qualification.

The exact v17 non-claiming run
`out/test-plan-convergence/v17-v4-auth-20260812T041428Z` bound root ELF
`3d0641bac42d21ce383c47f38628a05db0d2474fab69fc6e14b67ba39a71bd47`,
the unchanged v5 child
`920883c5e706688a65e7f168a643dbc527d09d7f48584bfb41fbd0c0ae823cb6`,
and CPIO
`fa478638d6d2b93b654a2615e4dcd1e1d7f666d0945d4e012adcf28da2292af1`.
All four authentication attempts wrote 18 bytes and read zero. Current fault
`.1` was root-control at outer-Yield PC `0xf6624`, with committed ordinary,
Runtime, and Operator successors `Runtime`, `Worker`, and `SerialDispatch`.
This proves compact dispatch and attribution but falsifies composing serial
driver admission/RX and TX flush in the selected `SerialIo` leaf; root-fault
v4 and child v5 were not falsified. V18 also suppresses the raw-UART RX trace
only while that admitted ordinary root-control turn is active, preventing
diagnostic formatting from joining the RX unit without changing generic/Pi
tracing.

The exact v18 artifact bound root ELF
`e7d34f018ff308c575fedb79ca7cef5542a7da8e753c09ddb9d55cf9daa79d4e`
and CPIO
`0dca41cc6fdd9a877144dcd2db610beaeafef95423a81ce6896b01bb9b8f5cf5`.
All four authentication attempts wrote 18 bytes and read zero. Root-control
consumed exactly `2750 us` and timed out at outer-Yield FaultIP `0xf66e4` after
Network. Ordinary successor `Operator(0)` and lower successor `Disconnect(2)`
identify selected `StageOutput(1)` with no pending egress or child signal.
Root-fault timeout `.2` at `suspend_tcb` SVC PC `0xce1f4`, with retained cursor
`SignalEmergency`, was downstream. V19 narrowed only the split QEMU Network
prelude; root-fault v4, child v5, authority, and external security contracts
remained unchanged. The exact clean v19 root/CPIO hashes were
`0737a6f008197fd5b931af104c95164ddcd925fa04a8440439895c1e76b26fca`
and `51e7b955b449b42b7a0cad569aa187e19a0f71464ffb81080d29733a589e7ed0`.
All four authentication attempts wrote 18 bytes and read zero. Root-control
timed out at outer-Yield PC `0xf66dc` after completed Network. Lower successor
`Ingress(3)` proves selected `Disconnect(2)` was a no-op without child signal;
pending egress was empty, the child was healthy at Wait PC `0x21343c`, and
root `smoltcp_polls` was `250098`. That failure isolates the post-leaf
counter-refresh, NETDIAG, and NineDoor aggregate.

The current QEMU root-control provenance is
`m26e-qemu-root-disconnect-single-issue-candidate-v26`; Pi retains V23. V26
keeps V25's envelope, V24's ACK split, and V23's compact predispatch
attempts physical-tail reconciliation or prompt-tail queueing before phase
selection. Clearing the relevant predicate returns exclusively; a still-pending
bounded attempt may run exactly one compact Operator unit before return, without
ordinary-phase advance. Ready reboot remains exclusive, and Runtime/Network
cursors are preserved. After ObserveChild validates, copies, and durably
retains a publication, V24 records ACK debt and returns. A later
`AcknowledgePublication` Network unit has priority over diagnostic, egress, and
lower work, clears the debt before Release+Signal, and on success forces the
following lower unit to ObserveChild. Root-fault provenance is
`m26e-qemu-root-fault-service-units-candidate-v6`; its exact cursor starts once
at `PrimeReceive`, then recurs as
`Receive -> Classify`, followed by either the legacy critical path
`SuspendCritical -> SignalEmergency -> Receive` or the service path
`ResolveService -> SuspendService -> RecoverPassiveService -> PublishService ->
Receive`. Active console services skip `RecoverPassiveService`. PrimeReceive
commits Receive and yields before any receive, copied value, or Reply
association. Receive commits
Classify before blocking receive, copies only label/badge, then yields.
Released classifications yield before another Receive. RetainedByDriver waits
for and validates the exact release badge and cleared busy state, then yields.
Critical commits SuspendCritical and yields; SuspendCritical commits
SignalEmergency, suspends the registered TCB, and yields; SignalEmergency
commits Receive, signals, and yields. Service resolution is one fixed generated
lookup plus a nonblocking registry-lock/scalar-snapshot attempt; contention
retries without loss. Service suspension is one quiet bounded syscall, passive
recovery issues at most one Reply while active console recovery issues zero,
and publication is one mailbox action that retains the snapshot on
backpressure. The sole Reply association remains serialized and every numeric,
ABI, capability, grammar, and authority value is unchanged. A sender arriving during PrimeReceive can remain queued on the
already constructed shared endpoint; the root-fault child is runnable but has
not accepted a message or created a Reply association. The immutable V29
artifact completed AUTH and ATTACH, then HELP returned zero bytes before the
unchanged 30-second timeout. Clean root/child snapshots and breakpoint plus
live-vtable proof showed the selected `DefaultNetStack` returned its default
bounded identity instead of delegating the isolated hooks. The later V30/V15
terminal-timeout artifact completed HELP and NETSTATS before terminalizing at
the Yield SVC with adjacent refills totalling `3000 us`. A non-claiming
local-Poll diagnostic then completed HELP, NETSTATS, and correct SMP16 on its
first boot; its second fresh boot reached `root-console.start.ok` and emitted
`root-emergency fail-stop` before a TCP probe. These are failure diagnostics,
not qualification. The subsequent V31/V17 Stage 03 passed its fixed matrix and
three operational `.coh` scripts before a third rapid TAIL reached target
command end but missed the unchanged five-second client response deadline;
CAT and QUIT were not reached. That result is failure evidence, not
qualification. The subsequent V32/V17 Stage 03 passed its fixed matrix and
`boot_v0.coh`, then failed `9p_batch.coh` because routine diagnostics entered
public `/log/queen.log` and displaced its exact ordered ECHO payload from the
bounded CAT preview. Only one selected operational `.coh` script passed before
the stop. That is V32 failure evidence, not qualification. The later V33/V17
run `out/m26e-qemu/stage03-v33-v17-20260814T031936Z` passed the fixed matrix
7/7, `boot_v0.coh`, and `9p_batch.coh`, then exact same-artifact evidence
identified root-control task 0 timeout badge `0x26ee0001`, label 5, immediately
after the final QUIT audit and before `host_absent.coh`; root-emergency was the
downstream fail-stop. Exact base/gated identities are retained in
[TEST_PLAN.md](TEST_PLAN.md). This is V33 failure evidence for V34.
The subsequent immutable V34/V18 staged state
`out/test-plan/m26e-console-qemu-v34-v18-oraclefix-20260814T104728Z`, Stage 03
attempt `20260814T105938.465736Z-11947-27f75501fecd`, passed Stage 01 and
Stage 02. Its Stage 03 base/gated artifact IDs were
`sha256:11921e2eedbf8e9c46f781c500b89acdcb9669ebda42eb6db0ed21a4eb47dac3`
and
`sha256:46ce91c8bffae218f557fedb19ec125cdded39118db641aee70db9e63949163b`.
The fixed matrix passed 7/7, all ten base scripts passed including 9P and
`session_pool.coh`, and a fresh telemetry boot passed `telemetry_ring.coh`;
`telemetry_push_create.coh` then failed when each replacement connection wrote
the complete 18-byte AUTH frame and read zero bytes. Immutable replay proved root-control task 0, timeout badge
`0x26ee0001`, label 5, at the outer Yield after ordinary Network/Timer with no
timer trace and tick `356343`, not divisible by 8,000. The adjacent refill
amounts were the exhausted current `38,090` ticks and the already-valid next
`93,910` ticks; together they equal the unchanged `132,000` ticks or
`5,500 us` at 24 MHz. Installing the terminal timeout endpoint converted
exhaustion of only the current refill, despite the valid adjacent refill, into
root-fault and downstream fail-stop. This is the exact security boundary for
the common `NaturalPostpone` repair authorized by discovery task
`m26e-console-network-service-isolation` and reopened Milestone 25 root-service
temporal restoration carried by `m26e-root-tcb-target-proof`.
V35/root-fault-V6/child-V18 remain pending fresh canonical QEMU
proof.
Fresh four-core GICv3 QEMU fixed response, repeated two-session lifecycle
regression, standard-fault terminal containment, and budget-exhaustion natural
postponement liveness/isolation remain
mandatory before the focused direct base `.coh` batch, Hive Gateway REST
core/parity plus Python smoke, Conditional D performance, or host-tool
validation. The selected QEMU root is
`m26e-qemu-root-adjacent-refill-natural-postpone-candidate-v35`; the selected
Pi root is `m26e-pi4-root-adjacent-refill-natural-postpone-candidate-v24`.
Both keep every temporal numeric and clock unchanged, omit only the root TCB
timeout endpoint, retain the reserved timeout authority, and keep standard
faults terminal. V35 preserves V34/V33/V32/V31/V30/V29's capture contract, V27's response cadence,
V28's terminal fence, V25's envelope, V24's ACK split, and V23's one NIC
service per three featured Network visits, so
performance must be measured rather than claimed from the unchanged interface
contract.
The repair changes no schema, API, wire frame, workload, retry, timeout,
host-tool, Python-library, benchmark, evidence-record, or report-schema
contract; the full cross-surface review requires no hand-authored compatibility
edit. Fresh V35 QEMU must pass complete staged, full `.coh`, REST, host-tool,
Python, and performance gates. QEMU cannot qualify Pi; V24 separately requires
a fresh generated 54 MHz build, flash/readback, cold boot, and applicable
hardware/performance proof.

Console parsing uses fixed-capacity buffers and a shared finite-state command
parser. A leaky-bucket rate limiter allows two failed authentication attempts in
a 60-second window; the next failure enters a 90-second cooldown. Root-task
adds bounded exponential backoff beginning at 250 ms for repeated authentication
failures. Denials and successful role assertions emit audit lines, while
pressure refusals use the bounded `busy`, `quota`, `cut`, and `policy`
categories exposed through `/proc/pressure/*`.

## Input, protocol, and memory controls

All user-controlled console lines, paths, 9P frames, JSON records, tickets, and
configuration values must be validated before side effects. The public
Secure9P red lines are 9P2000.L only, `msize <= 8192`, walk depth at most 8, no
`..`, and no fid reuse after clunk. Short writes, tag concurrency, cursor
advances, scope rates, and retained bytes are generated and bounded. See
[Secure9P](SECURE9P.md) for session invariants and
[Interfaces](INTERFACES.md) for record schemas.

Network RX/TX, serial, pending console lines, driver rings, logs, telemetry,
evidence, and retry work use fixed or manifest-bounded storage. Overload must
surface a deterministic refusal or counter; it must not create an unbounded
queue or silent retry loop. The event pump serializes authoritative target
state. SMP is used for separate single-threaded tasks, not shared-memory
multithreading of authority state.

The Milestone 26e namespace-service contract treats path, payload, partial
frame, sequence, and generation fields as hostile. The as-built
`nine-door-runtime` source validates them in a restricted `no_std` child and
returns only a typed bounded prepared operation; the root-side contract
independently checks the exact response identity and bytes before policy or
mutation. Root's endpoint cap is Write + GrantReply with neither Read nor
Grant, while the child's endpoint cap is Read-only. Request and response
mappings are directionally restricted, exactly two pages each, and backed by
disjoint live frame handles validated against generated virtual addresses.
The QEMU constructor validates the embedded image digest, entry, and W^X load
span, allocates only the compiler-budgeted anchor generation, and registers the
still-suspended TCB before registry seal. During construction, root configures
and binds one compiler-budgeted, root-retained bootstrap scheduling context;
the child receives neither that scheduling-context cap nor `SchedControl`
authority. After registry seal and root-fault activation, root resumes the
child, validates an empty `Log` parser probe, observes the child's atomic
`ReplyRecv` transition into the next receive, and unbinds the exact bootstrap
scheduling context before declaring the service passive. Activation, probe, or
unbind failure revokes the namespace boundary and suspends the child where
possible, so no later Call can block on a failed bootstrap. The receive-loop
Reply object is shared only with the generated root-fault recovery slot. On
fault, an outstanding donor receives exactly one typed `Closed` failure before
the durable containment record is published;
without an outstanding Call no Reply is attempted. Recovery authority is then
retained and serialized until all four request/response mapping lifecycles are
scrubbed and unmapped. The recovery Reply cap is then quietly deleted before
the two fault caps and retained anchor are revoked. Root-control advances that fixed NineDoor containment cursor one
material unit per exclusive Recovery turn and only after all higher-priority
console-network recovery work is absent. Steady operation is passive donation
only after the bootstrap
scheduling context has been unbound; no `SetAffinity` placement path is used.
The active console service cannot enter this passive path. Closing
with a partial frame, queue saturation, cancellation, child-generation
revocation, replay, and late completion fail closed. The child receives only
its service endpoint/Reply object and two bounded shared-frame mappings; it
receives no Queen policy, root CSpace, device, broad namespace, scheduling
context cap, or `SchedControl` authority. The bootstrap scheduling-context
candidate remains subject to live QEMU qualification. This internal ABI does
not relax the rule
that the authenticated console is the only in-VM TCP listener. Live QEMU fault
injection remains evidence required beyond source and target checks; it is not
Pi hardware acceptance.

Operational host/GPU projection begins `unavailable source=none`; target source
contains no fabricated provider or GPU topology. GPU snapshots use
`gpu-bridge-snapshot/v2` and arrive only through an authenticated Queen
session. Root validates the production source identity, epoch and strictly
increasing sequence, observation time, bounded TTL, catalog and per-manifest
digests, CAS/base/adapter compatibility, and activation generation/receipt
before atomically replacing a generation. Fixture-mode, stale, replayed,
forged, incompatible, or oversized snapshots fail closed. Accepted data is
withdrawn at TTL and direct writes to the active-model pointer are denied.
Console/REST placeholder credentials fail before connection, and fixture
signing keys are forbidden from operational target and release closures.
Operational manifests select `cas.signing.verification_key_path`; coh-rtc
validates a public Ed25519 point and emits only those public bytes. The
corresponding signing key remains in an external secret store. The checked-in
fixture signing seed and its public counterpart are test-only and cannot be
selected by the QEMU/Pi runtime or release manifest.

<!-- coh-rtc:ticket-quotas:start -->
### Ticket quota limits (generated)
- `ticket_limits.max_scopes`: `8`
- `ticket_limits.max_scope_path_len`: `128`
- `ticket_limits.max_scope_rate_per_s`: `64` (0 = unlimited)
- `ticket_limits.bandwidth_bytes`: `131072` (0 = unlimited)
- `ticket_limits.cursor_resumes`: `16` (0 = unlimited)
- `ticket_limits.cursor_advances`: `256` (0 = unlimited)

_Generated by coh-rtc (sha256: `1b869521f68c26d43c1ad278fbc557f2442e438ab12d443a142e53a33e4466fb`)._
<!-- coh-rtc:ticket-quotas:end -->

These values are the committed default-profile snapshot. The selected source
manifest and resolved manifest govern a target build.

## Hardware, DMA, and firmware

QEMU is the reference development and regression target; it is not physical
hardware proof. On Raspberry Pi 4, selected physical devices are admitted to
isolated driver runtimes after HAL coverage checks. BCM2711 has no supported
IOMMU/SMMU isolation path in the current Cohesix profile, so DMA safety relies
on HAL-owned ranges, bounded rings, cache policy, quarantine rules, and
single-owner driver admission. Documentation must not describe that as hardware
DMA isolation.

The GENETv5 implementation uses Linux device-tree and driver behavior plus
U-Boot bring-up as reference material. The CYW43455/SDIO implementation uses
OpenBSD `bwfm`, Zephyr/Infineon WHD layering, and Linux `brcmfmac` edge cases as
reference material. These are provenance references, not code sources; source
lift is prohibited. CYW43455 and SDIO runtimes are implemented research
surfaces, but current-image association, DHCP, TCP, and repeatability remain
evidence-gated in the build plan.

Pi firmware, U-Boot, Wi-Fi firmware/NVRAM, and the external seL4 build are
deployment dependencies. Pin their provenance, verify staged hashes, and keep
flash proof separate from proof that the board booted the same image. The
hardware runbook owns the accepted evidence sequence.

## Secrets and sensitive data

- Do not commit deployment credentials or copy them into examples, issue text,
  evidence, command history, or screenshots.
- Root-task and the primary `coh`, `cohsh`, Python, and live-gateway paths reject
  the literal placeholder `changeme`. Some ancillary compatibility binaries
  still expose legacy placeholder defaults; that value cannot authenticate a
  current target and must be overridden. Select real, distinct target-console
  and gateway request secrets.
- Pass secrets through protected deployment configuration or environment
  variables, prefer hidden shell input, and unset them after use.
- Host-ticket arguments must contain opaque secret references, not bearer
  tokens or raw credentials.
- Treat manifest ticket secrets, Wi-Fi PSKs, CAS signing material, evidence
  packs, and raw serial logs as sensitive even when the repository contains
  development fixtures.

Boot-time `cohesix.env` may persist only documented network policy fields. It is
not a general secret store, and writing saved policy is a separate proof state
from successfully using that policy on the current image.

## Audit, evidence, and replay

Security-relevant accepts and denials write bounded audit lines to
`/log/queen.log` and, when enabled by the selected profile, `/audit` records.
Host ticket actions use versioned, allowlisted schemas, idempotency keys, and
explicit lifecycle receipts so operators can distinguish requested, claimed,
running, terminal, and dead-letter states.

`coh evidence pack` records captured and missing paths instead of inventing
data. It hashes audit `ticket` fields and recursively redacts JSON keys
containing token, secret, password, signing-key, or API-key material. Redaction
reduces accidental disclosure; it is not a substitute for reviewing a pack
before sharing it. The exact pack contract and CI/SIEM recipes live in
[Operator recipes](OPERATOR_RECIPES.md#capture-an-evidence-pack).

Replay is limited to retained Cohesix control-plane records. It cannot recreate
external host state, reverse a host side effect, or prove that an omitted event
did not occur. Policy approvals are single-use; replaying a consumed approval
fails deterministically and emits an audit record.

## Sidecars and host actions

Sidecar mounts and providers are manifest-gated. Namespace collisions receive
deterministic hash-prefixed labels; role and path scopes are checked on each
operation. Offline spool and replay are bounded by selected manifest limits and
must not exceed Secure9P `msize`. Sidecars do not add in-VM listeners.

Host actions under `/host/tickets/*` are requests, not implicit target access to
the host. The host ticket agent validates schema, action allowlist, arguments,
idempotency, and state before a configured host adapter performs a side effect.
Use dedicated host identities, least-privilege adapter configuration, and the
request/result/federation contracts in [Interfaces](INTERFACES.md#host-tickets-and-federation).

## Content-addressed storage

CAS updates are file-backed and Queen-writable; they do not create another
network service. Chunks are verified by SHA-256, invalid content is quarantined
and audited, and signature requirements are selected by the manifest. A delta
must identify and validate a non-delta base epoch before it can be accepted.

<!-- coh-rtc:cas-security:start -->
### CAS integrity stance (generated)
- `cas.signing.required`: `true`
- Hash mismatches are rejected, quarantined, and audited without side effects.
- Signature failures emit deterministic ERR plus audit entries.
- `/models` exposure remains gated by `ecosystem.models.enable`.

_Generated by coh-rtc (sha256: `674f8c3ed5412b48f6d8e4804d75735aa6b40237b15fa0be463f06e777132101`)._
<!-- coh-rtc:cas-security:end -->

## Generated observability limits

<!-- coh-rtc:observability-security:start -->
### Observability tolerances (generated)
- `observability.proc_ingest.latency_samples`: `32`
- `observability.proc_ingest.latency_tolerance_ms`: `5`
- `observability.proc_ingest.counter_tolerance`: `1`
- `observability.proc_ingest.watch_min_interval_ms`: `50`

_Generated by coh-rtc (sha256: `aae20e12321a8a009e32d6e163c28d7ab51ca76a211a6ef0f1dd753f88b1c6ce`)._
<!-- coh-rtc:observability-security:end -->

The generated `cohsh` pooling, retry, heartbeat, and trace limits are owned by
[`docs/snippets/cohsh_ticket_policy.md`](snippets/cohsh_ticket_policy.md) and
embedded in [Userland and CLI](USERLAND_AND_CLI.md); they are not a second
security policy.

<!-- metrics:latency:start -->
### Telemetry Ring Latency (generated)
- Suite: `nine-door/telemetry_ring`
- Samples: `7`
- P50: `0.014 ms`
- P95: `0.025 ms`
- Unit: `ms`
_Generated from `apps/nine-door/out/metrics/telemetry_ring_latency.json`._
<!-- metrics:latency:end -->

This generated microbenchmark record is regression evidence for its named
suite, not an end-to-end deployment latency claim. Benchmark methodology and
publishable report requirements live in [Benchmarks](BENCHMARKS.md).

## Profile-qualified security claims

The committed default manifest is not universal deployment policy. Security
claims must identify the selected source manifest, resolved manifest hash,
seL4 output profile, target, commit, and evidence run. Features disabled by that
profile are absent, not protected by an undocumented fallback.

Current target status and proof boundaries are maintained in
[Hardware bring-up](HARDWARE_BRINGUP.md) and the
[Build plan](BUILD_PLAN.md). The NIST 800-53 crosswalk is an evidence index, not
a certification; see [NIST mapping](SECURITY_NIST_800_53.md).
