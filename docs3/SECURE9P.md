# SECURE9P — Policy & Integration (v0–v1)

**Date:** 2025-10-10

## Position
- Reuse **codec + core state machine** if it's clean; quarantine POSIX shims and heavy async.
- Implement **9P2000.L** (dot-L) only. Plain 9P2000 is insufficient for locking semantics.
- Security is **capability tickets + per-session FIDs + namespace isolation**, not "TLS everywhere" inside the VM.

## Requirements (MUST/SHOULD)
- **MUST** bound `msize` (≤ 8192 in v0); hard reject oversize frames.
- **MUST** validate lifetimes: no reuse of clunked FIDs; close on error paths.
- **MUST** deny `..` escapes; bounded walk depth (≤ 8); reject empty/zero-length names.
- **MUST** be `no_std + alloc` capable behind a feature flag; no OS I/O in core.
- **MUST** have corpus tests for malformed headers, length mismatches, and truncated frames.
- **SHOULD** fuzz the decoder; zero panics on untrusted input.
- **SHOULD** keep transports abstract (`read_exact`/`write_all`) with no `std::net` assumption.
- **SHOULD** keep error model small and deterministic: `NotFound`, `Permission`, `Busy`, `Invalid`, `TooBig`, `Closed`

## Layering
```
secure9p-codec     // no_std + alloc: types, encode/decode, msize enforcement
secure9p-core      // fid table, session, request routing, role/ticket hooks
secure9p-transport // adapters: InProc ring, Sel4Endpoint, (host-only) Tcp
nine-door          // server that mounts synthetic providers and enforces roles
```
- `secure9p-transport::Tcp` is **host-side only** (dev tools). Never in the TCB.

## Capability Hooks
Core must delegate authN/authZ to the OS layer:
```rust
pub trait AccessPolicy {
  fn can_attach(&self, ticket: &Ticket, role: Role) -> bool;
  fn can_open(&self, role: Role, path: &str, mode: OpenMode) -> bool;
}
```
`nine-door` provides an `AccessPolicy` that enforces role-scoped mounts.

## Wire Compat
- Implement **2000.L** fields (`iounit`, lock ops reserved for future use). No union mounts exposed yet.
- Strings are UTF-8; reject nul bytes; limit path elems to 255 bytes.

## Testing Matrix
- **Round-trip**: version/attach/walk/open/read/write/clunk (happy path).
- **Negative**: oversize frame; invalid fid reuse; walk `..` past root; write to RO file; clunk twice.
- **Fuzz**: length-prefix mutations; field truncation; random tail bytes.

## TLS / Auth
- **Inside VM**: no TLS. Sessions are intra-VM endpoints or shared rings.
- **Host tools**: if remote, terminate TLS outside VM; map to `secure9p-transport::Tcp` at the edge.
- Identity is **tickets**, not Unix uids/gids.

## Salvage Guidance
- If existing Secure9P is entwined with POSIX emulation or Tokio/tower: split it per the layering above and use only codec/core for the VM.
- Keep the TCP server as a **dev/host tool** crate, not part of `nine-door`.
