<!-- Author: Lukas Bower -->
# Secure9P Policy & Implementation Guide

## 1. Scope
Secure9P provides the 9P2000.L codec, core request dispatcher, and transport adapters used by NineDoor. It must remain usable in `no_std + alloc` environments and cannot depend on POSIX APIs.

## 2. Layering
```
secure9p-codec      // Frame encode/decode, length guards, no_std
secure9p-core       // Session + fid tables, AccessPolicy hooks
secure9p-transport  // Optional adapters: InProc ring, Sel4Endpoint, (host-only) Tcp
nine-door           // Filesystem providers, role enforcement, logging
```
- `secure9p-transport::Tcp` is host-only and never packaged into the VM image.

## 3. Mandatory Defences
- Bound `msize` ≤ 8192 and reject frames exceeding negotiated size.
- Validate UTF-8 strings, forbid NUL bytes, and cap walk depth at 8 path components.
- Disallow `..` traversal and empty path elements.
- Prevent fid reuse after `clunk`; double clunk returns `Rerror(Closed)`.
- Deny writes to read-only nodes and enforce append-only semantics by ignoring offsets.

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

## 8. Future Enhancements
- Opportunistic support for 9P lock extensions once namespace bind/mount stabilises.
- Optional TLS termination in host tools prior to entering the VM transport adapter.
