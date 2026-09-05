<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define the as-built external transport, namespace, control-file, and schema contracts. -->
<!-- Author: Lukas Bower -->

# External Interfaces

This document is the index and human-authored contract for Cohesix external
interfaces: transport selection, target console framing, namespace paths,
control files, and non-generated record schemas. It links to generated snippets
for compiler-owned values instead of copying them.

Protocol internals belong in [SECURE9P.md](SECURE9P.md), role and ticket policy
in [ROLES_AND_SCHEDULING.md](ROLES_AND_SCHEDULING.md), system boundaries in
[ARCHITECTURE.md](ARCHITECTURE.md), and operator command usage in
[USERLAND_AND_CLI.md](USERLAND_AND_CLI.md).

See the [Glossary](GLOSSARY.md) for Cohesix-specific protocol, role, and
authority terms.

## Stability and authority

The selected profile manifest, resolved manifest, generated tables, and
generated snippets are authoritative for enabled providers, exact capacities,
paths, and defaults. Source and fixtures are authoritative for handwritten
parsers and response grammar.

The following are breaking changes:

- a Secure9P request, response, or error-code change;
- a target console verb or `OK`/`ERR`/`END` grammar change;
- a namespace path, mode, or record-format change;
- a role/ticket authority change; or
- a generated interface, bound, or default change.

Breaking changes require the process in `AGENTS.md`: manifest schema changes
where applicable, regenerated artifacts, updated fixtures, tests, and canonical
documentation in the same authorized change.

### MCS operator-inspection records

The Pi-only verbose `wifi: pair_handoff` v1 diagnostic is additive; its passive
ring layout and field semantics are defined in [DRIVERS.md](DRIVERS.md).
It changes no runtime-init ABI version, generated authority, console command,
authentication/framing, namespace, or REST/SDK/report schema. Absence is
unavailable evidence, never a readiness or performance result.

Pi `smp mcs` adds five passive `mcs_idle*` v1 rows after its existing 22-row
batch, as specified in [USERLAND_AND_CLI.md](USERLAND_AND_CLI.md). They retain
the existing idle fences and timer-enable rejection, with no new scheduling
authority. Ordinary `netstats`, QEMU output, authentication, framing and
host-tool/REST/SDK APIs are unchanged.

The console provides bare `caps`, bare `smp`, and the first `bi` record plus
strict `caps mcs`, `smp mcs`, `[bi:v2]`, `[caps:mcs/v1]`, and
`[smp:mcs/v1]`. Every record labels kernel, generated, or runtime authority.
Early-console and registry contention are typed unavailable states; generated
rows never fabricate live registration, binding, activation, or containment.
`[caps:mcs/v1]` emits one or more independently source-labelled records per
runtime or generated scope. Each record is at most 77 bytes so the Pi linked
HDMI fallback preserves every field; consumers join records by command,
`source`, and `scope` rather than assuming one positional row per scope.

## Pi 4 pre-kernel policy interface

The Pi 4 U-Boot menu is a bounded pre-kernel configuration surface, not a new
Cohesix protocol or authority path. `scripts/pi4-image-build.sh` generates the
executable `boot.cmd`/`boot.scr.uimg` contract. Generic persistent U-Boot
environment storage is disabled; only the FAT-side `cohesix.env` file is
eligible for Cohesix boot-policy import.

The import contract is:

- maximum file size: 384 bytes;
- text line endings: LF or CRLF;
- allowlisted keys only: `coh_net_mode`, `coh_net_interface`,
  `coh_static_ip`, `coh_static_prefix_len`, `coh_static_gateway`,
  `coh_wifi_ssid`, `coh_wifi_psk`, and `coh_show_logo`;
- coherent network tuples: `dhcp|static` plus `wired|wifi`, with static address
  and prefix required for static mode and SSID required for Wi-Fi;
- logo preference: absent or exactly `0|1`; any other value rejects the saved
  policy;
- absent, empty, oversized, malformed, or incoherent input: clear network
  overrides and use selected-manifest defaults.

File presence and saved-network state are deliberately distinct. A logo-only
or cleared `cohesix.env` remains a valid file but produces **Default network
settings active** with the **Boot with default settings** action. A coherent
imported network tuple produces **Saved network settings loaded** with **Boot
with saved settings**. Back/discard transitions reload the file before
returning to **Cohesix boot menu**, so working values cannot masquerade as
persisted policy.

The root menu exposes **Change network settings**, a stateful **Boot logo: On
(select to turn off)** / **Boot logo: Off (select to turn on)** action, **Reset
saved settings to defaults**, **Save settings and restart**, and **Advanced:
Open U-Boot shell**. Reset has a separate **Reset saved settings?** confirmation
page; `1` confirms, `0` cancels, and no persistent state changes before
confirmation. Cancel is the Enter-key default. A confirmed reset writes a
verified default policy rather than requiring deletion of `cohesix.env`.

The menu uses an iterative page dispatcher for root, IPv4 configuration,
network connection, Wi-Fi, manual IPv4, review, and reset-confirmation pages.
Submenus consistently use `0` for **Back** or **Cancel** and `9` for
**Advanced: Open U-Boot shell**. Operator-facing choices are **Automatic
(DHCP)** / **Manual (static IPv4)** and **Ethernet (wired)** / **Wi-Fi
(wireless)**. The review page distinguishes **Boot once without saving** from
**Save settings and restart**; discard reloads persisted policy before returning
to the root menu.

Existing Wi-Fi settings can be kept or changed, and a reset followed by
**Change network settings** creates a fresh saved policy for a different Wi-Fi
network without a reflash. Credential entry is enabled only on the
USB-keyboard/HDMI console. The display explicitly warns that the network name
and password are visible on that display while serial output is disabled;
temporary replacement variables are not part of the export allowlist. SSIDs
are redacted from summaries so untrusted imported text cannot forge terminal or
serial evidence. Save and reset operations export into a bounded buffer, write
`cohesix.env`, verify its size, privately compare a readback copy, and set
success only after exact agreement. Restart is never invoked after a failed
export, write, size check, load, or comparison.

At boot, selected non-empty overrides are projected into the staged DTB as
`/chosen/cohesix,net-mode`, `cohesix,net-interface`,
`cohesix,static-ipv4`, `cohesix,static-prefix-len`,
`cohesix,static-gateway`, `cohesix,wifi-ssid`, and `cohesix,wifi-psk`.
Root-task validates these properties and either selects the DTB-backed policy
or reports a bounded rejection and retains the generated manifest defaults.
The source manifest is never rewritten by U-Boot.

## Transport matrix

| Surface | Wire contract | Authentication and authority | Runtime boundary |
| --- | --- | --- | --- |
| Host/in-process NineDoor | Bounded Secure9P 9P2000.L subset | `Tattach` selects role/identity and carries the ticket | Host `std` server and tests only; not an in-target listener |
| Target serial console | Unframed console lines | Physical serial access, then application `ATTACH` for role/ticket authority | Root-task console |
| Target TCP console | Four-byte little-endian length plus one console line | Transport `AUTH <token>`, then application `ATTACH <role> [ticket]` | `console-network-runtime` smoltcp listener; the only permitted in-target TCP listener |
| REST, UI, GPU, sidecar, and federation tools | Host-specific projection | Must preserve the underlying ticket and namespace authority | Host only; never a new VM authority path |

Secure9P messages are not transported through the target TCP console. A
console command such as `CAT` or `ECHO` invokes the target namespace adapter; it
does not synthesize a 9P frame on the wire.

### Internal target namespace-service ABI

`namespace-service/v1` is an internal, pointer-free ABI between root and the
restricted `nine-door-runtime` child. It is not an operator transport, host
RPC, or second policy plane.

The 32-byte request header carries the ABI and header versions, operation,
sequence, supervisor generation, bounded path and payload lengths, and
zero-valued reserved fields. Path and payload bytes occupy separate shared
frames bounded to 256 and 4096 bytes. The child prepares only the declared
`attach`, `tail`, `spawn`, `kill`, `echo`, `cat`, `list`, and `log`
operations; absolute paths have at most eight components and reject `.` and
`..`.

The response repeats the operation, sequence, and generation with a typed
status and bounded prepared bytes. Root independently validates that identity
before ticket, Queen-policy, or mutation authority is exercised. Unknown
labels, operations, error codes, extra capabilities, short replies, stale
generations, mismatched bytes, and partial records fail closed.

Each direction uses two distinct 4 KiB pages with complementary read/write
mappings. Shared records contain no pointers. Root's endpoint capability is
Write + GrantReply without Read or Grant; the child's receive capability is
Read-only. The selected child image is digest-bound and mapped W^X.

The service receives one compiler-budgeted bootstrap scheduling context while
suspended. After registry seal, root resumes it, validates one empty parser
probe, observes its atomic `ReplyRecv` transition, and removes that scheduling
context. Steady-state work runs only on the bounded root-donated Call/Reply
chain. Fault or activation failure closes any exact outstanding caller, scrubs
and unmaps the shared frames, removes Reply authority, and revokes the retained
child anchor before reuse.

### Internal target console-network ABI

`console-network-service/v6` is the private ABI of the active-SC
`console-network-runtime` child. The child owns target Ethernet/IP/TCP state,
frame parsing, transport authentication, and bounded response emission. It is
not the passive NineDoor service and is not a second external protocol.

The console-network ABI uses four fixed 4096-byte pointer-free, sequence-last
pages:

- root-produced packet ingress and control, mapped read-only in the child; and
- child-produced packet egress and events, mapped read-write in the child.

Compact packet and control/event headers plus the validated active payload are
authoritative. Inactive tails are not protocol fields. Producers clear commit,
write the validated fields and bytes, publish the final sequence with release
ordering, and signal only afterward. Consumers accept a record only when the
surrounding commit observations agree and all reserved fields are zero.

Control kind `SendBatch = 3` carries encoding version 1, one through eight
records, exact used length, zero reserved fields, and `1..=256` bytes of valid
UTF-8 per record with no CR or LF. Root encodes the complete batch before
publication; the child validates the complete copied payload before emitting
the unchanged sequence of individually length-prefixed records. Internally, the
child may admit consecutive records as one at-most-1,400-byte socket wire train,
below the qualified Pi TCP MSS and always ending at a complete record boundary.
A maximum eight-record, 256-byte-per-record batch therefore uses exactly two
trains of five and three records; smaller records may fit in one. Preparation
does not advance the private response cursor. Only a complete socket enqueue
commits the next cursor, and that commit must still match the authenticated
connection, batch identity, and prepared cursor. Backpressure or stale identity
consumes nothing. A partial enqueue is a terminal invariant failure and never
advances or retries the cursor. The train is only an internal enqueue unit: it
adds no aggregate external frame, changes no four-byte length prefix, and does
not change ABI v6 or the public `AUTH`/`ATTACH` and `OK`/`ERR`/`END` grammar.
Legacy `SendLine = 1` and Disconnect remain valid controls.

Event kind `CommandBatch = 27` carries encoding version 1 and one through eight
consecutive authenticated commands for one exact connection. Each record
retains its original `now_ms`, exact length, and `1..=2304` UTF-8 bytes. The
child batches only records already admitted in FIFO order, stops before the
fixed 2368-byte payload bound, and never batches across a lifecycle or
connection-identity event. Root validates the complete private copy and
reserves capacity for every command before admitting any of them. One batch
consumes one publication credit; it does not add capability, scheduling, or
command authority.

One complete wire-frame or wire-train commit may retain one following bounded
service cycle. A no-progress cycle quiesces; pending state without sendability
or capacity does not create an uncredited spin. Publication requires explicit
credit returned only after root has validated and durably handled every
indicated event and egress record. Duplicate, stale, late, or
identity-mismatched acknowledgements fail closed.

The generated descriptor supplies target timer frequency, image identity,
mappings, capabilities, notifications, scheduling authority, and fault routes.
Root policy, role attachment, namespace mutation, and command execution remain
outside the child. The public `AUTH`/`ATTACH` and `OK`/`ERR`/`END`
contract is unchanged.

ABI v6 assigns the descriptor's former `reserved0` field to fixed child slot 6.
HAL mints into that slot only a Write-only, badge-1 capability to the additional
root-control wake notification; it grants no Read, Reply, Grant, scheduling,
packet, or policy authority. After committing any durable child event or packet
TX publication, the child signals the existing component-specific notification
first and this fan-in notification second. Both notifications are coalescing
scheduling hints. The sequence-last shared record remains authoritative, so
root must recheck the exact durable condition before sleeping and must never
infer publication or identity from the wake badge alone. The descriptor change
does not alter the four shared pages or any public framing or console grammar.

On Pi, compiler truth places root-control on core 0 and this child on core 2 at
equal priority 200. Direct GENET and mediated WiFi both use the same cross-core
handoff: root commits the durable record, executes a release fence, and signals
the exact one-hot child notification. Neither backend pre-drains the child SC
or calls `SchedContext_YieldTo`. Exact authenticated direct-GENET control or
publication-ACK work may bridge the remaining boundary only through the
condition-before-block root fan-in after every identity, lifecycle, operator,
recovery, containment, and durable-frontier check passes. QEMU direct-VirtIO
retains its existing signal-only selector and cannot consume the Pi fan-in.
Invalid backend, authentication, lifecycle, core, priority, MCP, SC, badge, or
publication state fails closed. The durable record remains work authority;
Signal and the fan-in add no work credit, public frame, or child authority.

When the selected descriptor sets `direct_virtio`, its sealed extension names
one QEMU VirtIO-net MMIO page, the exact IRQHandler slot, two fixed queue
pages, and 16 RX plus 16 TX coherent DMA pages. Only the child maps those
resources during steady state. It negotiates the required modern/MAC/mergeable
RX feature set, rejects multi-buffer or out-of-range completion metadata, and
moves at most one bounded frame per public data-path call. Queue saturation is
typed backpressure; it cannot allocate, extend a descriptor chain, or fall
back to a root-owned adapter. Containment suspends the child, resets the exact
device status register, acknowledges no future IRQ, scrubs the admitted DMA
pages, and then revokes the child bundle.

The compiler derives `direct_genet` only for the exact Pi `bcmgenet-v5`
profile; there is no manifest-authored runtime toggle. The sealed ABI v6
extension names one 32-page CPU-only range shared only by the GENET and console
child generations. Page 0 is the aligned sequence-last control page, pages 1
through 15 are GENET-to-console RX slots, and pages 16 through 31 are
console-to-GENET TX slots. Both children map the pages as cacheable Normal/XN
memory. The descriptor conveys no physical address, MMIO, DMA, device-visible,
or root-policy authority.

Root obtains DHCP through the legacy bounded path, proves every root-mediated
GENET command, RX, and TX cursor quiescent, publishes the exact handoff-pending
generation, and sends one generation-bound zero-payload `DGHO`. Only the exact
`PROGRESS/READY` terminal activates the direct link. During an exact unfaulted
`IDLE/QUIESCING` phase only the bounded legacy drain may run; a retry cannot
switch while the coordinator or root RX/TX queues retain work. After READY,
GENET remains the sole MMIO/DMA/IRQ and private-DMA-ring owner and copies frames
to and from the CPU link. Console-network remains the sole
smoltcp/TCP/authentication owner. Root retains lifecycle, control-event, and
fault supervision but performs no steady packet copy, poll, or GENET packet
command.

Each direction is a fixed single-producer/single-consumer ring. Producers
publish generation-bound monotonic cursor and sequence-last state with release
ordering; consumers validate generation, sequence, length, and stable commit
with acquire ordering. The reciprocal send-only peer notifications are
coalescing wake hints only. Durable cursor state carries work, and both peers
perform a final state recheck before waiting. One direct MCS slice handles at
most one material TX or RX operation, including retained cursor
reconciliation. The GENET owner may retain up to 16 such slices in one dense
window; successive slices alternate their first TX/RX choice, an empty side
donates its slice, and continuous bidirectional pressure receives an exact
eight/eight split. Every slice rechecks the elapsed-time, attempt, progress,
command-freshness, and refill guards; a notification does not itself reset
that accounting. A final slice with no durable successor blocks before any
guard/cap/stalled Yield. Sustained legal work remains bounded by GENET's exact
`3,000 us / 10,000 us` SC and is naturally postponed until replenishment;
standard, protocol, cursor, DMA, IRQ, and paired-containment faults remain
terminal.

Within control page 0, bytes `[0,64)` hold the immutable control header and
bytes `[64,320)` hold the four 64-byte RX-producer, RX-consumer, TX-producer,
and TX-consumer records. Console-network ABI v6 additionally assigns the
formerly reserved bytes `[320,640)` to an optional, separately versioned
direct-GENET diagnostic-v5 record. The record is exactly 320 bytes, aligned to
64 bytes, assigns record-relative offset 12 to the maximum measured direct MCS
packet-slice duration in microseconds, retains offset 108 for cumulative
`dpc_level_adoptions`, and publishes its nonzero sequence last at
record-relative offset 312 (control-page offset 632). Offsets 160, 168, and 176
hold cumulative nonzero receive-boundary notification receipts, receipts
rejected by the exact GENET badge filter, and the bitwise OR of every received
badge. Flag bits 9 through 13 retain whether a command-freshness, elapsed
guard, counter fault, attempt cap, or bounded stalled-retry boundary occurred.
The duration and reason fields are observational and cannot reset a scheduling
window or prove the Pi WCET. The counts are sampled before filtering and grant no DPC, packet,
IRQ-acknowledgement, retry, or scheduling authority. Each actual nonzero GENET
notification is counted once at the initial ring-aware receive or a later
combined poll/wait receive; zero/command wakes, synthetic grants, physical-level
adoption, and unrelated local steady waits are excluded. The GENET child is
the sole writer. Root accepts only two identical stable commit observations around
a bounded copy and an exact match to the live nonzero direct generation;
magic, version, length, flags, counter relations, 32-bit badge width,
IRQ-mask relation, cursor-validity rules, and sequence/commit must all
validate. Older or missing publications remain compatible unavailable
evidence rather than authorizing an alternate path.
Bytes `[640,4096)` remain reserved and non-authoritative.

Diagnostic v5 adds a 128-byte maximum-slice receipt at record offset 184.
It retains the first longest valid slice, its counter timestamps after source
adoption/reclaim, packet work, IRQ work and the final durable check, plus an
optional confirmed RX publication timestamp. Direction and exact RX/TX cursor
identify the packet operation; a validated nonfragmented IPv4/TCP header adds
addresses, ports, sequence, acknowledgement and flags without payload bytes.
Absent stages and reserved fields are zero. Present timestamps must be ordered;
RX and TX are mutually exclusive. An invalid observational sample cannot alter
driver service or replace a valid maximum. Counter ticks use the generated
timer frequency and include descheduling and kernel time. They do not measure
wire arrival, consumed SC budget or console admission, and an unrelated TX or
empty slice maximum cannot be assigned to a delayed incoming TCP request.

The record is not inspected during ordinary direct packet service. One
operator `netstats` request may ask the already active GENET owner for one
idempotent generation-bound `DGHO` replay, retain the stable pre-replay record,
and read the replacement publication before normal post-command idle service.
The replay can wake the owner and thereby allow that existing idle service to
drain durable RX, so this is a causal probe rather than a passive snapshot. It
adds no packet, IRQ-acknowledgement, retry, recovery, polling, or fallback
authority and cannot establish throughput or acceptance.

A failed handoff, invalid cursor or sequence, stale generation, descriptor
drift, peer fault, IRQ completion failure, or containment error poisons the
link, signals the peer when possible, and enters coupled containment. Root
suspends GENET, removes both reciprocal notification caps, and unmaps and
deletes all 32 external console mapping caps before anchor revoke. No fault path
may revive root packet mediation or transfer either peer's authority.

The exact Pi direct-GENET console image spans 66 PT_LOAD pages. Its generated
service inventory is 104 frames (66 image, 32 stack, one IPC buffer, one init
frame, and four ordinary ABI pages) and 161 retained root CSpace slots. The
one-page increase is one immutable executable image frame and its retained
mapping cap; it does not enlarge a data-plane or scheduling budget. The 32
direct pages are reused external GENET frames and therefore add console mapping
caps rather than data-plane frame objects or a larger child untyped. These
private ABI and resource facts do not change the public TCP framing, console
grammar, authentication, attachment, or response schema. Static validation and
image construction do not prove Pi boot, READY, packet correctness, latency,
throughput, QEMU parity, or acceptance.

## Target TCP console sequence

```mermaid
sequenceDiagram
  participant Client as cohsh
  participant Tcp as console-network-runtime
  participant Pump as root-task event pump
  participant Namespace as NineDoorBridge

  Client->>Tcp: framed AUTH token
  alt transport token valid
    Tcp-->>Client: framed OK AUTH
  else invalid or timed out
    Tcp-->>Client: framed ERR AUTH
    Tcp--xClient: close connection
  end

  Client->>Tcp: framed ATTACH role and optional ticket
  Tcp->>Pump: bounded console command
  Pump->>Namespace: validate role and attach context
  alt application authority valid
    Namespace-->>Pump: attached
    Pump-->>Tcp: bounded OK ATTACH response
    Tcp-->>Client: framed OK ATTACH
  else denied or rate limited
    Namespace-->>Pump: refusal
    Pump-->>Tcp: bounded ERR ATTACH response
    Tcp-->>Client: framed ERR ATTACH
  end

  Client->>Tcp: framed TAIL path
  Tcp->>Pump: bounded console command
  Pump->>Namespace: authorize and read retained window
  Namespace-->>Pump: bounded records
  Pump-->>Tcp: bounded OK TAIL response
  Tcp-->>Client: framed OK TAIL
  loop retained records
    Pump-->>Tcp: bounded response record
    Tcp-->>Client: framed record
  end
  Pump-->>Tcp: bounded END response
  Tcp-->>Client: framed END
```

The diagram shows protocol order, not a claim that `NineDoorBridge` is a host
NineDoor server or that every path is available in every profile.

## Target console contract

### Framing

Serial accepts console lines directly. TCP frames every inbound and outbound
line with a four-byte little-endian total length that includes the header.

- Declared total length must be at least 4 and no greater than the Secure9P
  implementation cap of 8192 bytes.
- The isolated QEMU console admits at most 2304 bytes for one authenticated
  command frame. The existing `echo` grammar permits at most 2048 payload
  bytes so one compiler-bounded host-ticket record fits with its verb and path;
  every other command retains its narrower field-specific bound. Physical
  serial/local-seat lines remain bounded independently at 256 bytes.
- Oversized unauthenticated frames terminate authentication. An authenticated
  session receives `ERR FRAME reason=invalid-length`; the child drains exactly
  the complete declared payload across any fragmentation, retains the same
  authenticated connection, and parses the next frame only after that drain.
- The first TCP payload must be exactly `AUTH <token>` with the configured
  token length. Invalid, missing, or late authentication closes the session.

Transport authentication does not select a namespace role. After `OK AUTH`,
the client uses `ATTACH <role> [ticket]`. Queen may omit the ticket; worker
roles require a valid ticket and subject identity. Failed application login
attempts use the current bounded limiter: three failures within 60 seconds
produce a 90-second cooldown.

`ATTACH` binds an application session to a role-scoped namespace; it never
creates or resumes a task. The selected profiles separately admit executable
Heartbeat, GPU, and LoRA roles, but only an authorized `/queen/ctl` lifecycle
operation can select one of their already constructed suspended children. The
child's durable READY record, not `ATTACH` or the control acknowledgement,
publishes its exact namespace identity.

An `OK ATTACH` means the target namespace prepare succeeded and both NineDoor
and root committed the same local role/ticket session. Audit, logger transport,
and boot-tracer observation occur after that authority commit and cannot turn a
successful prepare into `ERR ATTACH`. The logger remains UART+EP mirrored at
this boundary; an optional later EP-only self-test is transport policy, not
application authority.

### Command grammar

The compiler-owned command inventory is generated in
[cohsh_grammar.md](snippets/cohsh_grammar.md). The parser applies per-field
bounds from [`cohsh-core`](../crates/cohsh-core/src/command.rs), including the
2304-byte authenticated network-command limit, 2048-byte `echo` payload limit,
bounded paths/tickets/JSON payloads, and a maximum of 256 requested tail lines.

Commands are case-insensitive at the verb parser where implemented; arguments
remain subject to their command-specific grammar. Unknown verbs, missing
arguments, extra arguments on strict commands, oversized values, and invalid
numeric fields return `ERR` and produce no documented side effect.

### Acknowledgements and streams

- Every accepted command produces `OK <VERB> ...`; every refused command
  produces `ERR <VERB> reason=<reason> ...`.
- Structured namespace streams such as successful `tail`, `cat`, `ls`, and
  `log` emit a leading `OK`, zero or more bounded records, and a terminal
  `END`.
- Bounded diagnostic commands may emit their diagnostic records before the
  terminal `OK`; their checked transcript, rather than the namespace-stream
  ordering rule, is authoritative.
- `PING` responds as `OK PING reply=pong`; it does not emit a bare `PONG`.
- `QUIT` and authenticated `REBOOT` have explicit connection/flush behavior;
  clients must wait for the documented acknowledgement rather than treating a
  disconnect as success. After exact `OK QUIT`, a TCP client half-closes its
  write side and requires peer EOF on the same connection within the existing
  timeout; QUIT is not retried after an ambiguous send or close.
- An `ERR` is terminal for that command and must not be treated as a successful
  no-op.

Some queued Queen commands acknowledge acceptance before the event pump
forwards the work; others validate and apply a bounded provider operation before
acknowledging success. Clients must use the command-specific status and
observability path rather than infer completion timing from a generic `OK`.
Canonical transcripts and negative cases live in
[`tests/integration`](../tests/integration) and the root-task tests.

## Host Secure9P contract

Host NineDoor accepts only `version`, `attach`, `walk`, `open`, `read`, `write`,
and `clunk`. It does not accept `stat`, `create`, or `remove`. The current codec
caps walk depth at eight and each walk component at 64 bytes. Exact session,
batching, fid, offset, and error rules are in [SECURE9P.md](SECURE9P.md).

The checked-in default profile uses one frame per batch. Where a selected
manifest enables larger batches, host NineDoor encodes responses in request
order and preserves response tags. Clients must still correlate by tag.

## Namespace conventions

All paths are absolute UTF-8 paths. `..`, NUL, undeclared wildcard expansion,
and traversal outside the attached role view are rejected. A path appearing in
this catalogue does not guarantee runtime presence: the selected manifest must
enable it and the active adapter must implement it.

Node modes have these meanings:

- **read-only:** clients cannot append or replace content;
- **append-only:** writes add one bounded record at the provider's expected
  offset; random overwrite and truncation are rejected;
- **control:** an append-only node whose record is parsed and may cause a
  bounded side effect; and
- **directory:** walk/list only.

Host providers and the target `NineDoorBridge` overlap but are not identical.
Parity must be established per path: a host-only GPU job, sidecar adapter, or
federation relay is not target evidence. Conversely, the target adapter does
implement selected bounded GPU publication, UI, and `/host` files, but their
presence proves only the namespace projection; operating-system actions and
federated delivery still execute in host agents.

## Core namespace

| Path | Mode | Contract |
| --- | --- | --- |
| `/log/queen.log` | Client read stream; system append | Bounded retained Queen/root log. Console `log`, `tail`, and `cat` are projections over this path. |
| `/proc/*` | Read-only unless a generated node explicitly says otherwise | Bounded session, lifecycle, pressure, ingest, scheduling, lease, and root-reachability observations. |
| `/queen/ctl` | Queen control, JSONL | Root-owned lifecycle control for compiler-selected executable Heartbeat, GPU, and LoRA children plus model-only WorkerBus. A successful spawn/admit write means accepted or queued, never READY; only the exact child's durable READY record publishes its canonical `/shard/<label>/worker/<id>` paths. Kill/teardown is generation-fenced and removes stale authority before recreation. |
| `/queen/lifecycle/ctl` | Queen control, token line | Node lifecycle transitions. |
| `/queen/schedule/ctl` | Queen control, JSONL | Bounded orchestration queue; not a direct seL4 scheduler interface. |
| `/queen/lease/ctl` | Queen control, JSONL | Grant, renew, preempt, and quota state for bounded control-plane leases. |
| `/queen/export/ctl` | Queen control, JSONL | Open and close bounded export windows. |
| `/shard/<label>/worker/<id>/telemetry` | Worker append; authorized read | Canonical worker telemetry path. |
| `/worker/<id>/telemetry` | Compatibility alias | Present only when `sharding.legacy_worker_alias` is enabled. |
| `/queen/telemetry/<device>/...` | Authorized control/append/read | OS-named, bounded telemetry-ingest segments. |

The `/queen/schedule/ctl`, `/queen/lease/ctl`, and `/queen/export/ctl`
readback mirrors retain the newest complete JSONL records within generated
`ctl_max_bytes`. Appending one individually valid record may evict oldest
complete mirror records; it does not erase the separately owned schedule,
lease, or export state. One record larger than the bound is rejected without
mutation, and independent queue/list/window capacity refusals remain typed
errors.

Exact generated `/proc` records and capacities are in
[observability_interfaces.md](snippets/observability_interfaces.md). Exact
sharding state and feature gates are in
[root_task_manifest.md](snippets/root_task_manifest.md).

## Queen control records

Control parsers are strict: unknown operations, invalid transitions, unknown
fields where the parser is strict, duplicate identifiers, out-of-range values,
and capacity exhaustion are deterministic errors.

### Worker and mount control

`/queen/ctl` accepts one JSON object per append. Representative accepted shapes
include:

```json
{"spawn":"heartbeat","ticks":100,"budget":{"ttl_s":120,"ops":500}}
{"kill":"worker-7"}
{"bind":{"from":"/shard","to":"/shadow"}}
{"mount":{"service":"gpu-bridge","at":"/gpu"}}
{"spawn":"gpu","lease":{"gpu_id":"GPU-0","mem_mb":4096,"streams":2,"ttl_s":120}}
{"spawn":"lora","budget":{"ttl_s":120,"ops":64}}
```

A successful parse is not permission by itself. Role, ticket, lifecycle,
provider presence, compiler-selected executable contract, queue capacity, and
available task slot remain mandatory. WorkerBus requests remain model/session
only and cannot acquire target-task authority.

For the selected executable roles an accepted `spawn`/admit record targets one
preconstructed suspended child and returns before READY. Publication occurs
only after the exact child commits READY and signals its generated completion
notification. Kill is generation-fenced, contains and revokes the complete
instance bundle, and prevents stale records or caps from affecting a later
fresh-generation reconstruction. Configuration, an ACK, or a host/model
projection is not target execution evidence.

The selected executable contract is `worker-task-abi/v2`. Its fixed,
pointer-free init, control, READY, completion, GPU-receipt, and PEFT-receipt
records retain sequence-last publication and the complete role/slot/lease/
supervisor/capability identity. Normal execution is passive: the GPU executor
may call only a GPU instance, while the LoRA executor may call LoRA or
Heartbeat. One call carries one validated action label and one instance-owned
Reply object; nested donation, a foreign donor, a stale generation, an unknown
label, or a second in-flight call fails closed. Success uses atomic Reply/Recv
so completion returns the donated SC exactly once before the instance waits
again. Standard or timeout containment may publish one typed failure reply to
release the caller, then revokes the exact generation.

The QEMU and Pi profiles each expose 256 such records (one Heartbeat, 127 GPU,
128 LoRA). Identical sealed role-image code
and read-only data may share physical frames. Writable image pages, stacks,
IPC buffers, shared ABI pages, CSpaces, VSpaces, endpoints, Reply objects, and
fault identities never share between instances.

### Lifecycle control

`/queen/lifecycle/ctl` accepts these single-line tokens:

```text
cordon
drain
resume
quiesce
reset
```

The provider validates the state transition and exposes the resulting state,
reason, and time through `/proc/lifecycle/*`. Worker attach, telemetry ingest,
GPU jobs, and host publication remain lifecycle-gated.

### Schedule control

`/queen/schedule/ctl` accepts bounded producer entries and FIFO consumer
acknowledgements:

```json
{"id":"sched-1","role":"worker-gpu","priority":2,"ticks":3,"budget_ms":120}
{"op":"dequeue","id":"sched-1"}
```

`id` and `role` are bounded tokens; numeric work fields must be positive; IDs
are unique within the retained queue. `dequeue` must name the exact FIFO head;
an empty queue, stale ID, or out-of-order ID fails without mutation. A
successful dequeue removes one pending record and increments `dequeued`; it
means the Queen consumer accepted responsibility for that request, not that a
Worker completed it. `/proc/schedule/summary` and `/proc/schedule/queue` are
the corresponding generated observations.

### Lease control

`/queen/lease/ctl` accepts `grant`, `renew`, `preempt`, and `quota` records:

```json
{"op":"grant","id":"lease-1","subject":"queen","resource":"gpu0","ttl_s":300,"priority":5}
{"op":"renew","id":"lease-1","ttl_s":600,"priority":6}
{"op":"preempt","id":"lease-1","reason":"timeout"}
{"op":"quota","subject":"queen","resource":"gpu0","max_active":4,"max_preemptions":8}
```

Active leases and quota definitions are bounded authoritative state. Completed
preemptions are bounded observation history: the newest
`preemptions_max_entries` records remain in chronological order and a later
successful preemption evicts the oldest record instead of refusing the lease
transition. The `preemptions` value in `/proc/lease/summary` is the cumulative
successful-preemption count, so saturation and eviction remain explicitly
accounted even though record storage is fixed. Generated summaries are
available at `/proc/lease/*` when enabled.

### Export control

`/queen/export/ctl` accepts bounded `open` and `close` records:

```json
{"op":"open","id":"export-1","ttl_s":900}
{"op":"close","id":"export-1","reason":"window-complete"}
```

## Telemetry contracts

### Worker telemetry

Worker records are appended to the canonical sharded path. Ring size, cursor
retention, and frame selection are manifest-controlled. The generated CBOR
format is defined in
[telemetry_cbor_schema.md](snippets/telemetry_cbor_schema.md). Plain-text and
CBOR evidence must identify which selected schema produced it.

Current telemetry records can be produced by authorized sessions, root-owned
model helpers, or host simulation. Their presence is not proof that a separate
target Worker task emitted them.

Root projects executable-Worker lifecycle truth as one bounded JSONL record
with `schema="worker-runtime-state/v2"`. The `identity` array is ordered as
`[slot, lease_epoch, supervisor_generation, cap_generation]`; the `sequence`
array is ordered as `[ready, control, receipt, completion]`. Every array value
is an unsigned 32-bit wire value, which keeps the maximum record, including its
newline and a 32-byte Worker id, within the fixed 256-byte console line. Root
fails closed instead of truncating an out-of-range value. Host consumers retain
read compatibility with `worker-runtime-state/v1` for already-retained model
and evidence records; the target emitter uses v2.

Target `TAIL` supports both retained Worker rings and bounded host nodes such
as `/host/tickets/{spec,status,deadletter}`. A logical UTF-8 record longer than
the 256-byte console frame is emitted as the same ordered, digest-bound `C1:`
frames used by `CAT`; Cohesix TCP clients reassemble those frames before
returning logical records. Cursor offsets advance by source-record bytes, not
wire-frame bytes. A stream stops at a complete logical-record boundary when
the fixed 64-frame response capacity is reached.

The checked-in profiles currently select `legacy-plaintext`. Under that
selection each append must be valid UTF-8 and is retained in the bounded
append-only worker ring; the transport does not add a JSON or CBOR wrapper.
Common heartbeat and GPU records may be JSON lines, but their application
fields are not a replacement for the selected telemetry frame contract. A
profile selecting `cbor-v1` uses `telemetry-frame/v1`; clients must not decode a
legacy record as CBOR or claim the generated CBOR schema was active merely
because the schema is available in this repository.

### Queen telemetry ingest

For `/queen/telemetry/<device_id>/`:

- `ctl` requests a new OS-named segment;
- `seg/<segment_id>` is append-only;
- `latest` is a read-only pointer to the newest segment; and
- the provider, not the client, chooses segment IDs.

The successful segment-control `ECHO` acknowledgement carries the selected
segment ID. Host REST projections may expose that existing receipt through
their response payload; clients retain a bounded `latest` read as the
compatibility fallback when the receipt is unavailable.

The inline `cohsh-telemetry-push/v1` envelope contains:

| Field | Type | Rule |
| --- | --- | --- |
| `schema` | text | Exactly `cohsh-telemetry-push/v1`. |
| `seq` | unsigned integer | Monotonic within the segment, starting at 1. |
| `mime` | text | MIME type for the source payload. |
| `payload` | text | UTF-8 payload chunk bounded by the provider record limit. |

Large host artifacts use a reference manifest rather than inline transfer. A
`coh-ref-c/v1` record contains `schema`, monotonic `seq`, contiguous `off`,
positive `len`, and a bounded `sha256` token. Inline and reference records
cannot be mixed in one segment. All segment, record, reference-count, reference
byte, and eviction bounds come from the selected manifest.

### Queen LoRA export

When telemetry ingest is enabled, host NineDoor can install a read-only
training handoff below `/queen/export/lora_jobs/<job_id>/`:

| Path | Contract |
| --- | --- |
| `telemetry.cbor` | Bounded telemetry bundle selected for the external training job. |
| `base_model.ref` | Single-line base-model identifier. |
| `policy.toml` | Policy snapshot that governed the export. |

The host publisher chooses `<job_id>`; clients do not append to these files.
The target adapter currently exposes the gated export root and its control
file, but it does not populate job directories or accept a job upload. The host
`coh peft export` flow may copy an installed handoff to external training
infrastructure, but the target does not train or import a model by exposing the
directory.

## Manifest-gated namespace families

| Family | Representative paths | Ownership |
| --- | --- | --- |
| GPU bridge and nodes | `/gpu/bridge/*`, `/gpu/<id>/*`, `/gpu/models/*`, `/gpu/telemetry/schema.json` | Host bridge owns GPU hardware and publication; target consumes only bounded files. See [GPU_NODES.md](GPU_NODES.md). |
| Sidecar buses | `/bus/<adapter>/*` | Host sidecar providers; adapter labels and gates are generated. Exact nodes are catalogued below. |
| Host services | `/host/systemd/*`, `/host/k8s/*`, `/host/docker/*`, `/host/nvidia/*`, `/host/tickets/*` | Target and host adapters expose selected bounded projections; host agents execute actions and federation. This document owns the schemas; [HOST_TOOLS.md](HOST_TOOLS.md) owns tool operation. |
| Content-addressed updates | `/updates/<epoch>/*`, `/models/<sha256>/*` | CAS provider, gated by generated policy. |
| Policy and actions | `/policy/*`, `/actions/*` | Generated rules plus bounded policy/action queues. |
| Audit and replay | `/audit/*`, `/replay/*` | Present only when audit and replay gates are enabled. |

Disabled providers return an explicit refusal or absence according to the
active adapter; clients must not silently create substitute paths.

## GPU publication

`/gpu/bridge/ctl` uses a bounded three-stage snapshot stream:

```text
begin bytes=<payload_bytes> sha256=<hex>
b64:<base64_chunk>
end
```

The bridge validates decoded size and SHA-256 before publishing nodes.
`/gpu/bridge/status` reports bounded `idle`, `receiving`, `ok`, or `err` state.
Lease and status breadcrumb formats are generated in
[gpu_breadcrumbs.md](snippets/gpu_breadcrumbs.md).

## Sidecar bus providers

Sidecar mounts exist only when the selected MODBUS or DNP3 `sidecars.*` gate is
enabled. The compiler resolves adapter labels, including collision handling;
clients must discover those labels rather than derive them. Both provider types
share the `/bus/<adapter>` file contract:

| Path | Mode | Contract |
| --- | --- | --- |
| `ctl` | Append-only | Bounded sidecar coordination/control records. |
| `telemetry` | Append-only | Accepted records; while the link is offline, writes are spooled for bounded replay. |
| `link` | Control | `online` or `offline`. |
| `replay` | Control | A write requests a bounded spool drain and records `replay entries=<n> bytes=<n>`. |
| `spool` | Read-only | Aggregate entry/byte bounds followed by retained frame summaries. |

Adapter-scoped role/ticket checks precede every side effect. A denial is not a
successful control operation or replay and is recorded through the existing
Queen log or audit surface; these providers do not create a second RPC
protocol. AI LoRA lifecycle interfaces are the
[Queen LoRA export](#queen-lora-export) above and the host-side PEFT workflows
owned by [GPU_NODES.md](GPU_NODES.md); lowercase `lora` identifiers denote that
AI lifecycle, not a sidecar bus family.

## UI provider projections

UI providers are optional, bounded, read-only representations of existing
state. Each text provider has a paired `.cbor` form only where listed; the
generated `ui_providers.*` gates and the underlying provider gate must both be
enabled.

| Family | Public nodes |
| --- | --- |
| 9P sessions | `/proc/9p/sessions`, `/proc/9p/outstanding`, `/proc/9p/short_writes`, and the corresponding `.cbor` nodes. |
| Ingest | `/proc/ingest/p50_ms`, `/proc/ingest/p95_ms`, `/proc/ingest/backpressure`, and the corresponding `.cbor` nodes. |
| Policy preflight | `/policy/preflight/req`, `/policy/preflight/req.cbor`, `/policy/preflight/diff`, and `/policy/preflight/diff.cbor`. |
| Update state | `/updates/<epoch>/manifest.cbor`, `/updates/<epoch>/status`, and `/updates/<epoch>/status.cbor`. |

The paired record fields are:

| Provider | Text form | CBOR map |
| --- | --- | --- |
| 9P sessions | The generated `/proc/9p/*` records below. | Sessions: `total`, `worker`, `shard_bits`, `shard_count`, and `shards[] {label, count}`; outstanding: `current`, `limit`; short writes: `total`, `retries`. |
| Ingest | The generated `/proc/ingest/*` scalar records below. | One unsigned field named `p50_ms`, `p95_ms`, or `backpressure`, matching the node. |
| Policy request preflight | Summary counts plus `req id=<id> target=<path> decision=<approve\|deny> state=<queued\|consumed>` lines. | `total`, `queued`, `consumed`, and `actions[] {id, target, decision, state}`. |
| Policy diff preflight | Summary counts plus `rule id=<id> target=<path> queued=<n> consumed=<n>` lines. | `rules`, `actions`, `unmatched`, and `entries[] {id, target, queued, consumed}`. |
| Update status | Epoch/state, manifest/chunk counts, payload digest, and optional delta-base lines. | The same `epoch`, `state`, byte/count, digest, and optional `delta {base_epoch, base_sha256}` fields. |

Text and CBOR variants describe the same snapshot but are separate bounded
reads. Disabled or oversized providers fail explicitly and emit a
`ui-provider` audit record. SwarmUI and other clients may render, cache, or
replay these records, but they do not own their schema or mutation policy.

## Host service projections

The host tree appears only when `ecosystem.host.enable` is selected; individual
provider roots follow `ecosystem.host.providers[]`. These append-only nodes are
projections or ticketed control sinks, not direct VM access to systemd,
Kubernetes, Docker, or NVIDIA APIs.

| Path | Contract |
| --- | --- |
| `/host/systemd/<unit>/status` | Host-published unit state; `start`, `stop`, and `restart` siblings are Queen-only control sinks. |
| `/host/k8s/node/<name>/status` | Host-published node state; `cordon` and `drain` siblings are Queen-only control sinks. |
| `/host/docker/status` | Host-published engine/container summary; `restart` and `stop` are Queen-only control sinks. |
| `/host/nvidia/gpu/<id>/status` | Host-published GPU summary; `power_cap` is a Queen-only control sink and `thermal` is host-published state. |
| `/host/jetson`, `/host/net` | Manifest-selected provider roots. The as-built namespace currently defines no portable child-record schema for these roots. |

Provider agents sanitize bounded status records. Systemd uses
`state=<state> sub=<substate>`; Kubernetes uses
`state=<state> role=<role> version=<version>`; Docker uses
`version=<version> containers=<n> running=<n> paused=<n> stopped=<n>`; NVIDIA
status uses `util_pct`, `mem_used_mb`, `mem_total_mb`, `temp_c`, and `power_w`,
while its thermal node uses `temp_c`. Collection failure uses `state=unknown`
or the provider's documented unknown scalar. A control-file append records a
request; only the corresponding host-ticket result or provider observation can
prove that the host action occurred.

## Host tickets and federation

When host tickets are enabled, the namespace exposes:

| Path | Mode | Record |
| --- | --- | --- |
| `/host/tickets/spec` | Append-only JSONL | Strict request records using the generated request schema (`host-ticket/v1` in the checked-in profile). |
| `/host/tickets/status` | Append-only JSONL | Lifecycle receipts using the generated result schema (`host-ticket-result/v1`). |
| `/host/tickets/deadletter` | Append-only JSONL | Terminal failure or expiry receipts using the result schema. |
| `/host/tickets/spec.snapshot` | Read-only | Bounded snapshot of `spec`. |
| `/host/tickets/status.snapshot` | Read-only | Bounded snapshot of `status`. |
| `/host/tickets/deadletter.snapshot` | Read-only | Bounded snapshot of `deadletter`. |

A request contains the required fields `schema`, `id`, `idempotency_key`, and
`action`; `target`, `args`, and `expires_unix_ms` are optional. A result contains
the required fields `schema`, `id`, `idempotency_key`, `action`, and `state`,
with optional `message`. Unknown fields are rejected. Actions and result states
must appear in the selected manifest's allowlists; the checked-in lifecycle is
`queued`, `claimed`, `running`, then `succeeded`, `failed`, or `expired`.

```json
{"schema":"host-ticket/v1","id":"ticket-1","idempotency_key":"restart-1","action":"systemd.restart","target":"/host/systemd/cohesix-agent.service/restart"}
{"schema":"host-ticket-result/v1","id":"ticket-1","idempotency_key":"restart-1","action":"systemd.restart","state":"succeeded","message":"ok"}
```

`id`, `idempotency_key`, and federation identifiers are at most 128 bytes and
use only ASCII letters, digits, `.`, `-`, `_`, and `:`. The manifest bounds the
full JSON line; the Secure9P `msize` remains an independent upper bound.

Federated requests and receipts add `source_hive`, `target_hive`, `relay_hop`,
and `relay_correlation_id`. Source and target are pair-required, `relay_hop` is
`1..=32` when present, and every relay revalidates its manifest-gated peer and
action policy. The correlation key is local `id + idempotency_key`, or
federated `id + idempotency_key + source_hive + target_hive`; it provides
idempotency/evidence correlation, not additional authority. Relay queues, WAL,
timeouts, peers, and credentials remain host-side and manifest-bounded.

## CAS updates

CAS layout, fixed chunk size, delta references, signing requirement, and
manifest template are generated in
[cas_interfaces.md](snippets/cas_interfaces.md). Clients append only to declared
manifest/chunk nodes, and a hash or size mismatch is an error. Do not duplicate
the generated manifest template in another canonical document.

The CBOR `cohesix-cas/manifest-v1` wire map remains the same eight fields:
`schema`, `epoch`, `chunk_bytes`, `chunks`, `payload_bytes`, `payload_sha256`,
`delta`, and `signature`. It admits at most eight chunk digests. The generated
host JSON template additionally carries tooling-only `limits.max_chunks` and
`limits.max_payload_bytes`; those keys are consumed by host validation and are
never serialized into the CBOR manifest. A legacy JSON template with no
`limits` uses the shared eight-chunk manifest maximum. If `limits` is present,
its maximum must equal that shared value exactly and its byte capacity must be
exactly `chunk_bytes * max_chunks`; neither a lower local policy nor a higher
wire claim is accepted as this interface.

Host packing and upload perform this structural check locally before any
target connection. Structural eligibility does not reserve target storage:
the bounded global CAS store may still return its typed `buffer-full` refusal
when other chunks or models occupy capacity. A caller must not treat host
preflight as target acceptance or answer that refusal by retrying, truncating,
or changing payload identity.

## Policy, audit, and replay

| Path | Mode | Contract |
| --- | --- | --- |
| `/policy/ctl` | Control JSONL | Strict `apply` and `rollback` policy-revision records. |
| `/policy/rules` | Read-only | Deterministic snapshot of the selected manifest rules. |
| `/actions/queue` | Control JSONL | Single-use `id`, `target`, and `approve`/`deny` decisions. |
| `/actions/<id>/status` | Read-only | `queued` or `consumed` decision state. |
| `/audit/journal` | Append-only JSONL | Bounded control-action journal. |
| `/audit/decisions` | Append-only JSONL | Policy decision records with role/ticket context. |
| `/audit/export` | Read-only | Retained cursor bounds and replay flags. |
| `/replay/ctl` | Control JSON | Bounded `{"from":<cursor>}` request. |
| `/replay/status` | Read-only | `idle`, `ok`, or `err` state with deterministic sequence fingerprint. |

Representative strict control records are:

```json
{"op":"apply","id":"rev-1","sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}
{"op":"rollback","id":"rev-1"}
{"id":"approve-1","target":"/queen/ctl","decision":"approve"}
{"from":42}
```

Policy nodes require `ecosystem.policy.enable`; audit nodes require
`ecosystem.audit.enable`; replay additionally requires
`ecosystem.audit.replay_enable`. Policy approvals are single-use. Replay is
limited to the retained audit window and to Cohesix-issued control actions;
out-of-window cursors, offset mismatches, or disabled gates fail without
creating an alternate control or authority path.

## Generated schema index

These files are generated by `coh-rtc` or the grammar generator and must be
updated only through their source:

- [root_task_manifest.md](snippets/root_task_manifest.md) — profile summary,
  mounts, sharding, sidecars, and ecosystem gates
- [observability_interfaces.md](snippets/observability_interfaces.md) — exact
  `/proc` paths, formats, and byte limits
- [telemetry_cbor_schema.md](snippets/telemetry_cbor_schema.md) — worker
  telemetry CBOR frame
- [gpu_breadcrumbs.md](snippets/gpu_breadcrumbs.md) — GPU lease and status
  breadcrumbs
- [cas_interfaces.md](snippets/cas_interfaces.md) — update and model registry
  layout
- [ticket_quotas.md](snippets/ticket_quotas.md) — ticket quota limits
- [cohsh_grammar.md](snippets/cohsh_grammar.md) — target console verbs

## Compiler-generated interface appendices

The following blocks are embedded because `coh-rtc` compares them with the
active default-profile output. Do not edit them by hand; update the manifest or
generator and run `scripts/check-generated.sh`.

<!-- markdownlint-disable MD022 MD032 MD031 -->
<!-- coh-rtc:telemetry-cbor:start -->
### Telemetry CBOR Frame v1 (generated)
- Schema: `telemetry-frame/v1`
- Version: `1`
- Encoding: CBOR map (major type 5)

| Field | CBOR type | Required | Description |
| --- | --- | --- | --- |
| `schema` | `text` | `yes` | Schema identifier; must be `telemetry-frame/v1`. |
| `worker_id` | `text` | `yes` | Worker identifier emitting the record. |
| `role` | `text` | `yes` | Worker role label (`worker-heartbeat`, `worker-gpu`). |
| `seq` | `uint` | `yes` | Monotonic frame sequence number. |
| `emitted_ms` | `uint` | `yes` | Unix epoch milliseconds captured by the worker. |
| `payload` | `map` | `yes` | Schema-specific payload map (e.g., heartbeat or GPU job data). |

_Generated by coh-rtc (sha256: `d1906bce668a4d73d95a8262734f1ec04a1480610ebfd9b6c3f3c8ad2e402b7e`)._
<!-- coh-rtc:telemetry-cbor:end -->

<!-- coh-rtc:observability-interfaces:start -->
### /proc observability nodes (generated)
- `/proc/9p/sessions` (read-only, max 8192 bytes): `sessions total=<u64> worker=<u64> shard_bits=<u8> shard_count=<u16>` plus `shard <hex> <count>` lines.
- `/proc/9p/outstanding` (read-only, max 128 bytes): `outstanding current=<u64> limit=<u64>`.
- `/proc/9p/short_writes` (read-only, max 128 bytes): `short_writes total=<u64> retries=<u64>`.
- `/proc/9p/session/active` (read-only, max 128 bytes): `active=<u64> draining=<u64>`.
- `/proc/9p/session/<id>/state` (read-only, max 64 bytes): `state=SETUP|ACTIVE|DRAINING|CLOSED`.
- `/proc/9p/session/<id>/since_ms` (read-only, max 64 bytes): `since_ms=<u64>`.
- `/proc/9p/session/<id>/owner` (read-only, max 96 bytes): `owner=<identity>`.
- `/proc/ingest/p50_ms` (read-only, max 64 bytes): `p50_ms=<u32>` (milliseconds).
- `/proc/ingest/p95_ms` (read-only, max 64 bytes): `p95_ms=<u32>` (milliseconds).
- `/proc/ingest/backpressure` (read-only, max 64 bytes): `backpressure=<u64>`.
- `/proc/ingest/dropped` (read-only, max 64 bytes): `dropped=<u64>`.
- `/proc/ingest/queued` (read-only, max 64 bytes): `queued=<u32>`.
- `/proc/ingest/watch` (append-only, max_entries=16, line_bytes=192, min_interval_ms=50): `watch ts_ms=<u64> p50_ms=<u32> p95_ms=<u32> queued=<u32> backpressure=<u64> dropped=<u64> ui_reads=<u64> ui_denies=<u64>`.
- `/proc/root/reachable` (read-only, max 32 bytes): `reachable=yes|no`.
- `/proc/root/last_seen_ms` (read-only, max 64 bytes): `last_seen_ms=<u64>`.
- `/proc/root/cut_reason` (read-only, max 64 bytes): `cut_reason=<none|network_unreachable|session_revoked|policy_denied|lifecycle_offline>`.
- `/proc/pressure/busy` (read-only, max 64 bytes): `busy=<u64>`.
- `/proc/pressure/quota` (read-only, max 64 bytes): `quota=<u64>`.
- `/proc/pressure/cut` (read-only, max 64 bytes): `cut=<u64>`.
- `/proc/pressure/policy` (read-only, max 64 bytes): `policy=<u64>`.
- `/proc/schedule/summary` (read-only, max 128 bytes): `queue=<u64> dequeued=<u64> dropped=<u64> max_entries=<u32>`.
- `/proc/schedule/queue` (read-only, max 256 bytes): `id=<id> role=<role> priority=<u32> ticks=<u32> budget_ms=<u32> seq=<u64>`.
- `/proc/lease/summary` (read-only, max 160 bytes): `active=<u64> preemptions=<u64> quotas=<u64> max_active=<u32> max_preemptions=<u32>`.
- `/proc/lease/active` (read-only, max 256 bytes): `id=<id> subject=<subject> resource=<resource> ttl_s=<u32> priority=<u32> state=<STATE> seq=<u64>`.
- `/proc/lease/by-id/<id>` (read-only, max 256 bytes): exact active-lease record using the `/proc/lease/active` line schema; an absent lease returns an empty payload.
- `/proc/lease/preemptions` (read-only, max 256 bytes): `id=<id> subject=<subject> resource=<resource> reason=<reason> seq=<u64>`.

_Generated by coh-rtc (sha256: `37d54e07dff302856782d808af12bbfac29080c17f147e03985f89ef570b156e`)._
<!-- coh-rtc:observability-interfaces:end -->

#### QEMU root-control flight records

The `release-qemu` profile additionally exposes the read-only
`/proc/schedule/qemu-flight` diagnostic. It is absent from Pi profiles. The
provider retains 61 fixed-layout activation records plus three aggregate
records in a bounded in-memory ring; saturation evicts the oldest activation
and increments `dropped`. Recording performs no allocation, formatting, UART
write, or synchronous host I/O. Text formatting occurs only when an
authenticated reader requests the snapshot after a run.

Each `QEMU_FLIGHT schema=v1` record contains the virtual-counter timestamp,
component, event, starting phase, activation sequence, child generation,
previous-activation gap, queue depth, maximum work available, work completed,
work remaining, bounded service-unit count, execution ticks, and one of
`NO_WORK`, `BLOCK`, `QUOTA`, `BUDGET_GUARD`, `YIELD`, `TIMEOUT`, or `FAULT`.
`QEMU_FLIGHT_SUMMARY`, `QEMU_FLIGHT_TIMING`, and `QEMU_FLIGHT_EXITS` retain
counter frequency, high-water marks, timing/unit aggregates, exit counts, and
drop accounting. These records diagnose scheduling and queue drainage; they do
not replace target correctness or MCS fault evidence. `QUOTA` with nonzero
`rem` may retain the outer root loop inside the same continuous counter window.
`BUDGET_GUARD`, idle, block, timeout, fault, and drained exits return to the
scheduler. The guard is checked before the next bounded leaf so a passive
service call cannot consume the donor's generated MCS reserve; kernel MCS
postponement remains the hard budget boundary.

NineDoor containment retains the exact fatal transport reason (`REVOKED`,
`STALE_IDENTITY`, or `INVALID_ABI`) in its bounded deferred diagnostic before
revoking the generation. This does not add hot-path output; rendering still
occurs only during the ordinary retained-output turn after containment.

The synchronous request and response message-register identities are each
published only after an AArch64 release fence over the corresponding cached
shared frame. The receiver performs the paired acquire fence before decoding
that frame. IPC supplies notification and Reply ownership; it is not used as a
substitute for the shared-memory publication ordering contract.

<!-- coh-rtc:gpu-breadcrumbs:start -->
### GPU status breadcrumb schema (generated)
- `coh.run.lease.schema`: `gpu-lease/v1`
- `coh.run.lease.active_state`: `ACTIVE`
- `coh.run.lease.max_bytes`: `1024`
- `coh.run.breadcrumb.schema`: `gpu-breadcrumb/v1`
- `coh.run.breadcrumb.max_line_bytes`: `512`
- `coh.run.breadcrumb.max_command_bytes`: `256`
- Lease entries are JSON lines with fields: `schema`, `state`, `gpu_id`, `worker_id`, `mem_mb`, `streams`, `ttl_s`, `priority`.
- Breadcrumb entries are JSON lines with fields: `schema`, `event`, `command`, `status`, `exit_code` (optional).

_Generated by coh-rtc (sha256: `80eff6277e0b97c54fc8996ffc01a54ccff20b899bcd0e9f63c30de1afb02f80`)._
<!-- coh-rtc:gpu-breadcrumbs:end -->

<!-- coh-rtc:cas-interfaces:start -->
### CAS update surfaces (generated)
- `cas.store.chunk_bytes`: `128`
- Manifest-v1 capacity: at most `8` chunks and `1024` payload bytes for this profile.
- `cas.delta.enable`: `true`
- `cas.signing.required`: `true`
- Base update layout: `/updates/<epoch>/manifest.cbor`, `/updates/<epoch>/chunks/<sha256>`.
- Model registry layout: `/models/<sha256>/weights`, `/models/<sha256>/schema`, `/models/<sha256>/signature`.
- Delta manifests supply `delta.base_epoch` and `delta.base_sha256`, referencing a non-delta base.
- Payloads are appended as raw bytes or `b64:`-prefixed base64.
- CAS host packaging template (`limits` is tooling-only; CBOR manifest-v1 remains eight fields):
```json
{
  "chunk_bytes": 128,
  "chunks": [
    "<sha256-hex>"
  ],
  "delta": {
    "base_epoch": "<epoch>",
    "base_sha256": "<sha256-hex>"
  },
  "epoch": "<epoch>",
  "limits": {
    "max_chunks": 8,
    "max_payload_bytes": 1024
  },
  "payload_bytes": "<payload-bytes>",
  "payload_sha256": "<sha256-hex>",
  "schema": "cohesix-cas/manifest-v1",
  "signature": "<ed25519-signature-hex>"
}
```

_Generated by coh-rtc (sha256: `e3cbe1a4366263ec8a5c448304602949d338dd40b18e8a33194fd3d7c6377831`)._
<!-- coh-rtc:cas-interfaces:end -->
<!-- markdownlint-enable MD022 MD032 MD031 -->

## Error surfaces

Secure9P wire errors are `Permission`, `NotFound`, `Busy`, `Invalid`, `TooBig`,
and `Closed`. Their semantics are defined in [SECURE9P.md](SECURE9P.md).

Console errors are textual `ERR` acknowledgements with a verb and bounded
reason/detail fields. Common reasons include authentication, permission,
policy, lifecycle, quota, busy/backpressure, invalid path or payload, and frame
length. Console rate limiting is not a Secure9P wire error.

Host REST or UI projections may map these failures into their own transport
status, but they must preserve the underlying refusal and must not retry a
non-idempotent operation unless the documented host contract explicitly permits
it.

## Verification and source map

- Console parser: [`crates/cohsh-core/src/command.rs`](../crates/cohsh-core/src/command.rs)
- TCP framing/authentication:
  [`apps/root-task/src/net/console_srv.rs`](../apps/root-task/src/net/console_srv.rs)
- Target dispatch: [`apps/root-task/src/event`](../apps/root-task/src/event)
- Target namespace adapter:
  [`apps/root-task/src/ninedoor.rs`](../apps/root-task/src/ninedoor.rs)
- Host Secure9P server: [`apps/nine-door/src/host`](../apps/nine-door/src/host)
- Host namespace and ticket validation:
  [`apps/nine-door/src/host/namespace.rs`](../apps/nine-door/src/host/namespace.rs)
- Host-ticket lifecycle and federation agent:
  [`apps/host-ticket-agent`](../apps/host-ticket-agent)
- Worker model/build-artifact contracts: [`apps/worker-bus`](../apps/worker-bus),
  [`apps/worker-lora`](../apps/worker-lora), and
  [`apps/worker-gpu`](../apps/worker-gpu)
- Codec operations: [`crates/secure9p-codec`](../crates/secure9p-codec)
- Generated interface checks: `scripts/check-generated.sh`
- Staged validation: [TEST_PLAN.md](TEST_PLAN.md)

Every new public path or record must be documented here, owned by manifest IR
when it changes generated behavior, and covered by positive and negative
fixtures before clients depend on it.
