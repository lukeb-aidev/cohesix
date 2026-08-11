<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document the protocol-faithful host-ticket v1/v2 execution and recovery contract. -->
<!-- Author: Lukas Bower -->

# Host ticket agent

`host-ticket-agent` executes generated host actions through the existing Cohesix
console/Secure9P namespace. It does not call a Worker directly and does not add
another authority path.

Version 1 remains the compatibility path for `receipt_mode=none`, including its
existing federation behavior. Version 2 is local-only and is accepted only for
the generated GPU lease grant/renew/release and PEFT export/import/activate/
rollback actions. Caller-authored version-2 lines on `/host/tickets/spec` are
validated but never executed. Root must resolve the request and append the
executable normalized record to `/host/tickets/spec.snapshot`.

## Root admission contract

Every version-2 request supplies `operation_id`, `subject_ref`,
`receipt_worker_role`, `receipt_worker_id`,
`receipt_supervisor_generation`, and `receipt_cap_generation`. The root-owned
snapshot echoes those values and adds `resolved_worker_slot`,
`resolved_lease_epoch`, and a strictly increasing `admission_sequence`. Root
must reject stale, cross-role, non-READY, or otherwise unpinned bindings before
publishing the snapshot. The strict canonical transport bounds are 1,422 bytes
for a maximal caller-authored request and 1,537 bytes for its maximal admitted
snapshot; both remain within the generated 2,048-byte ticket line bound.

Before provider dispatch, the host agent independently observes the exact READY
identity on the canonical `/shard/<label>/worker/<id>/telemetry` projection. It
rejects federation fields, a free-form target, unknown or incompatible action
arguments, traversal, symlinks, and GPU identifiers absent from root's `/gpu`
inventory.

Version-2 results echo the complete admitted binding. Their `result_digest` is
lowercase SHA-256 over compact JSON in `HostTicketResult` field order with only
`result_digest` omitted. The current maximal valid encoded result is 1,467
bytes; the representative conformance vector is 506 bytes. The console accepts
a 2,048-byte ECHO payload within a 2,304-byte command line. Version-1 results
retain the historical 224-byte compaction bound.

Root maps only terminal version-2 results through the existing Worker receipt
namespace: `succeeded` to `confirmed`, and both `failed` and `expired` to
`rejected`. Intermediate `queued`, `claimed`, and `running` records are audit
state, not Worker receipts. An otherwise valid terminal result is `stale` only
when its pinned Worker identity has changed or been torn down; root records that
fact without sending the old receipt to a replacement generation.

Long version-2 JSON lines returned by `CAT` use only the existing console
stream. Each wire line is at most 256 bytes and has the exact form
`C1:<seq4hex>:<count4hex>:<full_sha256>:<utf8_payload>`. Sequence and count are
four lowercase hexadecimal digits, sequence starts at zero and is contiguous,
count is at most 64, and the full lowercase SHA-256 of the reconstructed JSON
appears in every chunk. `cohsh` rejects missing, reordered, replayed,
mixed-digest, oversized, or noncanonical chunks and returns the reconstructed
canonical JSON line to its caller. Ordinary short CAT lines are unchanged.

## Execution durability

One process owns the local agent fence. Each receipt-bearing action advances a
synced, atomically renamed journal through `prepared`, `executing`,
`provider-result-persisted`, `result-published`, and `terminal` before the
cursor advances. Provider work is never blindly repeated from `executing`.
Action-specific observers reconcile the exact operation; an unexpired ambiguous
outcome remains executing. An expired ambiguous outcome terminates the local
journal as `stale` and publishes the schema state `expired`, which root maps to
the explicit rejected receipt; the provider call is not repeated. The exact
result path and JSON bytes are persisted before VM publication so a write that
committed but returned an error is reconciled without another provider call.

GPU version-2 actions write only `/queen/lease/ctl`; grant, renew, release, and
preemption never spawn or kill the receipt Worker. PEFT version-2 paths come
only from the configured registry, export, and adapter roots. The agent locks
those roots, rejects symlink/traversal paths, validates regular files and
available hashes, and uses the bounded `coh::peft` helpers for export, import,
activation, and rollback. Atomic local files use unique create-new temporary
files, file sync, rename, and parent-directory sync.
