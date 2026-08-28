<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define the as-built Cohesix console, cohsh, and .coh command surfaces. -->
<!-- Author: Lukas Bower -->
# Cohesix Userland and CLI

This document is the operator-facing reference for the Cohesix root console,
the host-side `cohsh` shell, and `.coh` scripts. Namespace schemas and payload
formats are defined in [INTERFACES.md](INTERFACES.md); host-tool composition is
defined in [HOST_TOOLS.md](HOST_TOOLS.md); the canonical live workflow is in
[OPERATOR_WALKTHROUGH.md](OPERATOR_WALKTHROUGH.md); and advanced, task-oriented
procedures are in [OPERATOR_RECIPES.md](OPERATOR_RECIPES.md).

See the [Glossary](GLOSSARY.md) for Cohesix-specific shell, namespace, and role
terms.

## Command surfaces

| Surface | Runs on | Primary use | Authority |
| --- | --- | --- | --- |
| Root console, prompt `cohesix>` | Root task through PL011; Pi 4 may also accept an admitted USB keyboard and mirror output to HDMI | Boot, capability, memory, network, and hardware diagnostics | Physical console policy plus command-specific checks |
| `cohsh`, prompt `coh>` | Host | Authenticated namespace reads, bounded writes, lifecycle control, tests, and automation | Attached role and optional capability ticket |
| `.coh` script | Host through `cohsh` | Deterministic command batches and assertions | Exactly the authority of the enclosing `cohsh` session |

The local Pi 4 seat and PL011 are separate input sources that feed the same
root-console parser. Serial remains the complete recovery surface when USB or
HDMI is unavailable. Hardware-specific proof commands and their interpretation
belong in [DRIVERS.md](DRIVERS.md) and
[HARDWARE_BRINGUP.md](HARDWARE_BRINGUP.md), not in this command reference.

## Transport and authority rules

- The target exposes one authenticated TCP console. In direct mode, only one host
  process can own it at a time.
- `hive-gateway` can own that TCP connection and multiplex bounded REST
  projections for concurrent host clients. See
  [API_GUIDELINES.md](API_GUIDELINES.md).
- TCP authentication proves access to the console listener. `ATTACH` then
  selects a role and validates any required ticket. Gateway request
  authentication protects HTTP writes but does not create a new target identity.
- Namespace visibility is profile- and role-dependent. The canonical worker
  path is `/shard/<label>/worker/<id>`; `/worker/<id>` exists only when the
  selected manifest enables the legacy alias.
- Policy, lifecycle, ticket scope, and quota checks remain authoritative for
  every transport. A client-side check is not an authorization decision.
- The `cohsh` mock transport is in-process and isolated. State created by one
  `cohsh` mock process is not shared with another process and is not live-target
  evidence. The Python `MockBackend` is instead filesystem-backed; see
  [PYTHON_SUPPORT.md](PYTHON_SUPPORT.md).

## Root console

The root console appears as `cohesix>` after root-task console initialization.
Run `help` on the active image: the command inventory is profile-gated and is
the most precise description of that boot.

### Core diagnostic commands

| Command | Behavior |
| --- | --- |
| `help` | Print commands available in the selected profile. |
| `bi` | Preserve the legacy line, then print source-labelled `[bi:v2]` kernel BootInfo and generated-profile records. |
| `caps` | Print the legacy key capability-slot summary. |
| `caps mcs` | Print bounded live MCS authority presence and generated fixed/capacity object counts as source-labelled records that fit the Pi linked-HDMI fallback width. |
| `smp` | Print bounded userspace activity and assignment diagnostics without claiming kernel CPU utilization. This is the preferred spelling. |
| `smp activity` | Compatibility spelling for `smp`; it produces the same bounded userspace activity report. |
| `smp mcs` | Print `[smp:mcs/v1]` generated per-core/per-task admission joined to one copied live registry snapshot. |
| `smp dump` | Request the raw kernel scheduler snapshot. This debug-only path is unavailable after linked-UART cutover. |
| `mem` | Print the RAM/device untyped summary. |
| `ping` | Return the liveness response. |
| `cachelog [n]` | Dump a bounded number of recent cache operations. |
| `nettest` | Start the profile-gated bounded network self-test. `OK NETTEST detail=started run_generation=<n>` is admission, not a terminal verdict. |
| `netstats` | Print bounded network state, counters, and the complete generation-tagged `nettest` terminal/running verdict. |
| `reboot` | Schedule a platform reboot only when Queen authorization and a reboot backend are both available. |
| `quit` | In the event-pump console, end the session and request network disconnect when applicable. The earlier bootstrap `RootConsole` phase reports `quit` as unsupported. |

Use `smp` for normal QEMU and Pi 4 diagnostics. The activity report follows the
linked serial owner and may be mirrored through the local-seat path. Use
the selected Pi network section for fresh activity-gated driver counters:
Wi-Fi emits the canonical CYW43 and SDIO owner snapshots, while wired mode
emits the GENET owner snapshot. Each selected snapshot is projected as seven
bounded `[smp] driver v=1 part=<turn|outcome|sched|retry|cache|traffic|role>`
rows so saturated counters fit the common 256-byte console line ABI. This
operator projection does not replace or change the complete 1,024-byte
`DRIVER_TASK_COUNTER` provenance record retained by boot/qlog evidence. A
missing activity-gated projection remains missing evidence rather than being
replaced by an unrelated driver's counters. Use
`smp dump` only when investigating kernel scheduler state on a compatible debug
profile before linked-UART cutover; the raw kernel text is UART-only. The
explicit `smp activity` spelling remains accepted for scripts and older
runbooks. `smp mcs` labels compiler truth `source=generated`, BootInfo
`source=kernel`, and copied live state `source=runtime`; unavailable is not a
missing registration. It never calls `seL4_SchedContext_Consumed`, which would
reset the accounting interval. QEMU output proves only its exact boot, while Pi
state requires a fresh exact-image Pi boot. The `serial_rx_drop` and `serial_rx_backpressure` values in this
report describe the root serial queue only. A zero value does not claim that
the isolated serial runtime queue or the mini-UART hardware FIFO could not
have overrun; paced serial acceptance still requires a complete command and
response transcript. Per-core rates follow the manifest assignment of the
specific driver: `seatPoll_s`, `kbdB_s`, `seat_drop_s`, and
`seat_no_reply_s` belong only to the USB core, while `hdmiB_s` and
`hdmi_drop_s` belong only to the HDMI core. HDMI mirror-queue drops are not USB
keyboard drops, and neither driver's rate is duplicated onto the other's core.

On a physical-console request with Wi-Fi selected, either `smp` spelling may
prepend a passive retained old-good transaction before the ordinary activity
report. It appears only when the current Wi-Fi attempt, pair, connection
generation, firmware identity, ordered association/EAPOL/DHCP receipt, and six
current driver-owner records form one complete snapshot. The prefix is one
atomic 37-line batch: owner records for serial, USB, HDMI, PCIe, CYW43, and SDIO
in that order, followed by a contiguous 31-line
`WIFI_OLDGOOD_RETAINED_BEGIN`/hash/26-step/`WIFI_OLDGOOD_RETAINED_END`
transaction. Its presence performs no device work; its absence remains missing
evidence. Fresh netstats, authenticated TCP, terminal nettest, and DPC rows
must follow it in the capture and cannot be supplied by the retained block.

`test` is present in the shared parser but the target root console directs the
operator to the host-side `cohsh` implementation. Pi 4 profiles may add `usb`
and `wifi` diagnostic families. Their gate meanings are documented in
[DRIVERS.md](DRIVERS.md).
The advertised Wi-Fi inventory is passive: `wifi help`, `wifi dump-state`, and
`wifi diag`. `wifi diag` is the bounded causal-triage surface: it emits at most
eight preflighted body lines plus its terminal/status/ACK tail, leads with the
first known failing gate, and carries retained CYW43/SDIO progress, physical
epoch/logical generation, parent/child identity, latest child timing receipts,
grant consumption, wake counters, and exact fault identity. Its snapshot is
explicitly `best-effort-multi-record`; downstream gates are `not-reached`
instead of being presented as current acceptance after an earlier failure.
`wifi dump-state` is the verbose acceptance, DPC, association, maintenance,
queue, TX, and Gate 7/8 inspection surface. Legacy `wifi probe-ht`, `wifi
load-fw`, and `wifi retry` spellings
remain recognized only to return one typed linked-runtime ownership refusal;
they do not invoke a debug callback, snapshot traversal, or physical operation.
The Pi USB inventory separates passive inspection from active operations:
`usb status`, `usb dump-state`, and `usb diag` are passive, while
`usb enable-kbd` and `usb probe-kbd` may change polling or advance one retained
probe slice. `usb diag` arms a post-command liveness observation without polling
the device. Its ten-gate result is startup history, not current keyboard proof.
After a real USB key is typed, `usb status` reports a one-shot pass only when
the linked HID byte, parser acceptance, parser drain, and echo counters all
advance with no new drop. `usb probe-kbd` reports `attached` only after its live
retained service turn completes; a cached ready latch with a pending request is
`keyboard-unavailable continuation=pending`. The same passive status also
reports HDMI queue state separately from the isolated display driver's
completion receipt.

Every passive `usb status`, `usb dump-state`, and `usb diag` response begins
with one atomic adjacent old-good pair:

```text
USB_OLDGOOD_RETAINED v=1 task=<u32> token=0x<8hex> link_epoch=<u32> link_token=0x<8hex> epoch=<u32> seq=<u32> mask=0x<8hex> topology=0x<8hex> input_gen=<u32> commit=<u32> source=<linked-runtime-hid|none>
USB_OLDGOOD_CURRENT contracts=usb-local-seat+pcie-root owners=<driver-owned|missing>+<driver-owned|missing> descriptors=<sealed|missing>+<sealed|missing> command_ready=<yes|no> proof_gate=<0|14> blocker=<none|receipt-missing|usb-owner-missing|pcie-owner-missing|usb-descriptor-missing|pcie-descriptor-missing|command-not-ready> root_pointer=no
```

The owner and descriptor pairs are ordered USB then PCIe. A complete current
receipt uses `mask=0x00003fff`, repeats `seq` in `commit`, names
`source=linked-runtime-hid`, and requires both owners, both sealed descriptors,
command readiness, `proof_gate=14`, and `blocker=none`. Missing evidence is
reported as `v=1` with zero identity/body fields and `source=none`; the command
does not fabricate or advance a hardware transition. Active `usb enable-kbd`
and `usb probe-kbd` do not emit either old-good row.

After `nettest` admission, allow its bounded 15-second window to finish and
query `netstats`. The authoritative line is
`nettest: generation=<connection> run_generation=<run> enabled=<bool> running=<bool> verdict=<none|running|pass|peer-assisted-pass|fail> tx_ok=<bool|na> udp_echo_ok=<bool|na> tcp_ok=<bool|na> console_ok=<bool|na> peer_assisted_ok=<bool|na>`.
The positive run generation must match the admission ACK; another run on the
same connection cannot satisfy the command.
When the compiler-declared console-network child owns TCP/IP, the same command
runs a bounded peer-assisted test instead of returning `detail=unsupported`.
A physical result requires an exact post-admission child response drain, a
later NIC TX completion, later RX/TCP counter progress, matching authenticated connection,
and listener readiness; direct VirtIO uses the child drain as its TX boundary.
The child's native ICMPv4 echo response is separate reachability behavior, so a
peer-assisted terminal may truthfully report `udp_echo_ok=false`.
Targets and backend identity are emitted separately as `nettargets:` so the
terminal verdict cannot be truncated by long target strings.
`profile_backend` is the backend selected by the resolved manifest,
`active_driver` is the physical or virtual driver selected for this boot, and
the compatibility `backend` field is an alias of `active_driver`. In Pi Wi-Fi
mode, for example, `profile_backend=bcmgenet-v5` and
`active_driver=backend=cyw43` is the truthful combination.
For isolated profiles, `netstats` also emits `isolated_progress`,
`isolated_units`, and `isolated_state` rows. They report the selected child
turn, last material progress, per-unit counts, bounded command/output queues,
pending egress and response-drain state, and ingress backpressure/drop. These
are diagnostic counters, not a substitute for the terminal `nettest`, ICMP,
authenticated TCP, or target-performance evidence.

When the exact Pi direct-GENET generation is active, one `netstats` command
also runs one bounded causal refresh and emits a complete available snapshot in
this order:

```text
netstats: genet_direct ...
netstats: genet_direct_flags ...
netstats: genet_direct_before ...
netstats: genet_direct_before_ring ...
netstats: genet_direct_irq ...
netstats: genet_direct_irq_source ...
netstats: genet_direct_dpc ...
netstats: genet_direct_dma ...
netstats: genet_direct_ring ...
netstats: genet_direct_peer ...
```

The summary reports one of `refresh=fresh`, `refresh=ready-stale`,
`refresh=ready-unverified`, `refresh=ready-missing`, `refresh=timeout`,
`refresh=rejected`, or `refresh=inactive`; a present replacement is labelled
`phase=pre-idle-service`. The two
`before` rows preserve the stable pre-replay owner/ring cut, while the remaining
rows carry the exact generation's flags, IRQ wake/ack and source state, DPC
counts, hardware DMA indices, direct-ring cursors and packets, peer hints, and
poison state. Missing optional records can shorten this diagnostic batch and
remain missing evidence. `ready-unverified` means no stable pre-replay record
was available, so a visible post-replay record cannot be proven fresh even
though the exact DGHO command returned READY. The refresh uses one exact
idempotent `DGHO` replay,
which can wake the GENET owner and allow its normal post-command idle service to
drain durable RX. These rows are therefore causal triage, not a passive
performance sample. A component field such as `genet_direct_flags active=yes`
does not select or override the canonical `NET_ACTIVE` backend, and no complete
or partial batch proves DHCP, ARP, ICMP, TCP, throughput, QEMU parity, or Pi
acceptance.

On the physical Pi local seat, serial may show `cohesix>` while USB is still
starting, but HDMI does not show the interactive prompt until USB command input
is admitted and the display path is healthy. `USB controller starting...` is
followed by bounded controller, keyboard-enumeration, or first-report feedback;
an unchanged stage appears at most once every two seconds. `USB console ready`
reports the observed stage timings, but it is a passive EventPump record and may
appear after the local seat has already released the prompt from that same
readiness transition. Its relative ordering does not gate the prompt. Once
visible, typed USB bytes update one canonical command row, backspace cannot
erase the prompt prefix, and held up/down arrows use counter-paced repeat plus
completed one-row viewport steps. If command readiness is invalidated, HDMI
retracts the prompt and stale console-ready banner while preserving the typed
suffix, and restores them only after fresh readiness. This changes no command
grammar or USB/HDMI authority. The isolated USB runtime additionally publishes
one passive fixed 48-byte old-good receipt for its exact linked controller,
hub, HID endpoint, interrupt-IN, first-report, and first-byte sequence. Local
seat does not release endpoint readiness or a pre-proof input byte until both
the PCIe and USB descriptor/owner proof chains are current; it retains the
bounded bytes meanwhile and releases them exactly once. A failed service or
recovery clears that cache, while an outstanding retained attach ticket cannot
be discarded by a cached-ready shortcut.

### Shared console line protocol

The TCP console and physical console use the same bounded parser and response
grammar. The generated command inventory is in
[snippets/cohsh_grammar.md](snippets/cohsh_grammar.md). The canonical protocol
rules are in
[INTERFACES.md#target-console-contract](INTERFACES.md#target-console-contract).

For the generated QEMU MCS build, `console-network-runtime` owns the sole TCP listener,
smoltcp packet state, `AUTH` parsing, and transport framing in a restricted
child. It forwards only a bounded command after authentication. Root still
performs every role, ticket, quota, namespace, and command-policy decision and
returns already-authorized response lines. This internal split adds no command,
prompt, listener, or host-visible framing change: `cohsh` continues to observe
the same `OK`/`ERR`/`END` stream. A child standard/protocol fault closes the
network session fail-closed without taking ownership of the serial or
local-seat input queues. Console SC exhaustion instead uses native seL4
postponement and does not manufacture a console Timeout teardown. When no
authenticated TCP session is active, root services serial and
then local-seat input first. During an authenticated session it gives bounded
TCP response flushing priority while continuing to service both physical
inputs and fatal output.

The selected internal contract is manifest schema 1.15 and console ABI/READY
v4. Root may authorize one through eight already-ordered response lines in one
binary `SendBatch` control, but the child still emits one ordinary
length-prefixed line per replenishment-bounded Session unit. For one exact
isolated authenticated connection, root captures synchronous HELP, NETSTATS,
SMP, or CACHELOG output and its exact terminal before publication, then drains
that immutable response through the same lane. The lane may perform eight
useful response units before paying exactly one ordinary
Operator/Runtime/Network debt turn. These are internal scheduling and
shared-page changes only; clients must not send, parse, or depend on SendBatch.
In the reverse direction, the child may coalesce up to eight consecutive
already-authenticated commands for one connection into one bounded
`CommandBatch` publication. Each command retains its own target timestamp and
root validates and capacity-reserves the complete batch before dispatch. The
batch never crosses connection lifecycle events and changes no public command,
framing, authentication, ordering, or response semantics.
After each committed frame the child retains one following service cycle, then
quiesces on a no-progress Session; pending state or capacity failure cannot
spin. Exact eligible retained work uses local Poll; idle or
publication-uncredited work goes directly to Wait with no ordinary Yield. The
child's TCB timeout handler is empty under `NaturalPostpone`, while its standard
fault remains terminal and its reserved timeout capability/resource stays
accounted. These scheduling details are likewise invisible to clients.

Full host compatibility is not yet accepted. The fixed one-socket target matrix
must return HELP 12 total lines, NETSTATS 16, first-call selected-QEMU SMP
activity 17, and CACHELOG 10 for count nine, then PING and QUIT without
reconnect, using the preexisting client response timeout. CACHELOG captures one
immutable bounded snapshot under a single short lock hold; later live-ring
changes cannot alter the response. Its internal 1920-record ring capacity is
not a separate five-second promotion gate. Until fresh exact-artifact evidence
passes the fixed matrix, standard-fault containment, and budget-exhaustion
postponement liveness/isolation, these commands block
Stage 03/REST/performance/26e promotion.

This is QEMU-first as-built behavior. It does not claim that the current Pi 4
network adapter has been moved or that GENET, CYW43, or SDIO has been exercised;
that hardware wiring and evidence are a separate phase.

- Commands and frames are bounded by the selected manifest.
- Serial and local-seat USB keyboard ingress retain independent partial-line
  buffers. Completing or rejecting a line from one physical source does not
  erase an unfinished line from the other; explicit session termination clears
  both.
- Successful commands begin with `OK <VERB>`; for a streaming command the
  acknowledgement is emitted before any payload. Refusals use
  `ERR <VERB> reason=<busy|quota|cut|policy>` when that refusal taxonomy
  applies, with bounded detail where available.
- Streaming `LS`, `CAT`, and `TAIL` responses end with `END`.
- An `ERR` has no side effects unless the interface contract explicitly says
  otherwise.
- Console `ECHO` uses path-first wire syntax. Interactive `cohsh` exposes the
  friendlier `echo <text> > <path>` syntax and translates it to the same
  operation.

## `cohsh`

`cohsh` is a host application. It does not run inside the target and does not add
authority or protocol verbs.

### Start a session

Provide credentials through the environment or an approved secret-management
mechanism. Do not commit tokens or put production tokens in example files.

Direct TCP, where `cohsh` is the sole console owner:

```bash
cargo run -p cohsh -- --transport tcp --tcp-host 127.0.0.1 \
  --tcp-port 31337 --role queen
```

REST, where a running gateway is the sole console owner:

```bash
export COH_REST_URL="http://127.0.0.1:8080"
cargo run -p cohsh -- --transport rest --role queen
```

Deterministic in-process development:

```bash
cargo run -p cohsh -- --transport mock --role queen
```

For TCP, `cohsh` resolves `--auth-token`, then `COHSH_AUTH_TOKEN`, then
`COH_AUTH_TOKEN`, and rejects missing or placeholder credentials. For REST it
resolves the URL from `--rest-url`, `COHSH_REST_URL`, `COH_REST_URL`, or
`HIVE_GATEWAY_URL`, and write authentication from `--rest-auth-token`,
`COHSH_REST_AUTH_TOKEN`, `COH_REST_AUTH_TOKEN`, or
`HIVE_GATEWAY_REQUEST_AUTH_TOKEN`.

REST filesystem operations use a response window composed from the gateway's
declared broker profile:

```text
5000 ms queue admission
+ max(control_response_ms, telemetry_response_ms)
+ 5000 ms HTTP response-delivery grace
```

The canonical `120000/120000 ms` Hive Gateway profile therefore uses a
`130000 ms` client window. `cohsh` accepts an explicit
`--rest-response-timeout-ms`; when the flag is absent it resolves
`COHSH_REST_RESPONSE_TIMEOUT_MS` before using the shared canonical default.
The selected value is applied to the primary REST transport and every pooled
REST transport and must be no smaller than the composed gateway window.
Metadata, name resolution, connection establishment, and response-body
transfer retain separate short bounds. This setting does not add retries or
change the REST request, response, console, or ACK/ERR/END contract.

`--role` attaches immediately. Without it, the shell starts detached and
expects `attach <role> [ticket]`. Supported role selectors are `queen`,
`worker-heartbeat` (alias `worker`), `worker-gpu`, `worker-bus`, and
`worker-lora`; the selected profile and ticket policy determine whether an
attachment is allowed. These selectors apply to direct attachments. The current
REST transport accepts only the local `queen` role, while every operation still
inherits the gateway's upstream role and optional ticket.

Use `cohsh --help` for command-line options. Generated pool, retry, heartbeat,
ticket, and client defaults are maintained in:

- [snippets/cohsh_client.md](snippets/cohsh_client.md)
- [snippets/cohsh_policy.md](snippets/cohsh_policy.md)
- [snippets/cohsh_ticket_policy.md](snippets/cohsh_ticket_policy.md)
- [snippets/ticket_quotas.md](snippets/ticket_quotas.md)

These snippets are `coh-rtc` outputs and must not be edited by hand.

### Interactive commands

Run `help` in the shell for the exact inventory compiled into the binary.

| Command | Purpose |
| --- | --- |
| `attach <role> [ticket]`, `login ...` | Open an attached session. |
| `detach` | Close the attached session without exiting the shell. |
| `ping` | Check the active attachment. |
| `ls <path>` | List a directory. |
| `cat <path>` | Read bounded file contents. |
| `tail <path> [lines]` | Read a bounded tail; `lines` is at most 256. |
| `log` | Tail `/log/queen.log`. |
| `log dump <file> [--force]` | Export the retained Queen log to a local file. |
| `echo <text> > <path>` | Append one validated line. |
| `spawn <heartbeat\|gpu\|lora> <key=value>...` | Validate role-specific arguments and submit a Queen Worker request. A successful ACK proves request admission only, not READY. |
| `kill <worker_id>` | Submit a Queen worker-termination request. |
| `lifecycle <cordon\|drain\|resume\|quiesce\|reset>` | Validate and submit a lifecycle transition. `reset` changes lifecycle state; it is not a platform reboot. |
| `telemetry push <src> --device <id>` | Upload a bounded telemetry segment or content-reference manifest. |
| `test [--mode quick\|full\|smp] [--json] [--timeout <s>] [--no-mutate]` | Run the installed Cohesix self-test scripts. |
| `nettest`, `netstats` | Run network diagnostics through the console grammar. |
| `reboot` | Request an authenticated Queen platform reboot. |
| `pool bench <options>` | Run the bounded host-side session-pool benchmark. |
| `tcp-diag [port]` | Diagnose TCP connectivity in TCP-enabled builds. |
| `bind <src> <dst>`, `mount <service> <path>` | Apply namespace operations provided by the selected profile. |
| `quit` | Close the session and exit. |

Payload schemas, control paths, and `/proc` nodes are intentionally not
duplicated here. Use [INTERFACES.md](INTERFACES.md).

### Session behavior

- Interactive TCP mode reconnects with bounded backoff after a transport loss;
  the operator must re-establish the attachment when required.
- Script mode fails the run on an unrecoverable transport or command error.
- TCP `quit` succeeds only after the client receives exact `OK QUIT`,
  half-closes its write side, and observes peer EOF on that same connection.
  A missing acknowledgement, timeout, post-terminal frame, or missing EOF
  fails script mode; QUIT is never retried on a replacement connection.
- Heartbeats and retry limits come from generated policy unless explicitly
  overridden.
- The `qemu` transport launches the staged QEMU artifacts and is diagnostic;
  its transport implementation rejects writes. Use TCP or REST for live
  control-plane writes.

### Self-test modes and report

`test` always performs a preflight ping, then runs the negative script and the
script selected by `--mode`:

| Mode | Selected script | Intended scope |
| --- | --- | --- |
| `quick` | `/proc/tests/selftest_quick.coh` | Fast control-plane health check; this is the default. |
| `full` | `/proc/tests/selftest_full.coh` | Broader installed regression sequence. |
| `smp` | `/proc/tests/selftest_smp.coh` | SMP-specific installed checks. |

The negative script is `/proc/tests/selftest_negative.coh`. The default timeout
is 30 seconds and the hard maximum is 120 seconds. `--no-mutate` skips
`spawn`, `kill`, and the associated worker telemetry tails; it does not bypass
the negative checks or any server-side policy. The installed scripts end their
sessions with `quit`; interactive `cohsh` attempts to restore its previous
attachment afterward, while an outer `--script` run remains detached.

`--json` emits one JSON object on one line. This example is expanded only for
readability; `transcript_excerpt` is omitted when no bounded transcript is
needed:

```json
{
  "ok": true,
  "mode": "quick",
  "elapsed_ms": 123,
  "checks": [
    {
      "name": "preflight/ping",
      "ok": true,
      "detail": "OK ping"
    }
  ],
  "version": "1"
}
```

Treat `version` as the report-schema version. Automation must fail the run when
`ok` is false rather than inferring success from process output text.

### Worker-spawn arguments

The interactive command accepts the three executable Worker declarations:
Heartbeat, GPU, and LoRA. Arguments use `key=value`; unknown, duplicate, or
missing keys are rejected before `/queen/ctl` is written. WorkerBus remains a
model/session-only role: `spawn bus` and `spawn worker-bus` fail deterministically
without writing `/queen/ctl`.

| Role selector | Required keys | Optional keys |
| --- | --- | --- |
| `heartbeat`, `worker`, `worker-heartbeat` | `ticks` | `ttl_s`, `ops` |
| `gpu`, `worker-gpu` | `gpu_id`, `mem_mb`, `streams`, `ttl_s` | `priority`, `budget_ttl_s`, `budget_ops` |
| `lora`, `worker-lora` | none | none |

```text
spawn heartbeat ticks=100 ttl_s=120 ops=500
spawn gpu gpu_id=GPU-0 mem_mb=4096 streams=2 ttl_s=120 priority=1
spawn lora
```

These commands construct the strict records documented in
[INTERFACES.md#worker-and-mount-control](INTERFACES.md#worker-and-mount-control).
An accepted append proves only that the bounded request was admitted. Observe
the structured record at the generated canonical
`/shard/<label>/worker/<id>/telemetry` path before reporting lifecycle state.
Declaration, lifecycle, artifact, receipt, and execution proof are independent
axes: configured/executable does not mean READY, a host-model record is not QEMU
proof, and package verification is not execution evidence. The compatibility
`/worker/<id>/telemetry` path exists only when the generated profile enables the
legacy alias.

### Telemetry file upload

`telemetry push` accepts a non-empty local file with one of these extensions:

| Extension | Declared MIME type |
| --- | --- |
| `.txt`, `.log` | `text/plain` |
| `.json` | `application/json` |
| `.ndjson` | `application/x-ndjson` |
| `.csv` | `text/csv` |

For bounded UTF-8 input that fits the selected manifest's segment budget,
`cohsh` writes `cohsh-telemetry-push/v1` inline records. Binary input, oversized
UTF-8 envelopes, or input larger than the inline segment budget is represented
instead by `coh-ref-c/v1` records containing sequence, offset, length, and a
SHA-256 digest for each host-side chunk. Reference mode transfers the manifest,
not the referenced file bytes; retain the source file under the deployment's
content-retention policy.

The acknowledgement reports `seg_id`, record count, encoded bytes, original
source bytes, and `mode=inline|reference`. Generated limits cap the source,
reference entry count, reference-manifest bytes, segment bytes, and per-device
retention; see [snippets/cohsh_client.md](snippets/cohsh_client.md).

## `.coh` scripts

`.coh` is a deterministic line-oriented format interpreted by `cohsh`. It is
not a general-purpose shell: it has no variables, expansion, branching, loops,
includes, macros, or runtime downloads.

### Grammar

- One statement per line.
- Blank lines are ignored.
- `#` begins a comment, including an inline comment.
- A normal line is executed by the same handler used at the `coh>` prompt.
- `EXPECT OK` requires the last response line to start with `OK`.
- `EXPECT ERR` requires the last response line to start with `ERR`.
- `EXPECT SUBSTR <text>` and `EXPECT NOT <text>` apply case-sensitive checks
  to the last response line.
- `WAIT <ms>` is a local delay capped at 2000 ms; it sends no target command.
- A script contains at most 256 non-empty statements.

Assertions apply to the most recent command response recorded by `cohsh`. A
failure reports the source line, command, last response, response source, and a
bounded recent-response history, then exits non-zero.

Example read-only health script:

```text
# health.coh
ping
EXPECT OK
cat /proc/lifecycle/state
EXPECT OK
EXPECT SUBSTR path=/proc/lifecycle/state
tail /log/queen.log 16
EXPECT OK
```

Validate without execution:

```bash
cargo run -p cohsh -- --check health.coh
```

Execute against the already selected transport:

```bash
cargo run -p cohsh -- --transport rest --role queen --script health.coh
```

The checked-in regression scripts and their transcript fixtures are governed by
[TEST_PLAN.md](TEST_PLAN.md). Generated scripts such as
[`scripts/cohsh/boot_v0.coh`](../scripts/cohsh/boot_v0.coh) must be regenerated,
not hand-edited.

## Compiler-generated reference

The following marker-delimited blocks are verified mirrors of the linked
standalone `coh-rtc` snippets. They are retained for generated-document and
compliance guards. Do not edit their contents by hand; change manifest/IR
inputs and regenerate every affected output.

<!-- markdownlint-disable MD022 MD031 MD032 MD033 -->

<details>
<summary>cohsh client policy</summary>

<!-- coh-rtc:cohsh-policy:start -->
### cohsh client policy (generated)
- `manifest.sha256`: `a9a50408519f33cf2e05932cffffa5dbb521958870b16edf2f36311ff60385a1`
- `policy.sha256`: `5ba90cc0b9624f21ac7737ecc83974a03511bb202dc4232fbcda97992add48a5`
- `cohsh.pool.control_sessions`: `2`
- `cohsh.pool.telemetry_sessions`: `24`
- `cohsh.tail.poll_ms_default`: `1000`
- `cohsh.tail.poll_ms_min`: `250`
- `cohsh.tail.poll_ms_max`: `10000`
- `cohsh.host_telemetry.nvidia_poll_ms`: `1000`
- `cohsh.host_telemetry.systemd_poll_ms`: `2000`
- `cohsh.host_telemetry.docker_poll_ms`: `2000`
- `cohsh.host_telemetry.k8s_poll_ms`: `5000`
- `retry.max_attempts`: `3`
- `retry.backoff_ms`: `200`
- `retry.ceiling_ms`: `2000`
- `retry.timeout_ms`: `5000`
- `heartbeat.interval_ms`: `15000`
- `trace.max_bytes`: `1048576`

_Generated from `configs/root_task.toml` (sha256: `a9a50408519f33cf2e05932cffffa5dbb521958870b16edf2f36311ff60385a1`)._
<!-- coh-rtc:cohsh-policy:end -->

</details>

<details>
<summary>cohsh client defaults</summary>

<!-- coh-rtc:cohsh-client:start -->
### cohsh client defaults (generated)
- `manifest.sha256`: `a9a50408519f33cf2e05932cffffa5dbb521958870b16edf2f36311ff60385a1`
- `worker.task_abi_schema`: `worker-task-abi/v2`
- `worker.task_abi_version`: `2`
- `worker.observation_schema`: `cohesix-worker-observation/v1`
- `worker.integration_evidence_schema`: `cohesix-worker-integration-evidence/v1`
- `worker.maximum_live_tasks`: `256`
- `worker.canonical_telemetry_template`: `/shard/<label>/worker/<id>/telemetry`
- `worker.shard_bits`: `6`
- `worker.legacy_worker_alias`: `true`
- `worker.lifecycle`: `absent, queued, starting, ready, closing, faulted, terminal`
- `worker.receipt`: `none, pending, confirmed, rejected, stale`
- `worker.artifact`: `missing, verified, mismatch`
- `worker.execution_proof`: `none, host-model, qemu, fresh-pi`
- `worker.role.worker-heartbeat`: declaration=`executable`, executable_slots=`1`
- `worker.role.worker-gpu`: declaration=`executable`, executable_slots=`127`
- `worker.role.worker-bus`: declaration=`model-only`, executable_slots=`0`
- `worker.role.worker-lora`: declaration=`executable`, executable_slots=`128`
- `secure9p.msize`: `8192`
- `secure9p.walk_depth`: `8`
- `trace.max_bytes`: `1048576`
- `client_paths.queen_ctl`: `/queen/ctl`
- `client_paths.queen_lifecycle_ctl`: `/queen/lifecycle/ctl`
- `client_paths.queen_schedule_ctl`: `/queen/schedule/ctl`
- `client_paths.queen_lease_ctl`: `/queen/lease/ctl`
- `client_paths.queen_export_ctl`: `/queen/export/ctl`
- `client_paths.policy_ctl`: `/policy/ctl`
- `client_paths.log`: `/log/queen.log`
- `telemetry_ingest.max_segments_per_device`: `4`
- `telemetry_ingest.max_bytes_per_segment`: `131072`
- `telemetry_ingest.max_total_bytes_per_device`: `524288`
- `telemetry_ingest.max_reference_entries_per_segment`: `1024`
- `telemetry_ingest.max_reference_manifest_bytes_per_segment`: `131072`
- `telemetry_ingest.max_reference_bytes_per_segment`: `1073741824`
- `telemetry_ingest.eviction_policy`: `evict-oldest`

_Generated from `configs/root_task.toml` (sha256: `a9a50408519f33cf2e05932cffffa5dbb521958870b16edf2f36311ff60385a1`)._
<!-- coh-rtc:cohsh-client:end -->

</details>

<details>
<summary>cohsh console grammar</summary>

<!-- coh-rtc:cohsh-grammar:start -->
### cohsh console grammar (generated)
- `help`
- `bi`
- `caps [mcs]`
- `smp [activity|mcs|dump]`
- `mem`
- `ping`
- `test`
- `nettest`
- `netstats`
- `reboot`
- `log`
- `cachelog [n]`
- `quit`
- `tail <path> [lines]`
- `cat <path>`
- `ls <path>`
- `echo <path> <payload>`
- `attach <role> [ticket]`
- `spawn <payload>`
- `kill <worker>`

_Generated from cohsh-core verb specs (20 verbs)._
<!-- coh-rtc:cohsh-grammar:end -->

</details>

<details>
<summary>cohsh ticket policy and quotas</summary>

<!-- coh-rtc:cohsh-ticket-policy:start -->
### cohsh ticket policy (generated)
- `ticket.max_len`: `224`
- `queen` tickets are optional; TCP validates claims when present, NineDoor passes through.
- `worker-*` tickets are required; role must match and subject identity is mandatory.

_Generated from cohsh-core ticket policy._
<!-- coh-rtc:cohsh-ticket-policy:end -->

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

</details>

<details>
<summary>coh policy and doctor defaults</summary>

<!-- coh-rtc:coh-policy:start -->
### coh policy defaults (generated)
- `manifest.sha256`: `a9a50408519f33cf2e05932cffffa5dbb521958870b16edf2f36311ff60385a1`
- `policy.sha256`: `fbcdae1a2715cd22354665232d4826200b0d520a16a4d9c2b7a672586e4a5b2e`
- `coh.worker.task_abi_schema`: `worker-task-abi/v2`
- `coh.worker.task_abi_version`: `2`
- `coh.worker.observation_schema`: `cohesix-worker-observation/v1`
- `coh.worker.integration_evidence_schema`: `cohesix-worker-integration-evidence/v1`
- `coh.worker.maximum_live_tasks`: `256`
- `coh.worker.canonical_telemetry_template`: `/shard/<label>/worker/<id>/telemetry`
- `coh.worker.shard_bits`: `6`
- `coh.worker.legacy_worker_alias`: `true`
- `coh.worker.lifecycle`: `absent, queued, starting, ready, closing, faulted, terminal`
- `coh.worker.receipt`: `none, pending, confirmed, rejected, stale`
- `coh.worker.artifact`: `missing, verified, mismatch`
- `coh.worker.execution_proof`: `none, host-model, qemu, fresh-pi`
- `coh.worker.role.worker-heartbeat`: declaration=`executable`, executable_slots=`1`
- `coh.worker.role.worker-gpu`: declaration=`executable`, executable_slots=`127`
- `coh.worker.role.worker-bus`: declaration=`model-only`, executable_slots=`0`
- `coh.worker.role.worker-lora`: declaration=`executable`, executable_slots=`128`
- `coh.mount.root`: `/`
- `coh.mount.allowlist`: `/proc, /queen, /shard, /worker, /log, /gpu, /host`
- `coh.telemetry.root`: `/queen/telemetry`
- `coh.telemetry.max_devices`: `32`
- `coh.telemetry.max_segments_per_device`: `4`
- `coh.telemetry.max_bytes_per_segment`: `131072`
- `coh.telemetry.max_total_bytes_per_device`: `524288`
- `coh.run.lease.schema`: `gpu-lease/v1`
- `coh.run.lease.active_state`: `ACTIVE`
- `coh.run.lease.max_bytes`: `1024`
- `coh.run.breadcrumb.schema`: `gpu-breadcrumb/v1`
- `coh.run.breadcrumb.max_line_bytes`: `512`
- `coh.run.breadcrumb.max_command_bytes`: `256`
- `coh.peft.export.root`: `/queen/export/lora_jobs`
- `coh.peft.export.max_telemetry_bytes`: `131072`
- `coh.peft.export.max_policy_bytes`: `8192`
- `coh.peft.export.max_base_model_bytes`: `1024`
- `coh.peft.import.registry_root`: `out/model_registry`
- `coh.peft.import.max_adapter_bytes`: `67108864`
- `coh.peft.import.max_lora_bytes`: `65536`
- `coh.peft.import.max_metrics_bytes`: `65536`
- `coh.peft.import.max_manifest_bytes`: `8192`
- `coh.peft.activate.max_model_id_bytes`: `128`
- `coh.peft.activate.max_state_bytes`: `4096`
- `retry.max_attempts`: `3`
- `retry.backoff_ms`: `200`
- `retry.ceiling_ms`: `2000`
- `retry.timeout_ms`: `5000`
<!-- coh-rtc:coh-policy:end -->

<!-- coh-rtc:coh-doctor:start -->
### coh doctor checks (generated)
- `check=policy` validates `coh_policy.toml` against manifest + policy hashes.
- `check=ticket` uses `ticket.max_len=224` and TCP policy (queen tickets optional, worker tickets required).
- `check=mount` validates allowlist under `coh.mount.root` and requires FUSE when not `--mock`.
- `check=nvml` prefers NVML when not `--mock`; Jetson-class NVML falls back to CUDA discovery.
- `check=runtime` checks `python3` and `qemu-system-aarch64` (QEMU skipped with `--mock`).
- `secure9p.msize`: `8192`
- `secure9p.walk_depth`: `8`
- `coh.mount.allowlist`: `/proc, /queen, /shard, /worker, /log, /gpu, /host`

_Generated by coh-rtc (sha256: `8ff5f5a73c1e4d454f1263e3235d01d2bde35adb6553bd578b64ae9f496b3b4b`)._
<!-- coh-rtc:coh-doctor:end -->

</details>

<details>
<summary>Python client defaults</summary>

<!-- coh-rtc:cohesix-py:start -->
### Cohesix Python defaults (generated)
- `manifest.sha256`: `a9a50408519f33cf2e05932cffffa5dbb521958870b16edf2f36311ff60385a1`
- `cohesix.defaults.sha256`: `d530c26d99955663b33f1d0a3a095f62834a95300c42aa748ee75c814557ad0e`
- `secure9p.msize`: `8192`
- `secure9p.walk_depth`: `8`
- `console.max_line_len`: `2304`
- `console.max_path_len`: `96`
- `console.max_json_len`: `192`
- `console.max_echo_len`: `2048`
- `telemetry_ingest.max_bytes_per_segment`: `131072`
- `telemetry_ingest.max_total_bytes_per_device`: `524288`
- `telemetry_ingest.max_reference_entries_per_segment`: `1024`
- `telemetry_ingest.max_reference_manifest_bytes_per_segment`: `131072`
- `telemetry_ingest.max_reference_bytes_per_segment`: `1073741824`
- `coh.mount.root`: `/`
- `coh.mount.allowlist`: `/proc, /queen, /shard, /worker, /log, /gpu, /host`
- `coh.telemetry.root`: `/queen/telemetry`
- `coh.run.breadcrumb.max_line_bytes`: `512`
- `coh.peft.import.registry_root`: `out/model_registry`

_Generated by coh-rtc (sha256: `af612d526cc50d3f18479b3302002d341ee3c5f7e0b4e3c80cf9380eeb8fbdb0`)._
<!-- coh-rtc:cohesix-py:end -->

</details>

<details>
<summary>SwarmUI defaults</summary>

<!-- coh-rtc:swarmui-defaults:start -->
### SwarmUI defaults (generated)
- `manifest.sha256`: `a9a50408519f33cf2e05932cffffa5dbb521958870b16edf2f36311ff60385a1`
- `swarmui.defaults.sha256`: `f7f3cec4d006acd3098085576cfe7c2a7e58da9aa38a64db664d9bc8194375e7`
- `swarmui.ticket_scope`: `per-ticket`
- `swarmui.cache.enabled`: `false`
- `swarmui.cache.max_bytes`: `262144`
- `swarmui.cache.ttl_s`: `3600`
- `swarmui.hive.frame_cap_fps`: `30`
- `swarmui.hive.step_ms`: `16`
- `swarmui.hive.lod_zoom_out`: `0.7`
- `swarmui.hive.lod_zoom_in`: `1.25`
- `swarmui.hive.lod_event_budget`: `512`
- `swarmui.hive.snapshot_max_events`: `4096`
- `swarmui.hive.overlay_lines`: `3`
- `swarmui.hive.detail_lines`: `50`
- `swarmui.hive.line_cap_bytes`: `160`
- `swarmui.hive.per_worker_bytes`: `2048`
- `swarmui.hive.pending_lines_per_worker`: `64`
- `swarmui.hive.pending_event_cap`: `4096`
- `swarmui.hive.poll_workers_per_tick`: `32`
- `swarmui.hive.status_poll_ms`: `500`
- `swarmui.hive.degrade_pressure`: `1.0`
- `swarmui.paths.telemetry_root`: `/worker`
- `swarmui.paths.proc_ingest_root`: `/proc/ingest`
- `swarmui.paths.worker_root`: `/shard`
- `swarmui.paths.namespace_roots`: `/proc, /queen, /shard, /worker, /log, /gpu`
- `swarmui.worker_runtime.maximum_live_tasks`: `256`
- `swarmui.worker_runtime.canonical_telemetry_template`: `/shard/<label>/worker/<id>/telemetry`
- `swarmui.worker_runtime.shard_bits`: `6`
- `swarmui.worker_runtime.legacy_worker_alias`: `true`
- `swarmui.worker_runtime.role.worker-heartbeat`: declaration=`executable`, executable_slots=`1`
- `swarmui.worker_runtime.role.worker-gpu`: declaration=`executable`, executable_slots=`127`
- `swarmui.worker_runtime.role.worker-bus`: declaration=`model-only`, executable_slots=`0`
- `swarmui.worker_runtime.role.worker-lora`: declaration=`executable`, executable_slots=`128`
- `trace.max_bytes`: `1048576`

_Generated from `configs/root_task.toml` (sha256: `a9a50408519f33cf2e05932cffffa5dbb521958870b16edf2f36311ff60385a1`)._
<!-- coh-rtc:swarmui-defaults:end -->

</details>

<!-- markdownlint-enable MD022 MD031 MD032 MD033 -->

## Related documentation

- [HOST_TOOLS.md](HOST_TOOLS.md) — host applications and safe composition.
- [API_GUIDELINES.md](API_GUIDELINES.md) — REST projection and authentication.
- [PYTHON_SUPPORT.md](PYTHON_SUPPORT.md) — Python client backends.
- [FAILURE_MODES.md](FAILURE_MODES.md) — evidence-led recovery.
- [OPERATOR_WALKTHROUGH.md](OPERATOR_WALKTHROUGH.md) — canonical live runbook.
- [OPERATOR_RECIPES.md](OPERATOR_RECIPES.md) — advanced operator workflows.
- [ROLES_AND_SCHEDULING.md](ROLES_AND_SCHEDULING.md) — role and namespace authority.
