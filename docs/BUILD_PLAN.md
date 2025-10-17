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
- Retype now targets the init root CNode with `(node_index=seL4_CapInitThreadCNode, node_depth=0, slot)` and validates capacity via `initThreadCNodeSizeBits`.

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


## Milestone 8 — Root-Task Compiler & Deterministic Profiles

**Why now (context):** Milestones 0–7d stabilised the execution core, but the path to the use cases in `docs/USE_CASES.md` demands a deterministic configuration layer that keeps `no_std` guarantees intact while preventing namespace or policy drift. A tiny IR-driven compiler lets us regenerate the root task, documentation, and tests from a single artefact so smart-factory, energy, and fintech operators can audit what actually runs.

**Goal**
Introduce the **root-task compiler** (`tools/coh-rtc`) that ingests `root_task.toml` and emits deterministic artefacts with explicit `no_std` validation hooks.

**Deliverables**
- IR v1.0 covering system profile (arch, tick), Secure9P bounds (msize ≤ 8192, walk depth ≤ 8, forbid `..`, disallow fid reuse), initial providers, and role budgets. The schema must capture the event-pump wiring noted in `docs/ARCHITECTURE.md §2-§3` so generated code preserves cooperative scheduling semantics.
- Validation pipeline that refuses builds when bounds, path hygiene, or feature gates would violate red lines or require `std`. This includes a `cargo check -p root-task --no-default-features` guard after generation.
- Code generation producing:
  - `apps/root-task/src/generated/bootstrap.rs` (init graph + policy tables, `#![no_std]` annotated).
  - `out/manifests/root_task_resolved.json` (audit trail consumed by docs/tests).
  - `tests/cli/boot_v0.cohsh` (baseline CLI regression) and a reproducible hash recorded beside the manifest.
- Documentation glue: compiler README, IR reference in `docs/ARCHITECTURE.md §11`, and task template linkage so planners can quote schema fields directly.

**Use-case alignment**
- Smart-factory gateways (Use Case §Edge 1) and HSM-adjacent signing (Security §11) need auditable boot manifests and deterministic policy regen to satisfy certification reviews.
- Energy micro-grids (Edge §2) rely on the manifest to prove walk limits, telemetry mounts, and ticket budgets remain within regulatory envelopes.

**Commands**
- `cargo run -p coh-rtc -- root_task.toml --out apps/root-task/src/generated`
- `cargo check -p root-task --no-default-features`
- `cohsh --script tests/cli/boot_v0.cohsh`

**Checks (DoD)**
- Regenerating artefacts twice without touching IR yields identical hashes and Rustfmt-stable output.
- QEMU regression passes `cat /proc/boot` and append-only `/queen/ctl` writes using generated bootstrap code.
- Compiler rejects manifests that would exceed Secure9P bounds, introduce path escapes, or enable features unavailable under `no_std`/`alloc`.

**Compiler touchpoints**
- Adds `root_task.schema = "1.0"`, ties schema version to docs, and emits manifest metadata consumed by the future as-built guard.
- Generates a `generated/mod.rs` index annotated with “GENERATED – do not edit” comments so auditors can diff artefacts cleanly.
- Captures feature toggles (`net`, `telemetry`, `gpu`) for later hardware planning without enabling them implicitly.

---

## Milestone 9 — 9P Pipelining & Batching (Foundational Concurrency)

**Why now (compiler):** High-throughput telemetry and remote operations (Edge §§3–5, Telco MEC, Retail vision) require multiple inflight requests without abandoning strict bounds. Compiler-enforced knobs let us dial concurrency per deployment while preserving determinism.

**Goal**
Enable multiple outstanding tags per session and optional batched frames while keeping Secure9P semantics and `no_std` constraints intact.

**Deliverables**
- `secure9p-core`: tag window manager supporting out-of-order `R*` replies with bounded alloc usage; property tests validating tag reuse and error handling.
- `secure9p-codec`: zero-copy iterator over batched Twrite/Tread frames, instrumented for fuzzing and tolerant of partial consumption.
- `nine-door`: provider API updates to accept frame slices, apply short-write backpressure, and expose queue metrics to `/proc/9p/*` once Milestone 13 lands.
- Compiler extensions for IR v1.1 capturing `tags_per_session`, `batch_frames`, and `short_write_policy` defaults plus docs cross-references.

**Use-case alignment**
- Logistics telemetry (Edge §4) and MEC orchestration (Edge §5) depend on deep pipelines to keep heartbeats and GPU leases responsive under load.
- Smart-city sensing (Edge §9) benefits from batched small sensor payloads without increasing `msize`.

**Commands**
- `cargo test -p secure9p-core -p secure9p-codec -p nine-door`
- `cargo test -p secure9p-core --features fuzz -- lib::batch_roundtrip` (feature-gated corpus replay)
- `cargo check -p nine-door --no-default-features`

**Checks (DoD)**
- Load test script drives ≥10k interleaved ops across four sessions with no tag mis-match or starvation.
- Batched frames round-trip within negotiated `msize` envelopes; optional CRC path documented and fuzzed.
- Short-write retry path verified from `cohsh`, including exponential back-off using IR-configured policy.

**Compiler touchpoints**
- IR v1.1 introduces concurrency knobs with validation to keep total batch bytes ≤ `msize` and tag counts ≥ 1.
- Codegen refreshes CLI samples (`tests/cli/9p_batch.cohsh`) and updates docs automatically when knob defaults change.

---

## Milestone 10 — Telemetry Rings & Cursors (Bounded Append-Only)

**Why now (compiler):** Offline-first sectors (Edge §§1,4,7; Science §13) need durable telemetry without unbounded growth. Rings with explicit cursors give deterministic memory and resumable reads consistent with the security posture.

**Goal**
Deliver ring-backed telemetry files with CBOR Frame v1 payloads and resumable cursors governed by the compiler schema.

**Deliverables**
- `nine-door` provider implementing fixed-size rings (4–16 MiB power-of-two) with short-write signalling and per-session cursors (`/worker/<id>/cursor`).
- CBOR Frame v1 spec: `{seq:u64, ts:u64, kind:u8, payload:bytes, meta?:map}` with schema excerpt embedded into `docs/INTERFACES.md` and regenerated from compiler output.
- CLI regression `tests/cli/telemetry_ring.coh` covering wraparound, cursor resume, and offline replay.
- Compiler IR v1.2 additions for `telemetry.ring_bytes_per_worker` and `telemetry.frame_schema`, with validation for aggregate RAM budgets vs. `docs/ARCHITECTURE.md` allowances.

**Use-case alignment**
- Logistics and ports (Edge §4) require spool-and-forward semantics with bounded memory.
- Autonomous depots (Edge §7) rely on resumable telemetry to sync after connectivity gaps.

**Commands**
- `cargo test -p nine-door`
- `cohsh --script tests/cli/telemetry_ring.coh`
- `cargo check -p nine-door --no-default-features`

**Checks (DoD)**
- Rings wrap without data loss; cursor resume validated across restarts and manifest regenerations.
- Latency budget (P50/P95) documented in `docs/SECURITY.md` with measurements from automated tests.

**Compiler touchpoints**
- Codegen emits ring provider scaffolding (`generated/schema.rs`) and ensures aggregate ring memory stays within IR-declared cap.
- Docs snippets for CBOR schema are sourced from compiler artefacts to avoid drift.

---

## Milestone 11 — Sharded Namespaces & Provider Split (Scale-Out)

**Why now (compiler):** Multi-tenant scheduling (Edge §5 Telco MEC) and healthcare telemetry (Security §12) need predictable namespace contention. Deterministic sharding keeps walk depth bounded while enabling thousands of workers.

**Goal**
Partition worker trees into deterministic shards with compiler-managed provider tables and legacy aliasing for compatibility.

**Deliverables**
- Namespace layout `/shard/<00..ff>/worker/<id>/*` with optional `/worker/<id>` symlink/alias preserved when configured.
- Per-shard provider instances with hash-based routing `hh = sha256(worker_id)[0..=1]`, fully documented in `docs/ROLES_AND_SCHEDULING.md`.
- Load/regression tests (`tests/shard_1k.rs`) demonstrating near-linear scaling without global locks.
- Compiler IR v1.2 fields `sharding.enable`, `sharding.scheme`, `sharding.legacy_worker_alias`, plus validation for depth ≤ 8.

**Use-case alignment**
- Telco MEC orchestrator (Edge §5) requires per-tenant shard isolation and predictable capacity planning.
- Retail CV hubs (Edge §3) benefit from sharding when many camera pipelines attach concurrently.

**Commands**
- `cargo test -p nine-door`
- `cargo test -p tests --test shard_1k`

**Checks (DoD)**
- 1k worker sessions attach and stream telemetry concurrently without violating event-pump fairness.
- Legacy aliases continue to function when enabled and are banned by compiler validation when disabled.

**Compiler touchpoints**
- Codegen writes per-shard provider maps and ensures alias nodes do not increase walk depth beyond limits.
- Scale-test harness seeded with manifest data so docs and tests stay aligned.

---

## Milestone 12 — Client Concurrency & Session Pooling (Reference Clients)

**Why now (compiler):** Operators need safe concurrency defaults when taking advantage of server pipelining. Embedding policy outputs from the compiler keeps CLI behaviour reproducible across deployments.

**Goal**
Ship pooled sessions, batched writers, and retry helpers in `cohsh`, backed by compiler-generated policy files and tests.

**Deliverables**
- `cohsh`: session pool (e.g., 2 control + 4 telemetry), Twrite batch builder, retry/back-off logic for short-write responses, and a sample worker agent demonstrating the pattern.
- CLI regression script `tests/cli/session_pool.coh` validating throughput improvements without exceeding `msize`.
- Compiler IR v1.2 fields for `client_policies.cohsh.pool` and `client_policies.retry`, with generated `out/cohsh_policy.toml` consumed by the CLI at runtime.
- Documentation updates in `docs/USERLAND_AND_CLI.md` mapping policy knobs to manifest fields and providing ops guidance.

**Use-case alignment**
- Broadcast signage (Edge §10) and smart-city sensing (Edge §9) rely on high-throughput uploads and resilient retries.
- Secure OTA lab appliance (Developer §15) needs deterministic scripts for demonstrating policy enforcement.

**Commands**
- `cargo test -p cohsh`
- `cohsh --script tests/cli/session_pool.coh`

**Checks (DoD)**
- Target throughput achieved at `msize ≤ 8192` using concurrency, not enlarged frames.
- Retry logic proves idempotent recovery from injected short-write failures.

**Compiler touchpoints**
- Generated policy files referenced in docs/tests; manifest validation ensures pool sizes remain within Secure9P tag budgets.

---

## Milestone 13 — Observability via Files (No New Protocols)

**Why now (compiler):** Compliance-heavy sectors (Security §§11–12, Healthcare §6) require auditable metrics without introducing new protocols. Observability nodes must be generated from the manifest to stay in lockstep with runtime behaviour.

**Goal**
Expose 9P-native observability endpoints under `/proc/*` with compiler-managed schema and documentation snippets.

**Deliverables**
- Read-only providers for `/proc/9p/{sessions,outstanding,short_writes}` and `/proc/ingest/{p50_ms,p95_ms,backpressure,dropped,queued}` plus append-only `/proc/ingest/watch` snapshots.
- Telemetry summariser tests exercising back-pressure counters and verifying zero heap allocation on hot paths.
- Ops runbook excerpts and monitoring appendix in `docs/SECURITY.md` sourced from compiler output.
- CLI `cohsh tail /proc/ingest/watch` regression covering typical operator flows.

**Use-case alignment**
- HSM gateways (Security §11) and OT/IT segmentation appliances (Security §12) require audit-ready metrics for change control.
- Healthcare imaging (Edge §6) must surface ingest latency for compliance.

**Commands**
- `cargo test -p nine-door`
- `cohsh tail /proc/ingest/watch`

**Checks (DoD)**
- Counters accurately reflect stress scenarios; `watch` output remains parseable and stable across releases.
- No additional allocations occur on hot telemetry paths; verified via instrumentation and unit tests.

**Compiler touchpoints**
- IR v1.2 fields `observability.proc_9p` and `observability.proc_ingest` define nodes/fields; codegen emits provider bindings and docs snippets.
- As-built guard compares generated observability schema with committed docs.

---

## Milestone 14 — Content-Addressed Updates (CAS) — 9P-first

**Why now (compiler):** Retail CV hubs, autonomous depots, and OTA lab appliances (Edge §§3,7; Developer §15) require signed, resumable updates without adding HTTP servers inside the VM. Compiler-managed layout prevents drift and enforces integrity red lines.

**Goal**
Serve model/content updates over 9P using content addressing, optional signatures, and deterministic chunk sizing validated by the compiler.

**Deliverables**
- Trait `CasStore` and NineDoor provider exposing `/updates/<epoch>/{manifest.cbor,chunks/<hash>}` with delta-pack support and resumable reads.
- Hash-based integrity checks (SHA-256 baseline) with optional Ed25519 signatures; host `cas_tool` packaging utility.
- CLI script `tests/cli/cas_roundtrip.coh` verifying resume after disconnect and manifest/delta application.
- Docs updates in `docs/INTERFACES.md` describing CAS file grammar, delta rules, and operational guidance.
- Compiler IR v1.3 fields `cas.enable`, `cas.store.*`, `cas.delta.enable`, with validation that chunk sizes fit `msize` and that signing keys are provided when required.

**Use-case alignment**
- Retail analytics (Edge §3) and signage control (Edge §10) need deterministic content updates with integrity proofs.
- Autonomous depots (Edge §7) benefit from resumable deltas over intermittent links.

**Commands**
- `cargo test -p nine-door`
- `cohsh cat /updates/<e>/manifest.cbor | cbor2json`
- `cargo test -p cas_tool`

**Checks (DoD)**
- Resume logic validated; delta application idempotent and integrity enforced.
- Signing path tested with fake keys; unsigned mode explicitly documented.

**Compiler touchpoints**
- Codegen emits CAS provider layout, host tooling manifest, and doc snippets. Validation rejects IR that would exceed storage budgets or break `no_std` guarantees.

---

## Milestone 15 — UEFI Bare-Metal Boot & Device Identity

**Why now (context):** To satisfy hardware deployment aspirations (Edge §3 retail hubs, Edge §8 defense ISR, Security §12 segmentation) we must run without QEMU, boot via aarch64 UEFI, and attest device identity while staying `no_std` and lean.

**Goal**
Produce a UEFI boot path for Cohesix on physical aarch64 hardware, integrating Secure Boot, TPM-backed device keys, and compiler-driven manifests so edge deployments mirror VM behaviour.

**Deliverables**
- UEFI loader crate (`apps/root-task-uefi` or module) building a PE/COFF binary that loads the same manifest-generated bootstrap code without introducing `std`.
- Boot packaging scripts creating a FAT image with `EFI/BOOT/BOOTAA64.EFI`, generated manifest, and rootfs CPIO, plus instructions in `docs/HARDWARE_BRINGUP.md`.
- Identity subsystem leveraging TPM 2.0 (or DICE fallback) to seal capability ticket seeds; attestation logs appended to `/proc/boot` and exported via NineDoor.
- Secure Boot guidance aligning with `docs/SECURITY.md` red lines; ensure measurements cover generated bootstrap and manifest hashes.
- Compiler IR v1.4 fields for hardware profiles (`hardware.profile = "uefi_aarch64"`) capturing UART/NET MMIO addresses, TPM presence, and boot policies.

**Use-case alignment**
- Defense ISR kits (Edge §8) require tamper logging and key rotation on real hardware.
- Energy micro-grids (Edge §2) and OT segmentation (Security §12) need attested boot records for compliance.

**Commands**
- `cargo build -p root-task-uefi --target aarch64-unknown-uefi`
- `python scripts/make_uefi_image.py --manifest out/manifests/root_task_resolved.json`
- `scripts/qemu-run.sh --uefi` (smoke test) and lab checklist for physical board boot.

**Checks (DoD)**
- UEFI image boots on QEMU TCG + reference dev board; serial/log output matches VM baseline.
- TPM-backed attestation chain generated and exported via `/proc/boot` without leaking secrets.
- Compiler rejects manifests missing hardware bindings for selected profile.

**Compiler touchpoints**
- IR v1.4 describes hardware profiles, TPM usage, and Secure Boot policy; codegen produces UEFI config headers and doc tables summarising MMIO regions.
- Docs-as-built guard extends to hardware bring-up instructions with manifest fingerprints.

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

