<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define the as-built Secure9P wire subset, session invariants, and implementation boundaries. -->
<!-- Author: Lukas Bower -->

# Secure9P Protocol Contract

Secure9P is Cohesix's bounded, ticket-aware 9P protocol for host NineDoor
sessions. It is not the target TCP console protocol and it is not the physical
driver-task ABI. Those interfaces share selected limits and policy concepts but
use different framing and dispatch paths.

This document owns Secure9P wire and session invariants. Namespace paths belong
in [INTERFACES.md](INTERFACES.md), role views in
[ROLES_AND_SCHEDULING.md](ROLES_AND_SCHEDULING.md), and the system boundary in
[ARCHITECTURE.md](ARCHITECTURE.md).

## As-built boundary

The implementation is split deliberately:

| Layer | As-built responsibility |
| --- | --- |
| [`secure9p-codec`](../crates/secure9p-codec) | `no_std + alloc` wire types, four-byte 9P length framing, request/response encoding, decoding, batch iteration, and codec fuzz entry points. |
| [`secure9p-core`](../crates/secure9p-core) | `no_std + alloc` tag windows, queue limits, active/retired fid tables, append-only offset helpers, short-write policy, and an access-policy trait. It is not a complete server dispatcher. |
| [`secure9p-transport`](../crates/secure9p-transport) | A `no_std` crate boundary only. It does not currently implement in-process, seL4-endpoint, or TCP adapters. |
| [Host NineDoor](../apps/nine-door/src/host) | The complete host-side session state machine, ticket verification, role policy, provider dispatch, batching, metrics, and error mapping. |
| [Target `NineDoorBridge`](../apps/root-task/src/ninedoor.rs) | A separate `no_std` namespace/control adapter reached through the console event pump. It does not decode Secure9P frames. |

Host NineDoor and target `NineDoorBridge` are required to preserve overlapping
namespace semantics, but they do not share runtime state and are not connected
by an implicit transport. There is no 9P-over-TCP listener inside the target.

## Wire subset

Secure9P negotiates the version string `9P2000.L` and implements this bounded
request subset:

| Request | Response | Purpose |
| --- | --- | --- |
| `Tversion` | `Rversion` | Negotiate `msize` and require `9P2000.L`. |
| `Tattach` | `Rattach` | Select the role/identity view and validate the ticket carried in `aname`. |
| `Twalk` | `Rwalk` | Resolve bounded path components into a new fid. |
| `Topen` | `Ropen` | Open a resolved fid with validated mode. |
| `Tread` | `Rread` | Read a bounded byte range. |
| `Twrite` | `Rwrite` | Append or write according to the provider contract. |
| `Tclunk` | `Rclunk` | Retire a fid for the remainder of the session. |

`stat`, `create`, and `remove` are not accepted wire requests in the current
codec. Documentation and clients must not present an unsupported operation as
implemented or describe `remove` as an accepted no-op.

Each wire frame begins with a four-byte little-endian total length, including
the length field. Responses echo the request tag. `Rerror` carries one of the
bounded Cohesix error codes described below.

## Negotiated and fixed bounds

The selected profile manifest owns the generated session limits. The checked-in
default currently generates `msize = 8192`, walk depth `8`, tag window `16`,
batch size `1`, and short-write policy `reject`. Those values are a profile
snapshot, not constants to copy into consumers; see
[the generated manifest snippet](snippets/root_task_manifest.md).

The codec also enforces fixed path-component rules:

- at most eight components per walk;
- each component is valid UTF-8 and between 1 and 64 bytes;
- `..`, `/`, and NUL are rejected inside a component; and
- empty components are rejected.

The 64-byte component limit comes from the current codec implementation. It is
distinct from ticket scope-path and mount-field limits, which govern different
inputs.

## Session state and validation order

Host NineDoor processes a normal session in this order:

1. Decode a complete length-bounded frame.
2. Negotiate `9P2000.L` and an `msize` no larger than the implementation cap.
3. Require `Tattach` before namespace operations.
4. Parse the requested role and identity.
5. Verify a supplied ticket MAC, role, subject, TTL, scopes, quotas, and
   manifest limits. Non-Queen roles require a ticket and identity; Queen may
   attach without a ticket.
6. Validate path and open mode before provider execution.
7. Apply lifecycle, operation-budget, and provider-specific checks.
8. Execute the provider operation and encode a deterministic response.

No provider should receive an unbounded walk. Role and ticket policy is
implemented in host NineDoor's server layer; the similarly named
`secure9p_core::AccessPolicy` trait is a reusable primitive, not evidence that
the host server delegates through a trait implementation.

## Fid, tag, and queue invariants

- Fids are scoped to one session.
- A clunked fid is retired and cannot be reused in that session.
- Reusing an active fid or a retired fid is rejected deterministically.
- An in-flight tag cannot be reserved twice.
- The tag window and queue depth are bounded by generated session limits.
- Queue saturation produces `Busy`; duplicate in-flight tags produce
  `Invalid`.
- Closing or revoking a session prevents further operations through that
  session.

These rules are implemented by
[`secure9p-core/src/session.rs`](../crates/secure9p-core/src/session.rs) and
applied by host NineDoor's
[`ServerCore`](../apps/nine-door/src/host/core.rs).

## Reads, writes, and append-only files

Provider mode is authoritative. A read-only node rejects writes. Append-only
nodes require the provider's expected append position; a stale or nonmatching
offset is an error rather than an instruction to overwrite retained data.
Bounded short reads may indicate the current end of a retained window.

Short-write behavior applies to transport writers, not provider permission:

- `reject` fails immediately on a short write and is the current default;
- `retry` permits at most three retries with an exponential delay based on a
  five-millisecond base.

Control files additionally validate their own JSONL, token, record-size, and
lifecycle contracts. Those schemas are indexed in
[INTERFACES.md](INTERFACES.md).

## Batching and ordering

A batch is a concatenation of complete Secure9P frames. Host NineDoor first
decodes the batch, reserves each allowed tag/queue slot, dispatches accepted
requests sequentially, and encodes responses in request order. Clients must
still use response tags as the protocol identity; they must not infer that
future implementations will preserve FIFO delivery beyond the documented
server behavior.

The following failures do not partially relax bounds:

- total batch bytes above negotiated `msize` produce `TooBig` responses;
- a frame above negotiated `msize` produces `TooBig`;
- frame count above `batch_frames` produces `Busy`; and
- a full tag or queue window refuses the affected request.

With the checked-in default `batch_frames = 1`, multi-frame batches are not
accepted.

## Error surface

The wire codec exposes exactly these error codes:

| Code | Meaning |
| --- | --- |
| `Permission` | The role, ticket, mode, lifecycle gate, or provider policy denies the request. |
| `NotFound` | The requested fid target or namespace node does not exist. |
| `Busy` | A bounded queue, tag window, or provider cannot accept work now. |
| `Invalid` | The request, state transition, offset, tag, or payload is invalid. |
| `TooBig` | A frame, batch, or bounded payload exceeds its limit. |
| `Closed` | A fid or session has been retired, clunked, or closed. |

Console `ERR` acknowledgements are a separate textual error surface. They may
carry analogous reasons, but there is no 1:1 wire-code translation contract.

## Observability

Host NineDoor records session, queue, backpressure, short-write, ingest, and
policy state in its provider layer. Generated `/proc/9p/*` and
`/proc/ingest/*` nodes appear only when the selected manifest enables them.
Their exact formats are generated in
[observability_interfaces.md](snippets/observability_interfaces.md).

Secure9P does not imply DMA-backed namespace storage. Device DMA and cache
maintenance are physical-driver concerns governed by [DRIVERS.md](DRIVERS.md).

## Security requirements

- Reject malformed lengths before allocating or dispatching provider work.
- Enforce negotiated `msize` on every request and response path.
- Validate all walk components before namespace lookup. Disallow `..`
  traversal; no path component may equal `..`.
- Prevent fid reuse after `clunk`; fid retirement lasts for the session.
- Verify tickets with the role's configured key before granting a role view.
- Role-to-namespace rules are enforced before provider execution and never
  inferred from a path name alone.
- Keep all queues, retained files, and provider payloads bounded.
- Preserve append-only and read-only modes; never convert a denial to a
  successful no-op.
- Keep host `std` transport facilities out of target closure profiles.
- Do not add an in-target Secure9P listener; the authenticated console remains
  the only permitted target TCP listener.

## Verification

Relevant implementation and regression surfaces include:

- [`secure9p-codec` tests](../crates/secure9p-codec)
- [`secure9p-core` tests](../crates/secure9p-core)
- [NineDoor integration tests](../apps/nine-door/tests)
- [root-task namespace bridge tests](../apps/root-task/src/ninedoor.rs)
- [the staged Test Plan](TEST_PLAN.md)

Changes to operations, errors, framing, namespace semantics, or generated
bounds require the breaking-change process in `AGENTS.md`, corresponding
fixtures, regenerated artifacts, and documentation updates in the same change.
