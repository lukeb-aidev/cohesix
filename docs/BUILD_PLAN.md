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
- Retype now targets the init root CNode with `(node_index=0, node_depth=0, slot)` and validates capacity via `initThreadCNodeSizeBits`.

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


## Milestone 8C — Root-Task Compiler v0 (“coh-rtc”)

**Why now (context):** M0–M7d stabilized boot, roles, NineDoor, console, and TCP/serial surfaces. To prevent drift as we scale concurrency and storage, we “freeze” today’s behavior into a tiny, declarative IR that compiles to `bootstrap.rs`, a resolved manifest, and CLI tests. From here on, **docs → IR → codegen** is the single source of truth.

**Goal**  
Introduce a minimal **root-task compiler** (`tools/coh-rtc`) that ingests `root_task.toml` and generates:
- `apps/root-task/src/generated/bootstrap.rs` (init, providers, policy)
- `out/manifests/root_task_resolved.json`
- `tests/cli/boot_v0.cohsh` (cat `/proc/boot`, append `/queen/ctl`)

**Changes**  
- IR v1.0: system (arch, tick), Secure9P limits (msize ≤ 8192, max walk ≤ 8, forbid `..`, no FID reuse), roles, mounts/providers.  
- Validation: fail build on bounds violations or path escapes.  
- Codegen: deterministic, sorted output; “GENERATED – do not edit” banner.

**Commands**  
- `cargo run -p coh-rtc -- root_task.toml --out apps/root-task/src/generated`  
- `cohsh --script tests/cli/boot_v0.cohsh`

**Checks (DoD)**  
- Deterministic regen (same IR ⇒ identical output hash).  
- QEMU script passes `cat /proc/boot` and `echo` to `/queen/ctl`.  
- Compiler rejects IR that would violate red lines (size/walk/escapes).

**Deliverables**  
- `tools/coh-rtc` crate, example `root_task.toml`, generated files + CLI script.

**Compiler touchpoints**  
- Adds `root_task.schema = "1.0"`.  
- Emits resolved manifest used by later milestones for planning and tests.

---

## Milestone 9 — 9P Pipelining & Batching (foundational concurrency)

**Why now (compiler):** M9 introduces many in-flight tags + batched frames **without changing 9P**. The compiler must gate and validate these knobs so runtime remains bounded and reproducible.

**Goal**  
Enable high concurrency via **N outstanding tags per session** and optional **batched frames** within `msize`.

**Changes**  
- `secure9p-core`: multiple outstanding tags; out-of-order `R*` support.  
- `secure9p-codec`: zero-copy frame iterator for length-prefixed sequences.  
- `nine-door`: provider API accepts frame slices; uses short-write for backpressure.

**Commands**  
- `cargo test -p secure9p-core -p secure9p-codec -p nine-door`

**Checks (DoD)**  
- Load test: 10k interleaved ops across 4 sessions; correct tag matching.  
- 4-frame batch round-trips; optional CRC documented.  
- Short-write path exercised; client retries tail reliably.

**Deliverables**  
- `SECURE9P.md` updated with concurrency model and short-write guidance.

**Compiler touchpoints**  
- IR v1.1 adds:  
  - `secure9p.tags_per_session: u16` (default 32)  
  - `secure9p.batch_frames: bool` (default true)  
  - `secure9p.short_write_policy: { enable: bool, backoff_ms: u32 }`  
- Validation: frame batch total ≤ `msize`; tags_per_session > 0.  
- Codegen: transport/tag window config + batched Twrite tests (`tests/cli/9p_batch.cohsh`).

---

## Milestone 10 — Telemetry Rings & Cursors (bounded append-only)

**Why now (compiler):** Moves from unbounded files to **bounded rings** with **persistent cursors**. The compiler ensures memory budgets and path safety.

**Goal**  
Ring-backed `/worker/<id>/telemetry` with **per-consumer cursor** and **CBOR Frame v1**.

**Changes**  
- Provider: ring (4–16 MB configurable) + backpressure via short-write.  
- Cursor file `/worker/<id>/cursor` (RW, u64) to resume reads (by `seq`).  
- CBOR Frame v1: `{seq:u64, ts:u64, kind:u8, payload:bytes, meta?:map}`.

**Commands**  
- `cargo test -p nine-door`  
- `cohsh --script tests/cli/telemetry_ring.coh`

**Checks (DoD)**  
- Wrap without crash; cursor resume never regresses; backpressure asserted.  
- Latency budget documented (P50/P95).

**Deliverables**  
- `INTERFACES.md` updated with CBOR schema v1, cursor semantics.

**Compiler touchpoints**  
- IR v1.2 adds:  
  - `telemetry.ring_bytes_per_worker: u32` (power-of-two)  
  - `telemetry.frame_schema: "cbor_v1"`  
- Validation: memory budget + walk depth constraints.  
- Codegen: ring provider scaffolding + schema in `generated/schema.rs`; wrap/resume tests.

---

## Milestone 11 — Sharded Namespaces & Provider Split (scale-out)

**Why now (compiler):** Shards must be derived deterministically and remain within path depth limits; compiler generates the provider instances and optional aliasing.

**Goal**  
Reduce contention and FID pressure by sharding worker trees.

**Changes**  
- Layout: `/shard/<00..ff>/worker/<id>/*`, with legacy `/worker/<id>` alias.  
- One provider per shard; hash `hh = sha256(worker_id)[0..=1]`.

**Commands**  
- `cargo test -p nine-door`  
- `cargo test -p tests --test shard_1k` (if present)

**Checks (DoD)**  
- 1k workers attach and write concurrently; near-linear scale w/ shards.  
- Walk depth ≤ 8; no hot-path global locks.

**Deliverables**  
- `SECURE9P.md` & `ROLES_AND_SCHEDULING.md` updated with sharding rules.

**Compiler touchpoints**  
- IR v1.2 adds:  
  - `sharding.enable: bool`  
  - `sharding.scheme: { kind: "hex_prefix", width: 2 }`  
  - `sharding.legacy_worker_alias: bool`  
- Validation: width ⇒ shard count; depth checks; forbid global provider locks.  
- Codegen: per-shard provider tables + optional alias nodes; scale test scaffold.

---

## Milestone 12 — Client Concurrency & Session Pooling (reference clients)

**Why now (compiler):** Provide **client-side policy** alongside server features so operators can use concurrency safely; compiler emits reference policy/config.

**Goal**  
Expose a session pool in `cohsh` and helpers for batched writes + retry.

**Changes**  
- `cohsh`: pool (e.g., 2 control + 4 telemetry), Twrite batch builder, retry on short-write; sample worker agent.

**Commands**  
- `cargo test -p cohsh`  
- `cohsh --script tests/cli/session_pool.coh`

**Checks (DoD)**  
- Target throughput at `msize ≤ 8192` via concurrency (not bigger frames).  
- Idempotent recovery on transient failures (by `seq`).

**Deliverables**  
- `USERLAND_AND_CLI.md` updated with pooling guidance.

**Compiler touchpoints**  
- IR v1.2 adds:  
  - `client_policies.cohsh.pool: { control: u8, telemetry: u8 }`  
  - `client_policies.retry: { short_write: { max_retries:u8, backoff_ms:u32 } }`  
- Codegen: output `out/cohsh_policy.toml` and example worker using the pool.

---

## Milestone 13 — Observability via Files (no new protocols)

**Why now (compiler):** Observability remains 9P-native. The compiler enumerates proc nodes to keep docs, code, and tests aligned.

**Goal**  
Expose runtime metrics/backpressure as files under `/proc/*`.

**Changes**  
- `/proc/9p/{sessions,outstanding,short_writes}`  
- `/proc/ingest/{p50_ms,p95_ms,backpressure,dropped,queued}`  
- `/proc/ingest/watch` periodic snapshots (append-only)

**Commands**  
- `cargo test -p nine-door`  
- `cohsh tail /proc/ingest/watch`

**Checks (DoD)**  
- Counters track stress accurately; `watch` parsable; no hot-path allocations.

**Deliverables**  
- `SECURE9P.md` monitoring appendix; ops runbook snippet.

**Compiler touchpoints**  
- IR v1.2 adds:  
  - `observability.proc_9p: [...]`  
  - `observability.proc_ingest: { fields:[...], watch_interval_ms:u32 }`  
- Codegen: read-only providers + `watch` cadence; sample `tail` test.

---

## Milestone 14 — Content-Addressed Updates (CAS) — 9P-first

**Why now (compiler):** CAS layout, chunk sizes, and integrity rules must be validated and generated centrally to avoid drift and HTTP creep inside the VM.

**Goal**  
Serve model/content updates over 9P with content addressing and optional signatures.

**Changes**  
- Trait `CasStore` with `put(bytes)->hash`, `get(hash)`.  
- `/updates/<epoch>/{manifest.cbor,chunks/<hash>}` layout; delta packs; integrity via hash (optional signatures).

**Commands**  
- `cargo test -p nine-door`  
- `cohsh cat /updates/<e>/manifest.cbor | cbor2json`

**Checks (DoD)**  
- Resume after disconnect using existing 9P paths; chunk integrity verified.  
- Manifest round-trips; delta application test passes.

**Deliverables**  
- `INTERFACES.md` section for CAS format & delta rules.

**Compiler touchpoints**  
- IR v1.3 adds:  
  - `cas.enable: bool`  
  - `cas.store: { root: "/updates", chunk_bytes: u32, hash: "sha256", sign?: { algo: "ed25519", pubkey_path: "…" } }`  
  - `cas.delta.enable: bool`  
- Validation: chunk_bytes fits `msize` envelopes; key presence when signing enabled.  
- Codegen: CAS provider layout + host `cas_tool` packing rules (doc’d); resume tests.

---

### Docs-as-Built Alignment (applies to M8C onward)

To prevent drift:

1. **Docs → IR → Code**  
   - Any new behavior MUST land as IR fields with validation and codegen.  
   - Build fails if IR references disabled gates or violates Secure9P bounds.

2. **Autogenerated Snippets**  
   - `coh-rtc` refreshes embedded snippets in `SECURE9P.md`/`INTERFACES.md` (CBOR schema, `/proc` tree, concurrency knobs) during release prep.

3. **As-Built Guard**  
   - A simple check (scripted) compares generated file hashes and resolved manifest fields to the examples embedded in docs. If mismatched, CI/docs review fails.  
   - Rule: **Documentation must describe the system “as built”** (post-codegen), not only “as intended”.

4. **Red Lines**  
   - Enforced in the compiler and restated here: 9P2000.L, `msize ≤ 8192`, walk depth ≤ 8, no `..`, no FID reuse after clunk, no TCP listeners inside VM unless feature-gated and documented, CPIO < 4 MB, no POSIX façade.

