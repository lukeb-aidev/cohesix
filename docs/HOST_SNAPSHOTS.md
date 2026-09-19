<!-- Author: Lukas Bower -->
<!-- Purpose: Define source-scoped host observation publication, bounded transfer and receiver freshness. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# Native host snapshots

Milestone 27b / `m27b-native-provider-discovery-and-actions` adds the
manifest-schema-1.23 receiver for authenticated host observations. Receiver and durable native publisher implementations have focused contract
checks and live macOS/Merlin publication through authenticated REST into QEMU. A received observation
is not an admission grant, provider execution receipt or Worker completion.

`ecosystem.host.snapshots` selects enablement, maximum serialized bytes, native
entry count, value bytes, TTL and explicit publisher/source pairs. A maximum of
eight sources and sixteen provider/source pairs is allowed. Each selected
provider must exist in the compiler-owned provider registry. The default
configuration enrolls `linux-reference` for systemd, Docker, Kubernetes, NVIDIA,
Jetson and network observations, and `mac-controller` for launchd and network.
The canonical provider id is `network`; the older `/host/net` fixture namespace
is not an alias for this interface. Source ids may be customized only through
compiler input. Distinct hosts publishing network state have distinct paths.

The receiver exposes:

- `/host/snapshots`: selected provider names.
- `/host/snapshots/<provider>`: enrolled source names for that provider.
- `/host/snapshots/<provider>/<source>/ctl`: append-only transfer control.
- `/host/snapshots/<provider>/<source>/snapshot`: current canonical source record;
  empty after expiry or before the first accepted observation.
- `/host/snapshots/<provider>/<source>/status`: receiver status, source, provider,
  last accepted sequence and refusal/expiry information.

All writes use the existing authenticated Queen namespace path, session scope,
lifecycle and policy checks. REST additionally applies the caller's delegated
path authority. No new listener, device access or host API is added to the VM.
Global snapshots are admin observations under generated read visibility. A
source label is an enrollment and correlation identity under the existing
transport authority; it is not hardware attestation or an external execution
signature. Only the selected native collector may substantiate native facts.

One `cohesix-host-snapshot/v1` record contains, in canonical serialization order:
`schema`, `provider`, `source_id`, `epoch`, `sequence`, `observed_unix_ms`,
`ttl_ms`, `available`, `reason`, and `entries`. Entries contain exactly `path`
and `value`, sorted by relative path, with no duplicates, traversal or NUL.
`epoch` must equal the generated writer epoch. Sequence and source-observation
Unix time cannot regress. An unavailable record has no entries and an explicit
`not_supported`, `not_enabled`, `source_failed` or `timed_out` reason. Unknown
fields, noncanonical JSON, unknown sources/providers, excessive values/counts
and changed bytes under a retained sequence are refused.

The default bound is 8,192 bytes, 64 entries, 1,024 value bytes and 30,000 ms
TTL. Absolute shared bounds are 8,192 bytes, 64 entries, 4,096 value bytes and
30,000 ms TTL. A source must produce a bounded projection rather than silently
truncate discovery or retain healthy data after an observation failure. These
bounds fit the existing 8 KiB CAT reassembly contract and do not enlarge console
buffers or Secure9P `msize`.

Transfer reuses the existing bounded namespace upload shape, one command per
ECHO append:

```text
begin bytes=<canonical-byte-count> sha256=<64-lowercase-hex>
b64:<base64-chunk>
end
```

Chunks remain within the selected console ECHO limit. At most one bounded
transfer is retained per provider/source pair. A fresh `begin` replaces only
that pair's pending upload. Only `end` may replace visible data, after the byte
count, digest, canonical record, enrollment, epoch and replay checks succeed.
A digest/shape failure leaves the previous accepted data until its existing TTL
expires. Incomplete uploads expire within the configured maximum TTL.

Receiver freshness begins at `begin`, measured by the selected HAL monotonic
timebase, so upload time consumes the TTL. A transfer that reaches `end` after
its TTL is refused. The publisher's Unix timestamp remains provenance metadata;
it is not substituted for a receiver clock. The source must also enforce its
own bounded capture-to-send age. An exact retry acknowledges the prior bytes
without refreshing the original TTL. Expiry clears native bytes but retains
the accepted sequence/digest fence for the current receiver epoch. Source
failure publishes an unavailable record with no healthy entries. A regressed
receiver clock withdraws data. Receiver restart does not claim a persistent
snapshot replay table; live publishers must retain their own monotonic sequence
and collect new observations after reconnect.

Focused receiver evidence is listed in the
[M27b implementation record](audit/M27B_IMPLEMENTATION_RECORD.md). Those checks
alone do not prove a live native publisher or QEMU/Pi execution.

## Native publisher and Python reads

`host-sidecar-bridge` live mode requires `--source-id` and `--state-dir`.
The source must be compiled into the selected manifest. The state root must
already exist, belong to the service account and have mode `0700`. One process
locks each source/epoch directory. It reserves the next sequence with file and
directory fsync before writing to the target. A crash may skip a sequence.
Missing/corrupt existing cursors refuse startup; deleting a cursor is not a
supported reset. Collection and disk reservation consume the source TTL.
A transport failure stops the publisher; a native observation failure publishes
a typed withdrawal and allows the remaining providers to be observed.

```sh
host-sidecar-bridge --source-id linux-reference --state-dir /var/lib/cohesix \
  --rest-url http://127.0.0.1:8080 --provider systemd --watch
host-sidecar-bridge --native-provider network
```

The diagnostic command reports actual native observations and typed availability;
it does not publish, grant authority or assert execution. `--mock` remains an
explicit in-process fixture workflow. Default live provider selection comes
from the source enrollment; it never discovers topology from target paths.

Native observations use systemd Manager D-Bus, the local Docker Engine API,
Kubernetes core/v1, Linux rtnetlink through fixed `ip -json` commands, Jetson
device tree/package/sysfs/power interfaces, the digest-pinned CUDA reference
helper when enrolled, or a fixed NVIDIA NVML inventory query. GPU reference
inventory takes `COHESIX_GPU_EXECUTOR_CONFIG`; the corresponding credential and
helper contract remains required. A Mac controller does not probe NVIDIA.

Linux interface, address, IPv4 route tables and nested counter/address tables
also use declared columns. Route entries contain groups of at most eight rows
and 768 serialized value bytes, in native order. Unknown native counter/address column names are retained
within the bounded table; missing cells are `null`. No row is truncated to
make a snapshot fit.

macOS network collection uses the tracked Swift source
`scripts/providers/network_macos.swift`, compiled to an absolute executable path
selected by `COHESIX_MACOS_NETWORK_HELPER`. Interface and address records are
column arrays with their exact column names in `fields/interfaces` and
`fields/addresses`. Every selected row is retained; `null` denotes a field the
native API did not expose. This representation avoids repeating long counter
names without dropping interfaces or counters. Native Darwin counters remain
32-bit wrapping snapshots. launchd observations require an explicit JSON array
of system service labels in `COHESIX_LAUNCHD_LABELS`; only selected public state
fields leave `launchctl print`, and environment values are never published.
Unsupported or unenrolled combinations remain typed unavailable.

Python's `SnapshotReader(backend, resolved_manifest)` requires an independently
enrolled target manifest and reads the source-scoped snapshot followed by its
receiver status. Source/epoch/bounds, sequence, digest and receiver availability
must agree. Expiry or replacement during the read is refused. `HostObservation`
contains `available_at_read` and immutable entry tuples; retaining that Python
object never turns it into an ongoing health check or execution receipt.
