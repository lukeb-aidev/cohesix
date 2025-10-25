<!-- Author: Lukas Bower -->
# Cohesix Build Plan (ARM64, Pure Rust Userspace)

**Host:** macOS 26 on Apple Silicon (M4)
**Target:** QEMU aarch64 `virt` (GICv3)
**Kernel:** Upstream seL4 (external build)
**Userspace:** Pure Rust crates (`root-task`, `nine-door`, `worker-heart`, future `worker-gpu`, `gpu-bridge` host tool)

The milestones below build cumulatively; do not advance until the specified checks pass and documentation is updated. Each step
is grounded in the architectural intent outlined in `docs/ARCHITECTURE.md`, the repository conventions from `docs/REPO_LAYOUT.md`,
and the interface contracts codified in `docs/INTERFACES.md`. Treat those documents as non-negotiable source material when
preparing and executing tasks.

## Milestone 0 — Repository Skeleton & Toolchain (1–2 days)
**Deliverables**
- Cargo workspace initialised with crates for `root-task`, `nine-door`, and `worker-heart` plus shared utility crates.
- `toolchain/setup_macos_arm64.sh` script checking for Homebrew dependencies, rustup, and QEMU - and installing if absent.
- `scripts/qemu-run.sh` that boots seL4 with externally built `elfloader`, `kernel.elf`, also creates and uses `rootfs.cpio`.
- `ci/size_guard.sh` enforcing < 4 MiB CPIO payload.
- Repository tree matches `docs/REPO_LAYOUT.md`, and architecture notes from `docs/ARCHITECTURE.md §1-§3` are captured in crate
  READMEs or module docs to prevent drift.

**Checks**
- `cargo check` succeeds for the workspace.
- `qemu-system-aarch64 --version` reports the expected binary.
- `ci/size_guard.sh out/rootfs.cpio` rejects oversized archives.

## Milestone 1 — Boot Banner, Timer, & First IPC
**Deliverables**
- Root task prints a banner and configures a periodic timer tick.
- Root task spawns a secondary user component via seL4 endpoints.
- Demonstrate one ping/pong IPC exchange and timer-driven logging.
- Added runtime CSpace-path assert for all retypes.
- Scaffold `cohsh` CLI prototype (command parsing + mocked transport) per `docs/USERLAND_AND_CLI.md §2-§4` so operators can
  observe logs via `tail` and exercise attach/login flows defined in `docs/INTERFACES.md §7`.

**Checks**
- QEMU serial shows boot banner and periodic `tick` line.
- QEMU serial logs `PING 1` / `PONG 1` sequence exactly once per boot.
- No panics; QEMU terminates cleanly via monitor command.

**M1 → M2 Transition Note**
- Retype now targets the init root CNode using the canonical tuple `(root=seL4_CapInitThreadCNode, node_index=0, node_depth=bootinfo.initThreadCNodeSizeBits, slot)` and validates capacity via `initThreadCNodeSizeBits`.

## Milestone 2 — NineDoor Minimal 9P
**Deliverables**
- Secure9P codec + fid/session table implementing `version`, `attach`, `walk`, `open`, `read`, `write`, `clunk`.
- Synthetic namespace publishing:
  - `/proc/boot` (read-only text)
  - `/log/queen.log` (append-only)
  - `/queen/ctl` (append-only command sink)
  - `/worker/<id>/telemetry` (append-only, created on spawn)
- In-VM transport (shared ring or seL4 endpoint wrapper). No TCP inside the VM.
- `cohsh` CLI upgraded to speak the live NineDoor transport (mock removed) while preserving operator workflows.
- Implementation satisfies the defences and layering requirements from `docs/SECURE9P.md §2-§5` and strictly adheres to
  `docs/INTERFACES.md §1-§6` for operation coverage, ticket validation, and error semantics.

**Checks**
- Integration test attaches, walks, reads `/proc/boot`, and appends to `/queen/ctl`.
- Attempting to write to `/proc/boot` fails with `Permission`.
- Decoder corpus covers malformed frames (length mismatch, fid reuse).

## Milestone 3 — Queen/Worker MVP with Roles
**Deliverables**
- Role-aware access policy implementing `Queen` and `WorkerHeartbeat` roles.
- `/queen/ctl` accepts JSON commands:
  - `{"spawn":"heartbeat","ticks":100}`
  - `{"kill":"<id>"}`
  - `{"budget":{"ttl_s":120,"ops":1000}}` (optional fields)
- Worker-heart process appends `"heartbeat <tick>"` lines to `/worker/<id>/telemetry`.
- Budget enforcement (ttl/ticks/ops) with automatic revocation.
- Access policy follows `docs/ROLES_AND_SCHEDULING.md §1-§5` and the queen control schema in `docs/INTERFACES.md §3-§4`; all
  JSON handling must reject unknown formats as defined in the error table (`docs/INTERFACES.md §8`).

**Checks**
- Writing spawn command creates worker directory and live telemetry stream.
- Writing kill removes worker directory and closes telemetry file.
- Role isolation tests deny cross-role reads/writes.

## Milestone 4 — Bind & Mount Namespaces
**Deliverables**
- Per-session mount table with `bind(from, to)` and `mount(service, at)` operations scoped to a single path.
- Queen-only commands for namespace manipulation exposed via `/queen/ctl`.
- Namespace operations mirror the behaviour defined in `docs/INTERFACES.md §3` and respect mount expectations in
  `docs/ARCHITECTURE.md §4`.

**Checks**
- Queen remaps `/queen` to a subdirectory without affecting other sessions.
- Attempted bind by a worker fails with `Permission`.

## Milestone 5 — Hardening & Test Automation (ongoing)
**Deliverables**
- Unit tests for codec, fid lifecycle, and access policy negative paths.
- Fuzz harness covering length-prefix mutations and random tail bytes for the decoder.
- Integration test: spawn heartbeat → emit telemetry → kill → verify revocation logs.
- Cohsh regression scripts (per `docs/USERLAND_AND_CLI.md §6-§7`) execute against mock and QEMU targets, ensuring CLI and
  Secure9P behaviours stay aligned.

**Checks**
- `cargo test` passes in CI.
- Fuzz harness runs N iterations (configurable) without panic.

## Milestone 6 — GPU Worker Integration
**Deliverables**
- Define `WorkerGpu` role and extend `/queen/ctl` schema with GPU lease requests.
- Host-side `gpu-bridge-host` tool implementing NVML-based discovery (feature-gated) and `--mock` namespace mirroring for `/gpu/<id>/*`.
- Job submission protocol (JSON) supporting vector add and matrix multiply kernels with SHA-256 payload validation, optional inline payloads, and status fan-out to `/gpu/<id>/status` and `/worker/<id>/telemetry`.
- Implementation must align with `docs/GPU_NODES.md §2-§7`, uphold the command schemas in `docs/INTERFACES.md §3-§5`, and keep
  VM-side responsibilities within the boundaries in `docs/ARCHITECTURE.md §7-§8`.
- All temporary scaffolds, mocks, or stubs have been replaced with production-grade integrations; the completed build plan
  represents the fully implemented Cohesix stack.

**Checks**
- Queen spawns a GPU worker (simulated if real hardware unavailable) and receives telemetry lines.
- Lease expiry revokes worker access and closes `/gpu/<id>/job` handle.
- Host integration tests run in `--mock` mode when GPUs are absent.

> **Rule of Engagement:** Advance milestones sequentially, treat documentation as canonical, and keep code/tests aligned after every milestone increment.

## Milestone 7a — Root-Task Event Pump & Authenticated Kernel Entry
**Deliverables**
- **Deprecate legacy spin loop**
  - Replace the placeholder busy loop in `kernel_start` with a cooperative event pump that cycles serial RX/TX, timer ticks, networking polls, and IPC dispatch without relying on `std` primitives.
  - Capture wake ordering and preemption notes in module docs so subsequent milestones can extend the pump without regressing determinism.
  - Instrument the transition with structured audit logs showing when the pump initialises each subsystem.
- **Serial event integration (no-std)**
  - Introduce a `root-task` `serial` module built atop OSS crates such as `embedded-io` and `nb` for trait scaffolding while maintaining zero-allocation semantics using `heapless` buffers.
  - Provide interrupt-safe reader/writer abstractions that feed the event pump, expose per-source back-pressure counters via `portable-atomic`, and enforce UTF-8 sanitisation before lines reach the command parser.
  - Add conformance tests that replay captured QEMU traces to guarantee debounced input (backspace, control sequences) behaves identically across boots.
- **Networking substrate bootstrapping**
  - Integrate the virtio-net PHY and `smoltcp` device glue behind a feature gate, seeding deterministic RX/TX queues using `heapless::{Vec, spsc::Queue}` and documenting memory bounds in `docs/SECURITY.md`.
  - Ensure the event pump owns the poll cadence for `smoltcp`, handles link up/down notifications, and publishes metrics to `/proc/boot` for observability.
  - Provide fault-injection tests that exhaust descriptors, validate checksum handling, and assert the pump survives transient PHY resets.
- **Authenticated command loop**
  - Embed a shared command parser (serial + TCP) constructed with `heapless::String` and finite-state validation to enforce maximum line length, reject unsupported control characters, and throttle repeated failures with exponential back-off.
  - Hook authentication into the root-task capability validator so privileged verbs (`attach`, `spawn`, `kill`, `log`) require valid tickets, emitting audit lines to `/log/queen.log` on denial.
  - Add integration tests that execute scripted login attempts, verify rate limiting, and confirm the event pump resumes servicing timers and networking during authentication stress.
- **Documentation updates**
  - Update `docs/ARCHITECTURE.md` and `docs/SECURITY.md` with the new event pump topology, serial/network memory budgets, and authenticated console flow diagrams.
  - Document migration steps for developers moving from the spin loop to the event pump, including feature flags and testing guidance in `docs/REPO_LAYOUT.md` or relevant READMEs.

**Checks**
- Root task boots under QEMU, initialises the event pump, and logs subsystem activation without reintroducing the legacy busy loop.
- Serial RX/TX, networking polls, and command handling execute deterministically without heap allocations; fuzz/property tests cover parser and queue saturation paths.
- Authenticated sessions enforce capability checks, rate limit failures, and keep timer/NineDoor services responsive during sustained input.

### Task Breakdown

```
Title/ID: m7a-event-pump-core
Goal: Replace the kernel_start spin loop with a cooperative no-std event pump.
Inputs: docs/ARCHITECTURE.md §§2,4; docs/SECURITY.md §§3-4; existing root-task entrypoint.
Changes:
  - crates/root-task/src/kernel.rs — remove spin loop, initialise serial/net/timer pollers, and document scheduling guarantees.
  - crates/root-task/src/event/mod.rs — new event pump coordinator orchestrating serial, timer, IPC, and networking tasks with explicit tick budgeting.
  - crates/root-task/tests/event_pump.rs — unit tests covering scheduling fairness, back-pressure propagation, and panic-free shutdown paths.
Commands: cd crates/root-task && cargo test event_pump && cargo check --features net && cargo clippy --features net --tests
Checks: Event pump drives serial, timer, and networking tasks deterministically; tests cover starvation and shutdown.
Deliverables: Root-task event pump replacing legacy loop with documented guarantees and regression tests.
```

```
Title/ID: m7a-serial-auth
Goal: Provide authenticated serial command handling with rate limiting and audit trails.
Inputs: docs/INTERFACES.md §§3,7-8; docs/SECURITY.md §5; embedded-io 0.4; heapless 0.8.
Changes:
  - crates/root-task/src/console/mod.rs — integrate heapless line editor, authentication state machine, and audit logging.
  - crates/root-task/src/console/serial.rs — implement no-std serial driver traits, UTF-8 sanitisation, and per-byte throttling metrics.
  - crates/root-task/tests/console_auth.rs — tests for login success/failure, rate limiting, control sequence rejection, and audit log outputs.
Commands: cd crates/root-task && cargo test console_auth && cargo check --features net && cargo clippy --features net --tests
Checks: Serial console authenticates commands, enforces throttling, and keeps event pump responsive under stress.
Deliverables: Hardened serial console with authentication, audit coverage, and passing tests.
```

```
Title/ID: m7a-net-loop
Goal: Embed the smoltcp-backed networking poller into the event pump with deterministic buffers.
Inputs: docs/ARCHITECTURE.md §§4,7; docs/SECURITY.md §4; smoltcp 0.11; heapless 0.8; portable-atomic 1.6.
Changes:
  - crates/root-task/src/net/mod.rs — finalise virtio-net PHY, smoltcp integration, and bounded queues with instrumentation.
  - crates/root-task/src/event/net.rs — event pump adapter scheduling smoltcp polls, handling link state, and surfacing metrics.
  - crates/root-task/tests/net_pump.rs — property tests for descriptor exhaustion, checksum validation, and PHY reset recovery.
Commands: cd crates/root-task && cargo test --features net net_pump && cargo check --features net && cargo clippy --features net --tests
Checks: Networking poller integrates with event pump, survives fault injection, and maintains deterministic buffer usage.
Deliverables: Networking subsystem integrated with event pump, documented, and guarded by targeted tests.
```

```
Title/ID: m7a-docs-migration
Goal: Update documentation for the event pump, authenticated console, and networking integration.
Inputs: docs/ARCHITECTURE.md, docs/INTERFACES.md, docs/SECURITY.md, existing milestone notes.
Changes:
  - docs/ARCHITECTURE.md — describe event pump topology, serial/net modules, and removal of spin loop.
  - docs/SECURITY.md — record authenticated console threat model, rate limiting strategy, and memory quotas.
  - docs/REPO_LAYOUT.md & crate READMEs — outline developer workflows, feature flags, and testing commands for the new pump.
Commands: cargo doc -p root-task --document-private-items && mdbook build docs (if configured)
Checks: Documentation builds cleanly, reflects new architecture, and guides developers through migration.
Deliverables: Synchronized documentation explaining event pump adoption, security posture, and developer workflows.
```

## Milestone 7b — Standalone Console & Networking (QEMU-first)
**Deliverables**
- **Serial console integration**
  - Implement a bidirectional serial driver for QEMU (`virtio-console` preferred, PL011 fallback) that supports blocking RX/TX (no heap, no `std`) and exposes an interrupt-safe API so the event pump can integrate timer and network wake-ups.
  - Replace the `kernel_start` spin loop with an event pump that polls serial input, dispatches parsed commands, services outgoing buffers, and yields to networking/timer tasks without starving the scheduler.
  - Enforce ticket and role checks before privileged verbs execute; log denied attempts to `/log/queen.log`, apply exponential back-off when credentials are wrong, and drop connections that exceed retry quotas.
- **Networking substrate**
  - Add `smoltcp` (Rust, BSD-2) to the root-task crate under a new `net` module with explicit feature gating so baseline builds stay minimal.
  - Implement a virtio-net MMIO PHY for QEMU, encapsulate the device behind a trait that abstracts descriptor management, and document the register layout alongside reset/feature negotiation flows.
  - Use `heapless::{Vec, spsc::Queue}` for RX/TX buffers to keep allocations deterministic; document memory envelopes in `docs/SECURITY.md` and prove queue saturation behaviour with tests.
- **Command loop**
  - Build a minimal serial + TCP line editor using `heapless::String` and a finite-state parser for commands (`help`, `attach`, `tail`, `log`, `quit`, plus `spawn`/`kill` stubs that forward JSON to NineDoor) with shared code paths so behaviours remain identical across transports.
  - Integrate the loop into the root-task main event pump alongside timer ticks, networking polls, and IPC dispatch while enforcing capability checks before invoking root-task RPCs.
  - Rate-limit failed logins, enforce maximum line length, reject control characters outside the supported set, and record audit events whenever a session hits throttling.


**Checks**
- QEMU boot brings up the root task, configures smoltcp, accepts serial commands, and listens for TCP attachments on the configured port.
- `cohsh --transport tcp` can attach, tail logs, and quit cleanly; regression scripts cover serial-only mode.
- Fuzz or property-based tests exercise the new parser and networking queues without panics.

### Task Breakdown

```
Title/ID: m7b-serial-rx
Goal: Provide bidirectional serial I/O for the root-task console in QEMU.
Inputs: docs/ARCHITECTURE.md §2; docs/INTERFACES.md §7; seL4 virtio-console/PL011 specs; `embedded-io` 0.4 (optional traits).
Changes:
  - crates/root-task/src/console/serial.rs — MMIO-backed RX/TX driver exposing `read_byte`/`write_byte` without heap allocation, plus interrupt acknowledgement helpers and a shared rate-limiter primitive for reuse by the console loop.
  - crates/root-task/src/kernel.rs — initialise the serial driver, hook it into the event pump, remove the legacy busy loop, and document the wake-up ordering for timer/net/serial sources.
  - crates/root-task/tests/serial_stub.rs — host-side stub verifying backspace/line termination handling, throttle escalation, and the audit log entries emitted by repeated authentication failures.
Commands: cd crates/root-task && cargo test serial_stub && cargo check --features net && cargo clippy --features net --tests
Checks: Serial RX consumes interactive input without panics; console loop handles backspace/newline, rate limiting, and audit logging in QEMU.
Deliverables: Root-task serial driver initialised during boot with regression tests for RX edge cases and throttling safeguards.
```

```
Title/ID: m7b-net-substrate
Goal: Wire up a deterministic networking stack for the root task.
Inputs: docs/ARCHITECTURE.md §§4,7; docs/INTERFACES.md §§1,3,6; docs/SECURITY.md §4; smoltcp 0.11; heapless 0.8; portable-atomic 1.6.
Changes:
  - crates/root-task/Cargo.toml — add `smoltcp`, `heapless`, and `portable-atomic` dependencies behind a `net` feature along with feature docs explaining footprint impact.
  - crates/root-task/src/net/mod.rs — introduce PHY trait, virtio-net implementation (descriptor rings, IRQ handler), smoltcp device glue, bounded queues, and defensive checks for descriptor exhaustion.
  - crates/root-task/src/main.rs — initialise networking, register poller within the root-task event loop, and expose metrics hooks so audit logs can capture link bring-up status.
  - docs/SECURITY.md — document memory envelopes, networking threat considerations, and mitigations for RX flooding or malformed descriptors.
Commands: cd crates/root-task && cargo check --features net && cargo test --features net net::tests && cargo clippy --features net --tests
Checks: Smoltcp interface boots in QEMU with deterministic heap usage; unit tests cover RX/TX queue saturation, link bring-up, error paths, and descriptor validation.
Deliverables: Root-task networking module with virtio-net backend, updated security documentation, and passing feature-gated tests reinforced by lint coverage.
```

```
Title/ID: m7b-console-loop
Goal: Provide an authenticated serial/TCP command shell bound to capability checks.
Inputs: docs/INTERFACES.md §§3-5,8; docs/SECURITY.md §5; existing root-task timer/IPC code; heapless 0.8.
Changes:
  - crates/root-task/src/console/mod.rs — add finite-state parser, rate limiter, shared line editor for serial/TCP sources, and an authentication/session manager that reuses ticket validation helpers.
  - crates/root-task/src/main.rs — integrate console loop with networking poller and ticket validator while ensuring timer/NineDoor tasks retain service guarantees.
  - crates/root-task/tests/console_parser.rs — unit tests for verbs, overlong lines, login throttling, Unicode/control character handling, and audit log integration.
Commands: cd crates/root-task && cargo test --features net console_parser && cargo clippy --features net --tests
Checks: Parser rejects invalid verbs, enforces max length, rate limits failed logins, normalises newline sequences, and verifies capability enforcement via mocks.
Deliverables: Hardened console loop with comprehensive parser tests integrated into root-task and lint-clean CI coverage.
```
## Milestone 7c
**Deliverables**
- **Remote transport**
  - Extend `cohsh` with a TCP transport that speaks to the new in-VM listener while keeping the existing mock/QEMU flows; expose reconnect/back-off behaviour and certificate-less ticket validation for the prototype environment.
  - Reuse the current NineDoor command surface so scripting and tests stay aligned, document the new `--transport tcp` flag with examples, and ensure help text highlights transport fallbacks when networking is unavailable.
- **Documentation & tests**
  - Update `docs/ARCHITECTURE.md`, `docs/INTERFACES.md`, and `docs/SECURITY.md` with the networking/console design, threat model, and TCB impact including memory budgeting tables for serial/net buffers.
  - Provide QEMU integration instructions (`docs/USERLAND_AND_CLI.md`) showing serial console usage, remote `cohsh` attachment, and recommended port-forwarding commands for macOS host tooling.
  - Add unit tests for the command parser (invalid verbs, overlong lines), virtio queue wrappers, and integration tests that boot QEMU, connect via TCP, run scripted sessions, and verify audit log outputs.
### Task Breakdown
```
Title/ID: m7c-cohsh-tcp
Goal: Extend cohsh CLI with TCP transport parity while retaining existing flows.
Inputs: docs/USERLAND_AND_CLI.md §§2,6; docs/INTERFACES.md §§3,7; existing cohsh mock/QEMU transport code.
Changes:
  - apps/cohsh/Cargo.toml — gate TCP transport feature and dependencies, annotate default-off status, and document cross-compilation requirements for macOS hosts.
  - apps/cohsh/src/transport/tcp.rs — implement TCP client with ticket authentication, reconnect handling, heartbeats, and telemetry logging for CLI operators.
  - apps/cohsh/src/main.rs — add `--transport tcp` flag and configuration plumbing, including environment overrides and validation for mutually exclusive serial parameters.
  - docs/USERLAND_AND_CLI.md — document CLI usage, examples, regression scripts covering serial and TCP paths, and troubleshooting steps for QEMU port forwarding.
Commands: cd apps/cohsh && cargo test --features tcp && cargo clippy --features tcp --tests && cargo fmt --check
Checks: CLI attaches via TCP to QEMU instance, tails logs, forwards NineDoor commands, retains existing regression flow for serial transport, and recovers gracefully from simulated disconnects.
Deliverables: Feature-complete TCP transport with documentation, tests validating CLI behaviour, and formatting/lint coverage.
```

```
Title/ID: m7c-docs-integration-tests
Goal: Finalise documentation updates and cross-stack integration tests for networking milestone.
Inputs: docs/ARCHITECTURE.md, docs/INTERFACES.md, docs/SECURITY.md, docs/USERLAND_AND_CLI.md; existing integration harness scripts.
Changes:
  - docs/ARCHITECTURE.md — describe networking module, console loop, PHY abstraction, and update diagrams to illustrate serial/net event pump interactions.
  - docs/INTERFACES.md — specify TCP listener protocol, authentication handshake, console commands, and error codes for throttling or malformed frames.
  - docs/SECURITY.md — extend threat model with networking attack surfaces, mitigations, audit expectations, and documented memory bounds.
  - tests/integration/qemu_tcp_console.rs — scripted boot + TCP session exercising help/attach/tail/log/quit verbs, plus negative tests for failed logins and overlong lines.
  - scripts/qemu-run.sh — accept networking flags, expose forwarded TCP port, document usage, and emit helpful diagnostics when host prerequisites (tap/tuntap) are missing.
Commands: ./scripts/qemu-run.sh --net tap --console tcp --exit-after 120 && cargo test -p tests --test qemu_tcp_console && cargo clippy -p tests --tests
Checks: Automated QEMU run brings up TCP console reachable from host; integration test passes end-to-end; documentation reviewed for consistency and security sign-off.
Deliverables: Updated documentation set, automation scripts, and passing QEMU TCP console integration test with lint coverage.
```

## Milestone 7d
**Deliverables**
- **Console acknowledgements**
  - Enable the root-task TCP listener to emit `OK`/`ERR` responses for `ATTACH`, heartbeat probes, and command verbs so remote operators receive immediate feedback.
  - Surface execution outcomes (success, denial, or validation failure) through the shared serial/TCP output path with structured debug strings suitable for regression tests.
- **Client alignment**
  - Ensure `cohsh` reuses the acknowledgement surface for telemetry, surfacing attach/session state changes and command failures consistently across transports.
- **Documentation & tests**
  - Update protocol documentation to describe the acknowledgement lifecycle, including reconnection semantics and error payloads.
  - Extend automated coverage so both serial and TCP transports assert the presence of acknowledgements during scripted sessions.

### Task Breakdown
```
Title/ID: m7d-console-ack
Goal: Implement bidirectional console responses covering attach handshakes and command execution outcomes.
Inputs: docs/INTERFACES.md §7; docs/USERLAND_AND_CLI.md §6; apps/root-task/src/event/mod.rs; apps/root-task/src/net/virtio.rs; apps/cohsh/src/transport/tcp.rs.
Changes:
  - apps/root-task/src/event/mod.rs — introduce an acknowledgement dispatcher that emits success/error lines for each validated command, wiring into both serial and TCP paths.
  - apps/root-task/src/net/virtio.rs & apps/root-task/src/net/queue.rs — plumb outbound console buffers so TCP clients receive the acknowledgement lines generated by the event pump without blocking polling guarantees.
  - apps/cohsh/src/transport/tcp.rs — consume acknowledgement lines for attach/command verbs, surfacing them in CLI output and telemetry, and hardening reconnect flows when acknowledgements are missing.
  - docs/INTERFACES.md & docs/USERLAND_AND_CLI.md — document the acknowledgement grammar, heartbeat expectations, and troubleshooting guidance for mismatched responses.
Commands: (cd apps/root-task && cargo test --features net && cargo clippy --features net --tests && cargo fmt --check) && (cd apps/cohsh && cargo test --features tcp && cargo clippy --features tcp --tests && cargo fmt --check)
Checks: TCP console responds with acknowledgements for attach/log/tail commands; serial harness mirrors the same output; regression suite covers success and failure cases with deterministic logs.
Deliverables: Bidirectional console acknowledgements spanning serial and TCP transports, updated protocol documentation, and passing unit/integration tests with lint/format coverage.
```

**Foundation Allowlist (for dependency reviews / Web Codex fetches)**
- `https://crates.io/crates/smoltcp`
- `https://crates.io/crates/heapless`
- `https://crates.io/crates/portable-atomic` (for lock-free counters)
- `https://crates.io/crates/embassy-executor` and `https://crates.io/crates/embassy-net` (future async extension, optional)
- `https://crates.io/crates/log` / `defmt` (optional structured logging while developing the stack)
- `https://crates.io/crates/embedded-io` (serial/TCP trait adapters)
- `https://crates.io/crates/nb` (non-blocking IO helpers)
- `https://crates.io/crates/spin` (lock primitives for bounded queues)

## Milestone 8a — Lightweight Hardware Abstraction Layer

**Why now (context):** Kernel bring-up now relies on multiple MMIO peripherals (PL011 UART, virtio-net). Tight coupling to `KernelEnv`
spread driver responsibilities across modules, making future platform work and compiler integration harder to reason about.

**Goal**
Carve out a lightweight Hardware Abstraction Layer so early boot and drivers consume a focused interface for mapping device pages
and provisioning DMA buffers.

**Deliverables**
- `apps/root-task/src/hal/mod.rs` introducing `KernelHal` and the `Hardware` trait that wrap device/DMA allocation, coverage queries,
  and allocator snapshots.
- `apps/root-task/src/kernel.rs` switched to the HAL for PL011 bring-up and diagnostics, keeping boot logging unchanged.
- `apps/root-task/src/drivers/virtio/net.rs` and `apps/root-task/src/net/stack.rs` updated to rely on the HAL rather than touching
  `KernelEnv` directly, simplifying future platform support.
- Documentation updates in this build plan describing the milestone and entry criteria.

**Commands**
- `cargo check -p root-task --features "kernel,net-console"`

**Checks (DoD)**
- Root task still boots with PL011 logging and virtio-net initialisation using the new HAL bindings.
- HAL error propagation surfaces seL4 error codes for diagnostics (no regression in boot failure logs).
- Workspace `cargo check` succeeds with the kernel and net-console features enabled.

---
## Milestone 8b — Root-Task Compiler & Deterministic Profiles

**Why now (context):** The event pump, HAL, and authenticated console now run end-to-end, but the configuration that wires tickets, namespaces, and capability budgets together still lives in hand-written Rust. A manifest-driven compiler lets us regenerate bootstrap code, docs, and CLI fixtures from one artefact so deployments stay auditable and reproducible.

**Goal**
Introduce the `coh-rtc` compiler that ingests `configs/root_task.toml` and emits deterministic artefacts consumed by the root task, docs, and regression suites.

**Deliverables**
- `configs/root_task.toml` capturing schema version, platform profile, event-pump cadence, ticket inventory, namespace mounts, Secure9P limits, and feature toggles (e.g., `net-console`).
- Workspace binary crate `tools/coh-rtc/` with modules:
  - `src/ir.rs` defining IR v1.0 with serde validation, red-line enforcement (walk depth ≤ 8, `msize ≤ 8192`, no `..` components), and feature gating that refuses `std`-only options when `profile.kernel = true`.
  - `src/codegen/` emitting `#![no_std]` Rust for `apps/root-task/src/generated/{mod.rs,bootstrap.rs}` plus JSON/CLI artefacts.
  - Integration tests under `tools/coh-rtc/tests/` that round-trip sample manifests and assert deterministic hashes.
- `apps/root-task/src/lib.rs` and `apps/root-task/src/kernel.rs` updated to include the generated module (behind `#[path = "generated/mod.rs"]`) and to use manifest-derived tables for ticket registration, namespace wiring, and initial audit lines.
- `apps/root-task/build.rs` gains a check that fails the build if generated files are missing or stale relative to `configs/root_task.toml`.
- Generated artefacts:
  - `apps/root-task/src/generated/bootstrap.rs` — init graph, ticket table, namespace descriptors with compile-time hashes.
  - `out/manifests/root_task_resolved.json` — serialised IR with SHA-256 fingerprint stored alongside.
  - `tests/cli/boot_v0.cohsh` — baseline CLI script derived from the manifest to exercise attach/log/quit flows.
- Documentation updates:
  - `docs/ARCHITECTURE.md §11` expanded with the manifest schema and regeneration workflow.
  - `docs/BUILD_PLAN.md` (this file) references the manifest in earlier milestones.
  - `docs/REPO_LAYOUT.md` lists the new `configs/` and `tools/coh-rtc/` trees with regeneration commands.

**Commands**
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json --cli-script tests/cli/boot_v0.cohsh`
- `cargo check -p root-task --no-default-features --features kernel,net-console`
- `cargo test -p root-task`
- `cargo test -p tools/coh-rtc`
- `cargo run -p cohsh --features tcp -- --transport tcp --script tests/cli/boot_v0.cohsh`

**Checks (DoD)**
- Regeneration is deterministic: two consecutive runs of `cargo run -p coh-rtc …` produce identical Rust, JSON, and CLI artefacts (verified via hash comparison recorded in `out/manifests/root_task_resolved.json.sha256`).
- Root task boots under QEMU using generated bootstrap tables; serial log shows manifest fingerprint and ticket registration sourced from generated code.
- Compiler validation rejects manifests that violate red lines (e.g., invalid walk depth, enabling `gpu` while `profile.kernel` omits the feature gate) and exits with non-zero status.

**Compiler touchpoints**
- Introduces `root_task.schema = "1.0"`; schema mismatches abort generation and instruct operators to upgrade docs.
- Adds `cargo xtask` style CI guard (or Makefile target) invoked by `scripts/check-generated.sh` that runs the compiler, compares hashes, and fails CI when committed artefacts drift.
- Exports doc snippets (e.g., namespace tables) as Markdown fragments consumed by `docs/ARCHITECTURE.md` to guarantee docs stay in lockstep with the manifest.

---

## Milestone 9 — Secure9P Pipelining & Batching

**Why now (compiler):** Host NineDoor already handles baseline 9P flows, but upcoming use cases demand concurrent telemetry and command streams. Enabling multiple in-flight tags and batched writes requires new core structures and manifest knobs so deployments tune throughput without compromising determinism.

**Goal**
Refactor Secure9P into codec/core crates with bounded pipelining and manifest-controlled batching.

**Deliverables**
- Split `crates/secure9p-wire` into:
  - `crates/secure9p-codec` — frame encode/decode, batch iterators, fuzz corpus harnesses (still `std` for now).
  - `crates/secure9p-core` — session manager, fid table, tag window enforcement, and `no_std + alloc` compatibility.
  Existing consumers (`apps/nine-door`, `apps/cohsh`) migrate to the new crates.
- `apps/nine-door/src/host/` updated to process batched frames and expose back-pressure metrics; new module `pipeline.rs` encapsulates short-write handling and queue depth accounting surfaced via `/proc/9p/*` later.
- `apps/nine-door/tests/pipelining.rs` integration test spinning four concurrent sessions, verifying out-of-order responses and bounded retries when queues fill.
- CLI regression `tests/cli/9p_batch.cohsh` executing scripted batched writes and verifying acknowledgement ordering.
- `configs/root_task.toml` gains IR v1.1 fields: `secure9p.tags_per_session`, `secure9p.batch_frames`, `secure9p.short_write.policy`. Validation ensures `tags_per_session >= 1` and total batched payload stays ≤ negotiated `msize`.
- Docs: `docs/SECURE9P.md` updated to describe the new layering and concurrency knobs; `docs/INTERFACES.md` documents acknowledgement semantics for batched operations.

**Commands**
- `cargo test -p secure9p-codec`
- `cargo test -p secure9p-core`
- `cargo test -p nine-door`
- `cargo test -p tools/coh-rtc` (regenerates manifest snippets with new fields)
- `cargo run -p cohsh --features tcp -- --transport tcp --script tests/cli/9p_batch.cohsh`

**Checks (DoD)**
- Synthetic load (10k interleaved operations across four sessions) completes without tag reuse violations or starvation; metrics expose queue depth and retry counts.
- Batched frames round-trip within negotiated `msize`; when the manifest disables batching the same tests pass with single-frame semantics.
- Short-write retry policies (e.g., exponential back-off) are enforced according to manifest configuration and verified by CLI regression output.

**Compiler touchpoints**
- `coh-rtc` emits concurrency defaults into generated Rust tables and CLI fixtures; docs snippets pull from the manifest rather than hard-coded prose.
- CI regeneration guard ensures manifest-driven tests fail if concurrency knobs drift between docs and code.

---

## Milestone 10 — Telemetry Rings & Cursor Resumption

**Why now (compiler):** Persistent telemetry is currently mock-only. Operators need bounded append-only logs with resumable cursors, generated from the manifest so memory ceilings and schemas stay auditable.

**Goal**
Implement ring-backed telemetry providers with manifest-governed sizes and CBOR frame schemas.

**Deliverables**
- `apps/nine-door/src/host/telemetry/` (new module) housing ring buffer implementation (`ring.rs`) and cursor state machine (`cursor.rs`), integrated into `namespace.rs` and `control.rs` so workers emit telemetry via append-only files.
- `crates/secure9p-core` gains append-only helpers enforcing offset semantics and short-write signalling consumed by the ring provider.
- CBOR Frame v1 schema defined in `tools/coh-rtc/src/codegen/cbor.rs`, exported as Markdown to `docs/INTERFACES.md` and validated by serde-derived tests.
- CLI regression `tests/cli/telemetry_ring.cohsh` exercising wraparound, cursor resume, and offline replay via `cohsh --features tcp`.
- Manifest IR v1.2 fields: `telemetry.ring_bytes_per_worker`, `telemetry.frame_schema`, `telemetry.cursor.retain_on_boot`. Validation ensures aggregate ring usage fits within the event-pump budget declared in `docs/ARCHITECTURE.md`.
- `apps/root-task/src/generated/bootstrap.rs` extended to publish ring quotas and file descriptors consumed by the event pump.

**Commands**
- `cargo test -p nine-door`
- `cargo test -p secure9p-core`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
- `cargo run -p cohsh --features tcp -- --transport tcp --script tests/cli/telemetry_ring.cohsh`

**Checks (DoD)**
- Rings wrap without data loss; on reboot the cursor manifest regenerates identical ring state and CLI replay resumes exactly where it left off.
- Latency metrics (P50/P95) captured during tests and recorded in `docs/SECURITY.md`, sourced from automated output instead of manual measurements.
- Attempts to exceed manifest-declared ring quotas are rejected and logged; CI asserts the rejection path.

**Compiler touchpoints**
- Codegen emits ring metadata for `/proc/boot` so operators can inspect per-worker quotas; docs pull from the generated JSON to avoid drift.
- Regeneration guard verifies that CBOR schema excerpts in docs match compiler output.

---

## Milestone 11 — Sharded Namespaces & Provider Split

**Why now (compiler):** Scaling beyond hundreds of workers will otherwise bottleneck on single-directory namespaces. Deterministic sharding keeps walk depth bounded and aligns provider routing with manifest entries.

**Goal**
Introduce manifest-driven namespace sharding with optional legacy aliases.

**Deliverables**
- Namespace layout `/shard/<00..ff>/worker/<id>/…` generated from manifest fields. `apps/nine-door/src/host/namespace.rs` grows a `ShardLayout` helper that maps worker IDs to providers using manifest-supplied shard count and alias flags.
- `apps/nine-door/tests/shard_scale.rs` spins 1k worker directories, measuring attach latency and ensuring aliasing (when enabled) doesn't exceed walk depth (≤ 8 components).
- `crates/secure9p-core` exposes a sharded fid table ensuring per-shard locking and eliminating global mutex contention.
- Manifest IR v1.2 additions: `sharding.enabled`, `sharding.shard_bits`, `sharding.legacy_worker_alias`. Validation enforces `shard_bits ≤ 8` and forbids aliases when depth would exceed limits.
- Docs updates in `docs/ROLES_AND_SCHEDULING.md` describing shard hashing (`sha256(worker_id)[0..=shard_bits)`), alias behaviour, and operational guidance.

**Commands**
- `cargo test -p nine-door`
- `cargo test -p secure9p-core`
- `cargo test -p tests --test shard_1k`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`

**Checks (DoD)**
- 1k worker sessions attach concurrently without starvation; metrics exported via `/proc/9p/sessions` demonstrate shard distribution.
- Enabling legacy aliases preserves `/worker/<id>` paths for backwards compatibility; disabling them causes the compiler to reject manifests that still reference legacy paths.

**Compiler touchpoints**
- Generated bootstrap code publishes shard tables for the event pump and NineDoor bridge; docs consume the same tables.
- Manifest regeneration updates CLI fixtures so scripted tests reference shard-aware paths automatically.

---

## Milestone 12 — Client Concurrency & Session Pooling

**Why now (compiler):** Server-side pipelining is useless unless the CLI and automation harness can take advantage of it safely. Manifest-driven client policy keeps retries and pooling deterministic across deployments.

**Goal**
Add pooled sessions and retry policies to `cohsh`, governed by compiler-exported policy files.

**Deliverables**
- `apps/cohsh/src/lib.rs` extends `Shell` with a session pool (default manifest value: two control, four telemetry) and batched Twrite helper. `apps/cohsh/src/transport/tcp.rs` gains retry scheduling based on manifest policy.
- `apps/cohsh/tests/pooling.rs` verifies pooled throughput and idempotent retry behaviour.
- Manifest IR v1.3: `client_policies.cohsh.pool`, `client_policies.retry`, `client_policies.heartbeat`. Compiler emits `out/cohsh_policy.toml` consumed at runtime (CLI loads it on start, failing if missing/out-of-sync).
- CLI regression `tests/cli/session_pool.cohsh` demonstrating increased throughput under load and safe recovery from injected failures.
- Docs (`docs/USERLAND_AND_CLI.md`) describe new CLI flags/env overrides, referencing manifest-derived defaults.

**Commands**
- `cargo test -p cohsh`
- `cargo run -p cohsh --features tcp -- --transport tcp --script tests/cli/session_pool.cohsh`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`

**Checks (DoD)**
- Throughput benchmark (documented in test output) demonstrates improvement relative to single-session baseline without exceeding `msize` or server tag limits.
- Retry logic proves idempotent: injected short-write failures eventually succeed without duplicating telemetry or exhausting tickets.
- CLI refuses to start when the manifest policy hash mismatches the compiled defaults.

**Compiler touchpoints**
- `coh-rtc` emits policy TOML plus hash recorded in docs/tests; regeneration guard compares CLI-consumed hash with manifest fingerprint.
- Docs embed CLI defaults via compiler-generated snippets to avoid drift.

---

## Milestone 13 — Observability via Files (No New Protocols)

**Why now (compiler):** Operators need structured observability without adding new protocols inside the VM. Manifest-defined `/proc` endpoints ensure metrics stay aligned with runtime behaviour.

**Goal**
Expose audit-friendly observability nodes under `/proc` generated from the manifest.

**Deliverables**
- `apps/nine-door/src/host/observe.rs` (new module) providing read-only providers for `/proc/9p/{sessions,outstanding,short_writes}` and `/proc/ingest/{p50_ms,p95_ms,backpressure,dropped,queued}` plus append-only `/proc/ingest/watch` snapshots.
- Event pump updates (`apps/root-task/src/event/mod.rs`) to update ingest metrics without heap allocation; telemetry forwarded through generated providers.
- Unit tests covering metric counters and ensuring no allocations on hot paths; CLI regression `tests/cli/observe_watch.cohsh` tails `/proc/ingest/watch` verifying stable grammar.
- Manifest IR v1.3 fields: `observability.proc_9p` and `observability.proc_ingest` enabling individual nodes and documenting retention policies. Validation enforces bounded buffer sizes.
- Docs: `docs/SECURITY.md` gains monitoring appendix sourced from manifest snippets; `docs/INTERFACES.md` documents output grammar.

**Commands**
- `cargo test -p nine-door`
- `cargo test -p root-task`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
- `cargo run -p cohsh --features tcp -- --transport tcp --script tests/cli/observe_watch.cohsh`

**Checks (DoD)**
- Stress harness records accurate counters; metrics exported via `/proc` match expected values within tolerance.
- CLI tail output remains parseable and line-oriented; regression test asserts exact output grammar.
- Compiler rejects manifests that attempt to enable observability nodes without allocating sufficient buffers.

**Compiler touchpoints**
- Generated code provides `/proc` descriptors; docs embed them via compiler output.
- As-built guard compares manifest-declared observability nodes with committed docs and fails CI if mismatched.

---

## Milestone 14 — Content-Addressed Updates (CAS) — 9P-first

**Why now (compiler):** Upcoming edge deployments need resumable, verifiable updates without bloating the VM with new protocols. Manifest-governed CAS ensures integrity rules and storage budgets remain enforceable.

**Goal**
Provide CAS-backed update distribution via NineDoor with compiler-enforced integrity policies.

**Deliverables**
- `apps/nine-door/src/host/cas.rs` implementing a CAS provider exposing `/updates/<epoch>/{manifest.cbor,chunks/<hash>}` with optional delta packs. Provider enforces SHA-256 chunk integrity and optional Ed25519 signatures when manifest enables `cas.signing`.
- Host tooling `apps/cas-tool/` (new crate) packaging update bundles, generating manifests, and uploading via Secure9P.
- CLI regression `tests/cli/cas_roundtrip.cohsh` verifying download resume, signature enforcement, and delta replay.
- Manifest IR v1.4 fields: `cas.enable`, `cas.store.chunk_bytes`, `cas.delta.enable`, `cas.signing.key_path`. Validation ensures chunk size ≤ negotiated `msize` and signing keys present when required.
- Docs: `docs/INTERFACES.md` describes CAS grammar, delta rules, and operational runbooks sourced from compiler output; `docs/SECURITY.md` records threat model.

**Commands**
- `cargo test -p nine-door`
- `cargo test -p cas-tool`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
- `cargo run -p cohsh --features tcp -- --transport tcp --script tests/cli/cas_roundtrip.cohsh`

**Checks (DoD)**
- Resume logic validated via regression script; delta application is idempotent and verified by hashing installed payloads before/after.
- Signing path tested with fixture keys; unsigned mode explicitly documented and requires manifest acknowledgement (e.g., `cas.signing.required = false`).
- Compiler rejects manifests where CAS storage exceeds event-pump memory budgets or chunk sizes exceed `msize`.

**Compiler touchpoints**
- Codegen emits CAS provider tables and host-tool manifest templates; docs ingest the same JSON to prevent drift.
- Regeneration guard checks CAS manifest fingerprints against committed artefacts.

---

## Milestone 15 — UEFI Bare-Metal Boot & Device Identity

**Why now (context):** To meet hardware deployment goals (Edge §3 retail hubs, Edge §8 defense ISR, Security §12 segmentation) we must boot on physical aarch64 hardware with attested manifests while preserving the lean `no_std` footprint.

**Goal**
Deliver a UEFI boot path that loads the generated manifest, performs TPM-backed identity attestation, and mirrors VM behaviour.

**Deliverables**
- New crate `apps/root-task-uefi/` (or feature within `apps/root-task`) building a PE/COFF binary that invokes the same generated bootstrap tables without introducing `std`. Integration with `sel4-runtime` remains feature-gated.
- `scripts/make_uefi_image.py` packaging FAT images containing `EFI/BOOT/BOOTAA64.EFI`, generated manifest JSON, manifest hash file, and rootfs CPIO. Script emits reproducible build logs captured in CI artefacts.
- Identity subsystem leveraging TPM 2.0 (or DICE fallback) to seal capability ticket seeds; attestation statements appended to `/proc/boot` and exported via NineDoor for remote verification.
- Secure Boot guidance aligning with `docs/SECURITY.md` red lines; measurement documentation and manifest fingerprints captured in `docs/HARDWARE_BRINGUP.md`.
- Manifest IR v1.4 fields: `hardware.profile = "uefi_aarch64"`, `hardware.devices[]` (UART, NET, TPM, RTC), `hardware.secure_boot`, `hardware.attestation`. Validation enforces required MMIO addresses and TPM availability when attestation is enabled.
- Host automation updates (`scripts/qemu-run.sh --uefi`) plus lab checklist for bring-up on the reference dev board.

**Commands**
- `cargo build -p root-task-uefi --target aarch64-unknown-uefi`
- `python scripts/make_uefi_image.py --manifest out/manifests/root_task_resolved.json`
- `scripts/qemu-run.sh --uefi --console serial --tcp-port 31337`
- Physical hardware checklist: run attestation script, capture `/proc/boot` output, and compare manifest hash to CI baseline.

**Checks (DoD)**
- UEFI image boots under QEMU TCG and on the reference dev board; serial output matches VM baseline including manifest fingerprint and ticket registration lines.
- TPM-backed attestation chain exported via `/proc/boot` matches manifest hash and does not leak secret material; failure to access TPM causes boot abort with audited error.
- Compiler rejects manifests selecting the UEFI profile without providing required hardware bindings or attestation settings.

**Compiler touchpoints**
- `coh-rtc` emits hardware tables for the selected profile; docs import them for `docs/HARDWARE_BRINGUP.md` and `docs/ARCHITECTURE.md`.
- Regeneration guard compares manifest fingerprints recorded in UEFI docs against generated outputs, failing CI on drift.

---

## Milestone 16 — Field Bus & Low-Bandwidth Sidecars (Host/Worker Pattern)

**Why now (context):** Remaining edge use cases (Edge §§1–4,8,9; Science §§13–14) depend on deterministic adapters for industrial buses and constrained links. Implementing them as sidecars preserves the lean `no_std` core while meeting operational demands.

**Goal**
Deliver a library of host/worker sidecars (outside the VM where possible) that bridge MODBUS/DNP3, LoRa, and sensor buses into NineDoor namespaces, driven by compiler-declared mounts and capability policies.

**Deliverables**
- Host-side sidecar framework (`apps/sidecar-bus`) offering async runtimes on macOS/Linux with feature gates to keep VM artefacts `no_std`. Sidecars communicate via Secure9P transports or serial overlays without embedding TCP servers in the VM.
- Worker templates (`apps/worker-bus`, `apps/worker-lora`) that run inside the VM, remain `no_std`, and expose control/telemetry files (`/bus/*`, `/lora/*`) generated from manifest entries.
- Scheduling integration for LoRa duty-cycle management and tamper logging, aligned with `docs/USE_CASES.md` defense and science requirements.
- Compiler IR v1.5 fields `sidecars.modbus`, `sidecars.dnp3`, `sidecars.lora` describing mounts, baud/link settings, and capability scopes; validation ensures resources stay within event-pump budget.
- Documentation updates (`docs/ARCHITECTURE.md §12`, `docs/INTERFACES.md`) illustrating the sidecar pattern, security boundaries, and testing strategy.

**Use-case alignment**
- Industrial IoT gateways (Edge §1) gain MODBUS/CAN integration without bloating the VM.
- Energy substations (Edge §2) receive DNP3 scheduling and signed config updates.
- Defense ISR kits (Edge §8) use LoRa scheduler + tamper logging, while environmental stations (Science §13) benefit from low-power telemetry scheduling.

**Commands**
- `cargo test -p worker-bus -p worker-lora`
- `cargo test -p sidecar-bus --features modbus,dnp3`
- `cohsh --script tests/cli/sidecar_integration.coh`

**Checks (DoD)**
- Sidecars operate within declared capability scopes; attempts to access undeclared mounts are rejected and logged.
- LoRa scheduler enforces duty-cycle constraints under stress tests.
- Offline telemetry spooling validated for MODBUS/DNP3 adapters with manifest-driven limits.

**Compiler touchpoints**
- IR v1.5 ensures mounts/roles/quotas for sidecars, generating documentation tables and manifest fragments consumed by host tooling.
- Validation prevents enabling sidecars without corresponding host dependencies or event-pump capacity.

---

### Docs-as-Built Alignment (applies to Milestone 8 onward)

To prevent drift:

1. **Docs → IR → Code**
   - Any new behaviour MUST land as IR fields with validation and codegen.
   - Build fails if IR references disabled gates, violates Secure9P bounds, or forces `std` where the runtime is `no_std`.

2. **Autogenerated Snippets**
   - `coh-rtc` refreshes embedded snippets in `SECURE9P.md`, `INTERFACES.md`, and `ARCHITECTURE.md` (CBOR schema, `/proc` tree, concurrency knobs, hardware tables) during release prep.

3. **As-Built Guard**
   - Script compares generated file hashes, manifest fingerprints, and doc excerpts against committed versions. Drift fails CI and blocks release notes.
   - Rule: **Documentation must describe the system “as built”** (post-codegen), not only “as intended”.

4. **Red Lines**
   - Enforced in the compiler and restated here: 9P2000.L, `msize ≤ 8192`, walk depth ≤ 8, no `..`, no fid reuse after clunk, no TCP listeners inside VM unless feature-gated and documented, CPIO < 4 MiB, no POSIX façade, maintain `no_std` for VM artefacts.

