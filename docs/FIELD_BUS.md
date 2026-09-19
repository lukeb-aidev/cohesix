<!-- Author: Lukas Bower -->
<!-- Purpose: Define the host-only mapped MODBUS/DNP3 transport, acknowledgement and recovery contract separately from WorkerBus model state. -->
<!-- Copyright 2026 Lukas Bower -->
# Host field-bus contract

`sidecar-bus` provides host-only protocol clients. It introduces no target
listener or device driver. WorkerBus remains model/session-only. The historical
in-memory `BusAdapter` always queues data, including when its model link is
marked online; that flag can never discard a frame or produce delivery proof.
The former MODBUS/DNP3 aliases are replaced by native adapters under the explicit
`live,modbus,dnp3` feature selection.

## Compiler-owned maps

`providers.field_bus` in `configs/host_integration_acceptance.toml` contains at
most 64 endpoint records. No endpoint is enabled implicitly. An endpoint names
one literal TCP socket address or one absolute serial device under `/dev`, the
protocol's unit/master/outstation identity, a whole-operation deadline (at most
five seconds), polling interval, observation TTL (at most 30 seconds), and at
most 64 named point maps. Broadcast addresses, DNS-selected addresses, invalid
ranges, duplicate point ids and arbitrary native functions are refused.

Each point selects one exact operation. MODBUS supports reads of coils,
discrete inputs, holding registers and input registers (functions 1–4), with
at most 256 bits or 64 registers. Control maps select one fixed single-coil or
single-register write (functions 5/6); coil values must be 0 or 0xff00. The
request supplies the endpoint and point ids, never register addresses, function
codes, write values or paths.

The DNP3 subset supports single-fragment static g1v2 binary inputs, g20v1
counters and g30v1 signed analog inputs, with exact ranges of at most 16 points.
Both 8-bit and 16-bit start/stop response qualifiers are checked. g12v1 controls
use a fixed 8-bit index, select-before-operate, count one, an allowed pulse/latch
code and at most one-second pulse timing. Both select and operate require the
exact echoed CROB and success status. Event scans, unsolicited responses,
segmentation, secure authentication and other object variations return typed
unsupported state. Link/application identity, CRCs, application sequence,
range, object widths and point quality are validated before delivery.

Class-data-available IIN bits remain observations. Restart, need-time, local
control, device trouble and request errors refuse this profile. The client does
not silently reset a device, set its clock, clear restart indications or enable
unsolicited reporting. Commissioning those device states is an independent
operator task. Protocol acknowledgement proves the remote protocol response;
it does not prove physical plant behavior or device attestation.

## Native ownership and delivery

TCP connection, framing and response reads share one deadline. Serial access
requires a character device, rejects symlinks, holds a host advisory ownership
lock, saves/restores terminal settings and applies the selected baud/parity.
MODBUS uses 11-bit serial characters and its bounded inter-frame silent interval;
DNP3 without parity uses 8N1. An operator must give the process sole device
access: a process that ignores advisory locks remains outside this contract.
Neither protocol is encrypted or authenticated by its base framing. Exact
endpoint maps and operating-system/network access controls remain necessary.

The private spool retains at most 64 operations within one MiB. Each entry binds
sequence, endpoint/point, immutable operation fingerprint, provider graph,
request/idempotency identity, attempts and state. Atomic replacement and file/
directory synchronization precede every wire attempt. A terminal `delivered`
entry contains the validated raw request and remote acknowledgement bytes,
decoded values and observation time. A local process exit, queued frame, link
flag or successful send cannot produce that state.

New read identities obey the generated per-point polling interval; a host clock
regression refuses another poll. Reads permit at most three explicit attempts within 60 seconds. A lost read
response remains pending and can be retried on a fresh connection. Controls
that might have reached the device become `ambiguous` if acknowledgement is
missing; restart never resubmits them. Recovery requires independent observation
and a new, separately authorized request. Exact terminal retries return the
retained entry without network I/O. Changed operation maps or registry hashes
refuse reuse of old request identity. Missing, corrupted, foreign-owned or
insecure WAL files fail closed; an existing owner file cannot reset a lost
journal. Terminal entries are not implicitly evicted to admit new work.

## Operator surface and proof

Build the native client with:

```sh
cargo build --locked --release -p sidecar-bus --features live,modbus,dnp3
sidecar-bus --state-dir "$PRIVATE_STATE" --request "$REQUEST_JSON"
sidecar-bus --state-dir "$PRIVATE_STATE" --status
```

A `cohesix-field-bus-request/v1` request contains `id`, `idempotency_key`,
`endpoint` and `point`. Reads are the default. A control also requires
`--admitted-ticket` and `--evidence-enrollment-dir`, containing an independently
enrolled, signature-verified grant for the exact `modbus.control` or
`dnp3.control` ticket, target, writer epoch, compiled manifest, executable digest
and point arguments. A boolean or request label cannot grant control authority.
The standalone output remains a non-authoritative operation report.

The normal host-ticket-agent dispatch path supports `modbus.read`,
`modbus.control`, `dnp3.read` and `dnp3.control` when selected by the manifest's
action allowlist. A version-1 ticket uses the **endpoint id** as `target` and
exactly `{"endpoint":"<id>","point":"<id>"}` as `args`. The Python
`HostTicketRequest` checks the same compiled map. The agent requires
`--field-bus-state-root` and retains native observation CAS before returning
success. Controls additionally require independent signed evidence enrollment.
Recovery reads the matching WAL and retains the original ACK time and native
operation identity. Pending reads may resume within their attempt/age bounds;
an uncertain control is never retransmitted or relabelled as a new result.

An enrolled `host-sidecar-bridge` source can publish `modbus`/`dnp3` through
`/host/snapshots/<provider>/<source>/ctl` using its ordinary delegated identity.
`COHESIX_FIELD_BUS_STATE_ROOT` names the same private WAL. Publication includes
only the latest successful read for **every** configured read point, its map
digest, unit/outstation, sequence, response digest, original time and expiry.
Missing, failed, stale or changed-map reads withdraw the complete provider.
Republishing subtracts elapsed age from TTL; it cannot refresh the ACK. Values
are split into bounded rows. Snapshot collection never accesses the device or
turns read data into action authority.

Native deployment profiles include `sidecar-bus`, this contract and the
agent/publisher service templates. Set `field_bus = true` and an absolute
`evidence_enrollment_dir` in service enrollment; the renderer refuses missing
compiled maps. Serial permissions derive only from exact compiled `/dev` paths.
TCP-only service enrollment retains private devices. Spool exhaustion is an
explicit refusal; archive evidence and enroll a new writer generation before
retiring state. Do not delete a journal to reuse request identity.

Focused codec/WAL tests and the independent OpenDNP3 reference lane retain host
contract/interoperability evidence. Software reference outstations are named
explicitly. Such records cannot promote a physical field device, WorkerBus,
QEMU/Pi target or production use case.

Protocol sources: [MODBUS application specification](https://modbus.org/docs/Modbus_Application_Protocol_V1_1b3.pdf),
[MODBUS specifications](https://www.modbus.org/modbus-specifications), and
[OpenDNP3 3.1.2](https://github.com/dnp3/opendnp3/tree/c1dc7165a79cc08edbf4b55d2ff4162efb176f92).
The independent reference stack is built into ignored test output, not vendored
or shipped as part of the Cohesix protocol client.

## Native reference command

Build OpenDNP3 at the pinned commit above outside the tracked tree, using CMake
with `DNP3_STATIC_LIBS=ON`, `DNP3_TESTS=OFF`, and `DNP3_EXAMPLES=OFF`.
Use an isolated Python environment with `pymodbus==3.11.4`, `pyserial==3.5` and
`cryptography==46.0.5`. The exact native conformance command is:

```sh
python scripts/ci/field_bus_conformance.py \
  --state-dir out/provider-conformance/field-bus-native \
  --opendnp3-source /absolute/reference/opendnp3 \
  --opendnp3-library /absolute/reference/build/cpp/lib/libopendnp3.a
```

Run it from a checkout with no canonical field-bus endpoints selected and no
concurrent generation. It builds and freezes a private loopback/PTY profile,
restores canonical generated files, then checks reads, MODBUS exceptions,
DNP3 restart refusal, separately signed fixture controls and durable read ACK
replay. It records exact source inputs, client/library/reference hashes, wire
logs, journals and an artifact inventory. The fixture keys are deleted at exit.
These keys are never Root or production enrollment; the resulting evidence
explicitly denies physical UART, plant, Worker and production qualification.
