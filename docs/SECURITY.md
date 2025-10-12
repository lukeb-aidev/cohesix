<!-- Author: Lukas Bower -->
# Cohesix Security Addendum — Networking & Console

## 1. Deterministic Memory Envelope
- `root-task::net::NetStack` provisions bounded `heapless::spsc::Queue` buffers sized for 16 frames × 1536 bytes on both RX and
  TX paths (≈49 KiB total). The queues are allocated once at boot via `Box::leak` to avoid dynamic growth and are shared across
  smoltcp and diagnostics handles.
- smoltcp is compiled without default features; only the IPv4/TCP stack is enabled. Random seeds and MAC addresses are
  deterministic to ensure reproducible boots inside QEMU.
- Console buffers (`heapless::String`) cap line length at 128 bytes and reject control characters beyond backspace/delete to
  prevent uncontrolled allocations.

## 2. Console Hardening
- A leaky-bucket rate limiter permits two consecutive authentication failures per 60-second window; the third failure triggers a
  90-second cooldown and surfaces `RateLimited` to both serial and TCP clients.
- All verbs (`help`, `attach`, `tail`, `log`, `spawn`, `kill`, `quit`) are parsed through a shared finite-state machine to ensure
  consistent validation across serial and TCP inputs. Unknown verbs and overlong values emit structured log lines and are
  ignored.
- The TCP console mirrors the serial surface exactly. Line-oriented commands are terminated by `END` sentinels so scripts can
  verify log completion without relying on socket closure.

## 3. Threat Model Extensions
- User networking in QEMU is only enabled when `scripts/qemu-run.sh --tcp-port <port>` is provided, limiting the window in which
  the guest exposes a TCP listener. The helper script prints the forwarded port to encourage operator audit.
- TCP handshake commands are human-readable (`ATTACH <role> <ticket?>` / `TAIL <path>`) to ease inspection. The transport
  validates line length before passing payloads to root-task components, and unexpected responses result in immediate disconnects.
- Tickets are still required for worker roles even over TCP; empty ticket submissions for worker roles fail with a transport-level
  error before touching NineDoor state.
