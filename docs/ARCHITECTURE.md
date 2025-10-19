<!-- Author: Lukas Bower -->
# Cohesix Architecture Overview

## 1. System Boundaries
- **Kernel**: Upstream seL4 for `aarch64/virt (GICv3)`; treated as an external dependency that provides the capability system, scheduling primitives, and IRQ/timer services.
- **Userspace**: Entirely Rust, delivered as a CPIO rootfs containing the root task and all services.
- **Host Tooling**: macOS 26 (Apple Silicon M4) developer workstation running QEMU for validation, plus auxiliary host workers (e.g., GPU bridge) that communicate with the VM over 9P or serial transports.

## 2. High-Level Boot Flow
1. **seL4 Bootstraps** using the external elfloader and enters the Cohesix root task entry point.
2. **Root Task Initialisation**
   - Configures serial logging and prints the boot banner.
   - Establishes a periodic timer and registers IRQ handlers.
   - Constructs the cooperative event pump that rotates through serial RX/TX, timer ticks, networking polls (behind the `net`
     feature), and IPC dispatch without relying on busy waits.
   - Creates the capability space for initial services, including the 9P endpoint and worker slots.
3. **Service Bring-up**
   - Spawns the **NineDoor** 9P server task and hands it the root capability set.
   - Registers static providers that expose `/proc`, `/queen`, `/log`, and the worker namespace.
4. **Operational State**
   - Queen and worker processes attach through NineDoor, exchanging capability tickets that encode their role and budgets.
   - The queen drives orchestration by appending JSON commands to `/queen/ctl`.
 - Telemetry and logs are streamed through append-only files in `/worker/<id>/telemetry` and `/log/queen.log`.
  - Remote operators attach via the TCP-backed console (`cohsh --transport tcp`) which mirrors serial semantics while applying
    heartbeat-driven keep-alives and exponential back-off so networking stalls cannot starve the event pump.

### Bootstrap CSpace Addressing

- All bootstrap `seL4_CNode_*` calls rely on invocation addressing so the kernel always interprets the destination as the current CNode slot: `dest_root = seL4_CapInitThreadCNode`, `dest_depth = 0`, and `dest_offset = 0`. Only the `dest_index` varies per allocation.
- Source capabilities for mint/copy/move likewise use invocation addressing with `src_root = seL4_CapInitThreadCNode`, `src_index = <slot>`, and `src_depth = 0`.
- Untyped retypes mirror the same invariant; the destination tuple is `(root=initThreadCNode, index=<slot>, depth=0, offset=0)` so the kernel never observes guard bits or offsets during bootstrap.
- Bootstrapping begins with a smoke copy of the init TCB capability into `bootinfo.empty.start`, confirming that the invocation-only policy succeeds before any mutable capability traffic occurs.
- Bootstrap uses invocation addressing (depth=0). Slots go in index; offset must be 0.

## 3. Component Responsibilities
### Root Task (crate: `root-task`)
- Owns seL4 initial caps, configures memory, and manages scheduling budgets.
- Provides a minimal RPC surface to NineDoor for spawning/killing tasks and for timer events.
- Enforces budget expiry (ticks, ops, ttl) and revokes capabilities on violation.
- Pre-seeds the device and DMA windows with translation tables so device mappings never trigger `seL4_FailedLookup` (error 6)
  when the kernel installs frames for peripherals.
- Exposes a deterministic event pump (`event::EventPump`) that coordinates serial, timer, networking, and IPC tasks. The pump
  emits structured audit lines whenever subsystems are initialised and ensures each poll cycle services every source without
  revisiting the legacy spin loop.
- Hosts the deterministic networking stack (`net::NetStack`) which wraps smoltcp with virtio-friendly, heapless RX/TX queues and
  a `NetworkClock` that advances in timer-driven increments.
- Provides a serial façade (`serial::SerialPort`) backed by a deterministic virtio-console driver that models the
  RX/TX descriptor rings with bounded `heapless::spsc::Queue` instances. All console IO flows through the shared
  UTF-8 normaliser which tracks back-pressure counters via `portable-atomic` and feeds the parser used by both
  serial and TCP transports.
- Runs the serial/TCP console loop (`console::CommandParser`) which multiplexes authenticated commands (`help`, `attach`, `tail`,
  `log`, `spawn`, `kill`, `quit`) alongside timer and networking events inside the root-task scheduler. Capability validation is
  driven by a deterministic ticket table (`event::TicketTable`) that records bootstrap secrets.

### NineDoor 9P Server (crate: `nine-door`)
- Implements the Secure9P codec/core stack and publishes the synthetic namespace.
- Delegates permission checks to a role-aware `AccessPolicy` using capability tickets minted by the root task.
- Tracks per-session state (fid tables, msize) and ensures append-only semantics on log/telemetry nodes.

### Workers (crate family: `worker-*`)
- Spawned by queen commands; each worker receives a ticket describing its role and budget.
- Communicate exclusively through their mounted NineDoor namespace—no raw IPC between workers.
- Heartbeat workers emit periodic telemetry; future GPU workers coordinate with host GPU bridges.

### Host GPU Bridge (future, crate: `gpu-bridge`)
- Runs **outside** the VM, using NVML/CUDA to manage real hardware.
- Mirrors GPU control surfaces into the VM via a 9P transport adapter (`secure9p-transport::Tcp` on the host side only).
- Maintains lease agreements and enforces memory/stream quotas independent of the VM.

## 4. Namespaces & Mount Tables
- Each session is mounted according to role:
  - **Queen**: `/`, `/queen`, `/proc`, `/log`, `/worker/*`, `/gpu/* (future)`.
  - **WorkerHeartbeat**: `/proc/boot`, `/worker/self/telemetry`, `/log/queen.log (read-only)`.
  - **WorkerGpu (future)**: Worker heartbeat view + `/gpu/<lease>/*` nodes.
- `bind` and `mount` operations are implemented via per-session mount tables maintained by NineDoor. Operations are scoped to a single path (no union mounts) and require queen privileges.

## 5. Capability & Role Model
- **Ticket**: 32-byte capability minted by the root task, bound to `{role, budget, mounts}`.
- **Session**: Contains ticket, negotiated `msize`, fid allocator, and mount table.
- NineDoor verifies every `walk`/`open`/`write` call against the ticket role and append/read mode before delegating to the provider.

## 6. Data Flow Highlights
- **Queen Control**: Append JSON commands to `/queen/ctl`; NineDoor forwards valid commands to root-task orchestration APIs.
- **Telemetry**: Workers append newline-delimited status records to `/worker/<id>/telemetry`. NineDoor enforces append-only semantics by ignoring offsets.
- **Logging**: Root task and queen append to `/log/queen.log`; workers read logs read-only for situational awareness.
- **GPU Integration (future)**: Host bridge exposes GPU metadata/control/job/status nodes; WorkerGpu instances mediate job submission and read back status via NineDoor.

## 7. Networking & Console Integration
- The networking substrate instantiates a virtio-style PHY backed by `heapless::spsc::Queue` buffers (16 frames × 1536 bytes) to
  preserve deterministic memory usage. smoltcp provides the IPv4/TCP stack while the PHY abstraction allows future hardware
  drivers to plug in without changing higher layers. The module is feature-gated (`--features net`) so developers can defer the
  footprint when working on console-only flows.
- A host-only virtio loopback is exposed via `QueueHandle` for testing; production builds will swap this out for the seL4
  virtio-net driver once the VM wiring is complete. The event pump owns the smoltcp poll cadence and publishes link status
  metrics into the boot log.
- The console loop multiplexes serial input and TCP sessions. A shared finite-state parser enforces maximum line length,
  exponential back-off for repeated authentication failures, and funnels all verbs through capability checks before invoking
  NineDoor or root-task orchestration APIs. Sanitised console lines are counted once in the event-pump metrics so `/proc/boot`
  can expose console pressure regardless of transport. TCP transports mirror the parser exactly, emitting `PING`/`PONG`
  heartbeats every 15 seconds (configurable) and logging reconnect attempts so host operators can correlate transient drops with
  root-task audit lines.
- Root-task’s event pump advances the networking clock on every timer tick, services console input, and emits structured log
  lines so host tooling (`cohsh`) can mirror state over either serial or TCP transports while timers and IPC continue to run.

## 8. Reliability & Security Considerations
- Minimal trusted computing base: no POSIX layers, no TCP servers inside the VM, no dynamic loading.
- All inter-process communication is file-based via 9P; no shared memory between workers.
- Timer and watchdog infrastructure ensures runaway workers are revoked cleanly.
- NineDoor core is `no_std + alloc` capable, allowing potential reuse in bare-metal contexts.

## 9. Roadmap Dependencies
- **Milestone alignment**: Architecture is realised incrementally per `BUILD_PLAN.md` milestones.
- **Documentation as Source of Truth**: Changes to components or interfaces must be reflected here to avoid drift.

## 10. Milestone 7 Migration Notes
- **Event pump adoption**: Developers upgrading from the legacy spin loop
  must initialise the `event::EventPump` in `kernel_start` and remove any
  ad-hoc busy waits. The pump now owns serial, timer, networking, and IPC
  poll cadence; new subsystems must register via typed handlers so audits
  continue to show `event-pump: init <subsystem>` lines during boot.
- **Serial driver integration**: The virtio-console façade replaces
  direct PL011 shims. It exposes heapless RX/TX queues and atomic
  back-pressure counters. Tests should exercise the shared console parser
  via `cargo test -p root-task console_auth` to confirm UTF-8
  sanitisation, rate limiting, and audit logging remain intact.
- **Networking feature flag**: The deterministic smoltcp glue is guarded
  by `--features net`. Enable the flag before touching networking code
  and run `cargo check -p root-task --features net` plus
  `cargo clippy -p root-task --features net --tests` to validate bounded
  queue usage. Disable the feature for serial-only builds to keep the
  baseline footprint minimal.
- **Integration workflow**: `scripts/qemu-run.sh --console serial --net`
  exercises the complete Milestone 7 event pump (serial + networking).
  Pair it with `tests/integration/qemu_tcp_console.rs` to confirm the TCP
  transport stays responsive while timers and NineDoor services continue
  to operate.

## 11. Manifest Compiler & As-Built Guarantees
- **Single manifest**: Beginning with Milestone 8, the root task, docs,
  and CLI scripts are generated from `root_task.toml` via the
  `tools/coh-rtc` compiler. The manifest encodes architecture profile,
  Secure9P bounds, provider topology, ticket policies, and feature gates
  that keep VM artefacts `#![no_std]`.
- **Validation**: The compiler enforces red lines (walk depth ≤ 8,
  `msize ≤ 8192`, no `..`, no fid reuse, capability scoping) and refuses
  manifests that would require `std`, exceed memory budgets, or enable
  unimplemented transports. Generated Rust modules carry `#![no_std]`
  annotations and are formatted deterministically to support reproducible
  builds and compliance audits.
- **Docs-as-built**: Architecture, interfaces, and security documents
  ingest compiler snippets (CBOR schemas, `/proc` layouts, concurrency
  knobs, hardware tables). CI compares manifest fingerprints and embedded
  excerpts to guarantee documentation reflects the running system. At
  release time, tools/coh-rtc regenerates Markdown fragments (e.g., `/proc`
  layout, concurrency knobs, CBOR schemas) and injects them into
  `docs/INTERFACES.md` and `docs/SECURE9P.md`, ensuring each release
  candidate’s documentation matches the compiled manifest exactly.
- **Policy export**: Compiler outputs `out/manifests/*.json` and
  operator policy files consumed by `cohsh`, enabling deterministic
  session pooling, retry budgets, and future hardware profiles without
  editing runtime code.

## 12. Hardware Trajectory & Host/Worker Sidecar Pattern
- **UEFI readiness**: Later milestones introduce an aarch64 UEFI loader
  that boots the generated manifest on physical hardware without a VM.
  The loader maps UART/NET MMIO regions defined in the manifest,
  initialises the same event pump, and emits attestation records to
  `/proc/boot`. Secure Boot measurements cover generated artefacts,
  ensuring retail, industrial, and defense deployments can trust the
  runtime state.
- **Device identity**: TPM (or DICE) integration seals ticket seeds and
  records boot hashes. NineDoor exposes attestation logs via read-only
  files so host tooling and operators can verify provenance before
  enabling privileged commands.
- **Host/Worker sidecar pattern**: *Sidecars* are auxiliary processes that
  run **outside the seL4 VM** whenever possible (on the host or another
  container). Each sidecar exposes its namespace into the VM **over 9P**
  using the same security model as internal providers. Only lightweight
  control stubs or schedulers (e.g., LoRa duty-cycle timers) execute
  **inside** the VM under strict manifest quotas to preserve a lean,
  deterministic TCB.
- **Lifecycle & discovery**: Sidecar mounts and capability scopes are
  declared in the **manifest**; the **host launcher** inspects the
  manifest at boot, spawns or connects required sidecars, and only then
  hands control to the root task. This keeps deployment topology in
  lockstep with what the compiler planned.
- **Common bridge trait (optional)**: All sidecars conform to a shared
  trait surface (e.g., `sidecar::ProviderBridge`) so new buses (MODBUS,
  DNP3, CAN, LoRa) can be added without modifying the VM.
- **Budget discipline**: Sidecar-related workers are feature-gated and
  bound by manifest quotas so the event pump remains deterministic even
  under constrained links. Host tooling validates dependencies before
  enabling sidecars, preventing drift between planned and deployed
  topologies.
