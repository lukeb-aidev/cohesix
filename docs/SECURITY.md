<!-- Author: Lukas Bower -->
# Cohesix Security Addendum — Networking & Console

## 1. Deterministic Memory Envelope
- `root-task::net::NetStack` provisions bounded `heapless::spsc::Queue` buffers sized for 16 frames × 1536 bytes on both RX and
  TX paths (≈49 KiB total). The queues are allocated once at boot via `Box::leak` to avoid dynamic growth and are shared across
  smoltcp, the virtio descriptor validator, and diagnostics handles.
- A monotonic `NetworkClock` backed by `portable_atomic::AtomicU64` bounds timestamp arithmetic while avoiding wrap for the
  lifetime of the VM. Pollers advance the clock using explicit millisecond timestamps supplied by the event pump so the heapless
  queues never rely on wall-clock drift.
- smoltcp is compiled without default features; only the IPv4/TCP stack is enabled. Random seeds and MAC addresses are
  deterministic to ensure reproducible boots inside QEMU.
- Console buffers (`heapless::String`) cap line length at 128 bytes and reject control characters beyond backspace/delete to
  prevent uncontrolled allocations. The serial façade uses `heapless::spsc::Queue` staging buffers sized at 256 bytes for RX and
  TX, and exposes atomic back-pressure counters so `/proc/boot` can surface saturation data without dynamic allocation.
- The virtio-console driver mirrors device descriptor rings with bounded `heapless::spsc::Queue` structures (mirroring the RX/TX
  staging buffers) so host tests can exercise the driver without MMIO. Pending TCP console lines are staged in a
  `heapless::Deque` (depth 8) before the event pump forwards them into the parser, providing a deterministic envelope for
  remote operator traffic.
- Networking telemetry (`link_up`, `tx_drops`, `last_poll_ms`) is captured in a copyable struct so audit sinks can log
  descriptor pressure without touching heap allocations. This telemetry is emitted whenever the event pump observes network
  activity.

## 2. Console Hardening
- A leaky-bucket rate limiter permits two consecutive authentication failures per 60-second window; the third failure triggers a
  90-second cooldown and surfaces `RateLimited` to both serial and TCP clients. The event pump layers an exponential back-off
  (250 ms × 2ⁿ) on top of the leaky bucket so automated brute force attempts stall progressively sooner.
- All verbs (`help`, `attach`, `tail`, `log`, `spawn`, `kill`, `quit`) are parsed through a shared finite-state machine to ensure
  consistent validation across serial and TCP inputs. Unknown verbs and overlong values emit structured log lines and are
  ignored. The serial façade sanitises UTF-8 input before handing bytes to the parser, dropping control characters outside the
  backspace/delete set.
- Tickets presented during `attach` are verified against a deterministic `TicketTable` seeded during boot. Audit lines are
  emitted for every denial and for each successful role assertion so operators can review access attempts in `/log/queen.log`.
- Host tooling mirrors these controls: `cohsh` validates worker tickets locally (64 hex or base64url) and emits connection
  telemetry (`[cohsh][tcp] reconnect attempt …`, heartbeat latency) to stderr so operators can correlate client-side failures
  with root-task audit trails.
- The TCP console mirrors the serial surface exactly. Line-oriented commands are terminated by `END` sentinels so scripts can
  verify log completion without relying on socket closure.

## 3. Event Pump & Threat Model Extensions
- User networking in QEMU is only enabled when `scripts/qemu-run.sh --tcp-port <port>` is provided, limiting the window in which
  the guest exposes a TCP listener. The helper script prints the forwarded port to encourage operator audit.
- TCP handshake commands are human-readable (`ATTACH <role> <ticket?>` / `TAIL <path>`) to ease inspection. The transport
  validates line length before passing payloads to root-task components, and unexpected responses result in immediate disconnects.
- Tickets are still required for worker roles even over TCP; empty ticket submissions for worker roles fail with a transport-level
  error before touching NineDoor state. Successful `attach` calls commit the session role into the event pump so subsequent verbs
  cannot escalate privileges without minting a fresh ticket.
- Port forwarding via `scripts/qemu-run.sh --tcp-port <port>` prints the forwarded endpoint and encourages operators to tunnel
  through localhost-only bindings. When the flag is omitted the listener remains inaccessible from the host, reducing the attack
  surface during bring-up.
- The event pump emits audit records (`event-pump: init <subsystem>`, `net: poll link_up=<bool> tx_drops=<count>`, `attach
  accepted`, `attach denied`) that flow to the serial log. These records are critical for forensic review because they show which
  subsystems were live at the time of an intrusion and whether the networking queues are under pressure.
