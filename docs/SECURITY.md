<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Summarize Cohesix security posture and audit expectations. -->
<!-- Author: Lukas Bower -->
# Cohesix Security Addendum — Networking & Console

The threat model applies to Cohesix running on ARM64 hardware booted via UEFI; QEMU `aarch64/virt` serves as the development/CI harness and is expected to mirror the same attack surface rather than being the deployment end-state.

## 1. Deterministic Memory Envelope
- `root-task::net::NetStack` binds smoltcp to HAL-provided NICs (RTL8139 by default on `dev-virt`, virtio-net feature-gated). DMA
  frames are allocated once via `KernelHal::alloc_dma_frame` and device mappings flow through HAL coverage checks so drivers never
  bypass allocator accounting.
- A monotonic `NetworkClock` backed by `portable_atomic::AtomicU64` bounds timestamp arithmetic while avoiding wrap for the
  lifetime of the Cohesix instance. Pollers advance the clock using explicit millisecond timestamps supplied by the event pump so the heapless
  queues never rely on wall-clock drift.
- smoltcp is compiled without default features; only the IPv4/TCP stack is enabled. Random seeds and MAC addresses are
  deterministic to ensure reproducible boots inside QEMU and when mirrored on hardware.
- Console buffers (`heapless::String`) cap line length at 256 bytes and reject control characters beyond backspace/delete to
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
- Host sidecar controls (`/host/*`) are append-only and queen-only; every write attempt (allowed or denied) emits a deterministic
  audit line that records the ticket and path, ensuring sensitive host actions leave an immutable trace in `/log/queen.log`.
- Host tooling mirrors these controls: `cohsh` validates worker tickets locally (64 hex or base64url) and emits connection
  telemetry (`[cohsh][tcp] reconnect attempt …`, heartbeat latency) to stderr so operators can correlate client-side failures
  with root-task audit trails.
- The TCP console mirrors the serial surface exactly. Line-oriented commands are terminated by `END` sentinels so scripts can
  verify log completion without relying on socket closure.

### Sidecar Isolation & Spooling
- Sidecar mounts are manifest-gated; adapters that are not declared are unreachable, and mount labels are hash-prefixed on collision.
- Capability scopes are enforced per adapter; unauthorized access yields deterministic `ERR` responses and appends `sidecar-deny` to `/log/queen.log`.
- Offline spooling is bounded by manifest limits; replay drains the spool deterministically and never exceeds `secure9p.msize`.
- LoRa duty-cycle enforcement rejects oversized or over-budget payloads and records bounded tamper entries for audit review.
- Sidecars never add in-VM TCP listeners; host-side sidecars communicate over the existing Secure9P/console boundary.

## 3. Event Pump & Threat Model Extensions
- User networking in QEMU is only enabled when `scripts/qemu-run.sh --tcp-port <port>` is provided, limiting the window in which
  the guest exposes a TCP listener. The helper script prints the forwarded port to encourage operator audit.
- TCP handshake commands are human-readable (`ATTACH <role> <ticket?>` / `TAIL <path>`) to ease inspection. The transport
  validates line length before passing payloads to root-task components; invalid-length frames on authenticated sessions yield
  `ERR FRAME reason=invalid-length` and are dropped, while pre-auth violations still terminate the connection.
- Tickets are still required for worker roles even over TCP; empty ticket submissions for worker roles fail with a transport-level
  error before touching NineDoor state. Successful `attach` calls commit the session role into the event pump so subsequent verbs
  cannot escalate privileges without minting a fresh ticket.
- Port forwarding via `scripts/qemu-run.sh --tcp-port <port>` prints the forwarded endpoint and encourages operators to tunnel
  through localhost-only bindings. When the flag is omitted the listener remains inaccessible from the host, reducing the attack
  surface during bring-up.
- All NIC backends remain HAL-bound; smoltcp plus the authenticated TCP console are the only in-VM network entry points regardless
  of whether RTL8139 (default) or virtio-net (feature-gated) is selected.
- The event pump emits audit records (`event-pump: init <subsystem>`, `net: poll link_up=<bool> tx_drops=<count>`, `attach
  accepted`, `attach denied`) that flow to `/log/queen.log` after the console handoff (boot-critical lines still appear on the
  serial log before the root shell starts). These records are critical for forensic review because they show which subsystems
  were live at the time of an intrusion and whether the networking queues are under pressure.
- The only control-plane interfaces are `cohsh` over serial/TCP and the Secure9P namespaces; any host-side WASM GUI is treated as an unprivileged client layered on top of these paths and does not expand the in-VM attack surface. One Queen orchestrating many workers keeps logging and audit scoped per hive (append-only `/log/*.log`).

## 4. CAS Threat Model
- CAS updates are file-backed only and writeable solely by the queen role; no additional network services or RPCs are introduced.
- Chunk integrity is enforced via SHA-256; mismatches are quarantined with deterministic audit lines and no side effects.
- Signature enforcement is manifest-gated; unsigned mode requires explicit opt-in and is documented alongside the manifest.
- Delta manifests must reference a non-delta base epoch and are validated against base payload hashes.

<!-- coh-rtc:cas-security:start -->
### CAS integrity stance (generated)
- `cas.signing.required`: `true`
- Hash mismatches are rejected, quarantined, and audited without side effects.
- Signature failures emit deterministic ERR plus audit entries.
- `/models` exposure remains gated by `ecosystem.models.enable`.

_Generated by coh-rtc (sha256: `674f8c3ed5412b48f6d8e4804d75735aa6b40237b15fa0be463f06e777132101`)._
<!-- coh-rtc:cas-security:end -->

## 5. Observability Tolerances (Generated)
<!-- coh-rtc:observability-security:start -->
### Observability tolerances (generated)
- `observability.proc_ingest.latency_samples`: `32`
- `observability.proc_ingest.latency_tolerance_ms`: `5`
- `observability.proc_ingest.counter_tolerance`: `1`
- `observability.proc_ingest.watch_min_interval_ms`: `50`

_Generated by coh-rtc (sha256: `aae20e12321a8a009e32d6e163c28d7ab51ca76a211a6ef0f1dd753f88b1c6ce`)._
<!-- coh-rtc:observability-security:end -->

## 6. cohsh Pooling & Retry Policy (Generated)
- `cohsh` preserves ACK/ERR ordering for strictly ordered flows (`attach`, `log`, `tail`, `quit`). Pooled sessions are reserved for concurrency benchmarks and telemetry batch writes, and they drain acknowledgements before returning leases to avoid cross-command reordering.
- Retry scheduling is bounded and manifest-driven; injected short-write retries re-authenticate and re-attach before resending, preventing duplicate telemetry writes in pooled workflows.

### cohsh client policy (generated)
- `manifest.sha256`: `fb3a4bc5434eaf31cc7ff4b1c2fcf33103f480a3ba30a60e3dc12bb5552a2861`
- `policy.sha256`: `3e6bfee24c10636655135e0036addc355f4ccab5843d1f28eb328c7efd50f256`
- `cohsh.pool.control_sessions`: `2`
- `cohsh.pool.telemetry_sessions`: `4`
- `retry.max_attempts`: `3`
- `retry.backoff_ms`: `200`
- `retry.ceiling_ms`: `2000`
- `retry.timeout_ms`: `5000`
- `heartbeat.interval_ms`: `15000`

_Generated from `configs/root_task.toml` (sha256: `fb3a4bc5434eaf31cc7ff4b1c2fcf33103f480a3ba30a60e3dc12bb5552a2861`)._

## 7. Telemetry Ring Latency Metrics (Generated)
<!-- metrics:latency:start -->
### Telemetry Ring Latency (generated)
- Suite: `nine-door/telemetry_ring`
- Samples: `7`
- P50: `0.014 ms`
- P95: `0.025 ms`
- Unit: `ms`
_Generated from `apps/nine-door/out/metrics/telemetry_ring_latency.json`._
<!-- metrics:latency:end -->

## Appendix A: Policy approval replay limits (manifest snapshot)
- Policy approvals are single-use: once consumed by a gated write, replaying the same approval yields `ERR EPERM` and emits a `policy-gate` audit line in `/log/queen.log`.
- Approval queue bounds are manifest-driven (`configs/root_task.toml`):
  - `ecosystem.policy.queue_max_entries = 32`
  - `ecosystem.policy.queue_max_bytes = 4096`
  - `ecosystem.policy.ctl_max_bytes = 2048`
  - `ecosystem.policy.status_max_bytes = 512`
- Gated control rules (manifest snapshot):
  - `queen-ctl` → `/queen/ctl`
  - `systemd-restart` → `/host/systemd/*/restart`

## Appendix B: AuditFS & ReplayFS bounds (manifest snapshot)
- Audit/replay surfaces are manifest-gated; when disabled the `/audit` and `/replay` trees are absent and replay attempts return deterministic `ERR` without side effects.
- Replay only applies Cohesix-issued control-plane actions recorded in `/audit/journal` and never attempts to reconstruct external host state.
- Replay cursor checks are bounded by the retained window and `replay_max_entries`; over-window requests update `/replay/status` to `err` and emit deterministic errors.
- Audit/replay bounds are manifest-driven (`configs/root_task.toml`):
  - `ecosystem.audit.enable = false`
  - `ecosystem.audit.journal_max_bytes = 8192`
  - `ecosystem.audit.decisions_max_bytes = 4096`
  - `ecosystem.audit.replay_enable = false`
  - `ecosystem.audit.replay_max_entries = 64`
  - `ecosystem.audit.replay_ctl_max_bytes = 1024`
  - `ecosystem.audit.replay_status_max_bytes = 1024`
