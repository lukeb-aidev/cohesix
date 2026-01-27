<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Track Cohesix milestones, deliverables, and completion criteria for ARM64 Pure Rust userspace builds. -->
<!-- Author: Lukas Bower -->
# Cohesix Build Plan (ARM64, Pure Rust Userspace)
Cohesix is designed for physical ARM64 hardware booted via UEFI as the primary deployment environment. Today’s reference setup runs on QEMU `aarch64/virt` for bring-up, CI, and testing, and QEMU behaviour is expected to mirror the eventual UEFI board profile.

**Host:** macOS 26 on Apple Silicon (M4)
**Target:** QEMU aarch64 `virt` (GICv3)
**Kernel:** Upstream seL4 (external build)
**Userspace:** Pure Rust crates (`root-task`, `nine-door`, `worker-heart`, future `worker-gpu`, `gpu-bridge-host` host tool)

Physical ARM64 hardware booted via UEFI is the planned deployment environment; early milestones stabilise against QEMU `aarch64/virt` as the reference development and CI profile while preserving semantics for the eventual hardware bring-up.

The milestones below build cumulatively; do not advance until the specified checks pass and documentation is updated. Each step
is grounded in the architectural intent outlined in `docs/ARCHITECTURE.md`, the repository conventions from `docs/REPO_LAYOUT.md`,
and the interface contracts codified in `docs/INTERFACES.md`. Treat those documents as non-negotiable source material when
preparing and executing tasks.

Cohesix is a hive-style orchestrator: one Queen coordinating many workers via a shared Secure9P namespace and commanded through `cohsh`.

## seL4 Reference Manual Alignment (v13.0.0)

We treat the seL4 Reference Manual v13.0.0 (`seL4/seL4-manual-latest.pdf`) as the authoritative description of kernel semantics. This plan
cross-checks each milestone against the relevant chapters to ensure we remain within the manual’s constraints:
- **Chapters 2 & 3 (Kernel Services, Objects, and Capability Spaces)** drive the capability discipline, retype requirements, and CSpace
  layout described in Milestones 0–4.
- **Chapters 4 & 5 (Message Passing and Notifications)** inform the NineDoor 9P transport, IPC patterns, and event/endpoint handling
  in Milestones 1–3.
- **Chapters 6 & 7 (Threads, Execution, and Address Spaces)** govern timer/tick handling, scheduling contexts, and deterministic memory
  budgets we rely on for the root-task event pump and worker isolation.
- **Chapter 8 (Hardware I/O)** constrains the virtio-console/net interaction surface and informs how we integrate serial/network drivers
  with the kernel’s interrupt/IO model.
- **Chapters 9 & 10 (System Bootstrapping and API Reference)** describe bootinfo, CPIO loading, and syscall behaviours, underpinning
  `scripts/qemu-run.sh`, `scripts/ci/size_guard.sh`, and all entrypoint work.

We revisit these sections whenever we specify new kernel interactions or manifest changes so that documentation and implementations remain aligned.

---

## Milestones ##
<a id="Milestones"></a>
| Milestone | Description | Status |
|----------|-------------|------|
| [0](#0) | Repository Skeleton & Toolchain | Complete |
| [1](#1) | Boot Banner, Timer, & First IPC | Complete |
| [2](#2) | NineDoor Minimal 9P | Complete |
| [3](#3) | Queen/Worker MVP with Roles | Complete |
| [4](#4) | Bind & Mount Namespaces | Complete |
| [5](#5) | Hardening & Test Automation (ongoing) | Complete |
| [6](#6) | GPU Worker Integration | Complete |
| [6a](#6a) | GPU Model Lifecycle & Telemetry Semantics (LoRA-ready) | Complete |
| [7a](#7a) | Root-Task Event Pump & Authenticated Kernel Entry | Complete |
| [7b](#7b) | Standalone Console & Networking (QEMU-first) | Complete |
| [7c](#7c) | TCP transport parity while retaining existing flows | Complete |
| [7d](#7d) | ACK/ERR broadcast is implemented across serial and TCP | Complete |
| [7e](#7e) | TraceFS (JSONL Synthetic Filesystem) | Complete |
| [8a](#8a) | Lightweight Hardware Abstraction Layer | Complete |
| [8b](#8b) | Root-Task Compiler & Deterministic Profiles | Complete |
| [8c](#8c) | Cache-Safe DMA via AArch64 VSpace Calls | Complete |
| [8d](#8d) | In-Session test Command + Preinstalled .coh Regression Scripts | Complete |
| [9](#9) | Secure9P Pipelining & Batching | Complete |
| [10](#10) | Telemetry Rings & Cursor Resumption | Complete |
| [11](#11) | Host Sidecar Bridge & /host Namespace (Ecosystem Coexistence) | Complete |
| [12](#12) | PolicyFS & Approval Gates | Complete |
| [13](#13) | AuditFS & ReplayFS | Complete |
| [14](#14) | Sharded Namespaces & Provider Split | Complete |
| [15](#15) | Client Concurrency & Session Pooling | Complete |
| [16](#16) | Observability via Files (No New Protocols) | Complete |
| [17](#17) | Content-Addressed Updates (CAS) — 9P-first | Complete |
| [18](#18) | Field Bus & Low-Bandwidth Sidecars (Host/Worker Pattern) | Complete |
| [19](#19) | cohsh-core Extraction (Shared Grammar & Transport) | Complete |
| [20a](#20a) | cohsh as 9P Client Library | Complete |
| [20b](#20b) | NineDoor UI Providers | Complete |
| [20c](#20c) | SwarmUI Desktop (Tauri, Pure 9P/TCP) | Complete |
| [20d](#20d) | SwarmUI Live Hive Rendering (PixiJS, GPU-First | Complete |
| [20e](#20e) | CLI/UI Convergence Tests | Complete |
| [20f](#20f) | UI Security Hardening (Tickets & Quotas) | Complete |
| [20f1](#20f1) | SwarmUI Host Tool Packaging + Tauri API Fix | Complete |
| [20g](#20g) | Deterministic Snapshot & Replay (UI Testing) | Complete |
| [20h](#20h) | Alpha Release Gate: As-Built Verification, Live Hive Demo, SwarmUI Replay, & Release Bundle | Complete |
| [21a](#21a) | Telemetry Ingest with OS-Named Segments (Severely Limited Create) | Complete |
| [21b](#21b) | Host Bridges (coh mount, coh gpu, coh telemetry pull) | Complete |
| [21c](#21c) | SwarmUI Interactive cohsh Terminal (Full Prompt UX) | Complete |
| [21d](#21d) | Deterministic Node Lifecycle & Operator Control | Complete |
| [21e](#21e) | Rooted Authority, Cut Detection, Explicit Session Semantics, and Live Hive Visibility | Complete |
| [22](#22) | Runtime Convenience (coh run) + GPU Job Breadcrumbs | Complete |
| [23](#23) | PEFT/LoRA Lifecycle Glue (coh peft) | Complete |
| [24](#24) | Python Client + Examples (cohesix) + Doctor + Release Cut | Pending |
| [25a](#25a) | UEFI Bare-Metal Boot & Device Identity | Pending |
| [25b](#25b) | UEFI On-Device Spool Stores + Settings Persistence | Pending |
| [25c](#25c) | SMP Utilization via Task Isolation (Multicore without Multithreading) | Pending |
| [25d](#25d) | Operator Utilities: Inspect, Trace, Bundle, Diff, Attest | Pending |
| [26](#26) | Edge Local Status (UEFI Host Tool) | Pending |
| [27](#27) | AWS AMI (UEFI → Cohesix, ENA, Diskless 9door) | Pending |

---

## Milestone 0 — Repository Skeleton & Toolchain <a id="0"></a> 
[Milestones](#Milestones)

**Status:** Complete — repo/workspace scaffolding, build scripts, and size guard are in place; keep regenerated artefacts in sync with toolchain outputs.
**Deliverables**
- Cargo workspace initialised with crates for `root-task`, `nine-door`, and `worker-heart` plus shared utility crates.
- `toolchain/setup_macos_arm64.sh` script checking for Homebrew dependencies, rustup, and QEMU - and installing if absent.
- `scripts/qemu-run.sh` that boots seL4 with externally built `elfloader`, `kernel.elf`, also creates and uses `rootfs.cpio`.
- QEMU launchers auto-select host acceleration (`hvf` on macOS, `kvm` on Linux when `/dev/kvm` is accessible), with
  `COHESIX_QEMU_ACCEL`/`QEMU_ACCEL` overrides and fallback to `tcg`.
- `scripts/ci/size_guard.sh` enforcing < 4 MiB CPIO payload.
- Repository tree matches `docs/REPO_LAYOUT.md`, and architecture notes from `docs/ARCHITECTURE.md §1-§3` are captured in crate
  READMEs or module docs to prevent drift.

**Checks**
- `cargo check` succeeds for the workspace.
- `qemu-system-aarch64 --version` reports the expected binary.
- QEMU launchers log the selected accelerator and pass `-accel` with host-appropriate defaults or explicit overrides.
- `scripts/ci/size_guard.sh out/rootfs.cpio` rejects oversized archives.

## Milestone 1 — Boot Banner, Timer, & First IPC <a id="1"></a> 
[Milestones](#Milestones)

**Status:** Complete — boot banner, timer tick, and initial IPC appear in current boot logs; retain existing log ordering.
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

## Milestone 2 — NineDoor Minimal 9P <a id="2"></a> 
[Milestones](#Milestones)

**Status:** Complete — Secure9P codec, fid/session handling, and the synthetic namespace are active; follow-up limited to ongoing fuzz coverage.
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
- These flows are defined for one Queen orchestrating many workers within a hive; host tools (CLI or GUI) drive them via `cohsh`.

**Checks**
- Integration test attaches, walks, reads `/proc/boot`, and appends to `/queen/ctl`.
- Attempting to write to `/proc/boot` fails with `Permission`.
- Decoder corpus covers malformed frames (length mismatch, fid reuse).

## Milestone 3 — Queen/Worker MVP with Roles <a id="3"></a> 
[Milestones](#Milestones)

**Status:** Complete — Queen/worker roles, budgets, and `/queen/ctl` JSON handling are live; keep tests aligned with current ticket and namespace semantics.
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
- These queen/worker flows assume one Queen orchestrating many workers within a hive, exercised through `cohsh` or clients speaking its protocol.

**Checks**
- Writing spawn command creates worker directory and live telemetry stream.
- Writing kill removes worker directory and closes telemetry file.
- Role isolation tests deny cross-role reads/writes.

## Milestone 4 — Bind & Mount Namespaces <a id="4"></a> 
[Milestones](#Milestones)

**Status:** Complete — Per-session mount tables are implemented; future changes must preserve established bind/mount semantics from `SECURE9P.md`.
**Deliverables**
- Per-session mount table with `bind(from, to)` and `mount(service, at)` operations scoped to a single path.
- Queen-only commands for namespace manipulation exposed via `/queen/ctl`.
- Namespace operations mirror the behaviour defined in `docs/INTERFACES.md §3` and respect mount expectations in
  `docs/ARCHITECTURE.md §4`.
- Mount and namespace flows remain scoped to one Queen orchestrating many workers inside a hive, driven by `cohsh` (and future GUI clients that speak its protocol).

**Checks**
- Queen remaps `/queen` to a subdirectory without affecting other sessions.
- Attempted bind by a worker fails with `Permission`.

## Milestone 5 — Hardening & Test Automation (ongoing) <a id="5"></a> 
[Milestones](#Milestones)

**Status:** Complete — Unit/fuzz/integration coverage exists; maintain regression packs as features evolve.
**Deliverables**
- Unit tests for codec, fid lifecycle, and access policy negative paths.
- Fuzz harness covering length-prefix mutations and random tail bytes for the decoder.
- Integration test: spawn heartbeat → emit telemetry → kill → verify revocation logs.
- Cohsh regression scripts (per `docs/USERLAND_AND_CLI.md §6-§7`) execute against mock and QEMU targets, ensuring CLI and
  Secure9P behaviours stay aligned.

**Checks**
- `cargo test` passes in CI.
- Fuzz harness runs N iterations (configurable) without panic.

## Milestone 6 — GPU Worker Integration <a id="6"></a> 
[Milestones](#Milestones)

**Status:** Complete (host-side scaffolding in place; VM-side worker stubs remain minimal until host bridge integration lands).
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

> **Future Note:** A host-side WASM GUI is expected as a hive dashboard layered on the `cohsh` protocol; it does not alter kernel/userspace boundaries or introduce new in-VM services.

## Milestone 6a — GPU Model Lifecycle & Telemetry Semantics (LoRA-ready) <a id="6a"></a> 
[Milestones](#Milestones)

**Status:** Complete — host bridge and documentation now define model lifecycle surfaces, schema-tagged telemetry, and export guarantees without altering in-VM capabilities.

**Why this exists (context)**  
Milestone 6 proved the **GPU lease boundary and host bridge mechanics** using kernel-style job submission. That validated the architecture, but it does not yet express **model lifecycle state** or **learning-oriented telemetry semantics**, which are required for PEFT / LoRA feedback loops at scale.

Milestone 6a adds **no new execution capabilities** and **no new control channels**. It introduces only **file-level conventions and minimal host-bridge extensions** so Cohesix can orchestrate *model state* and *learning telemetry* without becoming an ML runtime.

This milestone is intentionally boring.

---

### Goal

Extend the existing `gpu-bridge-host` and GPU namespace with:
1. **Model lifecycle surfaces** (selection + activation, not execution)
2. **Well-defined telemetry semantics** suitable for LoRA / PEFT pipelines

while preserving:
- CUDA/NVML strictly outside the VM
- Secure9P as the only control plane
- WorkerGpu as a namespace-only role
- Deterministic memory and rate bounds

---

### Deliverables

#### 1. GPU Model Lifecycle Namespace (Host-side only)

Extend the mirrored GPU namespace with a **model lifecycle view**:

/gpu/models/
available/
<model_id>/
manifest.toml
active -> <model_id>

Properties:
- `available/` is read-only to VM roles
- `active` is a writable symlink-like pointer (atomic swap)
- Model artifacts live on the host filesystem; Cohesix sees references only
- Activation semantics are host-defined (reload / restart / hot-swap)

**Non-goals**
- No model uploads via 9P
- No artifact streaming
- No training or conversion logic

---

#### 2. Telemetry Schema for Learning Loops

Define and document a **versioned telemetry schema** for GPU learning feedback.

Required fields (minimum):
- `schema_version`
- `device_id`
- `model_id`
- `lora_id` (optional)
- `time_window`
- `token_count`
- `latency_histogram`

Optional fields:
- confidence / entropy
- drift indicators
- operator feedback flags

Telemetry continues to flow through existing paths:

/gpu/telemetry/*
/worker/<id>/telemetry

Constraints:
- Size-bounded records
- Append-only semantics
- Explicit windowing (no unbounded streams)

---

#### 3. Worker Behavior (No New Roles)

WorkerGpu behavior remains minimal:
- Observe `/gpu/models/active`
- Include `model_id` / `lora_id` in forwarded telemetry
- Enforce existing rate and size limits

No new worker types or privileges are introduced.

---

#### 4. Queen Export Compatibility (No Training Logic)

Ensure telemetry emitted under the new schema can be **exported unchanged** via:

/queen/telemetry/*
/queen/export/lora_jobs/*

Milestone 6a does **not** implement training, scheduling, or PEFT tooling.
It only guarantees that exported telemetry is:
- Structured
- Bounded
- Policy-checkable
- ML-pipeline friendly

---

### Files & Components Touched

- `gpu-bridge-host`
  - Add model lifecycle surfaces
  - Implement atomic model activation
  - Emit telemetry records with schema tags

- `docs/GPU_NODES.md`
  - Document `/gpu/models/*`
  - Clarify separation between job execution vs model state

- `docs/INTERFACES.md`
  - Telemetry schema definition
  - Explicit size and rate limits

- `docs/USE_CASES.md`
  - Reference LoRA / PEFT edge feedback loop (informational)

No changes to:
- seL4 kernel usage
- Secure9P protocol
- NineDoor access policy logic
- Worker role definitions

---

### Checks (Definition of Done)

- Existing Milestone 6 GPU kernel tests still pass unchanged
- Switching `/gpu/models/active` causes host-side model reload
- Telemetry records include valid schema headers
- Oversized or malformed telemetry is rejected
- Worker cannot upload models or bypass leases
- No new in-VM dependencies introduced

---

### Outcome

After Milestone 6a:
- Cohesix can safely coordinate **model evolution at the edge**
- PEFT / LoRA pipelines can consume telemetry without bespoke glue
- GPU execution remains host-owned
- The control plane remains deterministic, auditable, and small

Milestone 6 stays about **capability**.  
Milestone 6a is about **intent**.

## Milestone 7a — Root-Task Event Pump & Authenticated Kernel Entry  <a id="7a"></a> 
[Milestones](#Milestones)

**Status:** Complete — Event pump replaces the spin loop; authenticated console flow and serial integration are live. Preserve PL011 logging and audit ordering during follow-up changes.
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

## Milestone 7b — Standalone Console & Networking (QEMU-first)   <a id="7b"></a> 
[Milestones](#Milestones)

**Status:** Complete — PL011 root console and TCP console co-exist; networking stack is feature-gated and non-blocking. Virtio-console is not used; PL011 remains the root console (see `ARCHITECTURE.md` for dual-console expectations).
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
## Milestone 7c - TCP transport parity while retaining existing flows   <a id="7c"></a> 
[Milestones](#Milestones)

**Status:** Complete — TCP transport, documentation updates, and integration tests are in tree; keep host build scripts and console fixtures in sync when toggling transport flags.
**Deliverables**
- **Remote transport**
  - Extend `cohsh` with a TCP transport that speaks to the new in-VM listener while keeping the existing mock/QEMU flows; expose reconnect/back-off behaviour and certificate-less ticket validation for the prototype environment.
  - Reuse the current NineDoor command surface so scripting and tests stay aligned, document the new `--transport tcp` flag with examples, and ensure help text highlights transport fallbacks when networking is unavailable.
- **Documentation & tests**
  - Update `docs/ARCHITECTURE.md`, `docs/INTERFACES.md`, and `docs/SECURITY.md` with the networking/console design, threat model, and TCB impact including memory budgeting tables for serial/net buffers.
  - Provide QEMU integration instructions (`docs/USERLAND_AND_CLI.md`) showing serial console usage, remote `cohsh` attachment, and recommended port-forwarding commands for macOS host tooling.
  - Add unit tests for the command parser (invalid verbs, overlong lines), virtio queue wrappers, and integration tests that boot QEMU, connect via TCP, run scripted sessions, and verify audit log outputs.
  - Record the TCP console toggle in `configs/root_task.toml` once the manifest compiler lands (Milestone 8b) so docs and fixtures remain in sync.
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

## Milestone 7d - ACK/ERR broadcast is implemented across serial and TCP  <a id="7d"></a> 
[Milestones](#Milestones)

**Status:** Complete — ACK/ERR broadcast is implemented across serial and TCP with shared fixtures, reconnection semantics, and documentation in place.
**Deliverables**
- Ensure the PL011 root console remains active alongside the TCP listener; TCP handling must stay non-blocking so serial recovery remains deterministic (see `ARCHITECTURE.md`).
- Attachments must respect the current NineDoor handshake and ticket validation; acknowledgements should reuse the parser grammar from `USERLAND_AND_CLI.md`.
- **Console acknowledgements**
  - Enable the root-task TCP listener to emit `OK`/`ERR` responses for `ATTACH`, heartbeat probes, and command verbs so remote operators receive immediate feedback.
  - Surface execution outcomes (success, denial, or validation failure) through the shared serial/TCP output path with structured debug strings suitable for regression tests.
- **Client alignment**
  - Ensure `cohsh` reuses the acknowledgement surface for telemetry, surfacing attach/session state changes and command failures consistently across transports.
- **Documentation & tests**
  - Update protocol documentation to describe the acknowledgement lifecycle, including reconnection semantics and error payloads.
  - Extend automated coverage so both serial and TCP transports assert the presence of acknowledgements during scripted sessions.

**Checks (DoD)**

- Adding ACK/ERR output MUST NOT change line prefixes, newline behaviour, or attach handshake timing established in Milestone 7c. The Regression Pack must pass without modifying any fixture.

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

## Milestone 7e — TraceFS (JSONL Synthetic Filesystem)   <a id="7e"></a> 
[Milestones](#Milestones)

**Status:** Complete — TraceFS provider backs `/trace/*` and worker traces; control-plane filters and CLI coverage are wired without regressing existing mounts (see `SECURE9P.md`).
**Purpose**
Add a minimal synthetic 9P provider (`tracefs`) exposing JSONL-based tracing and diagnostic streams.  
Enable root-task and userspace components to log, filter, and stream events via append-only 9P files, following the Plan 9 “everything is a file” model.

**Deliverables**
- New `nine-door` provider `/trace/ctl`, `/trace/events`, `/kmesg`, and per-task `/proc/<tid>/trace`.
- Root-task `Trace` facade with zero-allocation ring buffer and `trace!()` macro.
- Category/level filters controllable by writing JSON commands to `/trace/ctl`.
- Persistent, append-only JSONL event format shared across roles.
- CLI (`cohsh`) integration for `tail`/`echo` commands against `/trace/*`.
- Optional host-side mirroring via a bridge mount.

**Commands (Mac ARM64)**
```bash
SEL4_BUILD_DIR=$HOME/seL4/build \
./scripts/cohesix-build-run.sh \
  --sel4-build "$HOME/seL4/build" \
  --out-dir out/cohesix \
  --profile release \
  --root-task-features kernel,bootstrap-trace,serial-console \
  --cargo-target aarch64-unknown-none \
  --transport qemu \
  --raw-qemu
cohsh> echo '{"set":{"level":"debug","cats":["boot","ninep"]}}' > /trace/ctl
cohsh> tail /trace/events
```

**Checks**

* `/trace/events` streams JSONL trace lines after boot.
* `/trace/ctl` accepts JSON control messages without panic.
* Per-task `/proc/<tid>/trace` returns filtered events.
* Host build passes `cargo test -p nine-door` and integration test `tests/cli/tracefs_script.sh`.

**Definition of Done**

* Boot completes and serial console shows `[Cohesix] Root console ready.`
* Writing to `/trace/ctl` dynamically changes categories/levels.
* Reading `/trace/events` shows bounded ring output with sequence continuity.
* No TCP or external logging inside the VM.
* Code aligned with `secure9p-*` layering; passes `cargo clippy -- -D warnings`.
* TCP console must remain non-blocking and PL011 stays active as the fallback root console (see `ARCHITECTURE.md`).

## Milestone 8a — Lightweight Hardware Abstraction Layer   <a id="8a"></a> 
[Milestones](#Milestones)

**Why now (context):** Kernel bring-up now relies on multiple MMIO peripherals (PL011 UART, virtio-net). Tight coupling to `KernelEnv`
spread driver responsibilities across modules, making future platform work and compiler integration harder to reason about.

**Goal**
Carve out a lightweight Hardware Abstraction Layer so early boot and drivers consume a focused interface for mapping device pages
and provisioning DMA buffers.

**Deliverables**
- `apps/root-task/src/hal/mod.rs` introducing `KernelHal` and the `Hardware` trait that wrap device/DMA allocation, coverage queries,
  and allocator snapshots.
- `apps/root-task/src/kernel.rs` switched to the HAL for PL011 bring-up and diagnostics, keeping boot logging unchanged.
- `apps/root-task/src/drivers/{rtl8139.rs,virtio/net.rs}` and `apps/root-task/src/net/stack.rs` updated to rely on the HAL rather than touching
  `KernelEnv` directly, simplifying future platform support and keeping NICs behind a shared `NetDevice` trait.
- Documentation updates in this build plan describing the milestone and entry criteria.

**Status:** Complete — Kernel HAL now owns device mapping, diagnostics, and NIC bring-up (RTL8139 by default on `dev-virt`, virtio-net behind
the feature gate) while keeping console output stable.

**Commands**
- `cargo check -p root-task --features "kernel,net-console"`

**Checks (DoD)**
- Root task still boots with PL011 logging and default RTL8139 initialisation via the HAL, with virtio-net available behind the feature gate for
  experiments.
- HAL error propagation surfaces seL4 error codes for diagnostics (no regression in boot failure logs).
- Workspace `cargo check` succeeds with the kernel and net-console features enabled.
- Run the Regression Pack (see “Docs-as-Built Alignment”) to confirm console behaviour, networking event pump cadence, and NineDoor flows are unchanged despite the new HAL. Any change in ACK/ERR or `/proc/boot` output must be documented and justified.
- HAL introduction MUST NOT alter device MMIO layout, IRQ numbering, or virtio feature-negotiation visible in QEMU logs. Any change requires a manifest schema bump and doc update.
- **Milestone 8a scope exception (authorized):** A narrow TCP/virtio-net stability effort is permitted to unblock console bring-up, limited to:
  - Minimal, feature-gated debug instrumentation in `apps/root-task/src/drivers/virtio/net.rs` and queue helpers.
  - TX/RX publish ordering + cache visibility fixes, without protocol or console grammar changes.
  - A host repro harness script (e.g. `scripts/tcp_repro.sh`) that drives the existing QEMU TCP console and cohsh smoke flow.
  - No refactors, no new in-VM services, and no manifest/schema changes.
  - **Scope note (authorized):** Feature-flag consolidation for root-task bring-up (`cleanup-1-feature-flags-consolidation`) is permitted, limited to adding a single public `cohesix-dev` umbrella, removing dead flags, and updating scripts/docs without changing default behavior or console grammar.
  - **Scope note (authorized):** Instrumentation noise reduction (`cleanup-2-instrumentation-noise-reduction`) is permitted, limited to heapless rate-limited counters and demoting/rate-limiting net/event pump spam without changing console protocol lines, ordering, or CLI/ACK semantics.

---
## Milestone 8b — Root-Task Compiler & Deterministic Profiles <a id="8b"></a> 
[Milestones](#Milestones)

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
  - `scripts/cohsh/boot_v0.coh` — baseline CLI script derived from the manifest to exercise attach/log/quit flows.
- Manifest IR gains optional `ecosystem.*` section (schema-validated, defaults to noop):
  - `ecosystem.host.enable` (bool)
  - `ecosystem.host.providers[]` (enum: `systemd`, `k8s`, `nvidia`, `jetson`, `net`)
  - `ecosystem.host.mount_at` (default `/host`)
  - `ecosystem.audit.enable` (bool)
  - `ecosystem.policy.enable` (bool)
  - `ecosystem.models.enable` (bool; future CAS hook)
  - Generated doc snippets call out that these nodes appear only when enabled.
- Documentation updates:
  - `docs/ARCHITECTURE.md §11` expanded with the manifest schema and regeneration workflow.
  - `docs/BUILD_PLAN.md` (this file) references the manifest in earlier milestones.
  - `docs/REPO_LAYOUT.md` lists the new `configs/` and `tools/coh-rtc/` trees with regeneration commands.

**Status:** Complete — local aarch64/QEMU validation and regression pack confirm the DoD checks.

**Commands**
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json --cli-script scripts/cohsh/boot_v0.coh`
- `cargo check -p root-task --no-default-features --features kernel,net-console`
- `cargo test -p root-task`
- `cargo test -p tools/coh-rtc`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/boot_v0.coh`

**Checks (DoD)**
- Regeneration is deterministic: two consecutive runs of `cargo run -p coh-rtc …` produce identical Rust, JSON, and CLI artefacts (verified via hash comparison recorded in `out/manifests/root_task_resolved.json.sha256`).
- Root task boots under QEMU using generated bootstrap tables; serial log shows manifest fingerprint and ticket registration sourced from generated code.
- Compiler validation rejects manifests that violate red lines (e.g., invalid walk depth, enabling `gpu` while `profile.kernel` omits the feature gate) and exits with non-zero status.
- Run the Regression Pack and reject any drift in `scripts/cohsh/boot_v0.coh` output or manifest fingerprints unless the docs and schema version are updated in the same change.
- Generated modules MUST NOT introduce new global state or reorder initialisation in a way that changes serial boot ordering or `/proc/boot` output.
- Compiler rejects manifests that set `ecosystem.host.enable = true` when memory budgets or Secure9P red lines (msize, walk depth, role isolation) would be exceeded; enabling the ecosystem section MUST NOT relax prior limits.
- Docs-as-built guard extends to the new schema nodes so generated snippets and rendered docs agree on the resolved manifest.

**Compiler touchpoints**
- Introduces `root_task.schema = "1.0"`; schema mismatches abort generation and instruct operators to upgrade docs.
- Adds `cargo xtask` style CI guard (or Makefile target) invoked by `scripts/check-generated.sh` that runs the compiler, compares hashes, and fails CI when committed artefacts drift.
- Exports doc snippets (e.g., namespace tables) as Markdown fragments consumed by `docs/ARCHITECTURE.md` to guarantee docs stay in lockstep with the manifest.

---

## Milestone 8c — Cache-Safe DMA via AArch64 VSpace Calls <a id="8c"></a> 
[Milestones](#Milestones)

**Why now (context):** DMA regions shared with host-side GPUs, telemetry rings, and future sidecars cross NineDoor and HAL boundaries, but our cache maintenance is still implicit. Section 10.9.2 of the seL4 manual exposes the AArch64-only `seL4_ARM_VSpace_{Clean, CleanInvalidate, Invalidate, Unify}_Data` invocations; wrapping them in Rust lets us publish deterministic cache semantics instead of trusting ad-hoc CPU flushes.

**Goal**
Wrap the AArch64-specific VSpace cache operations in the HAL, wire them into manifest-driven DMA contracts, and call them whenever pages are pinned for host DMA so shared buffers remain coherent and auditable.

**Deliverables**
- `apps/root-task/src/hal/cache.rs` (new module) defining `CacheMaintenance` helpers around `seL4_ARM_VSpace_Clean_Data`, `CleanInvalidate_Data`, `Invalidate_Data`, and `Unify_Instruction` plus error/trace plumbing so callers can treat range, alignment, and domain failures deterministically.
- HAL integration updates (telemetry rings, GPU windows, future sidecar buffers) that execute the helpers immediately before handing memory to host-side actors and right after reclaiming pins, ensuring caches flush/invalidates happen in lockstep with page sharing.
- `tools/coh-rtc` schema additions (`cache.dma_clean`, `cache.dma_invalidate`, `cache.unify_instructions`) plus generated bootstrap tables and docs (`docs/ARCHITECTURE.md §11`, `docs/SECURE9P.md`) describing why AArch64 cache ops are necessary for deterministic DMA. The manifest rejects configurations that omit `cache.kernel_ops = true` while requesting DMA cache maintenance, preventing bizarreness.
- `apps/root-task/tests/cache_maintenance.rs` (QEMU/host shim) covering success/error paths of the helpers and asserting audit logs for flushed ranges before the shared region becomes available to NineDoor clients.

**Status:** Complete — cache maintenance helpers and DMA audit traces verified; coh-rtc rejects missing `cache.kernel_ops`; tests pass.

**Commands**
- `cd apps/root-task && cargo test cache_maintenance --features cache-maintenance`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json --cli-script scripts/cohsh/boot_v0.coh`
- `cargo test -p coh-rtc`

**Checks (DoD)**
- Cache helpers succeed for valid, aligned ranges and surface `seL4_RangeError`/`seL4_InvalidArgument` in logs when misaligned.
- Serial logs around NineDoor/DMA transitions mention cache flush/invalidate audit lines, proving the helpers run before sharing buffers.
- `coh-rtc` refuses to emit bootstrap tables for DMA cache maintenance when `cache.kernel_ops` is disabled, keeping docs/code aligned with the manual’s capability requirements.

---
## Milestone 8d — In-Session `test` Command + Preinstalled `.coh` Regression Scripts <a id="8d"></a> 
[Milestones](#Milestones)

**Why now (context):** The TCP console is now viable, but operators and CI need a deterministic, single-command proof that `cohsh` protocol semantics and server-side Secure9P/NineDoor behaviours remain intact. An in-session `coh> test` that exercises client↔server flows via preinstalled scripts ensures regressions surface immediately, including namespace side effects and negative paths.

**Goal**
Provide `coh> test` that runs a bounded suite validating the `cohsh` control-plane contract end-to-end (client + server), returning deterministic PASS/FAIL plus optional machine-readable JSON suitable for CI.
Following the .coh script format as documented in docs/USERLAND_AND_CLI.md "## coh scripts" section.

**Deliverables**
- Interactive command surface
  - `coh> test` defaults to a bounded “quick” suite; `--mode quick|full` switches coverage depth.
  - Flags: `--json` (stable output schema), `--timeout <s>` (hard upper bound to prevent hangs), and optional safety `--no-mutate` (skips spawn/kill when operators prohibit mutation). Mutation is otherwise permitted for “full” coverage.
  - Assumes session is already AUTH’d and ATTACH’d but revalidates both up front and fails fast if either is missing.
- Preinstalled `.coh` regression scripts on the server filesystem (rootfs-installed by the build script, never fetched at runtime)
  - Canonical path: `/proc/tests/` within the mounted namespace; scripts are installed into the CPIO rootfs during packaging.
  - Versioned artefacts (names fixed):
    - `selftest_quick.coh` — validates session state (AUTH/ATTACH), ping/ack grammar, bounded request/response round-trips.
    - `selftest_full.coh` — validates Secure9P/NineDoor semantics and performs one disposable worker lifecycle (spawn → observe namespace/telemetry evidence → kill) to prove mutation paths.
    - `selftest_negative.coh` — validates deterministic ERR paths (forbidden role action, `..` traversal rejection, bounded walk depth, oversized request vs `msize`, and no unintended mutation).
- Script execution model
  - `coh> test` executes the server-hosted `.coh` scripts (e.g., internally equivalent to `coh> run /proc/tests/selftest_full.coh` if the verb exists) so real client↔server control flow and namespace semantics are exercised; no client-embedded shortcuts.
- Output contract
  - Human output: checklist-style PASS/FAIL with the first failing step and a concise reason.
  - JSON (`--json`): `{ ok, mode, elapsed_ms, checks:[{name, ok, detail, transcript_excerpt?}], version }` (versioned for compatibility).

**Status:** Complete — `coh> test` runs against preinstalled `/proc/tests` scripts, emits PASS/FAIL plus JSON, and rerun guidance is documented for operators.

**Test coverage (what “full” must prove)**
- AUTH/ATTACH validation with deterministic failure when missing.
- Protocol grammar: deterministic OK/ERR acknowledgements, bounded retries, no silent failures.
- Role enforcement: queen-only actions rejected when attached as a non-queen role (or simulated negative in the script when role switching is unavailable).
- Secure9P correctness: walk/open/read/write/clunk flows, rejection of `..`, bounded walk depth, `msize`/frame bounds, read-only vs append-only semantics.
- Disposable worker lifecycle: spawn a short-lived worker, observe namespace/telemetry evidence, kill the worker, and verify cleanup.

**Commands**
- `coh> test`
- `coh> test --mode full`
- `coh> test --mode full --json`
- `coh> test --mode full --timeout 10`
- `coh> test --mode full --no-mutate`
- Example referencing the installed scripts: `coh> run /proc/tests/selftest_full.coh` (only if the existing verb is available; otherwise the `test` command drives the same execution path internally).

**Checks (DoD)**
- From an active interactive session, `coh> test --mode quick` completes within the default timeout and reports PASS on a healthy system.
- `coh> test --mode full` completes within the default timeout and exercises: AUTH/ATTACH validation, at least one read-only read from `/proc/*`, at least one permitted control write (append-only where applicable), disposable worker spawn → observe → kill, and at least one negative test producing deterministic ERR output.
- `--json` output matches the documented schema and remains stable for CI consumption (include `version`).
- `.coh` scripts exist at `/proc/tests/`, are installed into the rootfs by the build process, and remain the single source of truth for the suite (rerun whenever console, Secure9P, namespace layout, or access policy changes).
- Regression command reruns are documented: operators must execute this suite whenever console handling, Secure9P transport, namespace structure, or access policies change.

---
## Milestone 9 — Secure9P Pipelining & Batching <a id="9"></a> 
[Milestones](#Milestones)

(Clarification) Milestones 9–15 intentionally build on the full 7d acknowledgement grammar. Do NOT attempt to pull 9P batching/pipelining earlier than 7d; doing so breaks test surfaces.

**Why now (compiler):** Host NineDoor already handles baseline 9P flows, but upcoming use cases demand concurrent telemetry and command streams. Enabling multiple in-flight tags and batched writes requires new core structures and manifest knobs so deployments tune throughput without compromising determinism and Regression Pack guarantees.

**Goal**
Refactor Secure9P into codec/core crates with bounded pipelining and manifest-controlled batching.

**Deliverables**
- Split `crates/secure9p-codec` / `secure9p-core` / `secure9p-transport` into:
  - `crates/secure9p-codec` — frame encode/decode, batch iterators, fuzz corpus harnesses (still `std` for now).
  - `crates/secure9p-core` — session manager, fid table, tag window enforcement, and `no_std + alloc` compatibility.
  Existing consumers (`apps/nine-door`, `apps/cohsh`) migrate to the new crates.
- `apps/nine-door/src/host/` updated to process batched frames and expose back-pressure metrics; new module `pipeline.rs` encapsulates short-write handling and queue depth accounting surfaced via `/proc/9p/*` later.
- `apps/nine-door/tests/pipelining.rs` integration test spinning four concurrent sessions, verifying out-of-order responses and bounded retries when queues fill.
- CLI regression `scripts/cohsh/9p_batch.coh` executing scripted batched writes and verifying acknowledgement ordering.
- `scripts/cohsh/9p_batch.coh` includes batching/overflow assertions and participates in the regression pack DoD.
- `configs/root_task.toml` gains IR v1.1 fields: `secure9p.tags_per_session`, `secure9p.batch_frames`, `secure9p.short_write.policy`. Validation ensures `tags_per_session >= 1` and total batched payload stays ≤ negotiated `msize`.
- Docs: `docs/SECURE9P.md` updated to describe the new layering and concurrency knobs; `docs/INTERFACES.md` documents acknowledgement semantics for batched operations.
- Explicit queue depth limits and retry back-off parameters documented; negative path covers tag overflow and back-pressure refusal.

**Status:** Complete — pipelining tests cover synthetic load, batching toggles, and back-pressure; `9p_batch.coh` regression (including overflow) passes with the full regression pack.

**Commands**
- `cargo test -p secure9p-codec`
- `cargo test -p secure9p-core`
- `cargo test -p nine-door`
- `cargo test -p coh-rtc` (regenerates manifest snippets with new fields)
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/9p_batch.coh`

**Checks (DoD)**
- Synthetic load (10k interleaved operations across four sessions) completes without tag reuse violations or starvation; metrics expose queue depth and retry counts.
- Batched frames round-trip within negotiated `msize`; when the manifest disables batching the same tests pass with single-frame semantics.
- Short-write retry policies (e.g., exponential back-off) are enforced according to manifest configuration and verified by CLI regression output.
- Abuse case: exceeding configured `tags_per_session` or queue depth yields deterministic `ERR` and audit lines without panics; regression script asserts failure ordering.
- Re-run the Regression Pack to ensure pipelining and batching do not alter existing single-request semantics (tags, errors, or short-write handling) as exercised by earlier CLI scripts.
- Tag scheduling MUST remain deterministic: single-request scripts from milestones ≤7 MUST still produce byte-identical ACK/ERR sequences.

**Compiler touchpoints**
- `coh-rtc` emits concurrency defaults into generated Rust tables and CLI fixtures; docs snippets pull from the manifest rather than hard-coded prose.
- CI regeneration guard ensures manifest-driven tests fail if concurrency knobs drift between docs and code.

**Task Breakdown**
```
Title/ID: m09-codec-core-split
Goal: Extract codec/core crates with bounded tag windows and batch iterators.
Inputs: crates/secure9p-codec, crates/secure9p-core, crates/secure9p-transport, configs/root_task.toml (new IR fields), docs/SECURE9P.md excerpts.
Changes:
  - crates/secure9p-codec/lib.rs — move frame encode/decode + batch iterators; add fuzz corpus harness.
  - crates/secure9p-core/lib.rs — session manager with tag window enforcement and queue depth accounting.
  - apps/nine-door/src/host/pipeline.rs — enforce queue limits and short-write retry back-off.
Commands:
  - cargo test -p secure9p-codec
  - cargo test -p secure9p-core
Checks:
  - Tag overflow attempt (tags_per_session + 1) returns deterministic ERR and audit line.
Deliverables:
  - Updated crate split, manifest IR additions, and queue depth limits documented in docs/SECURE9P.md.

Title/ID: m09-batched-io-regression
Goal: Prove batched write ordering and back-pressure across CLI + Regression Pack.
Inputs: scripts/cohsh/9p_batch.coh, apps/nine-door/tests/pipelining.rs.
Changes:
  - apps/nine-door/tests/pipelining.rs — four-session interleave with induced short writes.
  - scripts/cohsh/9p_batch.coh — add overflow case asserting ERR on batch > msize.
Commands:
  - cargo test -p nine-door --test pipelining
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/9p_batch.coh
Checks:
  - Out-of-order responses preserved; batch larger than msize is rejected with logged ERR.
Deliverables:
  - Regression outputs archived; docs/INTERFACES.md snippet refreshed from manifest.
```
---

## Milestone 10 — Telemetry Rings & Cursor Resumption <a id="10"></a> 
[Milestones](#Milestones)

**Why now (compiler):** Persistent telemetry is currently mock-only. Operators need bounded append-only logs with resumable cursors, generated from the manifest so memory ceilings and schemas stay auditable.

**Goal**
Implement ring-backed telemetry providers with manifest-governed sizes and CBOR frame schemas.

**Deliverables**
- `apps/nine-door/src/host/telemetry/` (new module) housing ring buffer implementation (`ring.rs`) and cursor state machine (`cursor.rs`), integrated into `namespace.rs` and `control.rs` so workers emit telemetry via append-only files.
- `crates/secure9p-core` gains append-only helpers enforcing offset semantics and short-write signalling consumed by the ring provider.
- CBOR Frame v1 schema defined in `tools/coh-rtc/src/codegen/cbor.rs`, exported as Markdown to `docs/INTERFACES.md` and validated by serde-derived tests.
- CLI regression `scripts/cohsh/telemetry_ring.coh` exercising wraparound, cursor resume, and offline replay via `cohsh --features tcp`.
- Manifest IR v1.2 fields: `telemetry.ring_bytes_per_worker`, `telemetry.frame_schema`, `telemetry.cursor.retain_on_boot`. Validation ensures aggregate ring usage fits within the event-pump budget declared in `docs/ARCHITECTURE.md`.
- `apps/root-task/src/generated/bootstrap.rs` extended to publish ring quotas and file descriptors consumed by the event pump.

**Status:** Complete — ring-backed telemetry and cursor retention are validated by tests and the regression pack, with latency metrics recorded in `docs/SECURITY.md`.

**Commands**
- `cargo test -p nine-door`
- `cargo test -p secure9p-core`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/telemetry_ring.coh`

**Checks (DoD)**
- Rings wrap without data loss; on reboot the cursor manifest regenerates identical ring state and CLI replay resumes exactly where it left off.
- Latency metrics (P50/P95) captured during tests and recorded in `docs/SECURITY.md`, sourced from automated output instead of manual measurements.
- Attempts to exceed manifest-declared ring quotas are rejected and logged; CI asserts the rejection path.
- Abuse case: late reader requesting stale cursor receives deterministic ERR and bounded rewind log; overflow write attempts drop frames with explicit audit.
- Re-run the Regression Pack to confirm that adding ring-backed telemetry does not change console grammar or existing `/worker/<id>/telemetry` semantics outside the new CBOR frames.
- Introduction of CBOR telemetry MUST NOT alter legacy plain-text worker telemetry unless explicitly gated by manifest field `telemetry.frame_schema`.

**Compiler touchpoints**
- Codegen emits ring metadata for `/proc/boot` so operators can inspect per-worker quotas; docs pull from the generated JSON to avoid drift.
- Regeneration guard verifies that CBOR schema excerpts in docs match compiler output.

**Task Breakdown**
```
Title/ID: m10-ring-impl
Goal: Implement bounded append-only rings with cursor state machine.
Inputs: apps/nine-door/src/host/telemetry/, configs/root_task.toml telemetry fields.
Changes:
  - apps/nine-door/src/host/telemetry/{ring.rs,cursor.rs} — ring write/read, cursor resume, wraparound handling.
  - crates/secure9p-core/lib.rs — append-only helpers for offsets and short-write signalling.
Commands:
  - cargo test -p nine-door --test telemetry_ring
  - cargo test -p secure9p-core
Checks:
  - Write past ring_bytes_per_worker rejects with ERR and audit entry; cursor resume returns deterministic frame ordering.
Deliverables:
  - Ring implementation and manifest-aligned quotas documented in docs/INTERFACES.md.

Title/ID: m10-cbor-schema-regen
Goal: Define CBOR frame schema and regenerate bootstrap/fixtures.
Inputs: tools/coh-rtc/src/codegen/cbor.rs, apps/root-task/src/generated/bootstrap.rs.
Changes:
  - tools/coh-rtc/src/codegen/cbor.rs — schema + Markdown export.
  - apps/root-task/src/generated/bootstrap.rs — emit ring quotas and cursor retention flags.
Commands:
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/telemetry_ring.coh
Checks:
  - CLI script proves wraparound and stale cursor rejection; regenerated schema matches docs snippet.
Deliverables:
  - Updated manifest IR, CBOR schema excerpts in docs/INTERFACES.md.
```
---

## Milestone 11 — Host Sidecar Bridge & /host Namespace (Ecosystem Coexistence)  <a id="11"></a> 
[Milestones](#Milestones)

**Why now (compiler):** Cohesix needs to govern existing fleets (systemd units, Kubernetes nodes, GPUs) without moving those systems into the VM. Mirroring host controls into `/host` via Secure9P keeps determinism and the tiny TCB while exposing file-driven levers.

**Flagship narrative:** Cohesix acts as a governance layer over existing ecosystems: external orchestrators, device managers, and schedulers are surfaced as files and policies so queens and workers can coordinate without new protocols or in-VM servers.

**Goal**
Provide a host-only sidecar bridge that projects external ecosystem controls into a manifest-scoped `/host` namespace with strict policy/audit boundaries and no new in-VM transports.

**Deliverables**
- New host tool crate `apps/host-sidecar-bridge/` (name can adjust) that connects to NineDoor from the host using existing transports, publishes a provider-driven synthetic tree under `/host`, and supports `--mock` mode for CI.
- Namespace layout (v1, minimal, file-only and append-only for controls):
  - `/host/systemd/<unit>/{status,restart}` (mocked)
  - `/host/k8s/node/<name>/{cordon,drain}` (mocked)
  - `/host/nvidia/gpu/<id>/{status,power_cap,thermal}` (mocked; honours GPU-outside-VM stance)
- Access policy:
  - Queen role can write control nodes; workers are read-only or denied based on manifest policy.
  - Control writes are append-only command files (no random writes); audit lines are appended for every write using existing logging/telemetry mechanisms (no new logging protocol).
- Host-only transport enforcement: no new in-VM TCP listeners; the sidecar uses the existing authenticated console/NineDoor boundaries from the host side only.
- CLI harness and commands (documented):
  - `cargo test -p host-sidecar-bridge`
  - `cargo run -p host-sidecar-bridge -- --mock --mount /host`
  - `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/host_absent.coh`
  - `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/host_sidecar_mock.coh`
- TODO: Add scripts/cohsh/host_sidecar_mock.coh to the regression pack once host-enabled manifests are wired into CI.
- Manifest/IR alignment: `/host` tree appears only when `ecosystem.host.enable = true` with providers declared under `ecosystem.host.providers[]` and mount point defaulting to `/host`.
- Docs include policy and TCB notes emphasising that the bridge mirrors host controls without expanding the in-VM attack surface.

**Status:** Complete — host-sidecar bridge and manifest-gated `/host` namespace are verified via host tool tests, `host_sidecar_policy`, and CLI regression coverage for disabled `/host`.

**Checks (DoD)**
- `/host/*` tree mounts only when enabled by the manifest; omitted otherwise.
- Writes to control nodes are rejected for non-queen roles and result in append-only audit lines; mock mode exercises this path in CI.
- No new in-VM TCP services are introduced; all transports remain host-side per Secure9P.
- Abuse case: denied write to `/host/systemd/*/restart` returns deterministic ERR and logged audit; disablement removes namespace entirely and CLI regression confirms absence.

**Compiler touchpoints**
- `coh-rtc` validation ensures enabling `ecosystem.host` respects existing Secure9P red lines (msize, walk depth, role isolation) and memory budgets.
- Codegen emits doc/CLI snippets advertising `/host` only when enabled; docs-as-built guard pulls from the resolved manifest.

**Task Breakdown**
```
Title/ID: m11-sidecar-skeleton
Goal: Create host sidecar bridge and manifest-gated /host namespace.
Inputs: apps/host-sidecar-bridge/, configs/root_task.toml (ecosystem.host.*).
Changes:
  - apps/host-sidecar-bridge/src/main.rs — provider mounts, append-only control writers, mock mode.
  - apps/nine-door/src/host/namespace.rs — conditional mount for /host based on manifest.
Commands:
  - cargo test -p host-sidecar-bridge
  - cargo run -p host-sidecar-bridge -- --mock --mount /host
Checks:
  - Disabled manifest omits /host entirely; enabling exposes mocked controls without TCP listeners inside VM.
Deliverables:
  - Host bridge crate and manifest toggles documented in docs/ARCHITECTURE.md.

Title/ID: m11-policy-roles
Goal: Enforce role-based append-only controls with audit.
Inputs: docs/INTERFACES.md control grammar, scripts/cohsh/host_sidecar_mock.coh.
Changes:
  - apps/nine-door/src/host/control.rs — queen-only write enforcement and append-only audit logging.
  - scripts/cohsh/host_sidecar_mock.coh — denied-write then approved-write flow.
Commands:
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/host_sidecar_mock.coh
  - cargo test -p nine-door --test host_sidecar_policy
Checks:
  - Non-queen write returns ERR EPERM; audit line includes ticket and path; approved write succeeds deterministically.
Deliverables:
  - CLI regression outputs captured; docs/SECURITY.md mentions audit expectations.
```
---

## Milestone 12 — PolicyFS & Approval Gates <a id="12"></a> 
[Milestones](#Milestones)

**Why now (compiler):** Host mirroring introduces higher-risk controls. Converting approvals into manifest-driven files keeps operations human-auditable without new protocols.

**Flagship narrative:** Governance is file-native: risky actions become append-only requests, policy gates decide via files, and the hive stays deterministic across transports.

**Goal**
Add a PolicyFS surface that captures human-legible approvals for sensitive operations before they reach `/queen/ctl` or `/host` controls.

**Deliverables**
- Namespace nodes (provider may live in NineDoor or host; keep consistent with existing architecture):
  - `/policy/ctl` (append-only JSONL commands for policy changes)
  - `/policy/rules` (read-only snapshot emitted from manifest)
  - `/actions/queue` (append-only requests)
  - `/actions/<id>/status` (read-only)
- Enforcement: selected control writes (e.g., `/queen/ctl`, `/host/*/restart`) require a policy gate when enabled; denials/approvals append to the audit log using existing telemetry logging.
- CLI regression demonstrating a denied action followed by an approved action under policy gating.
- TODO: Implement scripts/cohsh/policy_gate.coh and add it to regression pack DoD.
- Manifest flag (e.g., `ecosystem.policy.enable`) toggles the gate and publishes rules; defaults keep policy off to preserve prior behaviour.

**Status:** Complete — PolicyFS surfaces and gating are manifest-driven, approval consumption is enforced, and the policy gate regression passes deterministically.

**Commands**
- `cargo test -p nine-door`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/policy_gate.coh`

**Checks (DoD)**
- Policy gate enablement is manifest-driven; disabling it reverts to prior control semantics without hidden defaults.
- Deterministic results: identical scripts produce identical ACK/ERR sequences and audit lines for denied vs. approved actions.
- Sensitive control writes are refused when gates are active and no approval exists; acceptance path appends deterministic audit lines.
- Abuse case: replaying an already-consumed approval yields ERR and does not double-apply actions; CLI script asserts refusal.

**Compiler touchpoints**
- `coh-rtc` emits policy/rule snapshots into generated docs and CLI fixtures; validation enforces append-only semantics and bounded queue sizes consistent with Secure9P limits.
- Docs-as-built guard ensures policy nodes and examples match the resolved manifest.

**Task Breakdown**
```
Title/ID: m12-policyfs-provider
Goal: Implement PolicyFS nodes and append-only gating for risky controls.
Inputs: apps/nine-door/src/host/{policy.rs,namespace.rs}, configs/root_task.toml (ecosystem.policy.*).
Changes:
  - apps/nine-door/src/host/policy.rs — providers for /policy/ctl, /policy/rules, /actions/*.
  - apps/nine-door/src/host/control.rs — enforcement hook requiring approvals before queen/host writes.
Commands:
  - cargo test -p nine-door --test policyfs
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json
Checks:
  - Missing approval produces ERR EPERM with audit; enabling flag publishes /policy tree; disabling removes it.
Deliverables:
  - PolicyFS provider code and manifest schema updates referenced in docs/INTERFACES.md.

Title/ID: m12-approval-regression
Goal: Demonstrate denied→approved flow and replay refusal.
Inputs: scripts/cohsh/policy_gate.coh.
Changes:
  - scripts/cohsh/policy_gate.coh — stepwise denied action, approval append, approved retry, replay attempt.
  - docs/SECURITY.md appendix note on approval replay limits (snippet refreshed from manifest).
Commands:
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/policy_gate.coh
Checks:
  - Replay attempt after approval consumption returns ERR and no duplicate action; ACK/ERR sequence deterministic.
Deliverables:
  - CLI transcript stored; manifest hash recorded for regression comparison.
```
---

## Milestone 13 — AuditFS & ReplayFS <a id="13"></a> 
[Milestones](#Milestones)

**Why now (compiler):** With host mirroring and policy gates, operators need deterministic replay for investigations without expanding the TCB. Bounded audit/replay surfaces make Cohesix operations repeatable and inspectable.

**Flagship narrative:** Cohesix treats control as data: every action and decision is recorded as append-only files that can be replayed deterministically to prove governance over external ecosystems.

**Goal**
Provide append-only audit logs and a bounded replay surface that re-applies Cohesix-issued control actions deterministically.

**Deliverables**
- `/audit/` subtree:
  - `/audit/journal` (append-only CBOR or JSONL aligned with existing telemetry choices)
  - `/audit/decisions` (policy approvals/denials)
  - `/audit/export` (read-only snapshot trigger)
- `/replay/` subtree:
  - `/replay/ctl` (append-only commands like “start replay from cursor X”)
  - `/replay/status` (read-only)
- Replay semantics:
  - Only replays Cohesix-issued control-plane actions (no arbitrary host scans) and respects bounded log windows.
  - Deterministic execution: same inputs → same ACK/ERR + audit lines regardless of transport (serial/TCP).
- CLI regression exercising record then replay of a scripted sequence with byte-identical acknowledgements.
- `scripts/cohsh/replay_journal.coh` is part of the regression pack.
- Audit logging integrates with telemetry rings without adding new protocols; storage remains bounded per manifest budget.

**Status:** Complete — AuditFS/ReplayFS surfaces, manifest gating, tests, and regression scripts are in place and pass deterministically.

**Commands**
- `cargo test -p nine-door`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/replay_journal.coh`

**Checks (DoD)**
- Scripted actions can be replayed to yield byte-identical ACK/ERR sequences for Cohesix control operations.
- Audit entries are emitted for all transports; replay refuses to exceed bounded windows or to replay non-Cohesix host state.
- Append-only semantics enforced for journal/control files; attempts at random-write are rejected and audited.
- Abuse case: request to replay beyond bounded window or disabled replay flag returns ERR and leaves system unchanged; CLI script asserts refusal.

**Compiler touchpoints**
- Manifest fields (e.g., `ecosystem.audit.enable`) gate audit/replay surfaces; validation enforces bounded storage and adherence to Secure9P limits.
- Generated docs reference audit/replay schemas derived from the resolved manifest; CI guard ensures snippets stay in sync.

**Task Breakdown**
```
Title/ID: m13-auditfs-journal
Goal: Add append-only audit journal and decision logs with bounded storage.
Inputs: apps/nine-door/src/host/audit.rs, configs/root_task.toml (ecosystem.audit.*).
Changes:
  - apps/nine-door/src/host/audit.rs — /audit/journal and /audit/decisions providers, append-only enforcement.
  - apps/nine-door/src/host/telemetry/mod.rs — hook to emit audit lines into telemetry ring.
Commands:
  - cargo test -p nine-door --test auditfs
Checks:
  - Random-write attempts to journal rejected with ERR; storage cap enforced with deterministic truncation policy.
Deliverables:
  - Audit schema documented in docs/INTERFACES.md via compiler snippet.

Title/ID: m13-replayfs-determinism
Goal: Implement bounded replay control with deterministic ACK/ERR.
Inputs: apps/nine-door/src/host/replay.rs, scripts/cohsh/replay_journal.coh.
Changes:
  - apps/nine-door/src/host/replay.rs — /replay/ctl, /replay/status, cursor handling within bounded window.
  - scripts/cohsh/replay_journal.coh — record then replay sequence plus over-window abuse case.
Commands:
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/replay_journal.coh
  - cargo test -p nine-door --test replayfs
Checks:
  - Replay beyond window or when disabled returns ERR; successful replay reproduces byte-identical ACK/ERR.
Deliverables:
  - Replay semantics and bounds captured in docs/SECURITY.md and docs/INTERFACES.md.
```
---

## Milestone 14 — Sharded Namespaces & Provider Split <a id="13"></a> 
[Milestones](#Milestones)

**Why now (compiler):** Scaling beyond hundreds of workers will otherwise bottleneck on single-directory namespaces. Deterministic sharding keeps walk depth bounded and aligns provider routing with manifest entries.

**Goal**
Introduce manifest-driven namespace sharding with optional legacy aliases.

**Deliverables**
- Namespace layout `/shard/<00..ff>/worker/<id>/…` generated from manifest fields. `apps/nine-door/src/host/namespace.rs` grows a `ShardLayout` helper that maps worker IDs to providers using manifest-supplied shard count and alias flags.
- `apps/nine-door/tests/shard_scale.rs` spins 1k worker directories, measuring attach latency and ensuring aliasing (when enabled) doesn't exceed walk depth (≤ 8 components).
- `crates/secure9p-core` exposes a sharded fid table ensuring per-shard locking and eliminating global mutex contention.
- Manifest IR v1.2 additions: `sharding.enabled`, `sharding.shard_bits`, `sharding.legacy_worker_alias`. Validation enforces `shard_bits ≤ 8` and forbids aliases when depth would exceed limits.
- Docs updates in `docs/ROLES_AND_SCHEDULING.md` describing shard hashing (`sha256(worker_id)[0..=shard_bits)`), alias behaviour, and operational guidance.
- `scripts/cohsh/shard_1k.coh` added to the regression pack DoD.

**Status:** Complete — sharded layouts, fid tables, manifest validation, CLI/regression coverage, and docs are aligned; regression pack is green.

**Commands**
- `cargo test -p nine-door`
- `cargo test -p secure9p-core`
- `cargo test -p tests --test shard_1k`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`

**Checks (DoD)**
- 1k worker sessions attach concurrently without starvation; metrics exported via `/proc/9p/sessions` demonstrate shard distribution.
- Enabling legacy aliases preserves `/worker/<id>` paths for backwards compatibility; disabling them causes the compiler to reject manifests that still reference legacy paths.
- Re-run the Regression Pack and compare paths: legacy `/worker/<id>` scripts must either continue to pass when aliases are enabled or fail deterministically when aliasing is disabled, with matching docs and manifest examples.
- Walk-depth MUST remain ≤8 at all times; CI must emit a hard error if shard/alias combinations ever generate deeper paths.
- Abuse case: deliberate alias + shard over-depth request fails manifest validation and produces deterministic compiler error recorded in docs.

**Compiler touchpoints**
- Generated bootstrap code publishes shard tables for the event pump and NineDoor bridge; docs consume the same tables.
- Manifest regeneration updates CLI fixtures so scripted tests reference shard-aware paths automatically.

**Task Breakdown**
```
Title/ID: m14-shard-mapping
Goal: Implement shard layout helper and sharded fid table.
Inputs: apps/nine-door/src/host/namespace.rs, crates/secure9p-core fid table.
Changes:
  - apps/nine-door/src/host/namespace.rs — ShardLayout mapping with alias toggle.
  - crates/secure9p-core/lib.rs — per-shard fid tables and lock partitioning.
Commands:
  - cargo test -p nine-door --test shard_scale
  - cargo test -p secure9p-core
Checks:
  - Over-depth shard+alias combination rejected by manifest validation; 1k worker attach latency within documented bounds.
Deliverables:
  - Shard tables emitted to generated bootstrap and referenced in docs/ROLES_AND_SCHEDULING.md.

Title/ID: m14-shard-regression
Goal: Validate legacy alias compatibility and sharded CLI flows.
Inputs: scripts/cohsh/shard_1k.coh (new), tests/integration shard_1k harness.
Changes:
  - scripts/cohsh/shard_1k.coh — attaches to shard and legacy alias paths; includes disabled-alias negative case.
  - docs/INTERFACES.md snippet showing shard path grammar generated from manifest.
Commands:
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/shard_1k.coh
Checks:
  - Legacy path fails deterministically when alias disabled; succeeds when enabled with identical ACK ordering.
Deliverables:
  - Regression transcript captured; manifest hash recorded for docs.
```
---

## Milestone 15 — Client Concurrency & Session Pooling <a id="15"></a> 
[Milestones](#Milestones)

**Why now (compiler):** Server-side pipelining is useless unless the CLI and automation harness can take advantage of it safely. Manifest-driven client policy keeps retries and pooling deterministic across deployments.

**Goal**
Add pooled sessions and retry policies to `cohsh`, governed by compiler-exported policy files.

**Deliverables**
- `apps/cohsh/src/lib.rs` extends `Shell` with a session pool (default manifest value: two control, four telemetry) and batched Twrite helper. `apps/cohsh/src/transport/tcp.rs` gains retry scheduling based on manifest policy.
- `apps/cohsh/tests/pooling.rs` verifies pooled throughput and idempotent retry behaviour.
- Manifest IR v1.3: `client_policies.cohsh.pool`, `client_policies.retry`, `client_policies.heartbeat`. Compiler emits `out/cohsh_policy.toml` consumed at runtime (CLI loads it on start, failing if missing/out-of-sync).
- CLI regression `scripts/cohsh/session_pool.coh` demonstrating increased throughput under load and safe recovery from injected failures.
- Docs (`docs/USERLAND_AND_CLI.md`) describe new CLI flags/env overrides, referencing manifest-derived defaults.

**Status:** Complete — session pooling, retry policies, policy hashing, CLI regression coverage, and docs updates are in place; regression pack is green.

**Commands**
- `cargo test -p cohsh`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/session_pool.coh`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`

**Checks (DoD)**
- Throughput benchmark (documented in test output) demonstrates improvement relative to single-session baseline without exceeding `msize` or server tag limits.
- Retry logic proves idempotent: injected short-write failures eventually succeed without duplicating telemetry or exhausting tickets.
- CLI refuses to start when the manifest policy hash mismatches the compiled defaults.
- Re-run the Regression Pack and assert that pooled sessions preserve ACK/ERR ordering and idempotent retries for all existing CLI scripts.
- Pooled sessions MUST NOT reorder ACKs across operations that were previously strictly ordered (attach/log/tail/quit baseline).
- Abuse case: pool exhaustion and forced retry after connection drop yields bounded retries and no duplicate commands; script asserts final counts.

**Compiler touchpoints**
- `coh-rtc` emits policy TOML plus hash recorded in docs/tests; regeneration guard compares CLI-consumed hash with manifest fingerprint.
- Docs embed CLI defaults via compiler-generated snippets to avoid drift.

**Task Breakdown**
```
Title/ID: m15-session-pool
Goal: Add pooled sessions and retry policies to cohsh.
Inputs: apps/cohsh/src/lib.rs, configs/root_task.toml client_policies.*.
Changes:
  - apps/cohsh/src/lib.rs — session pool, batched Twrite helper, policy hash enforcement.
  - apps/cohsh/src/transport/tcp.rs — retry scheduling and reconnect handling.
Commands:
  - cargo test -p cohsh --tests
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/session_pool.coh
Checks:
  - Connection drop triggers retries without duplicate telemetry; pool exhaustion returns deterministic ERR and audit.
Deliverables:
  - Updated client policy TOML emitted by compiler and referenced in docs/USERLAND_AND_CLI.md.

Title/ID: m15-throughput-benchmark
Goal: Measure throughput improvements and ensure ordering stability.
Inputs: apps/cohsh/tests/pooling.rs, scripts/cohsh/session_pool.coh outputs.
Changes:
  - apps/cohsh/tests/pooling.rs — throughput benchmark comparing single vs pooled sessions with injected short writes.
  - docs/SECURITY.md — note on ordering/idempotency with snippet from manifest.
Commands:
  - cargo test -p cohsh --test pooling
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/session_pool.coh
Checks:
  - ACK/ERR ordering unchanged from baseline; retries logged once per failure; benchmark shows expected throughput gain.
Deliverables:
  - Benchmark data archived; manifest hash updated in docs.
```
---

## Milestone 16 — Observability via Files (No New Protocols) <a id="16"></a> 
[Milestones](#Milestones)

**Why now (compiler):** Operators need structured observability without adding new protocols inside the VM. Manifest-defined `/proc` endpoints ensure metrics stay aligned with runtime behaviour.

**Goal**
Expose audit-friendly observability nodes under `/proc` generated from the manifest.

**Deliverables**
- `apps/nine-door/src/host/observe.rs` (new module) providing read-only providers for `/proc/9p/{sessions,outstanding,short_writes}` and `/proc/ingest/{p50_ms,p95_ms,backpressure,dropped,queued}` plus append-only `/proc/ingest/watch` snapshots.
- Event pump updates (`apps/root-task/src/event/mod.rs`) to update ingest metrics without heap allocation; telemetry forwarded through generated providers.
- Unit tests covering metric counters and ensuring no allocations on hot paths; CLI regression `scripts/cohsh/observe_watch.coh` tails `/proc/ingest/watch` verifying stable grammar.
- Manifest IR v1.3 fields: `observability.proc_9p` and `observability.proc_ingest` enabling individual nodes and documenting retention policies. Validation enforces bounded buffer sizes.
- Docs: `docs/SECURITY.md` gains monitoring appendix sourced from manifest snippets; `docs/INTERFACES.md` documents output grammar.

**Status:** Complete — /proc observability providers, ingest metrics hooks, CLI regressions, doc snippets, and regression pack coverage are aligned and green.

**Commands**
- `cargo test -p nine-door`
- `cargo test -p root-task`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/observe_watch.coh`

**Checks (DoD)**
- Stress harness records accurate counters; metrics exported via `/proc` match expected values within tolerance.
- CLI tail output remains parseable and line-oriented; regression test asserts exact output grammar.
- Compiler rejects manifests that attempt to enable observability nodes without allocating sufficient buffers.
- Re-run the Regression Pack to ensure `/proc` additions are strictly additive and do not change existing node sizes, EOF behaviour, or latency characteristics beyond documented tolerances.
- `/proc` nodes MUST NOT change default units, JSON keys, or column spacing without a manifest schema revision and updated golden outputs.
- Abuse case: rapid poll of `/proc/ingest/watch` under back-pressure does not allocate or drop counters; deterministic throttling logged.

**Compiler touchpoints**
- Generated code provides `/proc` descriptors; docs embed them via compiler output.
- As-built guard compares manifest-declared observability nodes with committed docs and fails CI if mismatched.

**Task Breakdown**
```
Title/ID: m16-proc-providers
Goal: Implement observability providers without new protocols.
Inputs: apps/nine-door/src/host/observe.rs, apps/root-task/src/event/mod.rs.
Changes:
  - apps/nine-door/src/host/observe.rs — providers for /proc/9p/* and /proc/ingest/* with bounded buffers.
  - apps/root-task/src/event/mod.rs — metrics update hooks without heap allocation.
Commands:
  - cargo test -p nine-door --test observe
  - cargo test -p root-task
Checks:
  - Under stress, metrics remain accurate; abuse case polling watch node throttles without allocations.
Deliverables:
  - Observability nodes documented in docs/INTERFACES.md and docs/SECURITY.md via compiler snippets.

Title/ID: m16-cli-regressions
Goal: Validate CLI grammar and negative cases for observability nodes.
Inputs: scripts/cohsh/observe_watch.coh.
Changes:
  - scripts/cohsh/observe_watch.coh — tail watch node, induce back-pressure, request unsupported node to assert ERR.
  - docs/SECURITY.md — capture latency/metric tolerances.
Commands:
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/observe_watch.coh
Checks:
  - Unsupported node returns deterministic ERR; watch grammar matches golden; back-pressure logs recorded.
Deliverables:
  - Regression transcript stored; manifest hash noted in docs.
```
---

## Milestone 17 — Content-Addressed Updates (CAS) — 9P-first <a id="17"></a> 
[Milestones](#Milestones)

**Why now (compiler):** Upcoming edge deployments need resumable, verifiable updates without bloating the VM with new protocols. Manifest-governed CAS ensures integrity rules and storage budgets remain enforceable.

**Goal**
Provide CAS-backed update distribution via NineDoor with compiler-enforced integrity policies.

**Deliverables**
- `apps/nine-door/src/host/cas.rs` implementing a CAS provider exposing `/updates/<epoch>/{manifest.cbor,chunks/<hash>}` with optional delta packs. Provider enforces SHA-256 chunk integrity and optional Ed25519 signatures when manifest enables `cas.signing`.
- Host tooling `apps/cas-tool/` (new crate) packaging update bundles, generating manifests, and uploading via Secure9P.
- CLI regression `scripts/cohsh/cas_roundtrip.coh` verifying download resume, signature enforcement, and delta replay.
- Models as CAS (registry semantics via files, no new service): expose `/models/<sha256>/{weights,schema,signature}` backed by the same CAS provider; include doc example binding a model into a worker namespace via mount/bind.
- CLI regression `scripts/cohsh/model_cas_bind.coh` uploads a dummy model bundle, verifies hash, and binds it into a worker namespace.
- Manifest IR v1.4 fields: `cas.enable`, `cas.store.chunk_bytes`, `cas.delta.enable`, `cas.signing.key_path`. Validation ensures chunk size ≤ negotiated `msize` and signing keys present when required.
- Docs: `docs/INTERFACES.md` describes CAS grammar, delta rules, and operational runbooks sourced from compiler output; `docs/SECURITY.md` records threat model.

**Status:** Complete — CAS provider, cas-tool, model bindings, compiler v1.4 CAS fields, doc snippets, and regression coverage are aligned and green.

**Commands**
- `cargo test -p nine-door`
- `cargo test -p cas-tool`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/cas_roundtrip.coh`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/model_cas_bind.coh`

**Checks (DoD)**
- Resume logic validated via regression script; delta application is idempotent and verified by hashing installed payloads before/after.
- Signing path tested with fixture keys; unsigned mode explicitly documented and requires manifest acknowledgement (e.g., `cas.signing.required = false`).
- Compiler rejects manifests where CAS storage exceeds event-pump memory budgets or chunk sizes exceed `msize`.
- Re-run the Regression Pack and verify that enabling CAS does not change baseline NineDoor error codes or 9P limits (e.g., `msize`, walk depth) enforced by earlier milestones.
- CAS fetch paths MUST NOT alter 9P latency or error codes for non-CAS workloads; regression pack MUST prove no change in baseline attach/log/tail flows.
- Model binding test proves `/models/<sha256>` mounts remain read-only and integrate with worker namespaces without introducing new services; manifest gating (e.g., `ecosystem.models.enable`) controls exposure.
- Abuse case: hash mismatch or signature verification failure rejects chunk and leaves partial downloads quarantined with deterministic audit.

**Compiler touchpoints**
- Codegen emits CAS provider tables and host-tool manifest templates; docs ingest the same JSON to prevent drift.
- Regeneration guard checks CAS manifest fingerprints against committed artefacts.
- Manifest validation ties CAS model exposure to `ecosystem.models.enable` and ensures model artefact sizes respect existing Secure9P `msize` and walk-depth limits.

**Task Breakdown**
```
Title/ID: m17-cas-provider
Goal: Implement CAS provider and manifest validation for updates/models.
Inputs: apps/nine-door/src/host/cas.rs, configs/root_task.toml cas.* fields.
Changes:
  - apps/nine-door/src/host/cas.rs — chunk integrity checks, delta packs, signature enforcement.
  - tools/coh-rtc/src/codegen/cas.rs — emit IR v1.4 fields and templates.
Commands:
  - cargo test -p nine-door --test cas_provider
  - cargo test -p tools/coh-rtc
Checks:
  - Hash mismatch causes ERR and quarantine; chunk_size > msize rejected at compile time.
Deliverables:
  - CAS grammar documented in docs/INTERFACES.md with compiler snippets.

Title/ID: m17-cas-regressions
Goal: Validate end-to-end CAS roundtrip and model binding.
Inputs: scripts/cohsh/cas_roundtrip.coh, scripts/cohsh/model_cas_bind.coh, apps/cas-tool/.
Changes:
  - apps/cas-tool/src/main.rs — bundle creation, manifest generation, upload helper.
  - scripts/cohsh/cas_roundtrip.coh — resume + signature paths including negative signature case.
  - scripts/cohsh/model_cas_bind.coh — bind model into worker namespace and assert read-only.
Commands:
  - cargo test -p cas-tool
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/cas_roundtrip.coh
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/model_cas_bind.coh
Checks:
  - Replay after interruption resumes without duplication; signature failure returns deterministic ERR and audit.
Deliverables:
  - Regression outputs stored; docs/SECURITY.md updated with CAS threat model references.
```
---

## Milestone 18 — Field Bus & Low-Bandwidth Sidecars (Host/Worker Pattern) <a id="18"></a> 
[Milestones](#Milestones)

**Why now (context):** Remaining edge use cases (Edge §§1–4,8,9; Science §§13–14) depend on deterministic adapters for industrial buses and constrained links. Implementing them as sidecars preserves the lean `no_std` core while meeting operational demands.

**Goal**
Deliver a library of host/worker sidecars (outside the VM where possible) that bridge MODBUS/DNP3, LoRa, and sensor buses into NineDoor namespaces, driven by compiler-declared mounts and capability policies.

**Deliverables**
- Host-side sidecar framework (`apps/sidecar-bus`) offering async runtimes on macOS/Linux with feature gates to keep VM artefacts `no_std`. Sidecars communicate via Secure9P transports or serial overlays without embedding TCP servers in the VM.
- Worker templates (`apps/worker-bus`, `apps/worker-lora`) that run inside the VM, remain `no_std`, and expose control/telemetry files (`/bus/*`, `/lora/*`) generated from manifest entries.
- Scheduling integration for LoRa duty-cycle management and tamper logging, aligned with `docs/USE_CASES.md` defense and science requirements.
- Compiler IR v1.5 fields `sidecars.modbus`, `sidecars.dnp3`, `sidecars.lora` describing mounts, baud/link settings, and capability scopes; validation ensures resources stay within event-pump budget.
- Documentation updates (`docs/ARCHITECTURE.md §12`, `docs/INTERFACES.md`) illustrating the sidecar pattern, security boundaries, and testing strategy.
- `scripts/cohsh/sidecar_integration.coh` integrated into the regression pack DoD.

**Status:** Complete — sidecar framework, worker templates, CLI regression coverage, and docs are aligned; Milestone 17 boundary remains `3e6faa33410af58ed8d1942ce58ab701a276b882`.

**Use-case alignment**
- Industrial IoT gateways (Edge §1) gain MODBUS/CAN integration without bloating the VM.
- Energy substations (Edge §2) receive DNP3 scheduling and signed config updates.
- Defense ISR kits (Edge §8) use LoRa scheduler + tamper logging, while environmental stations (Science §13) benefit from low-power telemetry scheduling.

**Commands**
- `cargo test -p worker-bus -p worker-lora`
- `cargo test -p sidecar-bus --features modbus,dnp3`
- `cohsh --script scripts/cohsh/sidecar_integration.coh`

**Checks (DoD)**
- Sidecars operate within declared capability scopes; attempts to access undeclared mounts are rejected and logged.
- LoRa scheduler enforces duty-cycle constraints under stress tests.
- Offline telemetry spooling validated for MODBUS/DNP3 adapters with manifest-driven limits.
- For this milestone, run the full Regression Pack both under QEMU and (where applicable) on the target hardware profile, and treat any divergence between the two as a bug unless explicitly documented.
- Sidecar mounts MUST NOT introduce new `/bus` or `/lora` names that collide with legacy namespaces; compiler must hash-prefix automatically if conflicts appear.
- Abuse case: unauthorized write into sidecar mount returns ERR and audit; duty-cycle violation attempt throttled and logged.

**Compiler touchpoints**
- IR v1.5 ensures mounts/roles/quotas for sidecars, generating documentation tables and manifest fragments consumed by host tooling.
- Validation prevents enabling sidecars without corresponding host dependencies or event-pump capacity.

**Task Breakdown**
```
Title/ID: m19-sidecar-framework
Goal: Build host sidecar framework and in-VM worker templates for bus/LoRa.
Inputs: apps/sidecar-bus/, apps/worker-bus/, apps/worker-lora/, configs/root_task.toml sidecars.*.
Changes:
  - apps/sidecar-bus/src/lib.rs — capability-scoped adapters with feature gates for modbus/dnp3.
  - apps/worker-lora/src/lib.rs — duty-cycle scheduler and tamper logging.
Commands:
  - cargo test -p sidecar-bus --features modbus,dnp3
  - cargo test -p worker-bus -p worker-lora
Checks:
  - Unauthorized mount access returns ERR; duty-cycle enforcement verified in tests.
Deliverables:
  - Sidecar patterns documented in docs/ARCHITECTURE.md and docs/INTERFACES.md.

Title/ID: m19-cli-regressions
Goal: Validate manifest-gated mounts and offline spooling behaviour.
Inputs: scripts/cohsh/sidecar_integration.coh, Regression Pack.
Changes:
  - scripts/cohsh/sidecar_integration.coh — mount enable/disable checks, offline spool replay, unauthorized write attempt.
  - docs/SECURITY.md — note on namespace collision avoidance via hash-prefix.
Commands:
  - cohsh --script scripts/cohsh/sidecar_integration.coh
Checks:
  - Disabled manifest hides mounts; offline spool flushes deterministically; unauthorized write produces ERR and audit.
Deliverables:
  - CLI transcript stored; manifest hash updated in docs.
```
---
## Milestone 19 — `cohsh-core` Extraction (Shared Grammar & Transport) <a id="19"></a> 
[Milestones](#Milestones)

**Why now (compiler):** UI and automation consumers need a shared grammar without duplicating console logic. Extracting a core library keeps ACK/ERR stability while enabling multiple frontends.

**Goal**
Publish a reusable `cohsh-core` crate with shared verb grammar and transports that mirror console semantics. cohsh-core is a grammar + transport library only; it adds no new verbs or semantics.

**Deliverables**
- New crate `crates/cohsh-core/` encapsulating verb grammar (`attach`, `tail`, `spawn`, `kill`, `quit`), ACK/ERR/END model, login throttling, and ticket checks. Supports `no_std + alloc` with optional smoltcp TCP transport feature.
- Golden transcript fixtures covering serial, TCP, and in-process transports to prove byte-identical ACK/ERR sequences.
- CLI harness using `cohsh-core` to ensure parity with existing `cohsh` commands; docs reference the shared grammar.

**Status:** Complete — cohsh-core grammar/ACK models are shared by console and CLI, transcript parity is enforced across transports, and coh-rtc emits guarded grammar/policy snippets.

**Commands**
- `cargo test -p cohsh-core`
- `cargo test -p cohsh --tests`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/boot_v0.coh`

**Checks (DoD)**
- Console (serial/TCP) ≡ `cohsh` CLI ≡ `cohsh-core` tests (byte-for-byte ACK/ERR/END); regression harness compares transcripts.
- Heapless build passes; no unbounded allocations and no POSIX dependencies.
- Abuse case: invalid ticket or throttled login returns deterministic ERR without advancing state; fixture captures denial.
- UI/CLI/console equivalence MUST be preserved: ACK/ERR/END sequences must remain byte-stable relative to the 7c baseline.

**Compiler touchpoints**
- `coh-rtc` emits grammar snippets and ticket policies into docs/USERLAND_AND_CLI.md; regeneration guard ensures hash alignment with `cohsh-core` fixtures.

**Task Breakdown**
```
Title/ID: m19-core-crate
Goal: Extract shared verb grammar and transports into cohsh-core.
Inputs: apps/cohsh/src/lib.rs existing grammar, scripts/cohsh/boot_v0.coh fixtures.
Changes:
  - crates/cohsh-core/lib.rs — verb parser, ACK/ERR model, smoltcp TCP transport feature.
  - apps/cohsh/src/lib.rs — refactor to consume cohsh-core.
Commands:
  - cargo test -p cohsh-core
  - cargo test -p cohsh --tests
Checks:
  - Invalid ticket returns deterministic ERR; heapless build passes without allocations beyond bounded buffers.
Deliverables:
  - Shared crate and regenerated grammar snippets in docs/USERLAND_AND_CLI.md.

Title/ID: m19-transcript-harness
Goal: Ensure transcript parity across console/TCP/core transports.
Inputs: scripts/cohsh/boot_v0.coh, new tests in crates/cohsh-core/tests/transcripts.rs.
Changes:
  - crates/cohsh-core/tests/transcripts.rs — compare serial vs TCP vs in-process transcripts.
  - scripts/regression/transcript_diff.sh — automated diff runner (if existing harness, extend).
Commands:
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/boot_v0.coh
  - cargo test -p cohsh-core --test transcripts
Checks:
  - Transcript diff produces zero-byte delta; abuse case with throttled login emits ERR and matches across transports.
Deliverables:
  - Stored golden fixtures and updated regression harness documentation.
```

---

## Milestone 20a — `cohsh` as 9P Client Library <a id="20a"></a> 
[Milestones](#Milestones)

**Why now (compiler):** Automation and UI need a library-level 9P client that reuses grammar without console coupling. A first-class client library keeps ordering/idempotency intact.

**Goal**
Refactor `cohsh` into a reusable 9P client library with helpers for control verbs and streaming tails.

**Deliverables**
- `CohClient` exposing `open/read/write/clunk` plus `tail()` streaming helper built atop `cohsh-core` 9P transport.
- Convenience helpers for `/queen/ctl` JSON (`spawn`, `kill`, `budget`) with manifest-derived defaults.
- Script harness replaying sessions via 9P (not console) to validate identical semantics; golden fixtures maintained.

**Commands**
- `cargo test -p cohsh --test client_lib`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/session_pool.coh`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/boot_v0.coh`

**Checks (DoD)**
- `tail()` stream over 9P matches console stream identically; diff harness reports zero variance.
- `spawn/kill` via file writes produce identical ACK/ERR semantics; clients may retry explicitly; operations are designed to be idempotent where applicable.
- Abuse case: attempt to walk `..` or access disabled namespace returns deterministic ERR without affecting state.
- UI/CLI/console equivalence MUST be preserved: ACK/ERR/END sequences must remain byte-stable relative to the 7c baseline.

**Compiler touchpoints**
- Manifest-derived paths and defaults emitted by `coh-rtc` into client templates; docs updated via snippets.

**Task Breakdown**
```
Title/ID: m20a-client-api
Goal: Build CohClient library with tail and control helpers.
Inputs: apps/cohsh/src/lib.rs, crates/cohsh-core transport.
Changes:
  - apps/cohsh/src/client.rs — CohClient struct with open/read/write/clunk and tail helper.
  - apps/cohsh/src/queen.rs — spawn/kill/budget helpers wrapping JSON writes.
Commands:
  - cargo test -p cohsh --test client_lib
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/boot_v0.coh
Checks:
  - Walking `..` or disabled namespace returns ERR; tail stream matches console transcript.
Deliverables:
  - Client API docs in docs/USERLAND_AND_CLI.md via compiler snippet.

Title/ID: m20a-replay-harness
Goal: Replay sessions over 9P and compare to console baselines.
Inputs: scripts/cohsh/session_pool.coh, new regression harness for 9P replay.
Changes:
  - scripts/cohsh/session_pool.coh — add 9P-only replay path and abuse case for forbidden walk.
  - scripts/regression/client_vs_console.sh — compares ACK/ERR across transports.
Commands:
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/session_pool.coh
Checks:
  - Replay harness shows zero diff between 9P and console outputs; abuse case logs ERR without side effects.
Deliverables:
  - Updated regression pack metadata and manifest hashes.
```

**Status:** Complete — CohClient API and queen helpers are in tree, client defaults are compiler-emitted, and the 9P replay harness matches console transcripts with regression coverage.

---

## Milestone 20b — NineDoor UI Providers <a id="20b"></a> 
[Milestones](#Milestones)

**Why now (compiler):** UI surfaces need read-only summaries without adding protocols. Providers must reuse existing `/proc` mechanics and stay bounded.

**Goal**
Expose UI-friendly read-only providers under NineDoor with cursor-resume semantics and CBOR/text variants.

**Deliverables**
- Providers for `/proc/9p/{sessions,outstanding,short_writes}`, `/proc/ingest/{p50_ms,p95_ms,backpressure}`, `/policy/preflight/{req,diff}`, `/updates/<epoch>/{manifest.cbor,status}` with deterministic EOF and 32 KiB read bounds.
- CBOR and text outputs aligned with manifest schemas; cursor resume for long reads.
- UI fixtures documenting provider outputs for SwarmUI and CLI parity.

**Commands**
- `cargo test -p nine-door --test ui_providers`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/observe_watch.coh`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/cas_roundtrip.coh`

**Checks (DoD)**
- Each provider ≤ 8192 bytes per 9P read; larger outputs must be cursor-resumed over multiple reads with deterministic EOF; fuzzed frames don’t panic or allocate unboundedly.
- Abuse case: request for disabled provider or oversized read returns deterministic ERR and audit line.
- UI/CLI/console equivalence MUST be preserved: ACK/ERR/END sequences must remain byte-stable relative to the 7c baseline.

**Compiler touchpoints**
- Manifest toggles for UI providers emitted via `coh-rtc` and referenced in docs/INTERFACES.md and docs/ARCHITECTURE.md.

**Task Breakdown**
```
Title/ID: m20b-provider-impl
Goal: Implement bounded UI providers with CBOR/text outputs.
Inputs: apps/nine-door/src/host/{observe.rs,policy.rs,updates.rs}, manifest toggles.
Changes:
  - apps/nine-door/src/host/observe.rs — add text + CBOR variants with cursor resume.
  - apps/nine-door/src/host/policy.rs — /policy/preflight providers with diff output.
Commands:
  - cargo test -p nine-door --test ui_providers
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/observe_watch.coh
Checks:
  - Disabled provider returns ERR; read beyond 32 KiB rejected; fuzz harness passes without panics.
Deliverables:
  - Provider docs and CBOR schemas refreshed in docs/INTERFACES.md.

Title/ID: m20b-updates-status
Goal: Surface update status for UI consumption via NineDoor.
Inputs: apps/nine-door/src/host/cas.rs status hooks, scripts/cohsh/cas_roundtrip.coh.
Changes:
  - apps/nine-door/src/host/cas.rs — expose /updates/<epoch>/{manifest.cbor,status} read-only nodes.
  - scripts/cohsh/cas_roundtrip.coh — add status fetch and disabled-provider abuse case.
Commands:
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/cas_roundtrip.coh
Checks:
  - Status node respects cursor resume; disabled updates return ERR without touching CAS store.
Deliverables:
  - UI fixture outputs stored; docs reference status grammar.
```

**Status:** Complete — UI providers are manifest-gated with bounded cursor-resume semantics, CBOR/text parity, and audit-deny paths; tests and regression pack are green.

---

## Milestone 20c — SwarmUI Desktop (Tauri, Pure 9P/TCP) <a id="20c"></a> 
[Milestones](#Milestones)

**Why now (compiler):** Desktop operators need a UI that *reflects the namespace* and reuses the existing 9P grammar without introducing new transports or control semantics. SwarmUI must prove strict parity with CLI behavior and respect ticket-scoped authority.

**Goal**  
Deliver a SwarmUI desktop (Tauri) that speaks 9P via `cohsh-core`, renders namespace-derived telemetry and fleet views, and supports deterministic offline inspection via cached CBOR snapshots. SwarmUI adds **no new verbs** and **no in-VM services**.

SwarmUI is a thin presentation layer only: all protocol semantics, state machines, parsing, and policy live in Rust (cohsh-core); any WASM or frontend code is rendering-only and must not implement verbs, retries, background polling, caching policy, or independent state.

**Deliverables**
- `apps/swarmui/` Tauri app with Rust backend linked to `cohsh-core`; **host-only**, no HTTP/REST dependencies.
- Namespace-driven panels:
  - **Telemetry Rings** (tail `/worker/*/telemetry`).
  - **Fleet Map** (read `/proc/ingest/*` + worker directories).
  - Optional **Namespace Browser** (read-only tree over `/proc`, `/queen`, `/worker`, `/log`, `/gpu`, indicating read/append-only paths).
- Offline inspection via bounded CBOR cache under `$DATA_DIR/snapshots/` (opt-in; read-only when offline).
- Ticket/lease auth identical to CLI; **per-ticket session views** supported; role-scoped interactions enforced client-side.

**Commands**
- `cargo test -p cohsh-core`
- `cargo test -p swarmui`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/telemetry_ring.coh`

**Checks (DoD)**
- UI renders telemetry with exact `OK …` then stream and terminates with `END`; transcript matches CLI byte-for-byte.
- Build proves no HTTP/REST dependencies (static link audit or cargo deny).
- Abuse case: expired or unauthorized ticket returns `ERR` surfaced verbatim in UI and logs audit; offline mode uses cached CBOR without network or retries.
- UI/CLI/console equivalence MUST be preserved: ACK/ERR/END sequences remain byte-stable relative to the 7c baseline.
- SwarmUI performs no background polling outside an active user view/session; no hidden watchers when idle.

**Compiler touchpoints**
- UI defaults (paths, cache size, ticket scope) emitted by `coh-rtc` for SwarmUI config; `docs/USERLAND_AND_CLI.md` references the same sources of truth.

**Task Breakdown**
```
Title/ID: m20c-ui-backend
Goal: Wire SwarmUI backend to cohsh-core with 9P-only transport and per-ticket sessions.
Inputs: apps/swarmui/src-tauri/, crates/cohsh-core, scripts/cohsh/telemetry_ring.coh.
Changes:
- apps/swarmui/src-tauri/main.rs — session management (per ticket), ticket auth, telemetry tail via cohsh-core.
- apps/swarmui/Cargo.toml — ensure no HTTP/REST deps; enable bounded offline cache feature.
Commands:
- cargo test -p swarmui
- cargo run -p cohsh –-features tcp – –transport tcp –script scripts/cohsh/telemetry_ring.coh
Checks:
- Unauthorized ticket returns ERR surfaced verbatim in UI; offline mode reads CBOR snapshot only.
Deliverables:
- UI backend notes and cache path documented in docs/USERLAND_AND_CLI.md.

Title/ID: m20c-ui-fixtures
Goal: Capture UI/CLI transcript parity and namespace-derived fleet rendering.
Inputs: UI snapshot fixtures, /proc/ingest providers.
Changes:
- apps/swarmui/tests/transcript.rs — compare UI-captured ACK/ERR/END to CLI golden.
- apps/swarmui/src/cache.rs — snapshot write/read with strict size bounds and expiry handling.
Commands:
- cargo test -p swarmui –test transcript
- cargo test -p cohsh-core
Checks:
- Transcript diff zero; cache bounded to documented size; expired cache rejected gracefully.
Deliverables:
- Updated docs/INTERFACES.md with SwarmUI consumption guidance and non-goals.
```
**Status:** Complete — SwarmUI desktop is wired to cohsh-core with ticket-scoped sessions, transcript parity tests, bounded offline cache, and no-HTTP dependency enforcement; regression pack remains green.
---
## Milestone 20d — SwarmUI Live Hive Rendering (PixiJS, GPU-First) <a id="20d"></a> 
[Milestones](#Milestones)

**Why now (SwarmUI):**  
Milestone 20d proves SwarmUI can act as a strict, ticket-scoped presentation layer over the 9P namespace with byte-stable CLI parity. The remaining risk is visual overload or architectural drift (SVG/D3 DOM graphs, UI-invented state, per-event rendering). This extension locks in a **single, elegant, GPU-first “Live Hive” renderer** that is visually compelling while remaining protocol-faithful, deterministic, and bounded.

**Goal**  
Extend SwarmUI with a PixiJS-backed “Live Hive” view that renders agents, work, and flow as a continuous simulation derived solely from existing telemetry and event streams. The renderer introduces **no new verbs**, **no new transports**, and **no UI-owned control logic**. All authority, parsing, and semantics remain in Rust (`cohsh-core`).

SwarmUI remains a thin presentation layer: frontend code renders only. All protocol behavior, retries, caching policy, and state machines live in Rust. Any frontend logic must be strictly lossy, bounded, and discardable.

### Deliverables
- **Live Hive Canvas**
  - PixiJS (WebGL) scene embedded in SwarmUI.
  - Visual primitives:
    - **Agents (“bees”)** — sprites with subtle motion and state-based glow.
    - **Work/messages (“pollen”)** — short-lived particles flowing between agents.
    - **Load/health** — aura intensity or soft heat field derived from telemetry.
    - **Errors** — transient pulse/shockwave effects surfaced from `ERR` events.
    - **Namespaces/groups** — faint, collapsible cluster hulls mapped from namespace paths.
  - SVG permitted **only** for labels and selection overlays; never for core rendering.

- **Simulation Model (Frontend)**
  - Lightweight, ephemeral world model decoupled from the render loop:
    - agent positions/velocities/state flags
    - ephemeral flows/messages
    - optional low-resolution heat grid
  - Fixed or semi-fixed update step; render capped (30–60fps).
  - Explicit level-of-detail rules:
    - Zoomed out → clusters + aggregate flow intensity.
    - Zoomed in → individual agents + per-message particles.
    - Under load → degrade to edge intensity; never drop frames.

- **Event Ingestion Contract**
  - Consume the same event streams used by telemetry panels (`tail` over namespace paths).
  - Event → simulation diff mapping only; no per-event draw guarantees.
  - No UI-specific protocol extensions.

- **Replay & Demo Mode**
  - Live Hive can be driven entirely from:
    - recorded transcripts
    - cached CBOR snapshots
  - Deterministic playback for demos, regression tests, and offline inspection.

### Non-Goals
- No SVG/D3 graph as the primary renderer.
- No Web-only (WASM-only) SwarmUI target yet.
- No UI-invented orchestration, scheduling, or heuristics.
- No attempt to visualise every raw event individually.

### Commands
- `cargo test -p cohsh-core`
- `cargo test -p swarmui`
- `cargo run -p swarmui -- --replay $DATA_DIR/snapshots/demo.cbor`

### Checks (DoD)
- Live Hive renders identically when driven by:
  1) a live Cohesix node
  2) a recorded transcript
- UI actions emit byte-identical `ACK/ERR/END` sequences to CLI for equivalent verbs.
- Sustained high event rates do not reduce UI responsiveness or violate frame caps.
- No HTTP/REST dependencies introduced; no background polling outside active views.
- Renderer remains discardable: restarting the UI reconstructs state solely from streams/snapshots.

### Compiler Touchpoints
- UI defaults (hive LOD thresholds, frame caps, snapshot limits) emitted by `coh-rtc`.
- `docs/INTERFACES.md` updated to describe Live Hive as a **rendering view only**, not a control surface.

### Task Breakdown
```
Title/ID: m20d-hive-renderer
Goal: Add GPU-first Live Hive renderer without altering protocol semantics.
Inputs: apps/swarmui/, crates/cohsh-core, telemetry streams, CBOR snapshots.
Changes:
- apps/swarmui/frontend/hive/ — PixiJS scene, simulation model, LOD rules.
- apps/swarmui/frontend/events.js — event → simulation diff mapping.
- apps/swarmui/src-tauri/ — replay mode wiring (no new verbs).
Commands:
- cargo test -p swarmui
- cargo run -p swarmui – –replay demo.cbor
Checks:
- Frame rate bounded; transcript parity preserved; no new deps.
Deliverables:
- Live Hive view documented as non-authoritative renderer in docs/INTERFACES.md.

Title/ID: m20d-hive-fixtures
Goal: Prove deterministic rendering and replay stability.
Inputs: golden transcripts, CBOR snapshots.
Changes:
- apps/swarmui/tests/replay.rs — snapshot-driven render smoke tests.
- docs/INTERFACES.md — Live Hive non-goals and degradation rules.
Commands:
- cargo test -p swarmui –test replay
Checks:
- Replay produces stable visual state; expired snapshots rejected cleanly.
Deliverables:
- Golden demo snapshots committed for CI and demos.

Title/ID: m20d-design-fonts
Goal: Establish a cross-platform, UI-safe font system aligned with Tauri and PixiJS best practices.
Inputs: apps/swarmui/, design guidelines, Tauri asset bundling.
Changes:
- apps/swarmui/frontend/assets/fonts/ — bundle Inter and JetBrains Mono font files (limited weights only).
- apps/swarmui/frontend/styles/fonts.css — define canonical font stacks and defaults.
- apps/swarmui/frontend/styles/tokens.css — expose font tokens (`--font-ui`, `--font-mono`, sizes, line-heights).
- Disable ligatures by default for monospace; expose opt-in toggle.
Commands:
- cargo test -p swarmui
Checks:
- Fonts load from local assets only (no system or network dependency).
- Text renders consistently across macOS, Windows, and Linux.
Deliverables:
- Documented font policy and usage rules in docs/INTERFACES.md.

Title/ID: m20d-design-colors
Goal: Define a minimal, dark-first color system shared by HTML UI and PixiJS hive renderer.
Inputs: SwarmUI frontend, PixiJS renderer.
Changes:
- apps/swarmui/frontend/styles/colors.css — base palette, semantic colors, opacity rules.
- apps/swarmui/frontend/styles/tokens.css — color tokens shared by UI and canvas overlays.
- apps/swarmui/frontend/hive/palette.js — PixiJS color constants derived from tokens.
Commands:
- cargo test -p swarmui
Checks:
- No hard-coded colors outside token files.
- Semantic colors (ACK/ERR/flow/load) map consistently between UI and hive.
Deliverables:
- Color token table and usage notes added to docs/INTERFACES.md.

Title/ID: m20d-design-layout
Goal: Lock down layout, spacing, and panel rules for a dense operator UI.
Inputs: SwarmUI frontend panels.
Changes:
- apps/swarmui/frontend/styles/layout.css — spacing scale (4/8/12/16/24/32), panel rules.
- Remove shadows; enforce separation via tone and spacing only.
- Standardise panel chrome (headers, dividers, empty states).
Commands:
- cargo test -p swarmui
Checks:
- No arbitrary spacing values outside the defined scale.
- Panels render consistently across platforms and DPI settings.
Deliverables:
- Layout and spacing rules documented for contributors.

Title/ID: m20d-design-icons
Goal: Standardise iconography for SwarmUI controls and panels.
Inputs: SwarmUI frontend.
Changes:
- apps/swarmui/frontend/assets/icons/ — bundle Phosphor Icons SVG subset.
- apps/swarmui/frontend/components/icon.js — single icon wrapper enforcing size/weight.
- Replace mixed or ad-hoc icons with Phosphor set.
Commands:
- cargo test -p swarmui
Checks:
- Single icon set used everywhere.
- Icon weights consistent for default vs active states.
Deliverables:
- Icon usage guidelines added to docs/INTERFACES.md.

Title/ID: m20d-hive-visual-language
Goal: Define and enforce the visual language for the Live Hive renderer.
Inputs: PixiJS hive renderer.
Changes:
- apps/swarmui/frontend/hive/style.js — shape, motion, glow, and blending constants.
- Enforce circle/soft-blob primitives only; no sharp geometry.
- Define motion easing and pulse rules for normal vs error states.
Commands:
- cargo test -p swarmui
Checks:
- Hive visuals conform to documented motion and shape rules.
- Error pulses are single-shot and bounded.
Deliverables:
- Live Hive visual language documented as non-authoritative rendering rules.

Title/ID: m20d-design-tokens
Goal: Centralise all design constants into a single token system.
Inputs: SwarmUI frontend, PixiJS renderer.
Changes:
- apps/swarmui/frontend/styles/tokens.css — fonts, colors, spacing, motion.
- apps/swarmui/frontend/hive/tokens.js — generated or mirrored constants for PixiJS.
- Remove duplicated constants across UI and renderer.
Commands:
- cargo test -p swarmui
Checks:
- No duplicated magic numbers in UI or hive renderer.
- Token changes propagate consistently.
Deliverables:
- Single source-of-truth design tokens referenced in docs/INTERFACES.md.
```
**Status:** Complete — Live Hive PixiJS rendering is wired with deterministic replay fixtures, compiler-emitted hive defaults, and documented design tokens; regression pack is green.
---

## Milestone 20e — CLI/UI Convergence Tests <a id="20e"></a> 
[Milestones](#Milestones)

**Status:** Complete — Convergence harness, shared fixtures, and CI guards enforce byte-stable ACK/ERR/END parity with documented timing tolerance; regression pack is green.

**Why now (compiler):** After UI/CLI/library convergence, we need hard regression proof across all frontends with deterministic timing windows.

**Goal**
Establish a convergence harness comparing console, `cohsh`, `cohsh-core`, SwarmUI, and coh-status transcripts with CI enforcement.

**Deliverables**
- Golden transcript harness comparing console, `cohsh`, `cohsh-core`, SwarmUI, and coh-status for `help → attach → log → spawn → tail → quit`.
- CI job that fails on any byte-level drift in ACK/ERR/END and records timing deltas (< 50 ms tolerance: test harness tolerance; not a protocol contract) in artifacts.
- Shared transcript fixtures stored in `tests/fixtures/transcripts/` consumed by all frontends.

**Commands**
- `cargo test -p cohsh-core --test transcripts`
- `cargo test -p cohsh --test transcripts`
- `cargo test -p swarmui --test transcript`
- `cargo test -p coh-status --test transcript`

**Checks (DoD)**
- Script matches across all frontends; timing deltas < 50 ms in smoltcp simulation (tolerance documented).
- Abuse case: intentionally corrupted transcript triggers CI failure and deterministic diff output.
- UI/CLI/console equivalence MUST be preserved: ACK/ERR/END sequences must remain byte-stable relative to the 7c baseline.

**Compiler touchpoints**
- Manifest fingerprints and transcript hashes recorded in docs/TEST_PLAN.md; regeneration guard verifies alignment.

**Task Breakdown**
```
Title/ID: m20e-transcript-suite
Goal: Build shared transcript fixtures and comparison harness.
Inputs: tests/fixtures/transcripts/, console + TCP outputs.
Changes:
  - scripts/regression/transcript_compare.sh — capture and diff transcripts.
  - crates/cohsh-core/tests/transcripts.rs — reuse fixtures for unit validation.
Commands:
  - cargo test -p cohsh-core --test transcripts
  - cargo test -p cohsh --test transcripts
Checks:
  - Corrupted fixture causes deterministic failure with clear diff; clean run matches byte-for-byte.
Deliverables:
  - Transcript fixtures stored; docs/TEST_PLAN.md references hashes.

Title/ID: m20e-ui-cli-sync
Goal: Integrate SwarmUI/coh-status into convergence CI.
Inputs: apps/swarmui/tests/transcript.rs, apps/coh-status/tests/transcript.rs.
Changes:
  - apps/swarmui/tests/transcript.rs — capture UI transcript and feed into shared fixtures.
  - apps/coh-status/tests/transcript.rs — same for status tool.
Commands:
  - cargo test -p swarmui --test transcript
  - cargo test -p coh-status --test transcript
Checks:
  - UI/CLI/console produce identical ACK/ERR/END; timing tolerances enforced.
Deliverables:
  - CI job definition referencing convergence tests; docs updated with expected tolerances.
```

---

## Milestone 20f — UI Security Hardening (Tickets & Quotas) <a id="20f"></a> 
[Milestones](#Milestones)

**Status:** Complete — Ticket scopes/quotas are enforced; multi-worker cohsh parity, command surface checks, and deterministic regression batching validated; host ticket mint one-shots shipped.

**Why now (compiler):** With UI parity established, enforce least privilege and quotas to protect interactive sessions.

**Goal**
Lock UI/CLI security quotas and console grammar parity while proving cohsh works cleanly with multiple workers and a deterministic regression batch.

**Deliverables**
- Ticket scopes `{path, verb, rate}` with per-ticket bandwidth and cursor quotas enforced in NineDoor and consumed by UI/CLI.
- `PumpMetrics` adds `ui_reads`, `ui_denies`; audit lines emitted for denials with manifest-driven limits.
- CLI/UI regression scripts prove permission denials and quota breaches across transports.
- Cohsh multi-worker regression coverage exercises spawn/tail/kill across multiple worker telemetry paths without ID drift.
- `scripts/cohsh/run_regression_batch.sh` is a reliable manual compliance pack for this milestone (base + gated, deterministic worker-id scripts).
- SwarmUI/CLI transcripts remain byte-stable against cohsh-core fixtures; no ACK/ERR/END drift.
- Cohsh and SwarmUI add host-only ticket mint one-shots that do not alter console grammar.

**Commands**
- `cargo test -p nine-door --test ui_security`
- `cargo test -p cohsh-core`
- `cargo test -p cohsh --test script_catalog`
- `cargo test -p swarmui --test security`
- `scripts/regression/transcript_compare.sh`
- `scripts/regression/client_vs_console.sh`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/telemetry_ring.coh`
- `scripts/cohsh/run_regression_batch.sh`

**Checks (DoD)**
- Write with read-only ticket → `ERR EPERM` across all transports; denial audited.
- Quota breach → `ERR ELIMIT`; audit lines consistent and deterministic; no duplicate frames.
- Abuse case: replayed ticket beyond expiry refuses access with deterministic ERR without consuming additional quota.
- Multi-worker regression: spawn two workers, tail each telemetry path, kill both, and observe no path errors.
- Cohsh command surface checklist passes for queen + worker sessions without console disconnects.
- SwarmUI console transport passes transcript/security tests with no grammar drift.
- Regression batch passes base + gated with deterministic worker-id scripts and archived logs.
- UI/CLI/console equivalence preserved: ACK/ERR/END sequences remain byte-stable relative to the 7c baseline.
- Cohsh `--mint-ticket` prints a worker/queen token; SwarmUI "Mint Ticket" button and `--mint-ticket` return the same token format and enforce worker subject requirements.

**Compiler touchpoints**
- `coh-rtc` emits ticket quota tables and hashes referenced by docs/SECURITY.md and docs/USERLAND_AND_CLI.md; regeneration guard enforces consistency.

**Task Breakdown**
```
Title/ID: m20f-ticket-quotas
Goal: Enforce per-ticket path/verb/rate quotas with audit metrics.
Inputs: apps/nine-door/src/host/security.rs, PumpMetrics.
Changes:
  - apps/nine-door/src/host/security.rs — quota checks, ui_denies/ui_reads metrics.
  - apps/nine-door/src/host/telemetry/mod.rs — audit lines for denials.
Commands:
  - cargo test -p nine-door --test ui_security
Checks:
  - Quota breach triggers ERR ELIMIT and increments metrics; replayed ticket denied deterministically.
Deliverables:
  - Quota tables documented via compiler output in docs/SECURITY.md.

Title/ID: m20f-cli-ui-regressions
Goal: Validate quota enforcement and multi-worker parity across CLI and UI clients.
Inputs: scripts/cohsh/telemetry_ring.coh, scripts/cohsh/shard_1k.coh, apps/cohsh/tests/script_catalog.rs, apps/swarmui/tests/security.rs.
Changes:
  - scripts/cohsh/telemetry_ring.coh — ensure read-only ticket write attempt and quota exhaustion loop remain deterministic.
  - scripts/cohsh/shard_1k.coh — add multi-worker coverage (second spawn + telemetry checks).
  - apps/cohsh/tests/script_catalog.rs — refresh script hashes to include updated regression scripts.
  - apps/swarmui/tests/security.rs — mirror quota abuse from UI.
Commands:
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/telemetry_ring.coh
  - cargo test -p cohsh --test script_catalog
  - cargo test -p swarmui --test security
Checks:
  - ERR EPERM/ELIMIT identical across transports; metrics observed in /proc/ingest/watch.
  - Multi-worker telemetry paths pass `tail` without invalid path errors.
Deliverables:
  - Regression outputs captured; manifest hash noted in docs/USERLAND_AND_CLI.md.

Title/ID: m20f-regression-batch-determinism
Goal: Make the regression batch deterministic for worker-id dependent scripts and document it in the test plan.
Inputs: scripts/cohsh/run_regression_batch.sh, docs/TEST_PLAN.md, resources/proc_tests/selftest_full.coh.
Changes:
  - scripts/cohsh/run_regression_batch.sh — isolate worker-id scripts into dedicated boots and archive logs per script.
  - resources/proc_tests/selftest_full.coh — align worker ids with deterministic spawn ordering.
  - docs/TEST_PLAN.md — add regression batch requirements and ordering.
Commands:
  - scripts/cohsh/run_regression_batch.sh
Checks:
  - Regression batch passes with no worker-id drift across scripts.
Deliverables:
  - Updated test plan documenting the manual regression pack.

Title/ID: m20f-cohsh-tcp-pool-safety
Goal: Stabilize cohsh TCP pooling against the single-console connection while preserving pool bench semantics.
Inputs: apps/cohsh/src/transport/tcp.rs, apps/cohsh/src/main.rs, scripts/cohsh/session_pool.coh.
Changes:
  - apps/cohsh/src/transport/tcp.rs — add pooled TCP wrapper that avoids extra ATTACH/QUIT on shared connections.
  - apps/cohsh/src/main.rs — use pooled TCP wrapper for session pool factory.
  - apps/cohsh/src/lib.rs — adjust pool bench TCP expectations, payload limits, and skip CAT readback on console transports.
  - docs/USERLAND_AND_CLI.md — document TCP console pool bench expectations.
Commands:
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/session_pool.coh
Checks:
  - cohsh interactive commands do not drop the console connection; pool bench reports OK.
Deliverables:
  - Regression logs covering session_pool.coh.

Title/ID: m20f-swarmui-console-alignment
Goal: Align SwarmUI with the TCP console transport and add telemetry tail support without changing ACK/ERR grammar.
Inputs: apps/swarmui/src-tauri/main.rs, apps/swarmui/src/lib.rs, apps/root-task/src/event/mod.rs, apps/root-task/src/ninedoor.rs, docs/USERLAND_AND_CLI.md, docs/INTERFACES.md.
Changes:
  - apps/root-task/src/event/mod.rs — stream telemetry ring contents for tail requests with cursor tracking.
  - apps/root-task/src/ninedoor.rs — expose worker telemetry reads for console tail.
  - apps/swarmui/src/lib.rs — add console backend using cohsh transport and server-managed telemetry tails.
  - apps/swarmui/src-tauri/main.rs — select console vs 9P transport via env settings.
  - docs/USERLAND_AND_CLI.md — document SwarmUI transport selection and console telemetry tail behavior.
  - docs/INTERFACES.md — record SwarmUI console transport alignment and non-goals.
Commands:
  - cargo check -p cohsh -p swarmui -p root-task
  - SEL4_BUILD_DIR=$HOME/seL4/build ./scripts/cohesix-build-run.sh --sel4-build "$HOME/seL4/build" --out-dir out/cohesix --profile release --root-task-features cohesix-dev --cargo-target aarch64-unknown-none --raw-qemu --transport tcp
Checks:
  - SwarmUI connects via console transport and renders live hive updates.
  - Interactive cohsh command set succeeds against TCP console.
Deliverables:
  - Updated docs and telemetry tail audit logs.

Title/ID: m20f-console-frame-integrity
Goal: Prevent partial TCP console sends from corrupting frame boundaries for cohsh and SwarmUI sessions.
Inputs: apps/root-task/src/net/stack.rs, apps/root-task/src/drivers/virtio/net.rs, logs/qemu-run.log, logs/cohsh-queen-interactive.log.
Changes:
  - apps/root-task/src/net/stack.rs — gate TCP sends on available TX capacity; abort on partial send.
  - apps/root-task/src/drivers/virtio/net.rs — fix TX written_len accounting to avoid payload truncation on repeated bytes.
Commands:
  - cargo check -p root-task -p cohsh -p swarmui
  - SEL4_BUILD_DIR=$HOME/seL4/build ./scripts/cohesix-build-run.sh --sel4-build "$HOME/seL4/build" --out-dir out/cohesix --profile release --root-task-features cohesix-dev --cargo-target aarch64-unknown-none --raw-qemu --transport tcp
  - ./out/cohesix/host-tools/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen
  - scripts/cohsh/run_regression_batch.sh
Checks:
  - Interactive cohsh commands (tail, ping, ls, cat, echo, spawn, kill, bind, mount) remain attached without reconnect loops.
  - SwarmUI console session stays connected and updates hive telemetry.
  - Regression pack passes unchanged.
  - virtio-net TX logs show written_len == payload_len for console frames; no invalid UTF-8 in console stream.
Deliverables:
  - Updated qemu + cohsh logs showing stable tail output.

Title/ID: m20f-console-utf8-safe-truncation
Goal: Prevent log truncation from emitting invalid UTF-8 that drops console sessions.
Inputs: apps/root-task/src/net/outbound.rs, logs/cohsh-queen-*.log.
Changes:
  - apps/root-task/src/net/outbound.rs — truncate log lines on UTF-8 boundaries; add regression test.
Commands:
  - cargo test -p root-task
Checks:
  - LineBuf truncation preserves valid UTF-8; cohsh no longer drops on bind/mount sequence.
Deliverables:
  - Updated root-task test output; cohsh interactive logs for bind/mount.

Title/ID: m20f-console-parity-plan
Goal: Capture a reproducible trace of TCP console frame integrity issues and validate cohsh/SwarmUI parity before fixes.
Inputs: logs/tcpdump-new-*.log, logs/qemu-run-*.log, apps/root-task/src/net/stack.rs, apps/root-task/src/net/outbound.rs, apps/cohsh/src/transport/tcp.rs, apps/cohsh/src/lib.rs.
Changes:
  - docs/BUILD_PLAN.md — record the console parity debug/validation plan and required logs.
  - docs/BUILD_PLAN.md — capture interactive vs script-mode differences (auto-log, REPL keepalive, console lock) and trace-correlation checklist.
Commands:
  - SEL4_BUILD_DIR=$HOME/seL4/build ./scripts/cohesix-build-run.sh --sel4-build "$HOME/seL4/build" --out-dir out/cohesix --profile release --root-task-features cohesix-dev --cargo-target aarch64-unknown-none --raw-qemu --transport tcp
  - COHSH_TCP_DEBUG=1 ./out/cohesix/host-tools/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen
  - COHSH_TCP_DEBUG=1 ./out/cohesix/host-tools/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role worker
  - scripts/cohsh/run_regression_batch.sh
  - rg "Flags \\[R\\]" logs/tcpdump-new-*.log
Checks:
  - Console frames remain valid (no invalid UTF-8 payloads, no send.partial aborts) during the full interactive command surface.
  - Interactive vs script traces show consistent ACK/ERR/END ordering for log/tail/cat without server-side RST.
  - Console lock semantics preserved; concurrent attachments require explicit COHSH_CONSOLE_LOCK=0 (debug only).
Deliverables:
  - Updated cohsh/QEMU/tcpdump logs documenting frame integrity and interactive parity.
  - Trace-correlation notes mapping cohsh commands to tcpdump RST/FIN events.

Title/ID: m20f-cohsh-interactive-parity
Goal: Ensure interactive cohsh commands and SwarmUI console sessions match script-mode behavior without connection churn.
Inputs: apps/root-task/src/event/mod.rs, apps/root-task/src/net/stack.rs, apps/root-task/src/net/console_srv.rs, apps/cohsh/src/transport/tcp.rs, apps/swarmui/src/lib.rs, logs/cohsh-*.log.
Changes:
  - apps/root-task/src/event/mod.rs — align CAT/TAIL streaming with pending stream handling and consistent END emission.
  - apps/root-task/src/net/stack.rs — tune console send pacing/backpressure handling for stream output; rate-limit `tcp.flush.blocked` audit spam.
  - apps/root-task/src/net/console_srv.rs — preserve END delivery without reordering stream data lines.
  - apps/cohsh/src/transport/tcp.rs — harden console stream reads/reconnect logic and enforce exclusive console locking.
  - apps/swarmui/src/lib.rs — match SwarmUI console error handling to cohsh transport semantics.
Commands:
  - cargo check -p root-task -p cohsh -p swarmui
  - SEL4_BUILD_DIR=$HOME/seL4/build ./scripts/cohesix-build-run.sh --sel4-build "$HOME/seL4/build" --out-dir out/cohesix --profile release --root-task-features cohesix-dev --cargo-target aarch64-unknown-none --raw-qemu --transport tcp
  - ./out/cohesix/host-tools/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen
  - ./out/cohesix/host-tools/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role worker
  - scripts/cohsh/run_regression_batch.sh
Checks:
  - All interactive cohsh commands succeed without reconnect loops for both queen and worker roles.
  - SwarmUI console transport remains attached and renders hive updates.
  - Regression pack passes unchanged.
Deliverables:
  - Updated QEMU + cohsh logs validating interactive parity and SwarmUI stability.

Title/ID: m20f-ticket-mint-oneshots
Goal: Add host-only ticket mint one-shots to cohsh and SwarmUI without changing console grammar.
Inputs: configs/root_task.toml, docs/WORKER_TICKETS.md, docs/HOST_TOOLS.md, docs/USERLAND_AND_CLI.md.
Changes:
  - apps/cohsh/src/main.rs — add `--mint-ticket` CLI path with config-backed secrets.
  - apps/cohsh/src/ticket_mint.rs — shared ticket mint helper (config parsing, defaults, subject validation).
  - apps/cohsh/tests/ticket_mint.rs — verify minted tickets decode with defaults and subject rules.
  - apps/cohsh/Cargo.toml — add test dependency for ticket mint fixtures.
  - apps/swarmui/src-tauri/main.rs — add `--mint-ticket` CLI path and Tauri command for UI button.
  - apps/swarmui/frontend/index.html — add Mint Ticket controls.
  - apps/swarmui/frontend/app.js — wire Mint Ticket button to backend.
  - docs/WORKER_TICKETS.md — document cohsh/SwarmUI one-shot minting.
  - docs/HOST_TOOLS.md — add CLI usage examples.
  - docs/USERLAND_AND_CLI.md — document flags and env vars.
  - docs/TEST_PLAN.md — add the ticket mint test step.
Commands:
  - cargo test -p cohsh --test ticket_mint
  - cargo test -p swarmui --test security
Checks:
  - Worker roles require subject; queen subject optional.
  - Minted ticket decodes with the role secret and includes default budgets.
Deliverables:
  - Cohsh/SwarmUI minting examples and updated docs.
```

---

## Milestone 20f1 — SwarmUI Host Tool Packaging + Tauri API Fix <a id="20f1"></a>
[Milestones](#Milestones)

**Status:** Complete — SwarmUI is packaged in cohesix-dev host tools, the Tauri invoke bridge is resilient, and a clean build is warning-free.

**Why now (host tools):** SwarmUI must be buildable and runnable from the standard `cohesix-dev` profile, and its frontend must reliably bind to the Tauri backend without changing protocol semantics.

**Goal**
Ensure SwarmUI is packaged with the `cohesix-dev` host tool set and fix the Tauri invoke bridge so the UI connects without altering 9P/console grammar.

**Deliverables**
- `cohesix-dev` host tool build includes SwarmUI in `out/*/host-tools`.
- SwarmUI frontend uses the supported Tauri invoke bridge (no "Tauri API unavailable").
- SwarmUI defaults to the console TCP port (`31337`) unless overridden by `SWARMUI_9P_PORT`.
- Clean `cohesix-dev` build emits no root-task warnings.
- Build from a clean `out/` and `target/` completes successfully.

**Commands (Mac ARM64)**
```bash
rm -rf out target
SEL4_BUILD_DIR=$HOME/seL4/build \
./scripts/cohesix-build-run.sh \
  --sel4-build "$HOME/seL4/build" \
  --out-dir out/cohesix \
  --profile release \
  --root-task-features cohesix-dev \
  --cargo-target aarch64-unknown-none \
  --raw-qemu \
  --transport tcp
```

**Checks (DoD)**
- SwarmUI binary is present in `out/cohesix/host-tools`.
- SwarmUI connects without `ERR CONNECT Tauri API unavailable`.
- No changes to ACK/ERR/END grammar or ordering.

**Task Breakdown**
```
Title/ID: m20f1-swarmui-packaging
Goal: Package SwarmUI with the cohesix-dev host tool set.
Inputs: scripts/cohesix-build-run.sh, apps/swarmui/Cargo.toml.
Changes:
  - scripts/cohesix-build-run.sh — include swarmui when cohesix-dev is enabled.
Commands:
  - rm -rf out target
  - ./scripts/cohesix-build-run.sh --root-task-features cohesix-dev ...
Checks:
  - out/cohesix/host-tools/swarmui exists.
Deliverables:
  - Updated build script; clean build output.

Title/ID: m20f1-tauri-invoke-bridge
Goal: Fix SwarmUI invoke bridge detection for Tauri.
Inputs: apps/swarmui/frontend/app.js.
Changes:
  - apps/swarmui/frontend/app.js — use supported invoke bridge.
Commands:
  - cargo run -p swarmui
Checks:
  - UI connects without "Tauri API unavailable".
Deliverables:
  - Updated frontend invoke path.

Title/ID: m20f1-default-port
Goal: Align SwarmUI default TCP port with the console listener.
Inputs: apps/swarmui/src-tauri/main.rs, crates/net-constants.
Changes:
  - apps/swarmui/src-tauri/main.rs — default to `COHSH_TCP_PORT` when `SWARMUI_9P_PORT` unset.
Commands:
  - cargo run -p swarmui
Checks:
  - SwarmUI connects with no `SWARMUI_9P_PORT` set when QEMU forwards port 31337.
Deliverables:
  - Default port matches console transport.

Title/ID: m20f1-clean-build-warnings
Goal: Eliminate root-task build warnings during cohesix-dev builds.
Inputs: apps/root-task/src/event/mod.rs.
Changes:
  - apps/root-task/src/event/mod.rs — remove unused assignments in log streaming path.
Commands:
  - ./scripts/cohesix-build-run.sh --root-task-features cohesix-dev ...
Checks:
  - No warnings emitted during root-task build.
Deliverables:
  - Clean build with zero root-task warnings.
```

---

## Milestone 20g — Deterministic Snapshot & Replay (UI Testing) <a id="20g"></a> 
[Milestones](#Milestones)

**Status:** Complete — Trace record/replay fixtures and parity tests land across cohsh, SwarmUI, and coh-status; trace policy snippet and hashes align; SwarmUI header branding is live; release bundle replay verified on macOS 26.x and Ubuntu 24 (cohsh + SwarmUI).

**Why now (compiler):** To stabilize UI regressions without live targets, we need deterministic trace capture and replay consistent with CLI/console semantics.

**Goal**
Add trace record/replay across `cohsh-core`, `cohsh`, SwarmUI, and coh-status to enable deterministic UI testing.

**Deliverables**
- `cohsh-core` trace recorder/replayer for 9P frames + ACKs (`.trace` files) with size targets ≤ 1 MiB per 10 s of tail traffic.
- `cohsh` CLI supports `--record-trace <FILE>` and `--replay-trace <FILE>` via `cohsh-core`; CLI usage is documented in `docs/USERLAND_AND_CLI.md` and referenced in `docs/TEST_PLAN.md`.
- SwarmUI “offline replay” mode consuming trace files; docs in `docs/TEST_PLAN.md`.
- coh-status offline replay hook for field diagnostics.
- SwarmUI frontend header includes the Cohesix SVG branding at the top of the shell.

**Commands**
- `cargo test -p cohsh-core --test trace`
- `cargo test -p cohsh --test trace`
- `cargo test -p swarmui --test trace`
- `cargo test -p coh-status --test trace`

**Checks (DoD)**
- Replay reproduces identical telemetry curves and ACK sequences across `cohsh`, SwarmUI, and coh-status; diff harness reports zero delta.
- Abuse case: tampered trace (truncated or modified hash) is rejected with deterministic error and no UI state change.
- UI/CLI/console equivalence MUST be preserved: ACK/ERR/END sequences must remain byte-stable relative to the 7c baseline.
All testing and verification for this milestone is governed by:

> **`docs/TEST_PLAN.md` — the sole authority on test phases, execution order, and acceptance criteria.**

**Compiler touchpoints**
- `coh-rtc` emits trace metadata schema and default size limits into docs/TEST_PLAN.md; regeneration guard verifies hash alignment.

**Task Breakdown**
```
Title/ID: m20g-trace-core
Goal: Implement trace recorder/replayer in cohsh-core with bounds.
Inputs: crates/cohsh-core, tests/fixtures/traces/, docs/TEST_PLAN.md.
Changes:
  - crates/cohsh-core/src/trace.rs — bounded recorder/replayer with hash validation.
  - crates/cohsh-core/tests/trace.rs — tampered trace negative case.
Commands:
  - cargo test -p cohsh-core --test trace
Checks:
  - Truncated or tampered trace rejected; valid trace replays byte-identical ACK/ERR.
Deliverables:
  - Trace schema referenced in docs/TEST_PLAN.md via compiler snippet.

Title/ID: m20g-cohsh-replay
Goal: Wire cohsh CLI record/replay to cohsh-core trace format.
Inputs: apps/cohsh/src/main.rs, apps/cohsh/src/lib.rs, apps/cohsh/src/trace.rs, tests/fixtures/traces/, tests/fixtures/transcripts/trace_v0/, docs/USERLAND_AND_CLI.md, docs/TEST_PLAN.md.
Changes:
  - apps/cohsh/src/main.rs — add `--record-trace` and `--replay-trace` CLI entry points using cohsh-core.
  - apps/cohsh/src/lib.rs — expose trace driver for CLI use.
  - apps/cohsh/tests/trace.rs — record/replay parity + tamper rejection.
Commands:
  - cargo test -p cohsh --test trace
Checks:
  - cohsh record/replay matches cohsh-core fixtures; tampered trace rejected deterministically.
Deliverables:
  - cohsh trace CLI documented; canonical trace capture path referenced in docs/TEST_PLAN.md.

Title/ID: m20g-ui-replay
Goal: Add offline replay to SwarmUI and coh-status for deterministic UI tests.
Inputs: apps/swarmui/src/transport.rs, apps/swarmui/src-tauri/main.rs, apps/swarmui/tests/trace.rs, apps/coh-status/src/lib.rs, apps/coh-status/tests/trace.rs, docs/TEST_PLAN.md.
Changes:
  - apps/swarmui/src/transport.rs — trace transport factory for replay.
  - apps/swarmui/src-tauri/main.rs — `--replay-trace` entry point and decoding.
  - apps/swarmui/tests/trace.rs — trace replay transcript parity.
  - apps/coh-status/src/lib.rs — trace replay client wrapper + policy.
  - apps/coh-status/tests/trace.rs — trace replay transcript parity.
Commands:
  - cargo test -p swarmui --test trace
  - cargo test -p coh-status --test trace
Checks:
  - Replay matches stored trace_v0 transcripts; tampered trace rejected deterministically.
Deliverables:
  - Offline replay documentation; trace fixture in tests/fixtures/traces/trace_v0.trace and transcripts in tests/fixtures/transcripts/trace_v0/.

Title/ID: m20g-swarmui-header
Goal: Add Cohesix header branding to the SwarmUI shell.
Inputs: apps/swarmui/frontend/index.html, apps/swarmui/frontend/assets/icons/cohesix-header.svg, apps/swarmui/frontend/styles/.
Changes:
  - apps/swarmui/frontend/index.html — add Cohesix header at the top of the SwarmUI shell.
  - apps/swarmui/frontend/styles/ — define header layout and spacing rules.
Commands:
  - cargo test -p swarmui
Checks:
  - Cohesix header renders at the top without disrupting layout or live hive rendering.
Deliverables:
  - SwarmUI displays the Cohesix header consistently across desktop and mobile sizes.
```

## Milestone 20h — Alpha Release Gate: As-Built Verification, Live Hive Demo, SwarmUI Replay, & Release Bundle <a id="20h"></a> 
[Milestones](#Milestones)

**Status:** Complete — Test Plan gates executed (clean build + CLI + regression + packaging), Live Hive + replay demos validated, and macOS/Ubuntu release bundles verified end-to-end.

**Why now (compiler):**  
Milestone 20g defines the point at which Cohesix becomes **control-plane complete and deterministic**. An alpha release is only valid if the **as-built system passes the full Test Plan**, from a clean checkout, with no hidden assumptions.

This milestone is a **release gate**, not a feature milestone.  
It adds **no new architecture, protocols, or semantics**.  
It exists to prove correctness, operability, and legibility.

---

## Goal

1. **Complete Milestone 20g** exactly as specified in `docs/BUILD_PLAN.md`.  
2. Verify the **as-built system** against **all applicable phases in `docs/TEST_PLAN.md`**.  
3. Deliver both:
   - a **Deterministic Replay Demo** (trust, auditability), and
   - a **Live Hive Demo** (real-time, exciting, but controlled).
4. Produce a **self-contained alpha release bundle** that a third party can run using only:
   - the bundle
   - the QEMU runner
   - `docs/QUICKSTART.md`

---

## Hard Preconditions

### A) Milestone 20g completion (blocking)
- All deliverables for Milestone 20g implemented.
- All Milestone 20g checks satisfied.
- Documentation reflects **as-built** behavior.

**Rule:** Milestone 20h MUST NOT be marked *Complete* unless Milestone 20g is already complete.

---

## Testing & Verification (Canonical)

All testing and verification for this milestone is governed by:

> **`docs/TEST_PLAN.md` — the sole authority on test phases, execution order, and acceptance criteria.**

Ad-hoc commands, manual test lists, or one-off scripts **must not** be used as acceptance criteria.  
They may be *inputs* to the Test Plan, but **DoD is defined only by Test Plan gates**.

---

### B) Clean Build & Reproducibility Gate  
(Per TEST_PLAN: *Build Integrity* + *Reproducibility* phases)

**Requirements**
- Remove all build artifacts (`target/`, `out/`, and equivalents).
- Rebuild Cohesix from a clean workspace using the canonical build flow.
- Re-run `coh-rtc` and verify generated artifacts match committed expectations.

**Acceptance**
- Clean build succeeds.
- No new build warnings remain unaddressed.
- No features are disabled or bypassed to achieve a clean build.
- Generated artifacts, manifests, and doc snippets are consistent.

---

### C) CLI & Control-Plane Surface Gate  
(Per TEST_PLAN: *CLI Semantics*, *Role Enforcement*, *Concurrency*)

The full `cohsh` command surface MUST be validated via the **CLI test phases** defined in `docs/TEST_PLAN.md`, including:

- Queen role coverage
- Worker role coverage
- Concurrent session behavior
- Deterministic ACK/ERR semantics
- Negative/denial cases

**Key properties verified (via TEST_PLAN)**
- Every documented `cohsh` command is exercised.
- Role-scoped authority is enforced (queen vs worker).
- Concurrent sessions do not corrupt state or reorder acknowledgements.
- All failures are explicit, deterministic, and auditable.

**Evidence**
- Test Plan artifacts (logs, transcripts, or summaries) are collected and referenced.
- No manual “it looked right” validation is acceptable.

---

### D) Regression & Stability Gate  
(Per TEST_PLAN: *Regression*, *Long-Run*, *Non-Regression*)

- Execute the **full regression batch** as defined by `docs/TEST_PLAN.md`.
- Long-running tests must complete within declared time bounds.
- Output drift fails the gate unless explicitly approved and documented.

**Acceptance**
- All regression phases PASS.
- No existing regression tests are weakened.
- New tests (if any) are additive and documented.

---

## Demo Deliverables (Post-Gate)

Only after all **TEST_PLAN gates pass** may the following demo artifacts be finalized.

---

### 1) Deterministic Replay Demo

- Canonical snapshot / trace generated under Milestone 20g.
- Canonical trace is shipped in the alpha bundle under `traces/` with its hash for tamper checks.
- Used by:
  - CLI replay demo (`cohsh --replay-trace <FILE>`)
  - SwarmUI Replay Mode
- Replay produces byte-identical behavior across runs.

This demo proves:
- determinism
- auditability
- UI correctness without live risk

---

### 2) Live Hive Demo (Controlled)

**Purpose:** demonstrate Cohesix *alive* — workers spawning, telemetry flowing — without violating control-plane discipline.

**Rules (strict)**
- Live mutation occurs only via:
  - `cohsh`
  - scripted flows covered by TEST_PLAN
- SwarmUI is **observational only** in live mode.
- No UI-initiated control.

**Validated via**
- TEST_PLAN live-operation phase
- Role enforcement + audit verification
- Deterministic logging under live load

---

### 3) SwarmUI — Dual-Mode Alpha

**Replay Mode (default)**
- Loads canonical snapshot from `traces/`
- Full timeline scrub (pause / rewind / step)
- Deterministic visualization

**Live Hive Mode**
- Read-only view of live state
- Mirrors CLI-driven actions in real time
- No write capability

SwarmUI behavior is validated under TEST_PLAN UI/CLI convergence criteria.

---

## Alpha Release Bundle

Produced **only after all TEST_PLAN gates pass**.

cohesix-alpha-/
├── bin/
├── image/
├── qemu/
├── scripts/
├── traces/
│   └── (canonical .trace + hash)
├── ui/
│   └── swarmui/
├── docs/
│   ├── QUICKSTART.md
│   └── (as-built snapshots)
├── VERSION.txt
└── LICENSE.txt
Bundle contents, integrity, and runnability are validated under TEST_PLAN *Packaging* phase.
Release bundles are emitted per host OS; the macOS tarball appends `-MacOS`, and the Linux aarch64
bundle appends `-linux` to the release name and carries Linux host tools in `bin/`.

---

## QUICKSTART.md

The Quickstart MUST reference:
- TEST_PLAN phases at a high level
- What has already been verified
- What the user is expected to run vs observe
- Where the canonical trace lives in the bundle and the replay commands already defined in the Test Plan

It must not introduce new testing procedures outside the Test Plan.

---

## Definition of Done (Authoritative)

Milestone 20h is **Complete** if and only if:

1. Milestone 20g is complete per `docs/BUILD_PLAN.md`.
2. All applicable phases in `docs/TEST_PLAN.md` PASS:
   - Build Integrity
   - CLI Semantics
   - Role Enforcement
   - Concurrency
   - Regression
   - Packaging
3. Clean rebuild from scratch succeeds.
4. Replay demo and Live Hive demo are both validated outcomes of the Test Plan.
5. SwarmUI behavior (replay + live read-only) is consistent with CLI behavior.
6. A third party can run the alpha using only:
   - the release bundle
   - `qemu/run.sh`
   - `docs/QUICKSTART.md`

If any Test Plan gate fails, this milestone remains **Incomplete**.

---

## Outcome

After Milestone 20h:
- Cohesix has a **test-plan-validated alpha**.
- Demos are exciting *and* trustworthy.
- There is one source of truth for correctness: `docs/TEST_PLAN.md`.
- The system is ready for external evaluation without hand-holding.

----
**Alpha Release 1 achieved here**
----

Next, Alpha Release 2 targets a plug-and-play operator experience immediately after Milestone 20.x. Milestones 21-24 define the Alpha track; the AWS AMI work follows as Milestone 25a.

## Milestone 21a — Telemetry Ingest with OS-Named Segments (Severely Limited Create) <a id="21a"></a> 
[Milestones](#Milestones)

**Why now (compiler):**  
Operators, demos, and UI testing need a safe way to inject telemetry from host tools without turning Cohesix into a general file transfer system. This milestone introduces a **Plan-9-style telemetry ingest path** that supports *severely constrained create*: the OS controls naming, retention, and quotas; clients can only append bounded records. This increases utility while preserving Cohesix’s control-plane boundary and minimal TCB.

---

### Goal

Provide a deterministic, bounded telemetry ingest surface where host tools can:
1. Request a new telemetry segment with **OS-assigned naming**, and  
2. Append bounded telemetry records into that segment using existing Secure9P primitives.

---

### Non-Goals (Explicit)

- No arbitrary file upload or “scp-like” behaviour  
- No client-chosen filenames or paths  
- No delete / remove / rename semantics  
- No random writes or truncation  
- No new in-VM TCP listeners beyond the existing console  
- No schema-aware parsing of CSV / XML / JSON payloads  

---

### Deliverables

- Fixed telemetry namespace under `/queen/telemetry/<device_id>/` with:
  - `ctl` (append-only control)
  - `seg/` (OS-named, append-only segments)
  - `latest` (read-only pointer to the most recent segment)
- OS-assigned segment creation via control file (no path-based create)
- Hard quotas on segment count and bytes with deterministic refuse/evict behaviour
- Bounded, versioned telemetry envelope (opaque payload)
- `cohsh telemetry push` host command
- CLI regression coverage added to the Regression Pack
- Documentation updated to reflect **as-built** semantics

---

### Task Breakdown
```
Title/ID: m21a-telemetry-namespace
Goal: Introduce a fixed telemetry namespace with OS-named segments.
Inputs: docs/ARCHITECTURE.md, docs/INTERFACES.md, existing NineDoor providers.
Changes:
	•	apps/nine-door/src/host/telemetry.rs — add provider for:
/queen/telemetry/<device_id>/ctl
/queen/telemetry/<device_id>/seg/<seg_id>
/queen/telemetry/<device_id>/latest
	•	apps/nine-door/src/host/namespace.rs — mount telemetry provider under /queen.
Commands:
	•	cargo test -p nine-door
Checks:
	•	Telemetry paths appear only when enabled.
	•	Segment files are append-only and OS-named.
Deliverables:
	•	Telemetry namespace live with no client-controlled naming.
```

```
Title/ID: m21a-telemetry-create-ctl
Goal: Implement severely limited “create” via control file.
Inputs: docs/INTERFACES.md (new schema), existing append-only control patterns.
Changes:
	•	apps/nine-door/src/host/telemetry.rs — handle ctl command:
{“new”:“segment”,“mime”:””}
	•	Emit deterministic ACK with assigned seg_id.
	•	Update /latest pointer on successful creation.
Commands:
	•	cargo test -p nine-door –test telemetry_create
Checks:
	•	Client cannot create files by path.
	•	Only ctl-based segment allocation is accepted.
Deliverables:
	•	OS-controlled segment allocation with deterministic responses.
```

```
Title/ID: m21a-telemetry-quotas
Goal: Enforce deterministic quotas and retention for telemetry segments.
Inputs: configs/root_task.toml (new fields), coh-rtc validation rules.
Changes:
	•	tools/coh-rtc/src/ir.rs — add telemetry_ingest.* fields:
max_segments_per_device
max_bytes_per_segment
max_total_bytes_per_device
eviction_policy (refuse | evict-oldest)
	•	apps/nine-door/src/host/telemetry.rs — enforce quotas and eviction.
Commands:
	•	cargo run -p coh-rtc – configs/root_task.toml
	•	cargo test -p nine-door –test telemetry_quotas
Checks:
	•	Quota exhaustion yields deterministic ERR or deterministic eviction.
Deliverables:
	•	Manifest-driven, bounded telemetry retention.
```
---
```
Title/ID: m21a-telemetry-envelope

Goal: Define and document the telemetry envelope format.
Inputs: docs/INTERFACES.md.
Changes:
	•	docs/INTERFACES.md — add schema cohsh-telemetry-push/v1.
	•	Enforce max_record_bytes (≤ 4096) server-side.
Commands:
	•	cargo test -p nine-door –test telemetry_envelope
Checks:
	•	Oversized records rejected deterministically.
Deliverables:
	•	Versioned, opaque telemetry envelope documented and enforced.
```

```
Title/ID: m21a-cohsh-telemetry-push
Goal: Add host-side telemetry push command to cohsh.
Inputs: docs/USERLAND_AND_CLI.md, existing cohsh 9P write helpers.
Changes:
	•	apps/cohsh/src/lib.rs — add command:
telemetry push <src_file> –device 
	•	Enforce file size limits, extension allowlist, chunking, and fixed destination.
	•	Resolve seg_id via ACK detail or /latest before appending.
Commands:
	•	cargo test -p cohsh
Checks:
	•	cohsh cannot write outside telemetry allowlist.
	•	Oversized files fail locally with deterministic ERR.
Deliverables:
	•	Safe host-side telemetry injection command.
```

```
Title/ID: m21a-telemetry-regression
Goal: Lock behaviour with deterministic CLI regression.
Inputs: scripts/cohsh/telemetry_push_create.coh.
Changes:
	•	scripts/cohsh/telemetry_push_create.coh — cover:
create success
push success
oversize failure
quota exhaustion behaviour
	•	Add script to the Regression Pack.
Commands:
	•	cargo run -p cohsh –features tcp – –transport tcp –script scripts/cohsh/telemetry_push_create.coh
Checks:
	•	Script passes unchanged across runs.
Deliverables:
	•	Regression coverage preventing scope
```

```
Title/ID: m21a-docs-sync
Goal: Update documentation to reflect as-built telemetry ingest.
Inputs: docs/ARCHITECTURE.md, docs/USERLAND_AND_CLI.md, docs/INTERFACES.md.
Changes:
	•	Document telemetry namespace, quotas, and create semantics.
Commands:
	•	mdbook build docs (if configured)
Checks:
	•	Docs match code behaviour exactly.
Deliverables:
	•	Docs-as-built alignment.
```
---

### Checks (Definition of Done)

- Telemetry segments are OS-named and append-only.
- Clients cannot choose names or paths.
- Quotas and eviction/refusal behaviour are deterministic.
- No new in-VM network services are introduced.
- Regression Pack passes unchanged.
- Documentation reflects actual behaviour.

---

### Outcome

After Milestone 21a, Cohesix supports **safe, Plan-9-style telemetry creation** with strict bounds and OS-owned lifecycle—improving utility for demos, UI testing, and early deployments without compromising the control-plane boundary.

---

## Milestone 21b — Host Bridges (coh mount, coh gpu, coh telemetry pull) <a id="21b"></a> 
[Milestones](#Milestones)

**Why now (adoption):** After Milestone 20.x, we need plug-and-play host UX that integrates with existing CUDA/MIG workflows without new protocols or VM expansion.

**Goal**
Deliver host-only mount views for Secure9P namespaces, GPU lease UX, and pull-based telemetry export while preserving Secure9P and console semantics (no new server-side filesystem behavior).

**Deliverables**
- `coh` host tool (single binary) with subcommands `mount`, `gpu`, and `telemetry pull`, built on `cohsh-core` transports and policy tables without introducing new verbs.
- `coh mount` FUSE mount of Secure9P namespaces (for example `/mnt/coh`) with strict path validation, append-only enforcement, and fid lifecycle checks; never bypasses policy. The mount is a **client convenience view only** and does not add POSIX semantics to the system.
- `coh gpu` discovery/status/lease UX with `--mock` backend for CI and non-NVIDIA hosts, plus NVML backend on Linux; MIG visibility only when defined in `docs/GPU_NODES.md`.
- `coh telemetry pull` pulls bundles from `/queen/telemetry/*` into host storage; resumable and idempotent (no streaming).
- Invariant envelope: `msize <= 8192`, walk depth <= 8, no `..`, ACK-before-side-effects, bounded work per command.

**Commands**
- `cargo test -p coh --features mock`
- `cargo run -p coh --features mock -- mount --mock --at /tmp/coh-mount`
- `cargo run -p coh --features mock -- gpu list --mock`
- `cargo run -p coh --features mock -- telemetry pull --mock --out out/telemetry`

**Checks (DoD)**
- `coh mount` works in `--mock` and against a dev instance; invalid paths return deterministic ERR with audit line.
- `coh gpu` lease grant/deny is deterministic and logged; mock and NVML backends produce identical lease semantics.
- `coh telemetry pull` resumes without duplicates and is idempotent across restarts; no streaming or background polling.
- Golden transcript markers or fixtures prove stable ACK/ERR ordering for `coh` subcommands.
- Deterministic denial semantics for invalid tickets/paths/quotas are verified in tests.
- Bounded memory and bounded work per operation (no unbounded queues, no infinite retries) are enforced by limits and tests.
- Secure9P invariants preserved (msize <= 8192, path validation, fid lifecycle).
- Console semantics preserved (ACK-before-side-effects) for console-backed flows.
- Regression pack runs unchanged; output drift fails and new tests are additive.
- CI runs mock-mode tests on x86_64.

**Compiler touchpoints**
- `coh-rtc` emits `coh` defaults (mount root, allowlisted paths, telemetry export bounds, retry ceilings) into a manifest snippet consumed by `coh` and documented in `docs/USERLAND_AND_CLI.md`.
- Manifest gates enforce host-tool-only features (FUSE, NVML) with explicit fallbacks to `--mock`.

**Task Breakdown**
```
Title/ID: m21b-coh-cli-skeleton
Goal: Introduce coh host CLI with strict subcommand parsing and policy loading.
Inputs: crates/cohsh-core, docs/USERLAND_AND_CLI.md, docs/INTERFACES.md.
Changes:
  - apps/coh/src/main.rs — CLI entrypoint with mount/gpu/telemetry pull subcommands.
  - apps/coh/src/policy.rs — manifest-backed limits and allowlist loader.
Commands:
  - cargo test -p coh --features mock
Checks:
  - Unknown subcommand or invalid args returns deterministic ERR without side effects.
Deliverables:
  - coh CLI skeleton and policy loader documented in docs/USERLAND_AND_CLI.md.

Title/ID: m21b-coh-mount
Goal: Implement Secure9P-backed FUSE mount with bounded operations.
Inputs: secure9p-core, docs/SECURE9P.md.
Changes:
  - apps/coh/src/mount.rs — FUSE adapter enforcing path validation, append-only, and fid lifecycle.
  - apps/coh/tests/mount.rs — invalid path and offset denial tests.
Commands:
  - cargo test -p coh --features mock --test mount
Checks:
  - `..` walk attempts and oversized reads return deterministic ERR; mount never bypasses policy.
Deliverables:
  - FUSE mount docs and regression fixtures.

Title/ID: m21b-coh-gpu-telemetry
Goal: Add coh gpu UX and telemetry pull with mock backend.
Inputs: docs/GPU_NODES.md, docs/INTERFACES.md.
Changes:
  - apps/coh/src/gpu.rs — list/status/lease UX with mock and NVML backends.
  - apps/coh/src/telemetry.rs — resumable pull from /queen/telemetry/*.
Commands:
  - cargo run -p coh --features mock -- gpu list --mock
  - cargo run -p coh --features mock -- telemetry pull --mock --out out/telemetry
Checks:
  - Lease grant/deny is deterministic; telemetry pull resumes without duplicates.
Deliverables:
  - coh gpu + telemetry pull behavior documented with transcript fixtures.
```

---

## Milestone 21c — SwarmUI Interactive cohsh Terminal (Full Prompt UX) <a id="21c"></a> 
[Milestones](#Milestones)

**Why now (operator UX):** SwarmUI already embeds `cohsh-core` and speaks the TCP console. A full terminal prompt improves operator ergonomics without adding new verbs or protocols.

**Goal**
Add a cohesive, terminal‑grade command prompt inside SwarmUI that reuses existing console semantics and `cohsh-core` parsing.

**Deliverables**
- SwarmUI “Console” panel with command input, scrollback, and clear/stop controls.
- Prompt supports multiline output, `OK/ERR/END` framing, and tail streams.
- Single‑session multiplexing: the prompt reuses SwarmUI’s existing console session (no second client).
- No new verbs, no new transports, and no VM changes.
- SwarmUI help output lists only console commands and points to `cohsh` for additional CLI features.

**Commands**
- `cargo check -p swarmui`
- `cargo test -p cohsh-core --test transcripts`

**Checks (DoD)**
- Prompt output matches `cohsh` transcript ordering (ACK/ERR/END) for `help → attach → log → spawn → tail → quit`.
- Tail streams can be stopped without breaking the shared session.
- Reconnect logic mirrors `cohsh` (connection loss surfaces clearly and resumes cleanly).
- Console lock is enforced (SwarmUI prompt does not allow a second TCP client).
- No new console verbs or transport semantics introduced.

**Task Breakdown**
```
Title/ID: m21c-swarmui-console-ui
Goal: Add a console panel with input, scrollback, and tail controls.
Inputs: apps/swarmui/frontend, docs/USERLAND_AND_CLI.md.
Changes:
  - apps/swarmui/frontend/components/console.js — input + scrollback UI.
  - apps/swarmui/frontend/styles/console.css — terminal styling.
Commands:
  - npm run lint (if configured) or cargo check -p swarmui
Checks:
  - Console renders without layout regressions; input accepts commands and displays output.
Deliverables:
  - SwarmUI console panel wired to the UI.

Title/ID: m21c-swarmui-console-bridge
Goal: Bridge console input/output through the existing SwarmUI session.
Inputs: apps/swarmui/src-tauri/main.rs, crates/cohsh-core.
Changes:
  - apps/swarmui/src-tauri/main.rs — expose send-line + stream events for prompt output.
  - apps/swarmui/src/lib.rs — reuse existing session; no new transport.
Commands:
  - cargo check -p swarmui
Checks:
  - Prompt uses the same TCP session; no parallel client sockets.
Deliverables:
  - Prompt input/output routed through existing console session.

Title/ID: m21c-swarmui-console-parity
Goal: Ensure prompt output ordering matches cohsh transcripts.
Inputs: crates/cohsh-core/tests/transcripts.rs, docs/TEST_PLAN.md.
Changes:
  - apps/swarmui/tests/console_parity.rs — compare prompt output framing to cohsh transcripts.
Commands:
  - cargo test -p swarmui --test console_parity
Checks:
  - ACK/ERR/END sequences match cohsh fixtures for baseline verbs.
Deliverables:
  - Parity test ensuring terminal output consistency.
```

---

## Milestone 21d — Deterministic Node Lifecycle & Operator Control <a id="21d"></a> 
[Milestones](#Milestones)

**Why now (operator):** Cohesix nodes must behave predictably across power loss, network partitions, maintenance windows, and redeployments. Lifecycle semantics must be explicit, inspectable, and controllable — not inferred from side effects.

**Goal**
Define and enforce a **finite lifecycle state machine** for Cohesix nodes, exposed entirely via file-shaped control surfaces, with deterministic transitions and regression coverage.

### Lifecycle states (normative)
- `BOOTING`
- `DEGRADED`
- `ONLINE`
- `DRAINING`
- `QUIESCED`
- `OFFLINE`

### State definitions
| State | Meaning |
| --- | --- |
| `BOOTING` | Root-task started, manifest loaded, identity pending. |
| `DEGRADED` | Identity ok, but one or more required dependencies are missing (network, storage, sidecar, or policy gates). |
| `ONLINE` | Full control-plane available; workers and telemetry allowed within policy bounds. |
| `DRAINING` | No new work accepted; telemetry ingestion remains enabled. |
| `QUIESCED` | All work drained; safe to reboot or power off. |
| `OFFLINE` | Explicitly disabled or unrecoverable failure; control-plane actions denied. |

### Control & observation (NineDoor)
**Observability (read-only)**
- `/proc/lifecycle/state`
- `/proc/lifecycle/reason`
- `/proc/lifecycle/since`

**Control (append-only, queen-only)**
- `/queen/lifecycle/ctl`

**Supported control commands (append-only, single line)**
`cordon`, `drain`, `resume`, `quiesce`, `reset`

### Hard rules
- Transitions are **explicit** and must occur only via `/queen/lifecycle/ctl` or deterministic system events enumerated in docs; no heuristic or hidden state changes.
- Invalid transitions return deterministic `ERR` and emit audit entries.
- Every transition emits an audit record in `/log/queen.log` with old/new state and reason.
- Tickets, telemetry ingest, worker authority, and host sidecar publishes are gated by lifecycle state.

### Telemetry Spool Policy (Addendum to Milestones 21a & 25b)

**Rationale:** Telemetry storage must be predictable under pressure. Operators must know *when*, *why*, and *how* data is retained or dropped. This addendum **aligns policy terminology** between 21a telemetry ingest quotas and the 25b persistent spool store; it does **not** retroactively change 21a's completed behavior.

#### Policy surface (alignment)
- **Telemetry ingest (21a):** keep `telemetry_ingest.eviction_policy` (`refuse` | `evict-oldest`) as the source of truth for per-device segment limits.
- **Persistent spool (25b):** use `persistence.spool.mode` (`refuse` | `overwrite_acked`) and `persistence.spool.max_record_bytes` to mirror 21a's refusal/eviction semantics while remaining crash-safe.

**Manifest example (bytes only)**
```toml
[telemetry_ingest]
max_segments_per_device = 64
max_bytes_per_segment = 262144
max_total_bytes_per_device = 16777216
eviction_policy = "refuse" # or "evict-oldest"

[persistence.spool]
max_bytes = 67108864
max_record_bytes = 32768
mode = "refuse" # or "overwrite_acked"
```

#### Operator visibility
- `/proc/spool/status` MUST expose policy and pressure fields (used_bytes, max_bytes, records, dropped, pressure, mode, ack_cursor).
- If additional nodes are required for UI providers, add `/proc/spool/policy` and `/proc/spool/pressure` **only** with corresponding updates to `ARCHITECTURE.md` and `INTERFACES.md`.

#### CLI surface (host-only, target milestone 25b or later)
- `cohsh telemetry status` — read spool/ingest status and render policy + pressure.
- `cohsh telemetry explain` — summarize current policy and refusal/eviction outcomes.

#### Mandatory regression cases
- Quota exhaustion: `refuse` vs `evict-oldest` (21a) and `overwrite_acked` (25b).
- Ack cursor behind overwrite window (25b).
- Record-too-large rejection (25b).
- Offline accumulation → online drain (21a + 25b).

#### Invariants
- All drops are auditable.
- Backpressure is observable.
- No data loss occurs without an explicit policy allowing it.

### Regression coverage
- `scripts/cohsh/lifecycle_basic.coh`
- `scripts/cohsh/lifecycle_drain_spool.coh`
- `scripts/cohsh/lifecycle_reboot_resume.coh`

### Checks (DoD)
- Lifecycle transitions are byte-stable across serial/TCP.
- Telemetry is not lost during `DRAINING` (spool or queue behavior is deterministic).
- `QUIESCED` guarantees zero outstanding leases.
- Replay reproduces identical state transitions and audit lines.

### Task Breakdown
```
Title/ID: m21d-lifecycle-state-machine
Goal: Implement the node lifecycle state machine in root-task.
Inputs: docs/ARCHITECTURE.md, docs/INTERFACES.md, docs/ROLES_AND_SCHEDULING.md.
Changes:
  - apps/root-task/src/lifecycle.rs — state machine + transition validation.
  - apps/root-task/src/lib.rs — hook lifecycle into boot + worker/ticket gating.
Commands:
  - cargo test -p root-task
Checks:
  - Invalid transitions return deterministic ERR with audit lines.
Deliverables:
  - Root-task lifecycle state machine with deterministic transitions.

Title/ID: m21d-lifecycle-ir
Goal: Add lifecycle policy fields to compiler IR and regenerate artifacts.
Inputs: configs/root_task.toml, tools/coh-rtc, docs/ARCHITECTURE.md.
Changes:
  - tools/coh-rtc/src/ir.rs — lifecycle policy fields (initial state, allowed auto transitions).
  - configs/root_task.toml — lifecycle policy configuration.
Commands:
  - cargo run -p coh-rtc
  - scripts/check-generated.sh
Checks:
  - Generated snippets update; invalid lifecycle policy is rejected.
Deliverables:
  - Manifest-backed lifecycle policy with validated defaults.

Title/ID: m21d-lifecycle-namespace
Goal: Expose lifecycle nodes via NineDoor and enforce permissions.
Inputs: apps/nine-door, docs/INTERFACES.md.
Changes:
  - apps/nine-door/src/host/lifecycle.rs — /proc/lifecycle/* + /queen/lifecycle/ctl.
  - apps/nine-door/src/host/namespace.rs — mount lifecycle provider.
Commands:
  - cargo test -p nine-door --test lifecycle
Checks:
  - `/proc/lifecycle/*` is read-only; `/queen/lifecycle/ctl` is queen-only append.
Deliverables:
  - Lifecycle nodes live with deterministic error semantics.

Title/ID: m21d-cohsh-lifecycle
Goal: Add cohsh lifecycle commands and update CLI docs.
Inputs: docs/USERLAND_AND_CLI.md, docs/INTERFACES.md.
Changes:
  - apps/cohsh/src/lib.rs — add `lifecycle` commands (cordon/drain/resume/quiesce/reset).
  - docs/USERLAND_AND_CLI.md — document lifecycle CLI surface and examples.
Commands:
  - cargo test -p cohsh
Checks:
  - CLI rejects invalid transitions locally with deterministic ERR.
Deliverables:
  - cohsh lifecycle commands with documented grammar.

Title/ID: m21d-lifecycle-regressions
Goal: Lock lifecycle behavior with deterministic regression scripts.
Inputs: scripts/cohsh/.
Changes:
  - scripts/cohsh/lifecycle_basic.coh
  - scripts/cohsh/lifecycle_drain_spool.coh
  - scripts/cohsh/lifecycle_reboot_resume.coh
Commands:
  - cohsh --script scripts/cohsh/lifecycle_basic.coh
Checks:
  - Scripts pass unchanged; transcript ordering stable.
Deliverables:
  - Regression coverage for lifecycle transitions and gating.

Title/ID: m21d-docs-failure-modes
Goal: Add operator-facing failure semantics and walkthroughs.
Inputs: docs/ARCHITECTURE.md, docs/INTERFACES.md, docs/USERLAND_AND_CLI.md.
Changes:
  - docs/FAILURE_MODES.md — explicit failure behavior and recovery actions.
  - docs/OPERATOR_WALKTHROUGH.md — end-to-end lifecycle narrative with artifacts.
Commands:
  - mdbook build docs (if configured)
Checks:
  - Docs describe as-built behavior and reference canonical interfaces.
Deliverables:
  - Failure modes and operator walkthrough docs committed and referenced.
```

---

## Milestone 21e — Rooted Authority, Cut Detection, Explicit Session Semantics, and Live Hive Visibility <a id="21e"></a> 
[Milestones](#Milestones)

**Why now (operator and systems):** Cohesix enforces strict Queen/Worker authority and bounded control, but operators need those guarantees to be visible and interpretable during live operation. This milestone strengthens control-plane semantics using rooted-network ideas and extends SwarmUI Live Hive to reflect these states explicitly. The goal is less ambiguity, not more intelligence.

No new protocols, no consensus, no background convergence - only explicit state, refusal, and audit.

## Goal
1. Make root reachability and network cuts explicit and inspectable.
2. Formalize session setup, drain, and teardown semantics.
3. Surface back-pressure (busy, quota, cut, policy) as first-class signals.
4. Ensure SwarmUI Live Hive visualizes these states so operators do not infer health incorrectly.

## Non-Goals (Explicit)
- No leader election or consensus protocols
- No automatic failover or self-promotion
- No hidden retries or background loops
- No new transports or networking paths
- No relaxation of Secure9P / NineDoor invariants
- No changes to ACK/ERR grammar beyond reason tags

## Deliverables

### 1) Root Reachability and Cut Detection
New read-only nodes (IR-gated, bounded):

`/proc/root/reachable` (ro: `reachable=yes|no`)  
`/proc/root/last_seen_ms` (ro: `last_seen_ms=<u64>`)  
`/proc/root/cut_reason` (ro: `cut_reason=<none|network_unreachable|session_revoked|policy_denied|lifecycle_offline>`)

**Rules**
- Workers MUST NOT exercise authority when `/proc/root/reachable=no`.
- Queen authority is never inferred or mirrored.
- `last_seen_ms` updates on authenticated Queen activity.
- `cut_reason=none` when `reachable=yes`; otherwise use deterministic priority: `lifecycle_offline` > `session_revoked` > `policy_denied` > `network_unreachable`.
- Cut detection feeds lifecycle state transitions per Milestone 21d (explicit events only, no heuristics).

---

### 2) Explicit Session Semantics (Telephone-Exchange Model)
Expose session lifecycle explicitly:

`/proc/9p/session/active`  
`/proc/9p/session/<id>/state`  
`/proc/9p/session/<id>/since_ms`  
`/proc/9p/session/<id>/owner`

Session states:
- `SETUP`
- `ACTIVE`
- `DRAINING`
- `CLOSED`

**Rules**
- No implicit resurrection of sessions.
- Revocation immediately transitions to `CLOSED`.
- `DRAINING` forbids new control actions but allows telemetry completion.
- `/proc/9p/sessions` remains unchanged; new per-session nodes are additive only.

---

### 3) Busy / Back-Pressure as First-Class Signals
Standard refusal reason tags (console):
- `ERR <verb> reason=busy`
- `ERR <verb> reason=quota`
- `ERR <verb> reason=cut`
- `ERR <verb> reason=policy`

NineDoor error codes remain within the existing error surface; no new error names are introduced.

Pressure counters (IR-gated, bounded):

`/proc/pressure/busy`  
`/proc/pressure/quota`  
`/proc/pressure/cut`  
`/proc/pressure/policy`

**Rules**
- No automatic retries inside Cohesix.
- All refusals are deterministic and audited.
- Callers decide retry behavior.

---

### 4) SwarmUI Live Hive — Visualizing Authority and Pressure
**Rationale:** Without visualization, correct behavior looks like failure. Live Hive must reflect authority, reachability, and contention so operators do not infer false health or blame the wrong layer.

**Constraints**
- No new protocols
- No new data collection
- Purely renders existing file-shaped state
- Live Hive reads text nodes (no CBOR requirement)

#### 4a) Root / Cut Status Badge
Live Hive reads:
- `/proc/root/reachable`
- `/proc/root/cut_reason`

UI:
- Prominent `ROOT OK` or `CUT` badge per node
- Cut reason displayed inline
- Nodes in CUT state are visually distinct (no "healthy" styling)

#### 4b) Session Indicator
Live Hive reads:
- `/proc/9p/session/active`
- `/proc/9p/session/<id>/state`

UI:
- Session count per node
- Highlight when sessions enter `DRAINING`
- Summary only (no per-session deep UI)

#### 4c) Back-Pressure Strip
Live Hive reads:
- `/proc/pressure/busy`
- `/proc/pressure/quota`
- `/proc/pressure/cut`
- `/proc/pressure/policy`

UI:
- Small pressure indicators or counters
- Makes contention visible instead of mysterious slowdown

#### 4d) Error Classification
Live Hive classifies ACK/ERR events:
- Distinguish `reason=busy`, `reason=quota`, `reason=cut`, `reason=policy`
- Display as categorized events, not generic failures

---

### 5) Audit and Replay Integration
All new semantics:
- Emit audit lines via existing AuditFS
- Are replayable via ReplayFS
- Produce byte-identical ACK/ERR sequences on replay

## Files and Components Touched
- `apps/root-task/` — root reachability state and refusal tagging
- `apps/nine-door/` — session lifecycle tracking and `/proc/9p/session/*`
- `apps/swarmui/` — Live Hive rendering of root, sessions, and pressure
- `tools/coh-rtc/` — observability gates and bounds
- `configs/root_task.toml`
- `docs/ARCHITECTURE.md`
- `docs/INTERFACES.md`
- `docs/SECURITY.md`
- `docs/USERLAND_AND_CLI.md`

## Regression Coverage (Required)
New scripts:
- `scripts/cohsh/root_cut_basic.coh`
- `scripts/cohsh/session_lifecycle.coh`
- `scripts/cohsh/busy_backpressure.coh`

Live Hive validation:
- Visual state matches `/proc/*` values
- Replay shows identical transitions and UI markers

## Checks (Definition of Done)
- Root cuts are explicit, auditable, and visible in Live Hive.
- No worker acts under partition.
- Session teardown is immediate and deterministic.
- Back-pressure is visible and never silent.
- Replay reproduces identical control outcomes.
- No new transports or background logic introduced.
- Regression Pack passes with additive coverage only.

## Compiler touchpoints
- `coh-rtc` emits observability gates and bounds for `/proc/root/*`, `/proc/9p/session/*`, and `/proc/pressure/*`.
- Generated snippets update `docs/ARCHITECTURE.md` and `docs/INTERFACES.md`; drift fails CI.

## Task Breakdown
```
Title/ID: m21e-root-reachability-ir
Goal: Add root reachability and pressure nodes to IR and regenerate artifacts.
Inputs: configs/root_task.toml, tools/coh-rtc, docs/ARCHITECTURE.md, docs/INTERFACES.md.
Changes:
  - tools/coh-rtc/src/ir.rs — add /proc/root/* and /proc/pressure/* gates + bounds.
  - configs/root_task.toml — manifest toggles and size limits.
Commands:
  - cargo run -p coh-rtc
  - scripts/check-generated.sh
Checks:
  - Generated snippets list the new nodes with correct bounds.
Deliverables:
  - Regenerated snippets and manifest artifacts.

Title/ID: m21e-root-reachability-runtime
Goal: Track and expose root reachability and cut reason deterministically.
Inputs: apps/root-task, docs/ROLES_AND_SCHEDULING.md.
Changes:
  - apps/root-task/src/lifecycle.rs — integrate cut reason updates.
  - apps/root-task/src/observability.rs — emit /proc/root/* values.
Commands:
  - cargo test -p root-task
Checks:
  - reachable/cut_reason updates are deterministic and audited.
Deliverables:
  - Root reachability state wired to observability.

Title/ID: m21e-session-semantics
Goal: Expose per-session state for NineDoor sessions.
Inputs: apps/nine-door, docs/INTERFACES.md.
Changes:
  - apps/nine-door/src/host/session.rs — state tracking + /proc/9p/session/*.
  - apps/nine-door/src/host/namespace.rs — mount session provider.
Commands:
  - cargo test -p nine-door --test session_state
Checks:
  - Session transitions match SETUP/ACTIVE/DRAINING/CLOSED with stable output.
Deliverables:
  - Per-session observability nodes with deterministic state.

Title/ID: m21e-pressure-refusal
Goal: Standardize refusal reason tags and pressure counters.
Inputs: apps/root-task, apps/nine-door, docs/SECURITY.md.
Changes:
  - apps/root-task/src/event/mod.rs — emit ERR reason tags (busy/quota/cut/policy).
  - apps/nine-door/src/host/security.rs — increment /proc/pressure/* counters.
Commands:
  - cargo test -p root-task
  - cargo test -p nine-door --test pressure_counters
Checks:
  - Refusals increment counters and emit reason tags without new error names.
Deliverables:
  - Deterministic refusal tagging and pressure counters.

Title/ID: m21e-swarmui-livehive
Goal: Render root, session, and pressure state in Live Hive.
Inputs: apps/swarmui/frontend, apps/swarmui/src-tauri, docs/INTERFACES.md.
Changes:
  - apps/swarmui/frontend/hive/ — badges, counters, and session summary.
  - apps/swarmui/src-tauri/ — read new /proc text nodes.
Commands:
  - cargo check -p swarmui
Checks:
  - Live Hive displays root/cut, sessions, and pressure when view is active.
Deliverables:
  - Live Hive visuals wired to text-based /proc nodes.

Title/ID: m21e-regressions
Goal: Lock behavior with deterministic regression scripts and UI replay.
Inputs: scripts/cohsh/, docs/TEST_PLAN.md.
Changes:
  - scripts/cohsh/root_cut_basic.coh
  - scripts/cohsh/session_lifecycle.coh
  - scripts/cohsh/busy_backpressure.coh
Commands:
  - cohsh --script scripts/cohsh/root_cut_basic.coh
Checks:
  - Scripts pass unchanged; ACK/ERR ordering stable.
Deliverables:
  - Regression scripts for reachability, session lifecycle, and pressure.
```

---

## Milestone 22 — Runtime Convenience (coh run) + GPU Job Breadcrumbs  <a id="22"></a> 
[Milestones](#Milestones)

**Status:** Complete — coh run + breadcrumb schema, docs, and tests are in place; regression pack and full test plan (source + macOS/Ubuntu bundles) completed.

**Why now (adoption):** Operators need a two-minute "lease -> run -> observe -> release" loop without introducing a runtime orchestrator.

**Goal**
Provide a `coh run` wrapper that validates leases, runs a user command, and records bounded lifecycle breadcrumbs.

**Deliverables**
- `coh run` subcommand that verifies an active lease via `/gpu/<id>/lease` before execution and refuses to run without one.
- Wrapper executes a user-specified command (Docker or local binary) and appends bounded lifecycle breadcrumbs to `/gpu/<id>/status` (per `docs/GPU_NODES.md`) through the host bridge interface.
- Denial path emits deterministic ERR with no side effects; wrapper remains non-orchestrating.

**Commands**
- `cargo test -p coh --features mock --test run`
- `cargo test -p coh --features mock --test transcript`
- `cargo run -p cohsh -- --transport mock --mock-seed-gpu --script scripts/cohsh/run_demo.coh`

**Checks (DoD)**
- Demo script proves "lease -> run -> observe -> release" in under two minutes using `--mock`.
- `coh run` denies when no valid lease exists and logs deterministic ERR without side effects.
- Breadcrumbs in `/gpu/<id>/status` are bounded, ordered, and schema-tagged; regressions enforce ordering and denial semantics.
- Deterministic denial semantics for invalid tickets/paths/quotas are verified in tests.
- Bounded memory and bounded work per operation (no unbounded queues, no infinite retries) are enforced by limits and tests.
- Secure9P invariants preserved (msize <= 8192, path validation, fid lifecycle).
- Console semantics preserved (ACK-before-side-effects) for console-backed flows.
- Regression pack runs unchanged; output drift fails and new tests are additive.
- CI runs mock-mode tests on x86_64.

**Compiler touchpoints**
- `coh-rtc` emits breadcrumb schema, max line bytes, and lease validation defaults into a manifest snippet consumed by `coh`.
- Manifest gates ensure breadcrumb fields match documented `/gpu/<id>/status` semantics.

**Task Breakdown**
```
Title/ID: m22-coh-run
Goal: Implement coh run wrapper with lease validation and bounded lifecycle logging.
Inputs: docs/GPU_NODES.md, docs/INTERFACES.md, cohsh-core transport.
Changes:
  - apps/coh/src/run.rs — lease check, command spawn, breadcrumb emission.
  - apps/coh/tests/run.rs — denial path and ordered breadcrumb tests.
Commands:
  - cargo test -p coh --features mock --test run
Checks:
  - No-lease path returns deterministic ERR; breadcrumbs are ordered and bounded.
Deliverables:
  - coh run behavior documented with transcript fixtures.

Title/ID: m22-breadcrumb-schema
Goal: Define and lock breadcrumb schema for /gpu/<id>/status entries.
Inputs: docs/GPU_NODES.md, manifest IR.
Changes:
  - tools/coh-rtc/ — emit breadcrumb schema and limits for host tooling.
  - docs/INTERFACES.md — update status schema snippet via codegen.
Commands:
  - cargo run -p coh-rtc
Checks:
  - Generated schema hash matches committed docs; invalid fields rejected by coh.
Deliverables:
  - Breadcrumb schema published and referenced by host tools.

Title/ID: m22-run-regressions
Goal: Add regression coverage for run wrapper ordering and denial semantics.
Inputs: scripts/cohsh/*.coh, tests/fixtures/transcripts/.
Changes:
  - scripts/cohsh/run_demo.coh — lease, run, observe, release sequence.
  - apps/coh/tests/transcript.rs — compare coh run transcript to cohsh baseline.
Commands:
  - cargo test -p coh --features mock --test transcript
Checks:
  - Transcript diff is zero; denial case emits deterministic ERR.
Deliverables:
  - Regression fixtures stored; CI hook updated.
```

---

## Milestone 23 — PEFT/LoRA Lifecycle Glue (coh peft) <a id="23"></a> 
[Milestones](#Milestones)

**Status:** Complete — PEFT lifecycle flows, dev-virt GPU mock entries, SwarmUI replay/path/layout fixes, regression pack, and full test plan (source + macOS/Ubuntu bundles) validated.

**Why now (adoption):** PEFT users need a file-native loop to export jobs, import adapters, and activate or rollback safely without a new control plane.

**Goal**
Provide `coh peft` commands that export LoRA jobs, import adapters, and atomically activate or rollback models.

**Deliverables**
- `coh peft export` pulls `/queen/export/lora_jobs/<job_id>/` (telemetry.cbor, base_model.ref, policy.toml) into a host directory; any new manifest/provenance file must be introduced via `coh-rtc` and documented in `docs/GPU_NODES.md` and `docs/INTERFACES.md` in the same change.
- `coh peft import` stages adapters into host storage and exposes them as `/gpu/models/available/<model_id>/manifest.toml` with hash/size/provenance checks.
- `coh peft activate` swaps `/gpu/models/active` atomically; `coh peft rollback` reverts to the previous pointer with a documented recovery path.
- No training in VM and no registry service; file-native only.
- New namespaces (`/queen/export/*`, `/gpu/models/*`) have explicit NineDoor provider ownership and manifest gating; docs are updated before code depends on them.
- `dev-virt` QEMU runs without a host GPU bridge expose mock `/gpu/<id>/{info,ctl,lease,status}` entries for CLI demos (GPU-0/GPU-1); `/gpu/models` remains host-mirrored only.
- SwarmUI replay flags (`--replay-trace`, `--replay`) accept absolute or relative paths so release bundles can replay fixtures without path assumptions.
- SwarmUI pressure/error labels break to their own line and chips render slightly smaller for readability.

**Commands**
- `cargo test -p coh --features mock --test peft`
- `cargo run -p coh --features mock -- peft export --job job_8932 --out out/lora_jobs`
- `cargo run -p coh --features mock -- peft import --model llama3-edge-v7 --from out/adapter`
- `cargo run -p coh --features mock -- peft activate --model llama3-edge-v7`

**Checks (DoD)**
- End-to-end demo covers export -> import -> activate -> rollback with deterministic outputs.
- Adapter hash/size/provenance checks reject invalid input with deterministic ERR and no side effects.
- Rollback procedure is documented and tested.
- Deterministic denial semantics for invalid tickets/paths/quotas are verified in tests.
- Bounded memory and bounded work per operation (no unbounded queues, no infinite retries) are enforced by limits and tests.
- Secure9P invariants preserved (msize <= 8192, path validation, fid lifecycle).
- Console semantics preserved (ACK-before-side-effects) for console-backed flows.
- Regression pack runs unchanged; output drift fails and new tests are additive.
- CI runs mock-mode tests on x86_64.

**Compiler touchpoints**
- `coh-rtc` emits any LoRA job schema/provenance fields (if/when added) and pointer-swap limits for `coh peft`.
- Generated snippets refresh `docs/INTERFACES.md` and `docs/GPU_NODES.md` to keep schema alignment.
- Manifest gating enumerates the new export/model namespaces and their provider ownership.

**Task Breakdown**
```
Title/ID: m23-peft-export
Goal: Implement coh peft export from /queen/export/lora_jobs/* with bounded pulls.
Inputs: docs/GPU_NODES.md, docs/INTERFACES.md.
Changes:
  - apps/coh/src/peft/export.rs — pull job directory with manifest validation.
  - apps/coh/tests/peft_export.rs — resumable pull and idempotency tests.
Commands:
  - cargo test -p coh --features mock --test peft_export
Checks:
  - Export resumes without duplicates; missing job returns deterministic ERR.
Deliverables:
  - Export workflow documented with fixtures.

Title/ID: m23-peft-import-activate
Goal: Import adapters and atomically activate model pointers.
Inputs: host model registry, docs/GPU_NODES.md.
Changes:
  - apps/coh/src/peft/import.rs — hash/size/provenance checks for adapters.
  - apps/coh/src/peft/activate.rs — atomic pointer swap with rollback metadata.
Commands:
  - cargo test -p coh --features mock --test peft_import
Checks:
  - Invalid hashes rejected; pointer swap is atomic and rollback restores previous model.
Deliverables:
  - Activation/rollback behavior documented with transcript fixtures.

Title/ID: m23-peft-regressions
Goal: Validate end-to-end PEFT lifecycle flows.
Inputs: scripts/cohsh/*.coh, tests/fixtures/transcripts/.
Changes:
  - scripts/cohsh/peft_roundtrip.coh — export/import/activate/rollback sequence.
  - apps/coh/tests/transcript.rs — parity check with cohsh output.
Commands:
  - cargo test -p coh --features mock --test transcript
Checks:
  - Transcript diff zero; rollback emits deterministic ACK/ERR ordering.
Deliverables:
  - Regression fixtures stored and referenced in docs.

Title/ID: m23-dev-virt-gpu-mock
Goal: Provide mock `/gpu` entries for CLI demos in dev-virt.
Inputs: docs/GPU_NODES.md, docs/INTERFACES.md.
Changes:
  - apps/root-task/src/ninedoor.rs — seed mock `/gpu/<id>` entries and bounded logs when no host GPU bridge is present.
Commands:
  - cargo run -p cohsh --features mock -- --transport tcp --host 127.0.0.1 --port 31337 --script resources/proc_tests/selftest_quick.coh
Checks:
  - `coh gpu list` returns mock GPU-0/GPU-1 and reads `/gpu/<id>/info` without errors.
Deliverables:
  - Mock `/gpu` entries documented in GPU/NineDoor interface docs.

Title/ID: m23-swarmui-replay-path
Goal: Normalize SwarmUI replay paths for release bundle replay.
Inputs: docs/TEST_PLAN.md, release bundle layout.
Changes:
  - apps/swarmui/src-tauri/main.rs — resolve replay paths as absolute when provided relative inputs.
Commands:
  - cargo test -p swarmui --test trace
Checks:
  - `swarmui --replay-trace <relative path>` and `swarmui --replay <relative path>` work from bundle root.
Deliverables:
  - Replay instructions remain consistent in docs/TEST_PLAN.md.

Title/ID: m23-swarmui-pressure-layout
Goal: Improve SwarmUI pressure/error strip readability.
Inputs: apps/swarmui/frontend/index.html, apps/swarmui/frontend/styles/layout.css.
Changes:
  - apps/swarmui/frontend/styles/layout.css — line-break pressure/error labels and shrink status chips slightly.
Commands:
  - cargo test -p swarmui --test console_parity
Checks:
  - Pressure and error labels render above chips; chip text remains legible.
Deliverables:
  - UI layout changes tracked in the milestone.
```

---

## Milestone 24 — Python Client + Examples (cohesix) + Doctor + Release Cut <a id="24"></a> 
[Milestones](#Milestones)

**Why now (adoption):** A thin, non-authoritative Python layer and a setup doctor reduce friction for CUDA, PEFT, and edge users without altering the control plane.

**Goal**
Deliver the `cohesix` Python client, runnable examples, `coh doctor`, and Alpha packaging/quickstart.

**Deliverables**
- `cohesix` Python library with filesystem backend (via `coh mount`) and TCP backend (via `cohsh-core` grammar); parity tests prove no new semantics.
- Examples (fast, inspectable artifacts): CUDA lease+run, MIG lease+run (when available), PEFT export/import/activate/rollback, edge telemetry write + `coh telemetry pull`.
- `coh doctor` subcommand for deterministic environment checks (tickets, mount capability, NVML or `--mock`, runtime prerequisites).
- Alpha packaging and minimal quickstart docs for `coh` + `cohesix`.
- Release bundle updated to include `coh`, `cohesix`, and the doctor/quickstart artifacts in the shipped tarballs.

**Commands**
- `cargo run -p coh --features mock -- doctor --mock`
- `python -m pytest -k cohesix_parity`
- `python tools/cohesix-py/examples/lease_run.py --mock`

**Checks (DoD)**
- Fresh host can run `coh doctor` then a demo in < 15 minutes using `--mock`.
- Python parity tests match `cohsh` or namespace behavior byte-for-byte where applicable; no new semantics introduced.
- Examples leave inspectable artifacts and exit deterministically.
- Deterministic denial semantics for invalid tickets/paths/quotas are verified in tests.
- Bounded memory and bounded work per operation (no unbounded queues, no infinite retries) are enforced by limits and tests.
- Secure9P invariants preserved (msize <= 8192, path validation, fid lifecycle).
- Console semantics preserved (ACK-before-side-effects) for console-backed flows.
- Regression pack runs unchanged; output drift fails and new tests are additive.
- CI runs mock-mode tests on x86_64.

**Compiler touchpoints**
- `coh-rtc` emits Python client defaults (paths, size limits, example fixtures) and `coh doctor` checks into manifest-backed snippets for docs.
- Parity fixtures are hashed and referenced in docs/TEST_PLAN.md.

**Task Breakdown**
```
Title/ID: m24-python-client
Goal: Build cohesix Python client with filesystem + TCP backends and parity tests.
Inputs: crates/cohsh-core, docs/USERLAND_AND_CLI.md, docs/INTERFACES.md.
Changes:
  - tools/cohesix-py/ — Python package with fs and TCP backends.
  - tools/cohesix-py/tests/parity.py — parity tests against cohsh transcripts.
Commands:
  - python -m pytest -k cohesix_parity
Checks:
  - Parity tests match cohsh transcripts; invalid ticket yields deterministic ERR.
Deliverables:
  - Python client package and parity fixtures.

Title/ID: m24-examples
Goal: Provide quick examples that leave inspectable artifacts.
Inputs: tools/cohesix-py/examples/, docs/GPU_NODES.md.
Changes:
  - tools/cohesix-py/examples/lease_run.py — lease -> run -> release example.
  - tools/cohesix-py/examples/peft_roundtrip.py — export/import/activate/rollback.
Commands:
  - python tools/cohesix-py/examples/lease_run.py --mock
Checks:
  - Example outputs are deterministic and bounded; artifacts stored under out/examples/.
Deliverables:
  - Example artifacts and docs/USERLAND_AND_CLI.md updates.

Title/ID: m24-doctor-release
Goal: Implement coh doctor and Alpha packaging/quickstart.
Inputs: apps/coh/, docs/USERLAND_AND_CLI.md, README.md.
Changes:
  - apps/coh/src/doctor.rs — deterministic checks for tickets, mounts, NVML/mock, runtime prerequisites.
  - docs/QUICKSTART_ALPHA.md — minimal Alpha quickstart for coh + cohesix.
Commands:
  - cargo run -p coh --features mock -- doctor --mock
Checks:
  - Doctor emits deterministic actionable output; packaging contains coh + cohesix.
Deliverables:
  - Alpha quickstart docs and packaging notes.
```

----
**Alpha Release 2 achieved here**
----

Next, Alpha release 3 targets bare metal UEFI and AWS native boot via AMI.

--

## Milestone 25a — UEFI Bare-Metal Boot & Device Identity <a id="25a"></a> 
[Milestones](#Milestones)

**Why now (context):**  
To meet hardware deployment goals (Edge §3 retail hubs, Edge §8 defense ISR, Security §12 segmentation), Cohesix must boot on physical aarch64 UEFI hardware with attested manifests while preserving the lean `no_std` footprint and the upstream seL4 boot model. This milestone transitions from the QEMU reference profile to physical UEFI deployment, with VM behavior expected to mirror the hardware target unless explicitly profile-gated.

**Non-negotiable constraint:**  
No networking, 9P semantics, or console behaviors may diverge between VM and UEFI profiles except where explicitly marked as hardware-profile-specific in `ARCHITECTURE.md`. Any divergence must be documented and schema-gated. UEFI firmware networking is out of scope; all TCP behavior remains post-seL4 boot in root-task.

---

### Prerequisite (must be completed before Milestone 25a)
**Upstream elfloader EFI support**
- Confirm and enable upstream seL4 **elfloader EFI build** to produce a valid PE/COFF EFI executable (`elfloader.efi`) for aarch64.
- The EFI-built elfloader must:
  - Relocate correctly under UEFI.
  - Load the seL4 kernel, initial user image, DTB (when present), and CPIO rootfs.
  - Preserve existing VM boot semantics once seL4 is entered.
- Any local build glue required to emit `elfloader.efi` must not fork elfloader logic and must track upstream.

---

### Goal
Deliver a **UEFI → elfloader.efi → seL4 → root-task** boot path that loads the generated manifest from boot media, performs TPM-backed (or DICE-fallback) identity attestation in root-task, and mirrors VM behavior deterministically.

---

### Deliverables

- **UEFI boot chain**
  - Use upstream **elfloader built as an EFI PE/COFF binary** (`EFI/BOOT/BOOTAA64.EFI`) as the sole UEFI application.
  - Root-task remains the first user process post-kernel boot; root-task is never executed as an EFI application.

- **UEFI image builder**
  - Introduce `scripts/uefi/esp-build.sh` to build a reproducible FAT ESP containing:
    - `EFI/BOOT/BOOTAA64.EFI` (elfloader EFI)
    - `kernel.elf`
    - `rootserver` (root task ELF)
    - optional `initrd.cpio`
    - `manifest.json` and `manifest.sha256`
    - optional `dtb/` assets (platform-specific)
  - Deterministic file ordering and hashes; build logs captured as CI artifacts.

- **Identity & attestation**
  - Identity subsystem implemented in root-task leveraging **TPM 2.0** or declared **DICE fallback**.
  - Capability ticket seeds are sealed only after successful attestation.
  - Attestation evidence bound to the manifest fingerprint is appended to `/proc/boot` and exported via NineDoor.
  - If attestation is enabled but unavailable, boot aborts deterministically with audited error and no partial state.

- **Schema & validation**
- Manifest IR (current schema) uses `profile.name: uefi-aarch64`.
  - Hardware declarations under a gated section (e.g., `hw.devices[]` with UART, NET, TPM, RTC; `hw.secure_boot`; `hw.attestation`).
  - Validation enforces required bindings and TPM availability when attestation is enabled.

- **Secure Boot documentation**
  - Secure Boot treated as firmware enforcement; OS records and validates observable state where trustworthy.
  - Measurements, manifest fingerprints, and bring-up notes captured in `docs/HARDWARE_BRINGUP.md` and aligned with `docs/SECURITY.md`.

- **Automation & bring-up**
  - Introduce `scripts/uefi/qemu-uefi.sh` using EDK2 pflash and the EFI-built elfloader.
  - Optional host-only TPM emulation (e.g., `swtpm`) for QEMU testing.
  - Lab checklist for the reference dev board.

---

### Commands
- `cmake --build seL4/build --target elfloader.efi`
- `scripts/uefi/esp-build.sh --manifest out/manifests/root_task_resolved.json`
- `scripts/uefi/qemu-uefi.sh --console serial --tcp-port 31337`
- Physical hardware checklist: capture `/proc/boot`, compare manifest hash to CI baseline.

---

### Checks (DoD)
- EFI-built elfloader boots under QEMU TCG and on the reference dev board.
- Serial startup ordering **exactly matches** VM baseline; any drift is a bug unless profile-gated and documented.
- Manifest fingerprint printed early and matches packaged hash.
- If `hw.attestation.enabled=true`, attestation succeeds and evidence hash matches the manifest fingerprint; if unavailable, boot aborts deterministically.
- Compiler rejects manifests selecting `uefi-aarch64` without required hardware bindings or attestation settings.
- Full Regression Pack passes under QEMU and (where applicable) on hardware; any divergence is treated as a bug unless explicitly documented.

---

### Compiler touchpoints
- `coh-rtc` emits hardware tables for the selected profile; docs import them into `docs/HARDWARE_BRINGUP.md` and `docs/ARCHITECTURE.md`.
- Regeneration guard compares manifest fingerprints recorded in UEFI docs against generated outputs, failing CI on drift.

---

## Task Breakdown

### Title/ID: m25a-uefi-bootchain
**Goal:** Boot via UEFI → elfloader.efi → seL4; load manifest from ESP; emit stable fingerprint lines.  
**Inputs:** EFI-built elfloader, `scripts/uefi/esp-build.sh`, `scripts/uefi/qemu-uefi.sh`, `configs/root_task.toml` (`profile.name`).  
**Changes:**
- `scripts/uefi/esp-build.sh` — build ESP with `BOOTAA64.EFI` (elfloader), kernel, rootserver, optional initrd, manifest + hash; deterministic logs.
- `scripts/uefi/qemu-uefi.sh` — UEFI QEMU path using EDK2 pflash; keep `virt` machine for parity.
- `apps/root-task` — print manifest fingerprint in the same serial ordering as VM baseline.
**Commands:**
- `cmake --build seL4/build --target elfloader.efi`
- `scripts/uefi/esp-build.sh --manifest out/manifests/root_task_resolved.json`
**Checks:**
- QEMU `--uefi` serial output matches VM ordering; missing/invalid manifest aborts before any ticket material.
**Deliverables:**
- UEFI boot artifacts and referenced fingerprints in `docs/HARDWARE_BRINGUP.md`.

---

### Title/ID: m25a-attestation
**Goal:** Implement TPM/DICE identity sealing and export via `/proc/boot` with strict determinism.  
**Inputs:** `apps/root-task/src/attest.rs`, `docs/SECURITY.md` (attestation section).  
**Changes:**
- `apps/root-task/src/attest.rs` — bounded TPM quote path + DICE fallback; seal ticket seeds only after successful attestation.
- `/proc/boot` — append evidence summary (hashes/IDs only; no secrets).
- Host docs/scripts for optional QEMU TPM emulation.
**Commands:**
- `scripts/uefi/qemu-uefi.sh --console serial --tcp-port 31337`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/boot_v0.coh`
**Checks:**
- Attestation failure aborts boot deterministically with audit.
- Successful evidence hash matches manifest fingerprint.
- VM vs hardware outputs compared; differences must be profile-gated.
**Deliverables:**
- Attestation evidence documented in `docs/SECURITY.md` and `docs/HARDWARE_BRINGUP.md`.

---

## Milestone 25b — UEFI On-Device Spool Stores + Settings Persistence <a id="25b"></a> 
[Milestones](#Milestones)

**Why now (resilience):** After UEFI boot + identity (25a), edge deployments need store/forward for telemetry and minimal settings that survive reboots and link outages without introducing a general filesystem or new protocols.

**Non-negotiable constraints**
- No changes to console grammar, 9P semantics, or TCP behavior vs VM unless profile‑gated and documented.
- No POSIX VFS; no general filesystem.
- Pure Rust userspace; no C‑FFI filesystems.
- Persistence is exposed only through NineDoor nodes (file‑shaped, bounded).

### Prerequisite
- Milestone **25a** completed (UEFI boot chain + device identity).

### Goal
Provide **bounded, crash‑resilient on‑device persistence** for:
1) telemetry store/forward (append‑only ring log), and  
2) minimal settings (A/B committed pages),
exposed through NineDoor without expanding the TCB.

### Deliverables

#### A) Storage plumbing (hardware + QEMU parity)
- Block‑device abstraction in HAL (role‑selected devices, not model‑selected).
- QEMU reference uses `virtio-blk`; hardware path is profile‑gated and documented.
- Manifest gates for persistence features; no `std` dependencies.

#### B) Telemetry spool store (append‑only ring log)
- Backing: fixed‑size block region/partition.
- Record format (versioned, bounded): `magic | version | kind | seq | ts | len | crc | payload`.
- Crash rule: a record is valid only if header + checksum validate; partial tail records are ignored.
- Bounded behavior:
  - max record size and deterministic scan budget.
  - explicit policy: **refuse when full** or **overwrite oldest only when acked**.
- NineDoor exposure (names must align with `ARCHITECTURE.md`):
  - `/proc/spool/status` (read‑only)
  - `/proc/spool/append` (write‑only, one record per write)
  - `/proc/spool/read` (bounded read stream)
  - `/proc/spool/ack` (write‑only cursor advance)

#### C) Settings store (A/B committed pages)
- Two fixed pages/blocks with `generation + checksum`.
- Update semantics: write inactive page fully, validate checksum, then commit by generation.
- Bounded settings size; strict UTF‑8 validation and max key/value lengths (if KV).

#### D) Identity binding
- Spool/settings metadata binds to the **manifest fingerprint** from 25a (e.g., recorded in `/proc/boot`), without introducing new trust roots.

#### E) Testing + regression hardening
- Crash‑fault simulation tests for both stores (power loss at every write boundary).
- Fuzz record decoder with strict size limits; reject malformed frames.
- Golden fixture: known block image → expected `status/read/ack` behavior.
- Regression pack additions:
  - `scripts/cohsh/spool_roundtrip.coh`
  - `scripts/cohsh/settings_roundtrip.coh`

### Commands
- `cargo test -p root-task`
- `cargo test -p nine-door`
- `cohsh --script scripts/cohsh/spool_roundtrip.coh`
- `cohsh --script scripts/cohsh/settings_roundtrip.coh`

### Checks (DoD)
- Spool append/read/ack semantics are deterministic and bounded; invalid tail records after crash are ignored.
- Store/forward works offline and resumes correctly after reboot.
- Settings updates are atomic across power loss (A/B semantics).
- No general filesystem or POSIX surface introduced.
- VM vs UEFI semantics remain byte‑stable unless explicitly profile‑gated.
- Regression pack passes unchanged; new tests are additive.

### Compiler touchpoints
- `coh-rtc` emits persistence limits (record size, max bytes, policy mode) into manifest IR; docs import the generated snippets.
- Manifest validation rejects persistence when storage devices are missing or mis‑declared for the UEFI profile.

### Task Breakdown
```
Title/ID: m25b-spool-core
Goal: Implement append‑only spool store over bounded block device.
Inputs: HAL block traits, docs/ARCHITECTURE.md, docs/INTERFACES.md.
Changes:
  - apps/root-task/src/storage/spool.rs — ring log + checksum validation.
  - apps/root-task/src/hal/block.rs — block traits + role‑selected device binding.
Commands:
  - cargo test -p root-task --test spool
Checks:
  - Partial tail records are ignored; bounded scan time enforced.
Deliverables:
  - Spool store core with deterministic semantics.

Title/ID: m25b-spool-namespace
Goal: Expose spool nodes via NineDoor.
Inputs: apps/nine-door, docs/INTERFACES.md.
Changes:
  - apps/nine-door/src/host/spool.rs — /proc/spool nodes.
  - apps/nine-door/src/host/namespace.rs — mount spool provider.
Commands:
  - cargo test -p nine-door --test spool
Checks:
  - Append/read/ack paths enforce quotas and policy mode.
Deliverables:
  - Spool namespace documented and wired.

Title/ID: m25b-settings-store
Goal: Implement A/B settings persistence with atomic commit.
Inputs: HAL block traits, docs/ARCHITECTURE.md.
Changes:
  - apps/root-task/src/storage/settings.rs — A/B pages + checksum.
Commands:
  - cargo test -p root-task --test settings
Checks:
  - Power‑loss simulations yield either old or new state, never corruption.
Deliverables:
  - Settings store with atomic semantics.

Title/ID: m25b-spool-regressions
Goal: Add deterministic regression scripts and fixtures.
Inputs: scripts/cohsh/, tests/fixtures/.
Changes:
  - scripts/cohsh/spool_roundtrip.coh — append/read/ack sequence.
  - scripts/cohsh/settings_roundtrip.coh — set/get + A/B markers.
Commands:
  - cohsh --script scripts/cohsh/spool_roundtrip.coh
  - cohsh --script scripts/cohsh/settings_roundtrip.coh
Checks:
  - Scripts pass unchanged; transcripts stable.
Deliverables:
  - Regression fixtures committed and referenced in docs/TEST_PLAN.md.
```

## Milestone 25c — SMP Utilization via Task Isolation (Multicore without Multithreading) <a id="25c"></a> 
[Milestones](#Milestones)

**Why now (platform and performance):** Cohesix targets modern aarch64 hardware where multicore CPUs are the norm. To scale throughput without sacrificing determinism, auditability, or TCB size, Cohesix must exploit seL4 SMP scheduling rather than introducing shared-memory multithreading. This milestone formalizes multicore usage through task isolation, sharding, and explicit authority boundaries.

This is a performance and clarity milestone, not a feature expansion.

## Goal
Enable Cohesix to take advantage of multicore aarch64 CPUs by:
1. Running multiple isolated seL4 tasks in parallel,
2. Keeping authoritative state single-threaded and serial, and
3. Scaling throughput through replication and partitioning, not threads.

The result must preserve:
- deterministic ACK/ERR ordering,
- replayability,
- bounded work per tick,
- and a minimal trusted computing base.

## Non-Goals (Explicit)
- No POSIX threads or shared-memory multithreading
- No async runtimes with implicit scheduling
- No background work queues with unbounded growth
- No relaxation of replay or audit guarantees
- No changes to Secure9P / NineDoor semantics
- No new protocols or transports

## Design Principles (Normative)
1. **Concurrency via isolation, not sharing**  
   All parallelism is achieved by running separate seL4 tasks.
2. **Single-threaded authority**  
   All authoritative decisions (tickets, lifecycle, policy, replay) are serialized through a single authority task.
3. **Parallelism at the edges**  
   Parsing, IO, and provider logic may scale horizontally, but must request decisions from the authority task.
4. **Explicit back-pressure**  
   When the authority or a shard is saturated, callers receive deterministic `ERR <verb> reason=busy`, not hidden queuing.

## Task-Level Parallelism Model

### Core Roles (Illustrative)
| Task | Responsibility | Parallelism Strategy |
|----|---------------|----------------------|
| `root-task` | Authority, lifecycle, policy | Single instance, serialized |
| `nine-door` | Secure9P parsing and routing | Sharded per session or subtree |
| `console-transport` | TCP/serial framing, auth | One task per transport |
| Providers (`/log`, `/proc`, `/gpu`, `/host`) | Namespace backends | One task per provider |
| Workers | Role-specific execution | One task per worker |

Each task runs a single-threaded event loop. seL4 schedules tasks across available cores.

## SMP Affinity and Partitioning
### Affinity Guidelines
- Authority task MAY be pinned to a single core for stability.
- IO-heavy tasks MAY be pinned near device IRQ affinity.
- Provider tasks MAY be distributed across remaining cores.

Affinity is optional and platform-specific but must be:
- declarative,
- bounded,
- and documented.

## Authority Interaction Contract
All non-authority tasks:
- Submit requests to the authority task via IPC,
- Receive explicit `OK` / `ERR` responses,
- MUST NOT mutate authoritative state directly.

If the authority task cannot accept work:
- It responds with `ERR <verb> reason=busy`,
- The refusal is audited and observable,
- No retries occur inside the VM.

## Determinism and Replay Guarantees
- Authoritative decisions are totally ordered.
- Parallel tasks must not reorder or speculate on outcomes.
- Replay executes the same authority decisions in the same order, regardless of task scheduling or core count.
- SMP must not introduce nondeterministic ACK/ERR sequences.

## Implementation Touchpoints
- `apps/root-task/`
  - Explicit authority IPC surface
  - Busy/back-pressure signaling
- `apps/nine-door/`
  - Optional sharding of protocol handling
- `apps/root-task/src/net/console_srv.rs` and `apps/root-task/src/serial/`
  - Transport isolation from authority logic
- `docs/ARCHITECTURE.md`
  - SMP model and invariants
- `docs/SECURITY.md`
  - Rationale for rejecting multithreading

## Testing and Validation

### Functional
- All existing regression scripts must pass unchanged.
- New SMP runs must produce byte-identical ACK/ERR sequences to single-core runs.

### Stress
- Saturate protocol handlers while authority remains correct.
- Verify `ERR <verb> reason=busy` emission under load.
- Confirm no state corruption or reordering.

### Replay
- Capture traces on multicore.
- Replay on single-core QEMU and assert identical outcomes.

## Checks (Definition of Done)
- Cohesix runs correctly on multicore aarch64 under QEMU and hardware.
- Parallel tasks execute on multiple cores without shared-memory races.
- Authority logic remains single-threaded and replayable.
- Back-pressure is explicit and observable.
- No new threads, runtimes, or hidden queues introduced.
- Documentation clearly explains the SMP model and its constraints.

## Task Breakdown
```
Title/ID: m25c-smp-kernel-enable
Goal: Enable seL4 SMP in the external kernel build and document requirements.
Inputs: seL4/build, docs/ARCHITECTURE.md, docs/BUILD_PLAN.md.
Changes:
  - seL4/build/ — regenerate kernel artifacts with SMP enabled.
  - docs/ARCHITECTURE.md — record SMP kernel requirements and QEMU CPU count.
Commands:
  - make -C seL4/build
Checks:
  - SMP-enabled kernel boots under QEMU with >1 core.
Deliverables:
  - SMP kernel artifacts and documented build requirements.

Title/ID: m25c-authority-ipc
Goal: Serialize authoritative decisions behind a single IPC surface.
Inputs: apps/root-task, docs/ROLES_AND_SCHEDULING.md.
Changes:
  - apps/root-task/src/authority.rs — authority IPC entrypoint and queueing.
  - apps/root-task/src/lib.rs — route all authority mutations through IPC.
Commands:
  - cargo test -p root-task
Checks:
  - Authority decisions are serialized and replay-stable.
Deliverables:
  - Single-threaded authority IPC with deterministic ordering.

Title/ID: m25c-sharded-tasks
Goal: Run IO, parsing, and providers in separate single-threaded seL4 tasks.
Inputs: apps/nine-door, apps/console, apps/root-task.
Changes:
  - apps/root-task/src/spawn.rs — spawn NineDoor shards and provider tasks.
  - apps/nine-door/src/lib.rs — shard-aware request handling.
Commands:
  - cargo check -p root-task
  - cargo test -p nine-door --test sharding
Checks:
  - Shards execute in parallel without shared-memory coupling.
Deliverables:
  - Task-isolated protocol handling.

Title/ID: m25c-affinity-ir
Goal: Add optional affinity hints to IR and enforce bounds.
Inputs: configs/root_task.toml, tools/coh-rtc, docs/ARCHITECTURE.md.
Changes:
  - tools/coh-rtc/src/ir.rs — affinity hints and validation.
  - configs/root_task.toml — optional affinity policy.
Commands:
  - cargo run -p coh-rtc
  - scripts/check-generated.sh
Checks:
  - Invalid affinity configurations are rejected deterministically.
Deliverables:
  - Manifest-driven affinity policy (optional).

Title/ID: m25c-smp-replay-regressions
Goal: Prove SMP determinism vs single-core runs.
Inputs: docs/TEST_PLAN.md, scripts/cohsh/.
Changes:
  - scripts/cohsh/smp_parity.coh — compare ACK/ERR sequences across core counts.
Commands:
  - cohsh --script scripts/cohsh/smp_parity.coh
Checks:
  - Multicore and single-core transcripts match byte-for-byte.
Deliverables:
  - SMP parity regression coverage.
```

## Milestone 25d — Operator Utilities: Inspect, Trace, Bundle, Diff, Attest <a id="25d"></a>
[Milestones](#Milestones)

**Why now (operator & adoption):**  
By this stage Cohesix is architecturally complete, SMP-aware, and UEFI-capable. What remains is operability: giving operators and integrators deterministic tools to understand, reproduce, compare, and prove system behavior without expanding the VM TCB or introducing new protocols.

This milestone delivers a small, opinionated set of host-side utilities that read existing file-shaped state and artifacts. They do not mutate system state, do not self-heal, and do not bypass policy.

---

## Goal
Provide a coherent operator toolkit that:
1. Explains current system state (`inspect`)
2. Records and replays control-plane behavior (`trace`)
3. Produces self-contained reproducibility artifacts (`bundle`)
4. Compares system state and policy deterministically (`diff`)
5. Verifies device identity and attestation evidence (`attest`)

All tools must be:
- host-side only
- deterministic and scriptable
- aligned with existing Secure9P / NineDoor surfaces
- auditable and replay-compatible

---

## Non-Goals (Explicit)
- No automatic remediation or self-healing
- No in-VM UI or interactive tooling
- No new protocols or transports
- No mutation of authority, policy, or state
- No dependency on POSIX filesystem semantics inside the VM

---

## Deliverables

### 1) `coh inspect` — Correlated System Explanation

**Purpose:**  
Provide a correlated, human-readable explanation of the system’s current operational state.

Reads (examples):

`/proc/lifecycle/*`  
`/proc/root/*`  
`/proc/9p/session/*`  
`/proc/pressure/*`  
`/proc/spool/status`  
`/proc/attest/*`

Output characteristics:
- Structured text (stable field ordering)
- No “healthy/unhealthy” judgment
- Explains why the system is in its current state
- Zero side effects

Exit codes:
- `0` — state internally consistent
- `>0` — invariant violation (corruption, impossible state)

---

### 2) `coh trace` — Deterministic Record & Replay

**Purpose:**  
Capture and replay control-plane behavior for debugging, testing, and UI validation.

Capabilities:
- Record Secure9P frames + ACK/ERR
- Snapshot relevant `/proc/*` state at trace boundaries
- Emit `.trace` artifacts with bounded size
- Replay traces against:
  - `cohsh`
  - SwarmUI Live Hive

Constraints:
- No live mutation during replay
- Byte-identical ACK/ERR ordering required

---

### 3) `coh bundle` — Reproducibility Pack

**Purpose:**  
Produce a single, self-contained artifact for bug reports, audits, and incident review.

Bundle contents (bounded):
- Manifest + resolved manifest hash
- Serial log excerpt (if available from the host capture)
- Trace files (if present)
- `/proc` snapshots (inspect-equivalent)
- Spool status summary
- Attestation summary

Output:
- Deterministic directory or archive layout
- No secrets unless explicitly authorized
- Hash recorded and printed

---

### 4) `coh diff` — Deterministic Comparison

**Purpose:**  
Answer “what changed?” without guesswork.

Supported comparisons:
- Two live targets
- Live target vs bundle
- Two bundles

Diff surfaces:
- Namespace shape
- Manifest-resolved limits
- Policy rules
- Lifecycle / root state
- Attestation fingerprints

Output:
- Minimal, ordered diff
- No semantic inference
- Script-friendly format

---

### 5) `coh attest` — Identity & Evidence Verification

**Purpose:**  
Verify device identity and boot provenance.

Capabilities:
- Parse TPM / DICE evidence from `/proc/attest`
- Verify manifest fingerprint binding
- Validate against provided trust anchors
- Emit clear PASS / FAIL + reason

This command is binary by design and suitable for CI and compliance workflows.

---

## Implementation Scope
- Host tools under `apps/coh/` (or equivalent)
- Reuse existing parsing and transport crates
- No changes to VM-side authority logic
- Minimal, additive code only

---

## Documentation Updates
- `docs/USERLAND_AND_CLI.md`
  - Command reference
  - Output guarantees
- `docs/SECURITY.md`
  - Operator tooling trust model
- `docs/ARCHITECTURE.md`
  - Operator interaction layer (read-only tools)

---

## Testing & Validation
- Golden output fixtures for each command
- Bundle → diff → inspect roundtrip tests
- Trace capture + replay regression
- Attestation positive and negative cases
- Tools must operate correctly against:
  - QEMU single-core
  - QEMU multicore
  - UEFI profile (where applicable)

---

## Checks (Definition of Done)
- All tools produce deterministic output
- No tool mutates system state
- No new protocols introduced
- Trace replay yields byte-identical ACK/ERR
- Bundles are sufficient for offline diagnosis
- Documentation reflects as-built behavior

---

## Outcome
After Milestone 25d:
- Cohesix is operable, not just correct
- Incidents are explainable and reproducible
- Operators can reason about state without guesswork
- Support and integration costs drop sharply
- The control plane remains small, auditable, and boring

## Milestone 26 — Edge Local Status (UEFI Host Tool)  <a id="26"></a> 
[Milestones](#Milestones)

**Why now (compiler):** Field techs need offline status on edge devices using the same 9P grammar. Tool must respect UEFI profile and attestation outputs.

**Goal**
Provide `coh-status` tool (CLI or minimal Tauri) for local read-only inspection of boot/attest data using the existing TCP console transport (or offline trace replay), without adding any in-VM 9P/TCP listener.

**Non-Goals**
- Repo-wide SPDX/NOTICE header sweeps (track separately; not required for the status tool).

**Deliverables**
- `coh-status` binary reading `/proc/boot`, `/proc/attest/*`, `/worker/*/telemetry` via the existing TCP console transport; offline-friendly.
- TPM attestation check displaying manifest fingerprint and verifying against cached reference.
- Shared CBOR parsing code with SwarmUI to preserve grammar.

**Commands**
- `cargo build -p coh-status`
- `cargo run -p coh-status -- --script scripts/cohsh/boot_v0.coh`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/telemetry_ring.coh`

**Checks (DoD)**
- Works offline; wrong/expired ticket → deterministic `ERR reason=Permission` surfaced to user.
- CBOR parsing identical to SwarmUI; transcript diff zero for shared flows.
- Abuse case: attempt to write via coh-status returns ERR and does not mutate state.
- UI/CLI/console equivalence MUST be preserved: ACK/ERR/END sequences must remain byte-stable relative to the 7c baseline.

**Compiler touchpoints**
- `coh-rtc` emits localhost binding guidance and attestation paths for UEFI profile into docs/HARDWARE_BRINGUP.md and docs/USERLAND_AND_CLI.md.

**Task Breakdown**
```
Title/ID: m26-status-tool
Goal: Build coh-status for offline/local status reads over 9P/TCP.
Inputs: apps/coh-status/, UEFI manifest outputs, attestation nodes.
Changes:
  - apps/coh-status/src/main.rs — read-only client using cohsh-core; offline cache for attest data.
  - apps/coh-status/tests/offline.rs — simulate offline read and expired ticket.
Commands:
  - cargo build -p coh-status
  - cargo run -p coh-status -- --script scripts/cohsh/boot_v0.coh
Checks:
  - Expired ticket returns ERR; offline cache used when transport unavailable.
Deliverables:
  - Tool usage documented in docs/HARDWARE_BRINGUP.md and docs/USERLAND_AND_CLI.md.

Title/ID: m26-attest-verify
Goal: Verify TPM attestation parsing parity with SwarmUI.
Inputs: /proc/attest outputs, SwarmUI CBOR parsers.
Changes:
  - apps/coh-status/src/attest.rs — verify manifest fingerprint against cached reference.
  - shared CBOR decoder module reused from SwarmUI/cohsh-core.
Commands:
  - cargo test -p coh-status --test attest
  - cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/telemetry_ring.coh
Checks:
  - Malformed attestation rejected with ERR; valid attestation matches manifest hash identically to SwarmUI.
Deliverables:
  - Verified attestation workflow documented; regression outputs stored.

```

## Milestone 27 — AWS AMI (UEFI → Cohesix, ENA, Diskless 9door)  <a id="27"></a> 
[Milestones](#Milestones)

**Why now (platform):**  
Cohesix is ready to operate as the operating system. To make EC2 a first-class, production target without Linux, agents, or filesystems, Cohesix must boot directly from UEFI and bring up Nitro networking natively. ENA is mandatory on AWS. This milestone establishes a diskless, stateless AMI whose only persistent artifact is the read-only ESP image (UEFI loader + kernel + rootserver + manifest).

**Goal**  
Boot Cohesix on AWS EC2 (Arm64) via **UEFI → elfloader.efi → seL4 → root-task**, then bring up ENA networking in root-task and mount the Cohesix 9door namespace over the network with **no local filesystem**, **no Linux**, and **no virtio**. The root-task acts as a 9P client over the existing TCP stack; no new in-VM listeners are introduced beyond the console.

**Deliverables**
- EFI System Partition containing:
  - `EFI/BOOT/BOOTAA64.EFI` (elfloader EFI)
  - `kernel.elf`
  - `rootserver` (root task ELF)
  - optional `initrd.cpio`
  - `manifest.json` and `manifest.sha256`
  - embedded, signed fabric bootstrap manifest (≥2 endpoints, root trust anchors)
- ENA driver (adminq + single TX/RX queue) in root-task.
- Minimal DHCP/TCP/TLS client in root-task (post-seL4), no firmware networking.
- Diskless bootstrap path **after seL4**: ENA → DHCP → TCP → TLS → 9door mount.
- Optional IMDSv2 bootstrap (instance identity + config) using a bounded, allowlisted HTTP client over the existing TCP stack. No listeners; no background refresh loop.
- AMI registration tooling for Arm64 (`uefi` / `uefi-preferred`).
- Documentation in `docs/AWS_AMI.md` covering boot path, failure modes, and recovery.

**Commands**
- `cmake --build seL4/build --target elfloader.efi`
- `scripts/uefi/esp-build.sh --manifest out/manifests/root_task_resolved.json`
- `scripts/aws/build-esp.sh`
- `scripts/aws/register-ami.sh`
- `scripts/aws/launch-smoke.sh`

**Checks (DoD)**
- EC2 instance boots directly into Cohesix with no intermediate OS.
- ENA link comes up deterministically; DHCP lease acquired within bounded time.
- 9door namespace mounts successfully and control plane is reachable.
- IMDSv2 metadata fetch is optional and bounded; if unavailable or denied, boot continues safely with explicit diagnostics and no unbounded retries.
- Power cycle returns to identical clean state (no persistence).
- Failure cases (no fabric, auth failure, link down) halt safely with explicit console diagnostics.

**Compiler touchpoints**
- `coh-rtc` emits:
  - ENA queue bounds and bootstrap retry limits.
  - Fabric bootstrap manifest schema and signature requirements.
  - IMDSv2 allowlist, max response bytes, and retry bounds (optional gate).
- Regeneration guard verifies EFI binary hash against recorded compiler output.

**Task Breakdown**
```
Title/ID: m27-uefi-esp
Goal: Build an EFI System Partition for AWS Arm64 using elfloader + seL4 artifacts.
Inputs: upstream elfloader EFI build, `scripts/uefi/esp-build.sh`, manifest outputs.
Changes:
- `scripts/uefi/esp-build.sh` — build ESP with BOOTAA64.EFI, kernel, rootserver, optional initrd, manifest + hash.
- `scripts/aws/build-esp.sh` — produce AMI-ready ESP image.
Commands:
- cmake --build seL4/build --target elfloader.efi
- scripts/uefi/esp-build.sh --manifest out/manifests/root_task_resolved.json
Checks:
- ESP boots to root-task via elfloader with deterministic serial output.
Deliverables:
- Documented ESP layout and build recipe for Arm64.

Title/ID: m27-ena-adminq
Goal: Implement ENA PCIe discovery and admin queue in root-task.
Inputs: apps/root-task drivers, HAL PCI helpers, docs/AWS_AMI.md.
Changes:
- apps/root-task/src/drivers/ena/pci.rs — PCIe enumeration, BAR mapping.
- apps/root-task/src/drivers/ena/adminq.rs — admin queue + completion queue.
- apps/root-task/src/net/ena.rs — ENA init wiring.
Commands:
- cargo test -p root-task --test ena_adminq
Checks:
- Feature negotiation succeeds with minimal feature set.
Deliverables:
- AdminQ protocol notes in docs/AWS_AMI.md.

Title/ID: m27-ena-io
Goal: Bring up minimal ENA dataplane.
Inputs: apps/root-task drivers, root-task net stack abstractions.
Changes:
- apps/root-task/src/drivers/ena/ioq.rs — single TX/RX SQ + CQ.
- apps/root-task/src/drivers/ena/poll.rs — polling dataplane (no interrupts).
- apps/root-task/src/net/mod.rs — integrate ENA dataplane into the runtime.
Commands:
- cargo test -p root-task --test ena_ioq
Checks:
- TX reclaim and RX refill invariants hold under sustained traffic.
Deliverables:
- Deterministic dataplane invariants documented.

Title/ID: m27-net-bootstrap
Goal: Network bootstrap to fabric (post-seL4, in root-task).
Inputs: apps/root-task net stack, TLS helpers, docs/AWS_AMI.md.
Changes:
- apps/root-task/src/net/dhcp.rs — bounded DHCP client.
- apps/root-task/src/net/tcp.rs — TCP bring-up for long-lived sessions.
- apps/root-task/src/net/tls.rs — fabric-auth TLS handshake.
- apps/root-task/src/net/bootstrap.rs — deterministic sequencing and retries.
Commands:
- cargo test -p root-task --test net_bootstrap
Checks:
- Network reaches "fabric-ready" state within defined bounds.
Deliverables:
- Bootstrap timing guarantees recorded.

Title/ID: m27-imdsv2-bootstrap
Goal: Read bounded instance metadata (IMDSv2) and feed boot policy inputs.
Inputs: apps/root-task net stack, docs/AWS_AMI.md.
Changes:
- apps/root-task/src/net/http.rs — minimal HTTP request/response parsing (bounded, no chunked).
- apps/root-task/src/net/imdsv2.rs — token fetch + allowlisted metadata queries.
- apps/root-task/src/boot/policy.rs — consume optional IMDS fields (instance-id, region, az, tags if enabled).
Commands:
- cargo test -p root-task --test imdsv2
Checks:
- IMDSv2 is optional: absence, timeout, or denial does not block boot and emits deterministic diagnostics.
Deliverables:
- IMDSv2 bootstrap flow documented with explicit bounds and allowlist.

Title/ID: m27-fabric-mount
Goal: Mount 9door namespace and enter steady state (post-seL4).
Inputs: root-task net stack, Secure9P client, docs/AWS_AMI.md.
Changes:
- apps/root-task/src/net/door9p_client.rs — minimal 9P client for fabric mounts.
- apps/root-task/src/net/bootstrap.rs — signed manifest verification.
- apps/root-task/src/net/mount.rs — mount orchestration and error handling.
Commands:
- cargo test -p root-task --test fabric_mount
Checks:
- Namespace mount is read/write correct; auth failures are terminal and explicit.
Deliverables:
- Fabric bootstrap flow documented.

Title/ID: m27-ami-pipeline
Goal: Produce and validate AWS AMI.
Inputs: scripts/aws/, docs/AWS_AMI.md.
Changes:
- scripts/aws/build-esp.sh — ESP image creation.
- scripts/aws/register-ami.sh — snapshot + AMI registration.
- scripts/aws/launch-smoke.sh — EC2 smoke test.
Commands:
- scripts/aws/register-ami.sh
Checks:
- AMI launches on supported Nitro instance family and passes smoke test.
Deliverables:
- Reproducible AMI build pipeline.
```
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

5. **Regression Pack (post–Milestone 7c)**
   - From Milestone 8 onward, any change that lands **MUST** re-run the shared regression pack from earlier milestones, not just new tests.
   - Note: `.coh` scripts live in `scripts/cohsh/` and follow `docs/USERLAND_AND_CLI.md`.
   - The regression pack includes at minimum:
     - `tests/integration/qemu_tcp_console.rs` (Milestone 7 TCP console flow).
     - `scripts/cohsh/boot_v0.coh` (baseline help/attach/log/quit script from the manifest compiler).
     - `tests/cli/tracefs_script.sh` (TraceFS JSONL flows).
     - `scripts/cohsh/9p_batch.coh` (Secure9P batching).
     - `scripts/cohsh/telemetry_ring.coh` (telemetry rings & cursor resumption).
     - `scripts/cohsh/observe_watch.coh` (observability `/proc` grammar).
     - `scripts/cohsh/cas_roundtrip.coh` (CAS update round-trip).
   - CI for each Milestone ≥ 8 must:
     - Run the full regression pack unchanged and fail on any output drift (including ACK/ERR/END lines, `/proc` grammars, and telemetry formats).
     - Only permit intentional behaviour changes when the relevant CLI scripts, doc snippets, and manifest fields are updated **in the same change**.
   - The regression pack is treated as the canonical “no-regression” harness; new tests are **additive**, not substitutes.

6. **Cross-Milestone Stability Rules**
   - Changes to console ACK/ERR/END grammar, NineDoor error codes, or `/proc` node formats MUST be treated as breaking changes and require:
     - (a) matching updates to all CLI fixtures under `scripts/cohsh/*`,
     - (b) regeneration of manifest-derived snippets,
     - (c) explicit doc updates in `INTERFACES.md`, and
     - (d) a version bump of the manifest schema.
   - Milestones ≥ 9 MUST NOT introduce new 9P verbs or extend grammars unless routed through the manifest compiler and validated by IR red lines.
   - Networking cadence and event-pump tick pacing MUST NOT shift across milestones unless the change is documented in `SECURITY.md` with updated bounds.
