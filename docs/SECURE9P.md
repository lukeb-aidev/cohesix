<!-- Author: Lukas Bower -->
# Secure9P Policy & Implementation Guide

## 1. Scope
Secure9P provides the 9P2000.L codec, core request dispatcher, and transport adapters used by NineDoor. It must remain usable in `no_std + alloc` environments and cannot depend on POSIX APIs.
It is the sole control-plane IPC surface; the TCP console path reuses the same NineDoor framing with a minimal 9P-style `attach`/auth handshake (role, optional ticket, idle/auth timeouts, reconnect-friendly) layered alongside the always-on PL011 root console rather than replacing it.
Secure9P sessions present the per-hive and per-role view into the namespace so queen and worker mounts expose different slices of the hive.

## 2. Layering
### Crate Structure

Secure9P is implemented across two core crates:

- **secure9p-codec** — provides the canonical wire encoding/decoding of 9P messages,
  size-prefixed framing, batch iterators, and fuzz-harness utilities.  
  This crate is no_std + alloc by default, with an optional std feature for fuzzing and harness utilities.

- **secure9p-core** — provides the protocol engine: session state machine,
  fid table management, tag window enforcement, attach semantics, and request pipelining.
  This crate is `no_std + alloc` compatible and transport-agnostic.

These replace the former `secure9p-wire` crate.  
All prior wire types and frame rules now live in `secure9p-codec`; all protocol
logic and state tracking now lives in `secure9p-core`.

```
secure9p-codec      // Frame encode/decode, length guards, no_std + alloc (std feature for fuzzing)
secure9p-core       // Session + fid tables, AccessPolicy hooks
secure9p-transport  // Optional adapters: InProc ring, Sel4Endpoint, (host-only) Tcp
nine-door           // Filesystem providers, role enforcement, logging
```
- `secure9p-transport::Tcp` is host-only and never packaged into the Cohesix instance image (including the QEMU development CPIO).
- The TCP console attaches through this stack with the same role selection semantics and remains bound to a single namespace per session; PL011 continues to service the root console in parallel. The TCP console reuses Secure9P’s framing rules but does not use secure9p-transport::Tcp; it runs entirely inside root-task via smoltcp and the event pump.

## 3. Mandatory Defences
- Bound `msize` ≤ 8192 and reject frames exceeding negotiated size.
- Validate UTF-8 strings, forbid NUL bytes, and cap walk depth at 8 path components.
- Disallow `..` traversal and empty path elements.
- Prevent fid reuse after `clunk`; double clunk returns `Rerror(Closed)`.
- Deny writes to read-only nodes and enforce append-only semantics by ignoring offsets.
- Attack surface constraints: fixed `msize` (≤ 8192) with no wildcard traversal; heap allocations are bounded by negotiated message sizes with no dynamic growth in the validator/dispatcher; walks validate every component (length, UTF-8, no `/` or `..`) and cap depth at 8; codec paths are deterministic and bounded; root-task event pump keeps dispatch non-blocking.

## 4. Access Policy Hooks
```rust
pub trait AccessPolicy {
    fn can_attach(&self, ticket: &TicketClaims) -> Result<(), AccessError>;
    fn can_open(&self, ticket: &TicketClaims, path: &str, mode: OpenMode) -> Result<(), AccessError>;
    fn can_create(&self, ticket: &TicketClaims, path: &str) -> Result<(), AccessError>;
}
```
- NineDoor implements the trait using role-aware mount tables.
- Policies run before provider logic executes.
- Role-to-namespace rules follow `docs/ROLES_AND_SCHEDULING.md` (queen = full tree, worker-heartbeat = `/proc/boot`, `/worker/self/telemetry`, `/log/queen.log` RO, worker-gpu future `/gpu/<lease>`), and capabilities are session-scoped tickets negotiated during `attach` (single attach per `cohsh` session with optional ticket injection before remaining bound to the resulting mounts).
- The AccessPolicy for queen versus worker roles enables the Queen’s ability to orchestrate many workers by controlling access to mount points and control files such as `/queen/ctl`, `/worker/<id>/telemetry`, and `/gpu/*`.
- AccessPolicy evaluation occurs after path validation and normalisation by secure9p-core; providers never receive unvalidated or unbounded paths.

## 5. Testing Matrix
| Suite | Coverage |
|-------|----------|
| Unit | Frame encode/decode round-trips, fid lifecycle, error mapping |
| Integration | Attach/walk/open/read/write across queen/worker roles using in-memory transport |
| Negative | Oversized frames, invalid qid types, path traversal attempts, write to RO nodes |
| Fuzz | Length-prefix mutations, truncated frames, random tail bytes |

## 6. TCB Sanity
- Bootstrap uses invocation addressing (depth=0). Slots go in index; offset must be 0.
- Boot-time helpers assert `slot < 1 << init_cnode_bits` against the kernel-provided radix before mint/copy/retype, ensuring
  decode errors surface as Rust panics instead of kernel faults.

## 7. Logging & Observability
- Core emits debug hooks (`on_attach`, `on_clunk`, `on_error`) that NineDoor subscribes to for logging into `/log/queen.log`.
- Transport adapters must expose counters for frames sent/received and error counts for CI dashboards.
- Namespaces honour Secure9P invariants: `/queen/ctl` is append-only; `/log/*.log` entries are append-only files; `/proc` hosts `boot` plus per-worker trace files without write or traversal backdoors; `/worker/<id>` directories expose append-only telemetry for the matching worker; `/gpu/<id>/` nodes are published by the host bridge per `docs/GPU_NODES.md` and remain read/write only to authorised GPU roles. Walks never permit `..`, no implicit wildcards exist, and depth stays bounded by the codec guard.

## 8. Future Enhancements
- Opportunistic support for 9P lock extensions once namespace bind/mount stabilises.
- Optional TLS termination in host tools prior to entering the development VM transport adapter; the same boundary applies when the transport targets physical hardware.
- Status (Build Plan ≤7c): root and TCP consoles run concurrently; Secure9P namespaces and role-aware mounts are live; upcoming milestones will extend worker-side bind/mount, flesh out worker/GPU namespace detail, and wire GPU lease paths from host bridge into `/gpu/<id>`.
