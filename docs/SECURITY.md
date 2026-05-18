<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Summarize Cohesix security posture and audit expectations. -->
<!-- Author: Lukas Bower -->
# Cohesix Security Addendum — Networking & Console

The threat model applies to Cohesix running on ARM64 hardware booted via the Pi 4 U-Boot chain (`Pi firmware -> U-Boot -> seL4 image -> root-task`); QEMU `aarch64/virt` serves as the development/CI harness and mirrors the same control-plane attack surface where profile-gated behavior allows.

## 1. Deterministic Memory Envelope
- `root-task::net::NetStack` binds smoltcp to HAL-provided NICs (RTL8139/virtio on QEMU profiles and GENETv5 on Pi 4 profiles). DMA
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
  staging buffers) so host tests can exercise the driver without MMIO. Pending TCP console command lines are staged in a
  `heapless::Deque` (depth 32), while remote response traffic uses bounded priority/non-priority queues sized for Pi 4 Wi-Fi
  bursts (128 priority lines and 512 non-priority lines) before the event pump forwards them into the parser or TCP socket.
- Pi 4 network backends use the board memory budget while staying bounded: CYW43455 Wi-Fi keeps RX glom descriptors/backlog at
  64 entries with a 64-frame RX pump budget, and GENETv5 uses the hardware 256-descriptor RX/TX rings plus a 128-frame software
  RX ready queue. Both paths share the enlarged smoltcp console windows (16 KiB RX, 64 KiB TX).
- Networking telemetry (`link_up`, `tx_drops`, `last_poll_ms`) is captured in a copyable struct so audit sinks can log
  descriptor pressure without touching heap allocations. This telemetry is emitted whenever the event pump observes network
  activity.

## 1.1 SMP Isolation & Multithreading Rejection
- Cohesix uses seL4 SMP scheduling to run **separate tasks** in parallel; each task remains single-threaded.
- Authoritative state (tickets, policy, replay) is serialized through the root-task authority surface and never mutated by parallel workers.
- Shared-memory multithreading is rejected because it introduces nondeterministic interleavings that break replay guarantees and
  expand the audit surface for data races and timing side effects.
- Back-pressure is explicit: overloaded tasks return deterministic `ERR ... reason=busy` instead of hidden queues or background work.

## 1.2 Milestone 26/26a/26b Pi 4 Identity + Networking Gates
- `coh-rtc` preserves Milestone 26 no-NIC mode (`hw.no_nic=true`, `features.net_console=false`) and emits deterministic evidence (`manifest.hw.networking=disabled-m26-baseline`).
- Milestone 26a/26b network-enabled Pi 4 mode is profile-gated and compiler-enforced:
- `profile.name` must be `pi4-uboot-aarch64` (legacy alias `uefi-aarch64` accepted only for migration).
- `hw.network.enabled=true` requires `hw.network.backend=bcmgenet-v5`.
- `hw.network.mode` is bounded to `off|static|dhcp`, `hw.network.interface` is bounded to `wired|wifi|auto`, and DHCP retry/timeout fields are compiler-bounded.
- `hw.network.static_ipv4.ip` must be non-zero IPv4 with `prefix_len` in `1..=32`; gateway is optional but if set must be non-zero IPv4 when `mode=static`.
- `hw.devices` must declare a required `net` device before backend selection is accepted.
- Boot logs include deterministic network evidence lines (`manifest.hw.network.enabled`, `manifest.hw.network.backend`, `manifest.hw.network.mode`, `manifest.hw.network.interface`, `manifest.hw.networking=...`) for audited before/after proofs.
- The DHCP client path is bounded and client-only: DHCPv4 DISCOVER/OFFER/REQUEST/ACK, fixed buffers, bounded retry/timeouts, strict packet validation, and no new listeners or protocol surfaces.
- Pi 4 boot scripts may persist only Cohesix policy fields in `cohesix.env`, reload them on boot, mirror `coh_net_mode`, `coh_net_interface`, `coh_static_ip`, `coh_static_prefix_len`, `coh_static_gateway`, `coh_wifi_ssid`, and `coh_wifi_psk` into a staged padded DTB under `/chosen/cohesix,*`, and hand that DTB to the elfloader through the U-Boot `uImage`/`bootm` path. Root-task accepts only bounded values and falls back to manifest defaults when the DTB handoff is absent or invalid; the build-time manifest is never rewritten on the SD card.
- The runtime now routes Pi 4 `wifi` policy through the HAL-backed CYW43455 SDIO path. Explicit `wifi` accepts bounded `static` or `dhcp`; SSIDs are limited to 1-32 printable ASCII bytes, and PSKs are empty for open networks, 8-63 printable ASCII bytes, or exactly 64 ASCII hex digits. `auto` remains DHCP-only and keeps single-active-interface behavior by attempting Wi-Fi first only when bounded credentials are present, then falling back to wired only after an explicit CYW43455 attach/join setup failure before DHCP ownership transfers to the active Wi-Fi stack.
- Attestation policy is manifest-gated through `hw.attestation.*`:
- `tpm-only` requires a TPM declaration.
- `tpm-or-dice` and `dice-only` are encoded deterministically and bound to the manifest fingerprint.
- Root-task evaluates attestation before ticket registration. If attestation is enabled and policy guarantees are unsatisfied, boot aborts deterministically and emits audited reason codes.
- `/proc/boot` includes `attestation.bound_manifest_sha256` and `attestation.evidence_sha256` when attestation is enabled.
- Local diagnostics seat policy is manifest-gated through `hw.local_seat.*` and declared keyboard/display devices.
- `hw.local_seat.required=true` is fail-fast: missing declarations or unavailable backend aborts boot before ticket material is published.
- `hw.local_seat.required=false` degrades to serial-only diagnostics with explicit audited boot lines.
- GENETv5 implementation provenance is documented and constrained: Linux `bcmgenet` behavior -> Linux `bcm2711` DT bindings -> U-Boot `bcmgenet`; these references are design-only and code lift is prohibited.
- Planned CYW43xx implementation provenance remains fixed as well: OpenBSD `bwfm` -> Zephyr/Infineon WHD HAL layering -> Linux `brcmfmac` SDIO recovery/link edge cases. No source code lift is permitted.

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
- Console refusals are tagged with `reason=busy|quota|cut|policy` plus a bounded `detail=` token; the same categories drive
  `/proc/pressure/*` counters so operators can distinguish contention from policy enforcement without additional RPCs.
- Host sidecar controls (`/host/*`) are append-only and queen-only; every write attempt (allowed or denied) emits a deterministic
  audit line that records the ticket and path, ensuring sensitive host actions leave an immutable trace in `/log/queen.log`.
- Host tooling mirrors these controls: `cohsh` validates worker tickets locally (64 hex or base64url) and emits connection
  telemetry (`[cohsh][tcp] reconnect attempt …`, heartbeat latency) to stderr so operators can correlate client-side failures
  with root-task audit trails.
- Host control tickets (`/host/tickets/spec|status|deadletter`) are schema- and allowlist-gated (`host-ticket/v1`,
  `host-ticket-result/v1`) with strict line-size bounds. Invalid schema/state/action lines fail deterministically before host
  side effects are attempted.
- `host-ticket-agent` applies at-least-once execution with idempotency keyed by `id + idempotency_key` and writes explicit
  lifecycle receipts (`claimed`, `running`, terminal states) so incident replay can prove whether side effects were attempted.
- Evidence exports redact sensitive JSON keys (`*token*`, `*secret*`, `*password*`, `signing_key`, `api_key`) in ticket and audit
  captures, and audit `ticket` fields are hashed (`sha256:<hex>`). Raw bearer/request auth tokens must never be stored in ticket
  `args`; operators should pass opaque references instead.
- The TCP console mirrors the serial surface exactly. Line-oriented commands are terminated by `END` sentinels so scripts can
  verify log completion without relying on socket closure.

<!-- coh-rtc:ticket-quotas:start -->
### Ticket quota limits (generated)
- `ticket_limits.max_scopes`: `8`
- `ticket_limits.max_scope_path_len`: `128`
- `ticket_limits.max_scope_rate_per_s`: `64` (0 = unlimited)
- `ticket_limits.bandwidth_bytes`: `131072` (0 = unlimited)
- `ticket_limits.cursor_resumes`: `16` (0 = unlimited)
- `ticket_limits.cursor_advances`: `256` (0 = unlimited)

_Generated by coh-rtc (sha256: `1b869521f68c26d43c1ad278fbc557f2442e438ab12d443a142e53a33e4466fb`)._
<!-- coh-rtc:ticket-quotas:end -->

### Sidecar Isolation & Spooling
- Sidecar mounts are manifest-gated; adapters that are not declared are unreachable, and mount labels are hash-prefixed on collision.
- Capability scopes are enforced per adapter; unauthorized access yields deterministic `ERR` responses and appends `sidecar-deny` to `/log/queen.log`.
- Offline spooling is bounded by manifest limits; replay drains the spool deterministically and never exceeds `secure9p.msize`.
- LoRa duty-cycle enforcement (radio sidecar, not model LoRA) rejects oversized or over-budget payloads and records bounded tamper entries for audit review.
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
- Root reachability is explicit: `/proc/root/reachable` and `/proc/root/cut_reason` reflect lifecycle-offline, session revocation,
  policy denial, or network cuts so operators never infer authority from liveness alone.
- Port forwarding via `scripts/qemu-run.sh --tcp-port <port>` prints the forwarded endpoint and encourages operators to tunnel
  through localhost-only bindings. When the flag is omitted the listener remains inaccessible from the host, reducing the attack
  surface during bring-up.
- All NIC backends remain HAL-bound; smoltcp plus the authenticated TCP console are the only in-VM network entry points regardless
  of whether RTL8139, virtio-net, or GENETv5 is selected.
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

<!-- Author: Lukas Bower -->
<!-- Purpose: Generated cohsh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->

### cohsh client policy (generated)
- `manifest.sha256`: `3a20adc55c8f975e20e8ef031422f8a09b4a7b8e524dd052bf69296ddf7ff1af`
- `policy.sha256`: `96262c617e5a15321d58f069f17664dfbe02ffa9e6e4df7a38169c21b4e37ee8`
- `cohsh.pool.control_sessions`: `2`
- `cohsh.pool.telemetry_sessions`: `4`
- `cohsh.tail.poll_ms_default`: `1000`
- `cohsh.tail.poll_ms_min`: `250`
- `cohsh.tail.poll_ms_max`: `10000`
- `cohsh.host_telemetry.nvidia_poll_ms`: `1000`
- `cohsh.host_telemetry.systemd_poll_ms`: `2000`
- `cohsh.host_telemetry.docker_poll_ms`: `2000`
- `cohsh.host_telemetry.k8s_poll_ms`: `5000`
- `retry.max_attempts`: `3`
- `retry.backoff_ms`: `200`
- `retry.ceiling_ms`: `2000`
- `retry.timeout_ms`: `5000`
- `heartbeat.interval_ms`: `15000`
- `trace.max_bytes`: `1048576`

_Generated from `configs/root_task.toml` (sha256: `3a20adc55c8f975e20e8ef031422f8a09b4a7b8e524dd052bf69296ddf7ff1af`)._

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
