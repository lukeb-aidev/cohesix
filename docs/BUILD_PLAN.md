<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Track Cohesix milestones, deliverables, and completion criteria for ARM64 Pure Rust userspace builds. -->
<!-- Author: Lukas Bower -->
# Cohesix Build Plan (ARM64, Pure Rust Userspace)
Cohesix targets physical ARM64 hardware with an official Raspberry Pi 4 bring-up path aligned to upstream seL4 guidance (`U-Boot + binary image`). QEMU `aarch64/virt` remains the reference setup for bring-up, CI, and deterministic regression testing.

**Host:** macOS 26 on Apple Silicon (M4)
**Target:** QEMU aarch64 `virt` (GICv3)
**Kernel:** Upstream seL4 (external build)
**Userspace:** Pure Rust crates (`root-task`, `nine-door`, `worker-heart`, future `worker-gpu`, `gpu-bridge-host` host tool)

Physical ARM64 hardware remains the deployment target; for Raspberry Pi 4, Milestone 26 onward follows the upstream seL4 U-Boot handoff model while preserving QEMU `aarch64/virt` semantics as the reference development and CI profile.

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
| [20d](#20d) | SwarmUI Live Hive Rendering (PixiJS, GPU-First) | Complete |
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
| [24](#24) | Python Client + Examples (cohesix) + Doctor + Release Cut | Complete |
| [24b](#24b) | Live GPU Bridge Wiring + PEFT Live Flow + Live Hive Telemetry Text | Complete |
| [24b1](#24b1) | Live Hive UX Patch: Performance, Labels, Clickability, Telemetry Harness | Complete |
| [24c](#24c) | Authoritative Scheduling Grammar + REST Gateway + Scheduler/Lease Observability | Complete |
| [24d](#24d) | Jetson CUDA Host Support (NVML Fallback + Doctor) | Complete |
| [24e](#24e) | REST Multiplexer Transports + SwarmUI Gateway Mode | Complete |
| [25](#25) | SMP Utilization via Task Isolation (Multicore without Multithreading) | Complete |
| [25a](#25a) | REST Live Hive Performance (Parallel Polling + Batching) | Complete |
| [25b](#25b) | Secure Scale Gateway (1k Worker Readiness + Due Diligence Closure) | Complete |
| [25c](#25c) | Python Orchestration SDK (1k Fleet Playbooks + Host Integrations) | Complete |
| [25d](#25d) | REST Request-Auth Parity Across Host Tools (Gateway Capability Max) | Complete |
| [25e](#25e) | Evidence Packs + Integration Kits (Audit-First Adoption) | Complete |
| [25f](#25f) | Gateway Broker Refactor + Large Telemetry Reference Manifests (No-Retry Reliability Gate) | Complete |
| [25g](#25g) | Host Control Tickets via FUSE (GPU/PEFT + systemd/docker + K8s Coexistence) | Complete |
| [25h](#25h) | Multi-Hive Federation via Ticket Relay (Single-Writer Preserved, 10x1k Fleet Pattern) | Complete |
| [26](#26) | Official Pi 4 Bring-up (U-Boot + Binary Image) | In Progress |
| [26a](#26a) | Pi 4 Networking Baseline (GENETv5 + Static IPv4, U-Boot Configurable) | Complete |
| [26b](#26b) | Pi 4 DHCP Baseline (NIC + Wi-Fi Policy, U-Boot Configurable) | Pending |
| [26c](#26c) | Humanized Surface Cleanup + Markdown Audit (Zero-Regression) | Pending |
| [27](#27) | Pi 4 On-Device Spool Stores + Settings Persistence | Pending |
| [28](#28) | Operator Utilities: Inspect, Trace, Bundle, Diff, Attest | Pending |
| [28b](#28b) | Authority Hardening: Delegated REST Identity, Fenced Failover, Idempotent Queen Intents | Pending |
| [29](#29) | Edge Local Status (Pi 4 Host Tool) | Pending |
| [30](#30) | AWS AMI (UEFI → Cohesix, ENA, Diskless 9door) | Pending |

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
  - Manifest/IR alignment: `/host` tree appears only when `ecosystem.host.enable = true` with providers declared under `ecosystem.host.providers[]` and mount point defaulting to `/host`.
  - Docs include policy and TCB notes emphasising that the bridge mirrors host controls without expanding the in-VM attack surface.

**Status:** Complete — host-sidecar bridge and manifest-gated `/host` namespace are verified via host tool tests, `host_sidecar_policy`, and regression scripts covering `/host` role enforcement.

**Checks (DoD)**
- `/host/*` tree mounts only when enabled by the manifest; omitted otherwise.
- Writes to control nodes are rejected for non-queen roles and result in append-only audit lines; mock mode exercises this path in CI.
- No new in-VM TCP services are introduced; all transports remain host-side per Secure9P.
- Abuse case: denied write to `/host/systemd/*/restart` returns deterministic ERR and logged audit; disablement removes namespace entirely and unit/integration tests assert absence.

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
- `scripts/cohsh/policy_gate.coh` is part of the regression pack and must remain deterministic.
- Manifest flag (e.g., `ecosystem.policy.enable`) toggles the gate and publishes rules; the current default manifest enables the gate and disabling it reverts to prior control semantics.

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
- `apps/cohsh/src/lib.rs` extends `Shell` with a session pool (default manifest value: two control, twenty-four telemetry) and batched Twrite helper. `apps/cohsh/src/transport/tcp.rs` gains retry scheduling based on manifest policy.
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
**Release 0.1.0 alpha**
----

Next, Alpha Release 2 targets a plug-and-play operator experience immediately after Milestone 20.x. Milestones 21-24 define the Alpha track; Pi 4 bare-metal (`U-Boot + binary image`) and AWS AMI work follows starting at Milestone 26.

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

### Telemetry Spool Policy (Addendum to Milestones 21a & 27)

**Rationale:** Telemetry storage must be predictable under pressure. Operators must know *when*, *why*, and *how* data is retained or dropped. This addendum **aligns policy terminology** between 21a telemetry ingest quotas and the 27 persistent spool store; it does **not** retroactively change 21a's completed behavior.

#### Policy surface (alignment)
- **Telemetry ingest (21a):** keep `telemetry_ingest.eviction_policy` (`refuse` | `evict-oldest`) as the source of truth for per-device segment limits.
- **Persistent spool (27):** use `persistence.spool.mode` (`refuse` | `overwrite_acked`) and `persistence.spool.max_record_bytes` to mirror 21a's refusal/eviction semantics while remaining crash-safe.

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

#### CLI surface (host-only, target milestone 27 or later)
- `cohsh telemetry status` — read spool/ingest status and render policy + pressure.
- `cohsh telemetry explain` — summarize current policy and refusal/eviction outcomes.

#### Mandatory regression cases
- Quota exhaustion: `refuse` vs `evict-oldest` (21a) and `overwrite_acked` (27).
- Ack cursor behind overwrite window (27).
- Record-too-large rejection (27).
- Offline accumulation → online drain (21a + 27).

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

**Status:** Complete — cohesix Python client + examples, coh doctor, and alpha2 release bundles validated; regression pack and full test plan (source + macOS/Ubuntu bundles) completed.

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

Title/ID: m24-swarmui-header-unify
Goal: Unify SwarmUI header alignment and scale so the banner reads as a single product surface.
Inputs: apps/swarmui/frontend/index.html, apps/swarmui/frontend/styles/layout.css.
Changes:
  - apps/swarmui/frontend/styles/layout.css — align banner grid, status rows, and header badge/chip sizing.
Commands:
  - npm run lint (if configured) or cargo check -p swarmui
Checks:
  - Header elements align cleanly at desktop and stack without overflow at 980px/720px breakpoints.
Deliverables:
  - Updated SwarmUI header styling.
```

## Milestone 24b — Live GPU Bridge Wiring + PEFT Live Flow + Live Hive Telemetry Text <a id="24b"></a> 
[Milestones](#Milestones)

**Status:** Complete — live `/gpu/models` publish verified, non-mock PEFT flow unblocked, Live Hive telemetry text overlays validated.

**Why now (adoption):** Operators can’t complete a non-mock PEFT flow because the live VM still does not expose `/gpu/models/*`. The Queen currently returns `ERR LS reason=policy detail=invalid-path` on `/gpu/models`, so `coh peft import --host ...` has nowhere to publish the registry. This milestone wires the host GPU bridge into the live namespace and adds bounded telemetry text in Live Hive without introducing new protocols.

**Goal**
Enable a non-mock PEFT flow by publishing live `/gpu/models` and `/gpu/telemetry` into the VM, extend Live Hive with bounded telemetry text overlays + details panel, and enforce per-path polling defaults for cohsh/SwarmUI.

**Deliverables**
- Live GPU bridge publish path that installs `/gpu/<id>`, `/gpu/models/*`, and `/gpu/telemetry/schema.json` inside the live VM using existing Secure9P file semantics (no new RPC services).
- `gpu-bridge-host` gains live publish mode (`--publish`, `--interval-ms`, TCP config flags) that pushes bounded `GpuNamespaceSnapshot` payloads to the Queen.
- `coh peft import` supports a live refresh step (`--publish`/`--refresh-gpu-models`) so the host registry is mirrored into `/gpu/models/available/*` and `/gpu/models/active`.
- Live host telemetry adapters for `systemd`, `k8s`, `docker`, and `nvidia` (NVML) publish read-only snapshots under `/host/*` (no new control semantics).
- Live Hive renders per-worker text overlays (last N lines) plus a selectable details panel; truncation/polling logic lives in `cohsh-core` only.
- Real-world telemetry defaults (manifest-backed):
  - Tail poll default: 1000 ms; min 250 ms; max 10_000 ms.
  - NVML status poll: 1000 ms; systemd: 2000 ms; docker: 2000 ms; k8s: 5000 ms.
  - Live Hive overlay: N=3 lines, details panel M=50 lines, line cap 160 bytes, per-worker text budget 2 KiB.
  - PEFT/LoRA telemetry window: 1s (`time_window` label `ms:<start>-<end>`); `lora_id` required for LoRA records.
- Documentation updates (as-built): `docs/TEST_PLAN.md`, `docs/HOST_TOOLS.md`, `docs/GPU_NODES.md`, `docs/INTERFACES.md`, `docs/USERLAND_AND_CLI.md`, `docs/ARCHITECTURE.md`, and `README.md` where behavior is surfaced.

**Commands**
- `cargo run -p coh-rtc`
- `scripts/check-generated.sh`
- `cargo test -p gpu-bridge-host`
- `cargo test -p host-sidecar-bridge`
- `cargo test -p cohsh-core`
- `cargo test -p swarmui --test console_parity`
- `scripts/cohsh/run_regression_batch.sh`
- `scripts/release_bundle.sh --name Cohesix-0.3.0-alpha2 --version 0.3.0-alpha2 --force`

**Checks (DoD)**
- Live Queen returns a non-error for `ls /gpu/models` after the GPU bridge publish step; `/gpu/telemetry/schema.json` is readable.
- Non-mock PEFT flow succeeds end-to-end:
  - `coh peft import --host ... --publish` installs `manifest.toml` under `/gpu/models/available/<model_id>/`.
  - `coh peft activate --host ...` updates `/gpu/models/active`.
  - `coh peft export --host ...` reads `/queen/export/lora_jobs/<job_id>/telemetry.cbor` within configured bounds.
- Live Hive text overlays and details panel render bounded telemetry lines without UI-owned polling logic.
- Per-path polling respects min/max bounds; defaults match the values above.
- `docs/TEST_PLAN.md` includes a non-mock PEFT flow section and live telemetry checks; new steps pass on macOS 26 and Ubuntu 24 bundles.
- `docs/HOST_TOOLS.md` and `docs/GPU_NODES.md` document the live publish flow and required commands.
- Regression pack runs unchanged; output drift fails.
- After Test Plan gates pass, release bundle minor version increments (e.g., `Cohesix-0.2.0-alpha2` → `Cohesix-0.3.0-alpha2`), with directory and tarball names updated.

**Compiler touchpoints**
- `coh-rtc` emits telemetry polling defaults and Live Hive text budgets into manifest-backed snippets.
- Any new `/gpu/bridge/*` nodes and provider ownership are represented in IR and docs snippets.

**Task Breakdown**
```
Title/ID: m24b-gpu-bridge-publish
Goal: Enable live GPU bridge publishing of `/gpu/models` and telemetry schema into a running Queen.
Inputs: apps/gpu-bridge-host, apps/nine-door, docs/GPU_NODES.md.
Changes:
  - apps/gpu-bridge-host/src/main.rs — add `--publish`, `--interval-ms`, TCP config flags; serialize bounded snapshots.
  - apps/nine-door/src/host/core.rs — accept snapshot payloads and update `/gpu` namespace deterministically.
  - docs/GPU_NODES.md — document live publish flow and error semantics.
Commands:
  - cargo test -p gpu-bridge-host
  - cargo test -p nine-door --test integration
Checks:
  - Live `ls /gpu/models` succeeds after publish; invalid payloads return deterministic ERR.
Deliverables:
  - Live GPU bridge publish path + updated GPU docs.

Title/ID: m24b-peft-live-flow
Goal: Unblock non-mock PEFT import/activate by refreshing `/gpu/models` after registry updates.
Inputs: apps/coh/, docs/USERLAND_AND_CLI.md, docs/HOST_TOOLS.md.
Changes:
  - apps/coh/src/main.rs — add `--publish`/`--refresh-gpu-models` option for `peft import`.
  - docs/USERLAND_AND_CLI.md — document live PEFT flow and required commands.
  - docs/HOST_TOOLS.md — update PEFT examples to include live publish.
Commands:
  - cargo test -p coh --features nvml
Checks:
  - `coh peft import --host ... --publish` yields `/gpu/models/available/<model_id>/manifest.toml` in the VM.
Deliverables:
  - Live PEFT flow documented and validated.

Title/ID: m24b-host-telemetry-live
Goal: Provide live host telemetry providers for systemd/k8s/docker/NVML.
Inputs: apps/host-sidecar-bridge, docs/INTERFACES.md, docs/ARCHITECTURE.md.
Changes:
  - apps/host-sidecar-bridge/ — add live adapters and bounded snapshot formatting.
  - docs/INTERFACES.md — document `/host/*/status` line formats.
  - docs/ARCHITECTURE.md — record telemetry polling defaults.
Commands:
  - cargo test -p host-sidecar-bridge
Checks:
  - Live publish works over TCP; mock path remains deterministic.
Deliverables:
  - Live host telemetry adapters + updated docs.

Title/ID: m24b-live-hive-text
Goal: Render bounded telemetry text overlays and a details panel in Live Hive using cohsh-core tails.
Inputs: crates/cohsh-core, apps/swarmui, docs/INTERFACES.md.
Changes:
  - crates/cohsh-core/ — add tail polling policy and line budget enforcement.
  - apps/swarmui/ — render overlays + details panel; no UI polling logic.
  - docs/INTERFACES.md — Live Hive text overlay rules.
Commands:
  - cargo test -p cohsh-core
  - cargo test -p swarmui --test console_parity
Checks:
  - Overlay shows last N lines; details panel shows last M; line truncation enforced.
Deliverables:
  - Live Hive text overlay + deterministic bounds.

Title/ID: m24b-test-plan-release
Goal: Update Test Plan and cut a new release bundle after gates pass.
Inputs: docs/TEST_PLAN.md, scripts/release_bundle.sh, releases/.
Changes:
  - docs/TEST_PLAN.md — add non-mock PEFT flow + live telemetry checks.
  - releases/ — bump minor version bundle and tarball names.
Commands:
  - scripts/cohsh/run_regression_batch.sh
  - scripts/release_bundle.sh --name Cohesix-0.3.0-alpha2 --version 0.3.0-alpha2 --force
Checks:
  - Test Plan additions pass on macOS 26 and Ubuntu 24 bundles.
Deliverables:
  - Updated Test Plan and new release bundle.
```

----
**Release 0.2.0 alpha**
----

## Milestone 24b1 — Live Hive UX Patch: Performance, Labels, Clickability, Telemetry Harness <a id="24b1"></a>
[Milestones](#Milestones)

**Status:** Complete — Live Hive responsiveness, labeling, clickability, and telemetry harness are in place and verified.

**Why now (adoption):** Operators cannot trust Live Hive when it is slow and ambiguous. The UI must remain responsive, worker dots must be identifiable at a glance, and clicking a worker must deterministically reveal telemetry. This patch is a focused UX + telemetry correctness fix, not a new protocol or capability change.

**Goal**
Diagnose and fix Live Hive slowness, add worker labels and type color-coding, guarantee clickability with tests, and add a deterministic performance + telemetry harness.

**Deliverables**
- Root-cause analysis of Live Hive slowness (profiling notes + findings captured in docs).
- Worker dots show a short numeric label (stable per worker id) in the Hive view.
- Worker dots are color-coded by role/type (worker-heartbeat, worker-gpu, worker-lora, worker-bus).
- Click selection is reliable and verified by an automated UI test.
- A performance + telemetry harness that validates:
  - UI remains responsive under N workers and M telemetry lines.
  - Telemetry lines appear when a worker is selected.
- Documentation updates: `docs/TEST_PLAN.md` and `docs/INTERFACES.md` updated with the new UI and test expectations.

**Commands**
- `cargo test -p swarmui --test console_parity`
- `cd tools/swarmui-ui-tests && npm test`
- `scripts/cohsh/run_regression_batch.sh`

**Checks (DoD)**
- Live Hive remains responsive at N=8 workers and 2 KiB per-worker telemetry text budget.
- Each worker dot renders a numeric label and role-specific color.
- Clicking a worker reliably selects it and reveals telemetry in the details panel.
- Playwright test asserts that clicking a worker changes the selected worker and loads telemetry text.
- Performance harness reports within thresholds: 60 FPS target with < 16 ms avg frame time (or explicit measured threshold captured in test output).
- No new protocols or console grammar changes.

**Task Breakdown**
```
Title/ID: m24b1-hive-perf-diagnose
Goal: Identify and fix the root cause of Live Hive slowness without changing protocols.
Inputs: apps/swarmui, crates/cohsh-core, docs/INTERFACES.md.
Changes:
  - apps/swarmui/ — profiling instrumentation + performance fixes (render loop, data diffs, throttling).
  - docs/INTERFACES.md — record any clarified UI constraints or telemetry rendering rules.
Commands:
  - cargo test -p swarmui --test console_parity
Checks:
  - UI remains responsive with 8 workers and active telemetry tails.
Deliverables:
  - Performance fix + recorded diagnosis summary.

Title/ID: m24b1-hive-labels-colors
Goal: Add worker labels and role-based color-coding to Live Hive.
Inputs: apps/swarmui, docs/INTERFACES.md.
Changes:
  - apps/swarmui/ — render numeric labels near worker dots; add role palette mapping.
  - docs/INTERFACES.md — document label format + role color scheme.
Commands:
  - cargo test -p swarmui --test console_parity
Checks:
  - Each worker dot has a stable numeric label; colors reflect worker role.
Deliverables:
  - Labeled and color-coded Live Hive nodes.

Title/ID: m24b1-hive-clickability
Goal: Guarantee worker dots are clickable and selection is deterministic.
Inputs: tools/swarmui-ui-tests, apps/swarmui.
Changes:
  - tools/swarmui-ui-tests/ — add Playwright test for worker selection.
  - apps/swarmui/ — ensure click targets map to selection state.
Commands:
  - cd tools/swarmui-ui-tests
  - npm test
Checks:
  - Playwright validates click selection updates the details panel.
Deliverables:
  - UI clickability test and stable selection behavior.

Title/ID: m24b1-hive-perf-telemetry-harness
Goal: Add a deterministic harness that checks performance and telemetry visibility.
Inputs: tools/swarmui-ui-tests, docs/TEST_PLAN.md.
Changes:
  - tools/swarmui-ui-tests/ — add perf + telemetry fixtures + thresholds.
  - docs/TEST_PLAN.md — add the harness run steps and expected thresholds.
Commands:
  - cd tools/swarmui-ui-tests
  - npm test
Checks:
  - Telemetry lines appear after selection; perf thresholds met.
Deliverables:
  - Performance + telemetry harness and Test Plan update.
```

----
**Release 0.3.0 alpha**
----

## Milestone 24c — Authoritative Scheduling Grammar + REST Gateway + Scheduler/Lease Observability <a id="24c"></a>
[Milestones](#Milestones)

**Status:** Complete.

**Why now (adoption):** Operators want frictionless API access without weakening Cohesix’s authority model. This milestone adds VM‑authoritative scheduling/lease/policy/export control grammar, a host‑only REST gateway that is a strict projection of file/console semantics, and read‑only `/proc` observability surfaced in Live Hive.

**Goal**
Add additive, non‑breaking control grammar for:
- Lease renewal/preemption + quotas
- Declarative VM scheduling queue
- Policy apply/rollback enforcement
- Export scheduling / data‑diode controls

Expose the new control surfaces through a host‑only REST gateway (OpenAPI 3.1), add Python REST backend parity, render read‑only scheduler + lease panels in SwarmUI using new `/proc` nodes, and support running the REST gateway as a systemd service on Linux with automatic reconnect to the Cohesix TCP console.

**Deliverables**
- New append‑only control files (strict JSONL schemas, bounded):
  - `/queen/lease/ctl` — lease quotas/renew/preempt
  - `/queen/schedule/ctl` — scheduling queue
  - `/queen/export/ctl` — export windows
  - `/policy/ctl` — apply/rollback schema
- Manifest‑gated bounds for new control files and `/proc` nodes (IR‑driven via `coh-rtc`).
- New `/proc` nodes (read‑only, bounded):
  - `/proc/schedule/summary` + `/proc/schedule/queue`
  - `/proc/lease/summary` + `/proc/lease/active` + `/proc/lease/preemptions`
- Host‑only REST gateway + OpenAPI 3.1 spec + Swagger UI.
- Linux systemd service unit for the REST gateway with restart/reconnect behavior.
- Python REST backend with parity tests.
- SwarmUI read‑only panels: Scheduler Queue and Lease/Preemption Timeline.
- Docs updates: `docs/INTERFACES.md`, `docs/ARCHITECTURE.md`, `docs/USERLAND_AND_CLI.md`,
  `docs/HOST_TOOLS.md`, `docs/QUICKSTART.md`, `docs/PYTHON_SUPPORT.md`, `docs/TEST_PLAN.md`.

**Commands**
- `cargo run -p coh-rtc`
- `scripts/check-generated.sh`
- `cargo test -p nine-door --test schedule_create`
- `cargo test -p nine-door --test schedule_bounds`
- `cargo test -p nine-door --test lease_bounds`
- `cargo test -p nine-door --test policy_ctl`
- `cargo test -p nine-door --test export_ctl`
- `cargo test -p cohsh-core`
- `cargo test -p cohsh --test transcripts`
- `cargo test -p swarmui --test console_parity`
- `cd tools/swarmui-ui-tests && npm test`
- `cargo test -p hive-gateway`
- `python -m pytest -k cohesix_parity`
- `scripts/cohsh/run_regression_batch.sh`

**Checks (DoD)**
- New control files accept valid JSONL and reject invalid fields with deterministic `ERR`.
- New `/proc` nodes respect manifest bounds and render deterministic, line‑oriented output.
- REST gateway returns OK/ERR/END‑equivalent responses and enforces manifest bounds.
- Python REST backend matches cohsh semantics in parity tests.
- SwarmUI panels render queue + lease state from `/proc` without adding any control verbs.
- REST gateway can be run under systemd on Linux, and auto‑reconnects to the Cohesix console after a QEMU restart.
- `docs/TEST_PLAN.md` is updated and its new steps are completed as part of DoD for this milestone.
- Regression pack passes with updated fixtures; no ACK/ERR/END drift outside new fixtures.

**Task Breakdown**
```
Title/ID: m24c-grammar-manifest
Goal: Add manifest gates + bounds for new control files and `/proc` nodes.
Inputs: configs/root_task.toml, tools/coh-rtc, docs/INTERFACES.md.
Changes:
  - configs/root_task.toml — add scheduler/lease/export/policy gates + size bounds.
  - tools/coh-rtc — emit generated bounds for new control and `/proc` nodes.
Commands:
  - cargo run -p coh-rtc
  - scripts/check-generated.sh
Checks:
  - Generated outputs hash-match; bounds appear in snippets.
Deliverables:
  - IR-driven gates and bounds for new control files and `/proc` nodes.

Title/ID: m24c-grammar-runtime
Goal: Implement VM/NineDoor handling of new control files with strict JSONL validation + audit lines.
Inputs: apps/root-task/src/ninedoor.rs, apps/nine-door, docs/INTERFACES.md.
Changes:
  - apps/root-task/src/ninedoor.rs — handlers for `/queen/lease/ctl`, `/queen/schedule/ctl`,
    `/queen/export/ctl`, `/policy/ctl`.
  - apps/nine-door/ — host-mode providers for the same paths.
  - docs/INTERFACES.md — document schemas + error semantics.
  - docs/ARCHITECTURE.md — update control surfaces/data flows.
Commands:
  - cargo test -p nine-door --test schedule_create
  - cargo test -p nine-door --test schedule_bounds
  - cargo test -p nine-door --test lease_bounds
  - cargo test -p nine-door --test policy_ctl
  - cargo test -p nine-door --test export_ctl
Checks:
  - Valid lines accepted; invalid lines rejected deterministically; audit lines emitted.
Deliverables:
  - Authoritative scheduling/lease/policy/export grammar in VM and host NineDoor.

Title/ID: m24c-proc-observability
Goal: Add read-only `/proc` nodes for schedule + lease observability (bounded).
Inputs: apps/root-task, docs/INTERFACES.md.
Changes:
  - apps/root-task — `/proc/schedule/*` and `/proc/lease/*` providers.
  - docs/INTERFACES.md — new `/proc` node formats.
Commands:
  - cargo test -p nine-door --test schedule_bounds
  - cargo test -p nine-door --test lease_bounds
Checks:
  - `/proc` nodes respect manifest byte/line limits and stable formatting.
Deliverables:
  - Read-only schedule/lease observability nodes.

Title/ID: m24c-swarmui-scheduler-lease-panels
Goal: Add read-only Live Hive panels for scheduler queue and lease/preemption timeline.
Inputs: apps/swarmui, tools/swarmui-ui-tests, docs/INTERFACES.md.
Changes:
  - apps/swarmui/ — panels that read `/proc/schedule/*` and `/proc/lease/*`.
  - tools/swarmui-ui-tests/ — replay fixtures + Playwright checks for panel wiring.
  - docs/INTERFACES.md — UI expectations (read-only).
Commands:
  - cargo test -p swarmui --test console_parity
  - cd tools/swarmui-ui-tests
  - npm test
Checks:
  - Panels render bounded content and update on replay fixtures.
Deliverables:
  - Read-only SwarmUI panels for schedule and lease state.

Title/ID: m24c-host-rest-gateway
Goal: Provide host-only REST gateway mapping 1:1 to file/console semantics with systemd service support and reconnect behavior.
Inputs: crates/cohsh-core, docs/HOST_TOOLS.md.
Changes:
  - apps/hive-gateway/ — new host tool (REST server + OpenAPI 3.1 + Swagger UI).
  - docs/HOST_TOOLS.md — add gateway usage + auth/ticket guidance.
  - docs/HOST_API.md — OpenAPI spec + examples.
  - systemd unit file (Linux) — run gateway as a service with restart policy and env overrides.
Commands:
  - cargo test -p hive-gateway
Checks:
  - REST responses mirror OK/ERR/END; no new semantics.
  - Systemd unit reconnects to QEMU after console disconnect.
Deliverables:
  - REST gateway + OpenAPI spec + systemd service unit.

Title/ID: m24c-python-rest-backend
Goal: Add REST backend to cohesix-py with parity coverage.
Inputs: tools/cohesix-py, docs/PYTHON_SUPPORT.md.
Changes:
  - tools/cohesix-py/cohesix/backends.py — RestBackend.
  - tools/cohesix-py/tests/test_parity.py — REST parity fixtures.
  - docs/PYTHON_SUPPORT.md — REST usage docs.
Commands:
  - python -m pytest -k cohesix_parity
Checks:
  - REST backend matches cohsh semantics.
Deliverables:
  - REST backend + parity tests.

Title/ID: m24c-docs-quickstart-testplan
Goal: Update Quickstart and Test Plan for REST gateway + new control grammar.
Inputs: docs/QUICKSTART.md, docs/TEST_PLAN.md, docs/USERLAND_AND_CLI.md.
Changes:
  - docs/QUICKSTART.md — REST gateway quickstart (mock + live QEMU).
  - docs/TEST_PLAN.md — add schedule/lease/export/policy checks + REST gateway tests (including systemd service + reconnect).
  - docs/USERLAND_AND_CLI.md — document new control files (`echo` JSONL).
Commands:
  - scripts/ci/check_test_plan.sh
Checks:
  - Quickstart steps run on macOS 26 + QEMU.
  - New Test Plan steps executed and recorded as part of DoD.
Deliverables:
  - Updated docs for frictionless adoption.

Title/ID: m24c-regression-pack
Goal: Add fixtures and run the regression pack unchanged.
Inputs: scripts/cohsh/run_regression_batch.sh, tests/fixtures/*.
Changes:
  - tests/fixtures/ — add schedule/lease/export/policy fixtures.
Commands:
  - scripts/cohsh/run_regression_batch.sh
Checks:
  - No ACK/ERR/END drift outside updated fixtures.
Deliverables:
  - Updated fixtures and regression evidence.
```

----
**Release 0.4.0 alpha**
----

## Milestone 24d — Jetson CUDA Host Support (NVML Fallback + Doctor) <a id="24d"></a>
[Milestones](#Milestones)

**Status:** Complete.

**Why now (adoption):** Jetson Orin hosts ship NVML with feature gaps. Host tools must still publish `/gpu/*` and pass `coh doctor` without weakening lease semantics or requiring mock mode.

**Goal**
Enable CUDA-based GPU discovery by default in host tools, with a deterministic NVML→CUDA fallback for Jetson-class NVML limitations. `coh doctor` must always succeed on Jetson by falling back to CUDA APIs when NVML is feature-limited.

**Deliverables**
- CUDA inventory backend for `gpu-bridge-host` (driver/runtime APIs) with deterministic NVML→CUDA fallback.
- CUDA support enabled by default in affected host tools (`gpu-bridge-host`, `coh`), while preserving NVML support for dGPU hosts.
- `coh doctor` treats NVML “not supported/feature-limited” as non-fatal and falls back to CUDA; emits an explicit degraded status line.
- Docs updates: `docs/HOST_TOOLS.md`, `docs/GPU_NODES.md`, `docs/USERLAND_AND_CLI.md` (Jetson guidance + backend behavior).
- Host setup supports Ubuntu 22.04 and documents Python 3.11 venv usage in Quickstart (for Jetson-class hosts).

**Commands**
- `cargo test -p host-cuda`
- `cargo test -p gpu-bridge-host`
- `cargo test -p coh --test transcript`
- `cargo test -p coh --test run`

**Checks (DoD)**
- On Jetson Orin (JP 6.2.1), `./bin/gpu-bridge-host --list` reports `memory_mb > 0`, `sm_count > 0`, and non-empty driver/runtime versions via CUDA.
- On Jetson Orin, `./bin/coh doctor` succeeds without `--mock` and emits a CUDA fallback status line.
- On dGPU hosts with NVML, `gpu-bridge-host` still uses NVML and returns unchanged inventory fields.
- `/gpu/<id>/info` schema remains unchanged and leases continue to enforce `mem_mb` bounds.

**Task Breakdown**
```
Title/ID: m24d-gpu-bridge-cuda
Goal: Add CUDA inventory backend and deterministic NVML→CUDA fallback for GPU discovery.
Inputs: apps/gpu-bridge-host/src/lib.rs, apps/gpu-bridge-host/Cargo.toml, docs/INTERFACES.md.
Changes:
  - crates/host-cuda/ — CUDA driver/runtime probe (host-only, unsafe isolated).
  - apps/gpu-bridge-host/src/lib.rs — add CUDA inventory implementation and fallback logic.
  - apps/gpu-bridge-host/Cargo.toml — enable CUDA support by default.
  - docs/INTERFACES.md — document backend selection and Jetson behavior.
  - docs/REPO_LAYOUT.md — document the new host CUDA crate.
Commands:
  - cargo test -p host-cuda
  - cargo test -p gpu-bridge-host
Checks:
  - CUDA discovery succeeds on Jetson; NVML remains active on dGPU.
Deliverables:
  - CUDA-backed GPU discovery with deterministic fallback.

Title/ID: m24d-coh-doctor-cuda
Goal: Ensure `coh doctor` passes on Jetson by falling back to CUDA when NVML is feature-limited.
Inputs: apps/coh/src/doctor.rs, apps/coh/Cargo.toml, docs/USERLAND_AND_CLI.md.
Changes:
  - apps/coh/src/doctor.rs — detect NVML limitations and fallback to CUDA APIs.
  - apps/coh/Cargo.toml — enable CUDA support by default alongside NVML.
  - docs/USERLAND_AND_CLI.md — document NVML fallback behavior.
Commands:
  - cargo test -p coh --test transcript
Checks:
  - `coh doctor` succeeds on Jetson without `--mock` and emits a degraded CUDA fallback line.
Deliverables:
  - Deterministic Jetson-friendly doctor checks.

Title/ID: m24d-docs-host-tools
Goal: Update host tool docs for Jetson CUDA discovery and fallback semantics.
Inputs: docs/HOST_TOOLS.md, docs/GPU_NODES.md.
Changes:
  - docs/HOST_TOOLS.md — clarify CUDA-by-default behavior and NVML fallback.
  - docs/GPU_NODES.md — describe Jetson inventory via CUDA APIs and limits.
Commands:
  - scripts/ci/check_test_plan.sh
Checks:
  - Docs accurately describe as-built host discovery behavior.
Deliverables:
  - Jetson-ready host tool documentation.

Title/ID: m24d-host-setup-ubuntu
Goal: Support Ubuntu 22.04 host setup and document Python 3.11 venv usage for Quickstart.
Inputs: scripts/setup_environment.sh, docs/QUICKSTART.md.
Changes:
  - scripts/setup_environment.sh — allow Ubuntu 22.04; add explicit override for unsupported versions with best-effort package selection.
  - docs/QUICKSTART.md — add non-mock `coh doctor` expectations (NVML vs Jetson) and document Python 3.11 venv path for cohesix-py.
Commands:
  - scripts/ci/check_test_plan.sh
Checks:
  - Quickstart steps are clear for Ubuntu 22.04 + Python 3.11 venv users.
Deliverables:
  - Jetson-friendly host setup and Quickstart guidance.

Title/ID: m24d-coh-fuse-default
Goal: Enable FUSE support by default for Linux `coh` builds while keeping macOS opt-in.
Inputs: apps/coh/Cargo.toml, docs/HOST_TOOLS.md, docs/QUICKSTART.md, docs/PYTHON_SUPPORT.md, docs/USERLAND_AND_CLI.md, docs/TEST_PLAN.md.
Changes:
  - apps/coh/Cargo.toml — default FUSE on Linux, macOS opt-in via feature.
  - docs/HOST_TOOLS.md — update OS-specific FUSE defaults.
  - docs/QUICKSTART.md — document macOS opt-in behavior.
  - docs/PYTHON_SUPPORT.md — align FUSE notes with OS defaults.
  - docs/USERLAND_AND_CLI.md — clarify live mount prerequisites.
  - docs/TEST_PLAN.md — note macOS default FUSE disabled.
Commands:
  - cargo check -p coh
  - cargo test -p coh --test transcript
  - cargo test -p coh --test run
Checks:
  - `coh doctor` passes the mount check when Linux `/dev/fuse` (or macOS `/dev/macfuse0`) is available.
  - Docs reflect OS-specific FUSE defaults.
Deliverables:
  - Linux-default FUSE-enabled `coh` builds with aligned documentation.

Title/ID: m24d-toolchain-linux
Goal: Provide a Linux toolchain bootstrap script aligned with Cohesix host tool requirements.
Inputs: toolchain/setup_macos_arm64.sh, toolchain/setup_linux_arm64.sh.
Changes:
  - toolchain/setup_linux_arm64.sh — install build/runtime prerequisites, rustup, and QEMU checks for Ubuntu hosts.
Commands:
  - toolchain/setup_linux_arm64.sh
Checks:
  - Script installs required packages and reports tool versions.
Deliverables:
  - Linux toolchain setup script for host builds.
```

## Milestone 24e — REST Multiplexer Transports + SwarmUI Gateway Mode <a id="24e"></a>
[Milestones](#Milestones)

**Status:** Complete.

**Why now (adoption):** Live multi-host publishing requires a single console client. We need host tools and SwarmUI to speak to the `hive-gateway` REST projection so multiple external workers can publish and observe without breaking the single-client console constraint.

**Goal**
Add REST-backed transports for host publishers (`gpu-bridge-host`, `host-sidecar-bridge`, `cas-tool`), `coh` (including a REST-backed mount mode), and SwarmUI so they can multiplex through `hive-gateway` while retaining all existing console and Secure9P features.

**Deliverables**
- REST client crate for hive-gateway (`/v1/fs/ls`, `/v1/fs/cat`, `/v1/fs/echo`).
- `cohsh` REST transport implementing the existing transport trait (no new semantics).
- `cohsh` CLI supports `--transport rest` with `--rest-url` (env: `COHSH_REST_URL`, `COH_REST_URL`, `HIVE_GATEWAY_URL`).
- `gpu-bridge-host` publish via REST (`/gpu/bridge/ctl`) with `--rest-url`.
- `host-sidecar-bridge` publish via REST (`/host/*`) with `--rest-url`.
- `cas-tool` upload via REST (`/updates/*`) with `--rest-url`.
- `coh` REST mode for `mount`, `gpu`, `telemetry pull`, `peft`, and `run` via `--rest-url` (REST mount is exclusive: one active mount per gateway URL).
- REST transport clamps `/proc` reads to manifest bounds so SwarmUI shows schedule/lease data in REST mode.
- SwarmUI transport option (`SWARMUI_TRANSPORT=rest|gateway`) that routes through hive-gateway and preserves all existing features in console/9p modes (REST transport is enabled by default; disable with `--no-default-features` if needed).
- SwarmUI live hive PixiJS renderer remains responsive under load (particle containers + capped sim steps).
- Docs updated for REST multiplexer usage and SwarmUI transport selection.
- REST multiplexer is queen-role only; worker-role attach remains console/9P-only.

**Commands**
- `cargo check -p cohesix-rest`
- `cargo test -p cohsh`
- `cargo test -p gpu-bridge-host`
- `cargo test -p host-sidecar-bridge`
- `cargo test -p swarmui`

**Checks (DoD)**
- REST transport maps `LS`/`CAT`/`ECHO` to gateway responses with deterministic errors.
- `cohsh --transport rest --rest-url` attaches and reads `/proc/schedule/*` + `/proc/lease/*` without max_bytes bound errors.
- REST publish paths populate `/gpu/*` and `/host/*` without console attachment.
- `cas-tool --rest-url` uploads CAS bundles via `/updates/*` without console attachment.
- `coh mount --rest-url` mounts via gateway (queen-role only, append-only semantics preserved, single REST mount per gateway URL).
- SwarmUI REST mode connects through hive-gateway and renders the same panels/features as console/9p modes (REST transport enabled by default).
- SwarmUI live hive view remains responsive (no PixiJS stalls) with live multi-worker telemetry.
- Multiplexer smoke coverage: `cohsh` REST attach/ping; `gpu-bridge-host --rest-url --publish`; `host-sidecar-bridge --rest-url --watch`; `cas-tool upload --rest-url`; `coh mount|gpu|telemetry|peft|run --rest-url`; `SWARMUI_TRANSPORT=rest` with live hive view and console commands.
- Docs describe REST multiplexer usage and transport selection clearly.

**Task Breakdown**
```
Title/ID: m24e-rest-client
Goal: Provide a shared hive-gateway REST client for host tools.
Inputs: docs/HOST_API.md.
Changes:
  - crates/cohesix-rest/ — add GatewayClient + response models.
  - Cargo.toml — add cohesix-rest to workspace.
Commands:
  - cargo check -p cohesix-rest
Checks:
  - REST client validates OK/ERR and preserves gateway error details.
Deliverables:
  - Shared REST client crate for host tools.

Title/ID: m24e-cohsh-rest-transport
Goal: Add a REST-backed transport that implements the cohsh Transport trait.
Inputs: apps/cohsh/src/lib.rs, apps/cohsh/src/transport.
Changes:
  - apps/cohsh/src/transport/rest.rs — implement Transport over hive-gateway.
  - apps/cohsh/src/transport/mod.rs — export rest transport.
  - apps/cohsh/Cargo.toml — add cohesix-rest dependency/feature.
Commands:
  - cargo test -p cohsh
Checks:
  - Transport methods (`list`, `read`, `write`, `tail`) map to REST without changing semantics.
Deliverables:
  - REST transport available to host tools and SwarmUI.

Title/ID: m24e-cohsh-rest-cli
Goal: Expose REST transport selection in the cohsh CLI.
Inputs: apps/cohsh/src/main.rs, docs/USERLAND_AND_CLI.md, docs/HOST_TOOLS.md.
Changes:
  - apps/cohsh/src/main.rs — add `--transport rest` + `--rest-url` resolution.
  - docs/USERLAND_AND_CLI.md — document REST transport option for cohsh.
  - docs/HOST_TOOLS.md — add cohsh REST example.
Commands:
  - cargo test -p cohsh
Checks:
  - `cohsh --transport rest --rest-url` connects to hive-gateway and supports core verbs.
Deliverables:
  - CLI access to REST multiplexer transport.

Title/ID: m24e-rest-proc-bounds
Goal: Clamp REST `/proc` reads to manifest bounds so SwarmUI shows schedule/lease data.
Inputs: apps/cohsh/src/transport/rest.rs, docs/HOST_API.md.
Changes:
  - apps/cohsh/src/transport/rest.rs — enforce `/proc` read bounds via gateway metadata.
Commands:
  - cargo test -p cohsh
Checks:
  - REST reads for `/proc/schedule/*` and `/proc/lease/*` succeed with bounded `max_bytes`.
Deliverables:
  - SwarmUI REST schedule/lease visibility retained.

Title/ID: m24e-bridge-rest-publish
Goal: Allow host bridge publishers to use hive-gateway as a multiplexer.
Inputs: apps/gpu-bridge-host/src/main.rs, apps/host-sidecar-bridge/src/main.rs.
Changes:
  - apps/gpu-bridge-host/src/main.rs — add `--rest-url` publish mode.
  - apps/host-sidecar-bridge/src/main.rs — add `--rest-url` publish mode.
  - apps/gpu-bridge-host/Cargo.toml — add cohesix-rest dependency.
  - apps/host-sidecar-bridge/Cargo.toml — enable cohsh REST transport feature.
Commands:
  - cargo test -p gpu-bridge-host
  - cargo test -p host-sidecar-bridge
Checks:
  - REST publish mode writes valid snapshot lines and respects bounds.
Deliverables:
  - REST-capable host publishers.

Title/ID: m24e-cas-tool-rest
Goal: Allow cas-tool to upload bundles through hive-gateway.
Inputs: apps/cas-tool/src/main.rs, docs/HOST_API.md.
Changes:
  - apps/cas-tool/src/main.rs — add `--rest-url` upload mode.
  - apps/cas-tool/Cargo.toml — add cohesix-rest dependency.
Commands:
  - cargo test -p cas-tool
Checks:
  - REST upload writes base64 chunks to `/updates/*` without console attachment.
Deliverables:
  - REST-capable cas-tool upload.

Title/ID: m24e-coh-rest-mount
Goal: Add REST-backed CohAccess for `coh` and support `coh mount --rest-url`.
Inputs: apps/coh/src/main.rs, apps/coh/src/mount.rs, apps/coh/src/rest.rs.
Changes:
  - apps/coh/src/rest.rs — implement REST CohAccess.
  - apps/coh/src/mount.rs — add REST-backed mount path.
  - apps/coh/src/main.rs — add `--rest-url` handling for live operations.
  - apps/coh/Cargo.toml — add cohesix-rest dependency.
Commands:
  - cargo test -p coh
Checks:
  - REST mount uses queen-role gateway, preserves append-only semantics, and enforces a single mount per gateway URL.
Deliverables:
  - `coh` REST mode including mount support.

Title/ID: m24e-swarmui-rest-transport
Goal: Add SwarmUI transport option that connects through hive-gateway.
Inputs: apps/swarmui/src-tauri/main.rs, apps/swarmui/src/lib.rs.
Changes:
  - apps/swarmui/src-tauri/main.rs — support `SWARMUI_TRANSPORT=rest|gateway`.
  - apps/swarmui/Cargo.toml — enable cohsh REST transport feature.
Commands:
  - cargo test -p swarmui
Checks:
  - SwarmUI REST mode renders existing panels and console commands without regressions (REST transport enabled by default).
Deliverables:
  - SwarmUI gateway mode for REST multiplexing.

Title/ID: m24e-swarmui-pixi-perf
Goal: Keep SwarmUI Live Hive responsive under multi-worker telemetry load.
Inputs: apps/swarmui/frontend/hive/renderer.js, apps/swarmui/frontend/hive/index.js.
Changes:
  - apps/swarmui/frontend/hive/renderer.js — shift to particle containers + sprite-based clusters.
  - apps/swarmui/frontend/hive/index.js — cap sim steps per frame + throttle render rate under pressure.
Commands:
  - cargo test -p swarmui
Checks:
  - Live Hive view remains responsive (no UI stalls) with active telemetry and multiple workers.
Deliverables:
  - PixiJS rendering optimizations for live mode.

Title/ID: m24e-docs-rest-multiplexer
Goal: Document REST multiplexer usage and SwarmUI transport selection.
Inputs: docs/HOST_TOOLS.md, docs/API_GUIDELINES.md, docs/HOST_API.md, docs/USERLAND_AND_CLI.md.
Changes:
  - docs/HOST_TOOLS.md — add REST publish examples for host bridges and SwarmUI.
  - docs/API_GUIDELINES.md — add transport guidance for REST multiplexer deployments.
  - docs/HOST_API.md — add `/gpu/bridge/ctl` and `/host/*` REST examples.
  - docs/USERLAND_AND_CLI.md — document SwarmUI transport options.
Commands:
  - scripts/ci/check_test_plan.sh
Checks:
  - Docs reflect as-built REST multiplexer behavior.
Deliverables:
  - Updated operator-facing documentation.
```

## Milestone 25 — SMP Utilization via Task Isolation (Multicore without Multithreading) <a id="25"></a> 
[Milestones](#Milestones)

**Why now (platform and performance):** Cohesix targets modern aarch64 hardware where multicore CPUs are the norm. To scale throughput without sacrificing determinism, auditability, or TCB size, Cohesix must exploit seL4 SMP scheduling rather than introducing shared-memory multithreading. This milestone formalizes multicore usage through task isolation, sharding, and explicit authority boundaries.

This is a performance and clarity milestone, not a feature expansion.

**Status:** Complete — SMP kernel builds, 4-core QEMU defaults, and task-isolation behaviors are validated. SMP selftests, REST regression batch, and host tool coverage pass on macOS and Linux with documented QEMU overrides.

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
- Run concurrent cohsh regression scripts over the REST multiplexer (`hive-gateway` as the sole console client) using `scripts/cohsh/REST_regression_batch.sh`.

### Replay
- Capture traces on multicore.
- Replay on single-core QEMU and assert identical outcomes.

## Checks (Definition of Done)
- Cohesix runs correctly on multicore aarch64 under QEMU and hardware.
- Parallel tasks execute on multiple cores without shared-memory races.
- Authority logic remains single-threaded and replayable.
- Back-pressure is explicit and observable.
- No new threads, runtimes, or hidden queues introduced.
- `scripts/cohsh/REST_regression_batch.sh` passes unchanged with `hive-gateway` as the sole console client (concurrent REST runs).
- `cohsh test --mode smp` completes without unexpected errors while the root console `smp` command reports activity (or deterministic `ERR reason=unsupported` on non-debug builds).
- Root console `smp` command emits per-core scheduler/CPU metrics (or deterministic `ERR reason=unsupported` when debug syscalls are unavailable).
- Documentation clearly explains the SMP model and its constraints.

## Task Breakdown
```
Title/ID: m25c-smp-kernel-enable
Goal: Enable seL4 SMP in the external kernel build and document requirements.
Inputs: ~/seL4/SMP_build, docs/ARCHITECTURE.md, docs/BUILD_PLAN.md.
Changes:
  - ~/seL4/SMP_build/ — regenerate kernel artifacts with SMP enabled (do not touch ~/seL4/build).
  - docs/ARCHITECTURE.md — record SMP kernel requirements and QEMU CPU count.
Commands:
  - cmake --build ~/seL4/SMP_build
Checks:
  - SMP-enabled kernel boots under QEMU with >1 core.
Deliverables:
  - SMP kernel artifacts and documented build requirements.

Title/ID: m25c-smp-build-mirror
Goal: Mirror SMP build outputs into the repo and make SMP the default build path.
Inputs: ~/seL4/SMP_build, seL4/SMP_build, scripts/cohesix-build-run.sh.
Changes:
  - seL4/SMP_build/ — copy SMP build outputs from ~/seL4/SMP_build (leave seL4/build untouched).
  - scripts/cohesix-build-run.sh — default SEL4_BUILD_DIR to seL4/SMP_build; allow override to seL4/build.
Commands:
  - rsync -a ~/seL4/SMP_build/ seL4/SMP_build/
Checks:
  - `scripts/cohesix-build-run.sh` uses seL4/SMP_build by default; `--sel4-build seL4/build` overrides correctly.
Deliverables:
  - Repo-local SMP build outputs and updated build defaults.

Title/ID: m25c-smp-qemu-defaults
Goal: Default QEMU SMP topology to four single-threaded cores while keeping overrides explicit.
Inputs: scripts/qemu-run.sh, scripts/cohesix-build-run.sh, scripts/release_bundle.sh, releases/*/qemu/run.sh, docs/QUICKSTART.md, docs/TEST_PLAN.md.
Changes:
  - scripts/qemu-run.sh — default to `-smp 4,cores=4,threads=1,sockets=1`; allow overrides via `COHESIX_QEMU_SMP` / `QEMU_SMP` (count) and `COHESIX_QEMU_SMP_TOPO` / `QEMU_SMP_TOPO` (full topology string).
  - scripts/cohesix-build-run.sh — same default and override handling as `scripts/qemu-run.sh`.
  - scripts/release_bundle.sh — bake the SMP defaults and env overrides into generated `qemu/run.sh`.
  - releases/<next minor>-* — bump minor version per policy; update bundled `qemu/run.sh` defaults and rename tarballs accordingly.
  - docs/QUICKSTART.md, docs/TEST_PLAN.md — document the SMP default and override variables.
Commands:
  - COHESIX_QEMU_SMP=1 scripts/cohesix-build-run.sh --no-run --cargo-target aarch64-unknown-none
  - COHESIX_QEMU_SMP=4 scripts/cohesix-build-run.sh --no-run --cargo-target aarch64-unknown-none
Checks:
  - Default QEMU launches use `-smp 4,cores=4,threads=1,sockets=1`.
  - SMP overrides apply deterministically with no validation regressions.
  - Release bundle minor version increments and tarball names match directory names.
Deliverables:
  - SMP-aware QEMU launch defaults with documented override behavior.

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

Title/ID: m25c-smp-cohsh-selftest
Goal: Add `cohsh test --mode smp` to pressure SMP and surface activity for the `smp` console command.
Inputs: apps/cohsh, apps/nine-door, resources/proc_tests, docs/USERLAND_AND_CLI.md, docs/TEST_PLAN.md.
Changes:
  - resources/proc_tests/selftest_smp.coh — SMP-oriented regression script.
  - apps/nine-door/src/host/namespace.rs — expose `/proc/tests/selftest_smp.coh`.
  - apps/cohsh/src/lib.rs — accept `test --mode smp` and map to the new script.
  - docs/USERLAND_AND_CLI.md — document the new selftest script and mode.
  - docs/TEST_PLAN.md — add `test --mode smp` to QEMU regression steps.
Commands:
  - cargo test -p cohsh
Checks:
  - `cohsh test --mode smp` completes without unexpected errors.
  - Root console `smp` shows activity (or deterministic `ERR reason=unsupported` on non-debug builds).
Deliverables:
  - SMP selftest script and cohsh mode support.

Title/ID: m25-smp-console-metrics
Goal: Add a root console verb to emit per-core SMP metrics for seL4.
Inputs: root console parser, seL4 debug syscall docs, docs/USERLAND_AND_CLI.md.
Changes:
  - apps/root-task/src/console.rs — add `smp` command (adjacent to `bi`/`caps`) that invokes seL4 debug scheduler/CPU dump APIs (`seL4_DebugDumpScheduler`, `seL4_DebugDumpCPUInfo`) when enabled; bounded output, no shared-memory access.
  - docs/USERLAND_AND_CLI.md — document `smp` root console output and debug-build gating.
Commands:
  - QEMU serial console: `smp`
Checks:
  - Debug builds emit per-core scheduler/CPU metrics (core id, runnable/idle summary) with bounded output.
  - Non-debug builds return deterministic `ERR reason=unsupported` with no side effects.
Deliverables:
  - Root console SMP metrics command with seL4-aligned semantics.

Title/ID: m25-smp-rest-regression-batch
Goal: Stress SMP using concurrent cohsh regression scripts via the REST multiplexer.
Inputs: scripts/cohsh/*.coh, apps/hive-gateway, docs/TEST_PLAN.md.
Changes:
  - scripts/cohsh/REST_regression_batch.sh — run multiple `cohsh --transport rest` scripts concurrently against `hive-gateway` using `COHESIX_GATEWAY_URL` (or `HIVE_GATEWAY_URL`/`COHSH_REST_URL`/`COH_REST_URL`); bounded concurrency, no script changes.
  - docs/TEST_PLAN.md — add a REST SMP stress run using the batch script.
Commands:
  - `COHESIX_GATEWAY_URL=http://<gateway-host>:<port> scripts/cohsh/REST_regression_batch.sh`
Checks:
  - All regression scripts pass unchanged under concurrent REST load.
  - `ERR ... reason=busy` appears only under saturation and is audited; ACK/ERR ordering remains deterministic.
Deliverables:
  - REST multiplexer SMP stress harness and documented runbook.

Title/ID: m25h-live-hive-turbo
Goal: Deliver ≥100x Live Hive responsiveness by prioritizing fresh telemetry visibility, bounded aggregation, and adaptive rendering without changing protocol semantics.
Inputs: apps/swarmui/src, apps/swarmui/frontend, tools/coh-rtc, docs/INTERFACES.md, docs/TEST_PLAN.md.
Changes:
  - apps/swarmui/src/hive.rs — O(1) telemetry line parsing, per-worker pending caps, fresh-only coalescing, round-robin worker sampling, overlay/detail caching.
  - apps/swarmui/src/lib.rs — Live Hive polling uses freshness-first policy; status snapshots rate-limited behind IR-driven bounds.
  - apps/swarmui/frontend/hive/index.js — ring-buffer ingestion, drop-oldest under pressure, index-range event application (no per-step slicing).
  - apps/swarmui/frontend/app.js + apps/swarmui/frontend/hive/index.js — keep Live Hive rendering active during scroll (no pause) while degrading cadence/quality; pause rendering only when the canvas is offscreen; throttle cadence, cap detail LOD, and drop render resolution when the canvas is idle to keep page interactions smooth.
  - apps/swarmui/frontend/hive/world.js — cache per-tick positions; in-place compaction for pollen/pulse particles.
  - apps/swarmui/frontend/hive/renderer.js — adaptive resolution/AA/particle budgets under degraded mode.
  - tools/coh-rtc/src/ir.rs + generated configs — manifest-backed Live Hive performance knobs (sample size, pending caps, degrade thresholds).
  - docs/INTERFACES.md — document freshness-first Live Hive guarantees and aggregation bounds.
  - docs/TEST_PLAN.md — add Live Hive performance regression harness (target freshness + frame budget).
Commands:
  - cargo run -p coh-rtc
  - cargo test -p swarmui
  - (SwarmUI UI regression) cd tools/swarmui-ui-tests && npm test
Checks:
  - Live Hive shows current telemetry within one poll under 3–100 workers with bounded backlog.
  - Aggregation/caching preserves latest state while historical detail remains bounded and replay-safe.
  - Renderer meets frame budget targets under load; degraded mode engages deterministically.
Deliverables:
  - Freshness-first Live Hive with documented bounds and ≥100x reduction in worst-case UI/ingest latency.
```
----
**Release 0.6.0 alpha**
----

## Milestone 25a — REST Live Hive Performance (Parallel Polling + Batching) <a id="25a"></a> 
[Milestones](#Milestones)

**Why now (demo readiness):** REST is the only transport that supports the demo multiplexer, but Live Hive poll latency is dominated by serial REST reads. This milestone reduces REST wall-clock latency without changing protocols or semantics, while preserving Secure9P bounds and auditability.

**Status:** Complete — REST status polling is parallelized, REST cache behavior honors `status_poll_ms`, pool sizing supports bounded concurrency, telemetry fan-out is pool-capped, and deterministic REST performance harness coverage is in place.

## Goal
Reduce Live Hive REST status poll wall-clock time with minimal risk by:
1. Parallelizing REST status reads in SwarmUI, and
2. Honoring `status_poll_ms` caching in REST mode,
3. Increasing REST session pool sizing to support concurrency,
4. Parallelizing REST telemetry tails after pool sizing, with concurrency capped to pool limits, and
5. Adding a deterministic REST performance harness, and
6. Applying manifest-driven CPU affinity to the root-task authority thread during bootstrap.

## Non-Goals (Explicit)
- No new protocols or transports.
- No changes to ACK/ERR/END grammar.
- No changes to Secure9P semantics or bounds.
- No release bundle updates under `releases/`.
- No UI feature expansion beyond performance.

## Design Principles (Normative)
1. **Protocol stability** — REST remains a thin wrapper over existing NineDoor semantics.
2. **Bounded concurrency** — parallelism must respect pool sizing and manifest limits.
3. **Deterministic behavior** — parallel reads must not reorder or coalesce event semantics.
4. **Opt-in batching** — new batch endpoints must be additive and backward compatible.

## Implementation Touchpoints
- `apps/swarmui/src/lib.rs` — REST status polling: cache + parallel reads.
- `configs/root_task.toml` — REST pool sizing adjustments (manifest-driven).
- `out/cohsh_policy.toml` + generated policy artifacts (via `coh-rtc`).
- `apps/swarmui/src/hive.rs` — REST telemetry tails: parallel with pool-capped concurrency.
- `scripts/rest_perf_harness.py` — deterministic REST performance harness.
- `apps/root-task/src/affinity.rs`, `apps/root-task/src/sel4.rs`, `apps/root-task/src/kernel.rs` — apply manifest-driven affinity during bootstrap.

## Testing and Validation

### Functional
- SwarmUI REST mode must continue to render Live Hive without regressions.

### Performance
- Run the REST performance harness to quantify sequential vs parallel status latency.
 - Verify pool sizing eliminates "session pool exhausted" under parallel polling.

## Checks (Definition of Done)
- SwarmUI REST mode reduces status polling wall-clock latency by at least 2x on the same host.
- `scripts/rest_perf_harness.py --suite status --assert-min-ratio 2.0` passes.
- `scripts/rest_perf_harness.py --suite telemetry --assert-min-ratio 1.3` passes when workers are present.
- `cargo test -p swarmui` passes.
- `cargo run -p coh-rtc` + `scripts/check-generated.sh` pass after manifest changes.

## Task Breakdown
```
Title/ID: m25a-rest-perf-harness
Goal: Add a deterministic REST performance harness for Live Hive polling.
Inputs: scripts/rest_perf_harness.py.
Changes:
  - scripts/rest_perf_harness.py — sequential vs parallel REST timing for status + telemetry.
Commands:
  - python3 scripts/rest_perf_harness.py --help
Checks:
  - Harness reports averages and enforces optional min speedup ratio.
Deliverables:
  - REST performance harness script.

Title/ID: m25a-swarmui-rest-status
Goal: Parallelize and cache REST status polling in Live Hive.
Inputs: apps/swarmui/src/lib.rs.
Changes:
  - apps/swarmui/src/lib.rs — parallel `/proc` reads and honor `status_poll_ms` in REST mode.
Commands:
  - cargo test -p swarmui
Checks:
  - Live Hive status remains correct; poll latency drops.
Deliverables:
  - SwarmUI REST status polling improvements.

Title/ID: m25a-rest-pool-sizing
Goal: Increase REST session pool sizing to support parallel polling.
Inputs: configs/root_task.toml.
Changes:
  - configs/root_task.toml — adjust `client_policies.cohsh.pool` sizes.
  - out/cohsh_policy.toml — regenerated.
  - apps/cohsh/src/generated/policy.rs — regenerated.
  - docs/snippets/cohsh_policy.md — regenerated.
Commands:
  - cargo run -p coh-rtc
  - scripts/check-generated.sh
Checks:
  - Generated policy artifacts match manifest.
Deliverables:
  - Updated REST pool sizing and generated artifacts.

Title/ID: m25a-swarmui-rest-telemetry
Goal: Parallelize REST telemetry tails per worker after pool sizing.
Inputs: apps/swarmui/src/hive.rs, configs/root_task.toml, out/cohsh_policy.toml.
Changes:
  - apps/swarmui/src/hive.rs — parallel `tail` calls within pool-sized bounds.
Commands:
  - cargo test -p swarmui
Checks:
  - No changes to telemetry semantics; poll latency improves with multiple workers.
  - Parallelism never exceeds configured REST telemetry pool size.
Deliverables:
  - SwarmUI REST telemetry polling improvements with pool-capped concurrency.

Title/ID: m25a-root-affinity-wire
Goal: Apply manifest-driven CPU affinity to the root-task authority thread and role-specific operations (NineDoor attach and worker spawns) and extend SMP debug output to iterate configured cores.
Inputs: apps/root-task/src/affinity.rs, apps/root-task/src/sel4.rs, apps/root-task/src/kernel.rs, tools/coh-rtc, configs/root_task.toml, docs/ARCHITECTURE.md.
Changes:
  - apps/root-task/src/affinity.rs — policy validation and role-based core selection helpers.
  - apps/root-task/src/sel4.rs — guarded TCB affinity syscall wrapper.
  - apps/root-task/src/kernel.rs — validate policy and pin the init TCB to `authority_core`.
  - apps/root-task/src/userland/mod.rs — apply NineDoor affinity during bridge attach.
  - apps/root-task/src/ninedoor.rs — apply worker affinity during worker spawns.
  - apps/root-task/src/console/mod.rs — per-core SMP debug dumps using configured affinity cores.
  - apps/root-task/src/event/mod.rs — per-core SMP debug dumps using configured affinity cores.
  - tools/coh-rtc/src/codegen/rust.rs — emit affinity policy tables.
  - tools/coh-rtc/src/codegen/docs.rs — include affinity fields in manifest snippet.
  - docs/ARCHITECTURE.md — document root-task affinity application.
Commands:
  - cargo test -p root-task
  - cargo run -p coh-rtc
  - scripts/check-generated.sh
Checks:
  - Root-task bootstrap logs authority core pinning when affinity is enabled.
  - NineDoor attach and worker spawns log role-core affinity applications.
  - `smp` debug output cycles through configured role cores.
  - Generated manifest snippet includes affinity fields and matches configs.
Deliverables:
  - Manifest-driven root-task affinity wiring for the init TCB.
```
----
**Release 0.7.0 alpha**
----

## Milestone 25b — Secure Scale Gateway (1k Worker Readiness + Due Diligence Closure) <a id="25b"></a>
[Milestones](#Milestones)

**Why now (release integrity + scale):** The live benchmark demonstrates 100 workers sustained with timeout onset under heavier activity before 100 in ramp mode, while due-diligence still contains open P1 blockers in gateway auth, token handling, regression coverage, and platform gates. This milestone addresses both concerns in one bounded track so scale gains are security-valid and release-valid.

**Status:** Complete - due-diligence gate baseline passed at `out/audit/gate/20260214T044955Z`, `P0/P1` blockers are closed in `docs/audit/findings.csv`, and Milestone 25b closure evidence is published in `docs/audit/AUDIT_REPORT_2026-02-14.md`.

## Goal
Deliver a single-console-compliant gateway path that can be validated toward 1,000-worker operation while closing release-blocking due-diligence findings by:
1. Hardening gateway edge authentication and secret handling.
2. Preserving the one-console-client design while improving multiplexed throughput and fairness.
3. Expanding benchmark methodology to deterministic 8..1000 worker ramps with reproducible evidence.
4. Restoring failing regression/workspace gates so performance evidence is trustworthy.
5. Publishing closure evidence in audit artifacts and benchmark docs.

## Non-Goals (Explicit)
- No new in-VM TCP listeners or alternate control-plane transports.
- No changes to ACK/ERR/END grammar or Secure9P protocol semantics.
- No bypass of role/ticket authorization inside the VM.
- No broad UI feature work unrelated to scale or due-diligence closure.
- No release bundle changes under `releases/` in this milestone.

## Design Principles (Normative)
1. **Single-console authority** - `hive-gateway` remains the sole TCP console client; all concurrent operators route through REST.
2. **Edge auth plus VM auth** - add caller authentication at REST ingress without changing VM attach semantics.
3. **Bounded fairness** - queueing, caching, and concurrency must be explicit, bounded, and manifest/policy constrained.
4. **Evidence-first closure** - every P1/P2 closure includes deterministic repro, command logs, commit SHA, and independent verification.
5. **No hidden drift** - docs, generated artifacts, and gate scripts must converge on one canonical command set.

## Implementation Touchpoints
- `apps/hive-gateway/src/main.rs` - request-auth middleware, bind/exposure guardrails, bounded broker queueing and metrics.
- `apps/cohsh/src/transport/tcp.rs`, `apps/cohsh/src/session_pool.rs` - transport/pool behavior required for gateway throughput under single console.
- `apps/root-task/src/net/mod.rs`, `apps/root-task/src/net/console_srv.rs` - default token removal and auth-log redaction.
- `scripts/rest_perf_harness.py` - deterministic ramp/seed support, resilient summaries, graph/CSV export.
- `tests/tests/shard_1k.rs`, `scripts/cohsh/shard_1k.coh`, `crates/sel4-runtime/src/lib.rs`, `crates/sel4-sys/src/lib.rs` - regression/workspace gate repairs.
- `docs/BENCHMARKS.md`, `docs/HOST_API.md`, `docs/USERLAND_AND_CLI.md`, `docs/TEST_PLAN.md`, `docs/audit/*` - as-built benchmark + assurance evidence alignment.

## Testing and Validation

### Security and Auth
- Verify unauthenticated REST writes are rejected deterministically.
- Verify startup fails fast when required auth token inputs are missing in non-test mode.
- Verify auth rejection logs never include raw token bytes or token-adjacent material.

### Scale and Performance
- Run fixed-seed worker ramps (`8..1000`) with preflight-gated VM boot/TCP/auth validation.
- Compare baseline vs aggressive profiles with identical topology and bounds.
- Confirm summary emission even on timeout-heavy runs to preserve evidence quality.

### Regression and Release Gates
- Re-run `cargo test -p tests` and `cargo test --workspace` after fixture/platform fixes.
- Re-run generated-artifact and test-plan guards with updated docs snippets.
- Re-run `scripts/ci/due_diligence_gate.sh` and attach logs in audit evidence.

## Checks (Definition of Done)
- Single-console design remains intact (no additional in-VM listener; gateway is sole console client in multiplexer mode).
- `DD-2026-0001`, `DD-2026-0002`, `DD-2026-0003`, and `DD-2026-0010` move to `CLOSED_VERIFIED` with reproducible evidence.
- `DD-2026-0004`, `DD-2026-0005`, and `DD-2026-0009` move to `CLOSED_VERIFIED` with passing gate commands.
- `scripts/rest_perf_harness.py` supports deterministic 8..1000 worker runs and always emits machine-readable summary output.
- `docs/BENCHMARKS.md` includes comparative 8..1000 results and graphs sourced from committed evidence logs.
- `scripts/ci/due_diligence_gate.sh` passes without skipped/incomplete steps on milestone closure runs.

## Task Breakdown
```
Title/ID: m25b-gateway-edge-auth
Goal: Close gateway-facing auth blockers while preserving VM authority semantics.
Inputs: apps/hive-gateway/src/main.rs, docs/HOST_API.md, docs/OPERATOR_WALKTHROUGH.md, docs/audit/findings.csv.
Changes:
  - apps/hive-gateway/src/main.rs - add required request authentication middleware for mutating routes and fail-fast startup when auth secrets are unset in non-test mode.
  - apps/hive-gateway/src/main.rs - enforce safe default exposure (loopback-only unless explicitly overridden with documented risk flags).
  - apps/hive-gateway/tests/* - add integration tests for unauthenticated/incorrect-token/authorized request paths.
  - docs/HOST_API.md - document request-auth contract, headers, and deployment hardening defaults.
Commands:
  - cargo test -p hive-gateway
Checks:
  - Unauthenticated POST `/v1/fs/echo` and equivalent write paths return deterministic authorization errors.
  - Gateway refuses insecure startup configuration in non-test mode.
Deliverables:
  - Closure evidence for DD-2026-0002 and DD-2026-0010 with logs and commit SHA.

Title/ID: m25b-console-token-hygiene
Goal: Remove insecure token defaults and prevent auth token leakage in logs.
Inputs: apps/root-task/src/net/mod.rs, apps/root-task/src/net/console_srv.rs, tools/coh-rtc, configs/root_task.toml, docs/audit/findings.csv.
Changes:
  - apps/root-task/src/net/mod.rs - remove production default auth token path and require explicit configured token source.
  - apps/root-task/src/net/console_srv.rs - replace raw auth-byte logging with structured redacted diagnostics.
  - apps/root-task/tests/* - add negative assertions proving token material is not logged on auth rejection.
  - tools/coh-rtc + generated outputs - wire token requirements through generated policy/config where applicable.
Commands:
  - cargo test -p root-task
  - cargo run -p coh-rtc
  - scripts/check-generated.sh
Checks:
  - Missing token config fails fast outside test profiles.
  - Auth failure logs do not expose token contents or byte-equivalent payload.
Deliverables:
  - Closure evidence for DD-2026-0001 and DD-2026-0003 with regenerated artifacts.

Title/ID: m25b-single-console-broker
Goal: Increase gateway multiplexing throughput under one attached console session.
Inputs: apps/hive-gateway/src/main.rs, apps/cohsh/src/session_pool.rs, apps/cohsh/src/transport/tcp.rs, docs/USERLAND_AND_CLI.md.
Changes:
  - apps/hive-gateway/src/main.rs - introduce bounded broker queueing with priority classes (control before telemetry), fairness scheduling, and explicit backpressure signals.
  - apps/hive-gateway/src/main.rs - add bounded read-through caches for hot `/proc` reads with deterministic TTL and invalidation.
  - apps/hive-gateway/src/main.rs - expose queue depth, saturation, timeout, and retry counters through gateway metrics/status paths.
  - docs/USERLAND_AND_CLI.md - clarify single-console behavior and REST multiplexer operational limits.
Commands:
  - cargo test -p hive-gateway
  - python3 scripts/rest_perf_harness.py --suite status --assert-min-ratio 2.0
Checks:
  - Parallel REST load no longer collapses into avoidable "session pool exhausted" behavior at configured policy limits.
  - No change to console grammar or in-VM listener topology.
Deliverables:
  - Single-console broker implementation with bounded throughput/fairness instrumentation.

Title/ID: m25b-rest-harness-1k
Goal: Extend the benchmark harness and report pipeline for deterministic 1k-worker readiness evidence.
Inputs: scripts/rest_perf_harness.py, docs/BENCHMARKS.md.
Changes:
  - scripts/rest_perf_harness.py - add deterministic seed handling for ramp profiles and worker bands up to 1000.
  - scripts/rest_perf_harness.py - require preflight boot/TCP/auth/LS checks before load execution.
  - scripts/rest_perf_harness.py - harden timeout/error handling so run summaries and per-op stats are always emitted.
  - scripts/rest_perf_harness.py - emit CSV/JSON artifacts consumable by benchmark graphs.
  - docs/BENCHMARKS.md - publish baseline vs aggressive 8..1000 runs, degradation thresholds, and graph-backed interpretation.
Commands:
  - python3 scripts/rest_perf_harness.py --mode simulate --workers-min 8 --workers-max 1000 --duration-mins 5 --seed 2501
  - python3 scripts/rest_perf_harness.py --mode simulate --workers-min 8 --workers-max 1000 --duration-mins 5 --seed 2501 --intensity-min 6 --intensity-max 6
Checks:
  - Both runs emit reproducible summaries and evidence files.
  - Report includes graphs and explicit onset points for buffer pressure and timeout behavior.
Deliverables:
  - 1k-worker readiness benchmark evidence pack and updated benchmark report.

Title/ID: m25b-regression-and-gate-closure
Goal: Repair failing release gates so scale evidence is release-grade.
Inputs: tests/tests/shard_1k.rs, scripts/cohsh/shard_1k.coh, crates/sel4-runtime/src/lib.rs, crates/sel4-sys/src/lib.rs, docs/TEST_PLAN.md, scripts/ci/due_diligence_gate.sh, docs/audit/*.
Changes:
  - tests/tests/shard_1k.rs + scripts/cohsh/shard_1k.coh - align policy queue and shard fixture expectations with current namespace semantics.
  - crates/sel4-runtime/src/lib.rs - add target-aware section annotation handling for macOS Mach-O compatibility.
  - crates/sel4-sys/src/lib.rs - cfg-gate affinity invocation wrapper for kernels lacking affinity labels.
  - docs/TEST_PLAN.md - normalize commands to `python3` and include `cargo test -p tests` plus `cargo test --workspace` gate coverage.
  - docs/audit/findings.csv + docs/audit/BLOCKERS.md + docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md - update dispositions, closure evidence, and independent verification records.
Commands:
  - cargo test -p tests --test shard_1k
  - cargo test --workspace
  - scripts/ci/check_test_plan.sh
  - scripts/ci/due_diligence_gate.sh
Checks:
  - DD-2026-0004, DD-2026-0005, DD-2026-0009, DD-2026-0006, DD-2026-0007, DD-2026-0011, and DD-2026-0012 have closure evidence and pass relevant gates.
  - Due-diligence gate completes with no `P0/P1` blockers open.
Deliverables:
  - Clean gate run logs and updated audit registers proving release-ready closure.
```

## Milestone 25c — Python Orchestration SDK (1k Fleet Playbooks + Host Integrations) <a id="25c"></a>
[Milestones](#Milestones)

**Why now (adoption + operator scale):** 1k-worker readiness is not only transport and gateway throughput; operators also need low-friction automation that plugs into existing Python tooling while preserving Cohesix control semantics and auditability.

**Status:** Complete - Python orchestration APIs, host integration adapters, playbook CLI, documentation updates, and local/G5g validation are complete.

## Goal
Deliver a world-class Python SDK surface that:
1. Keeps Cohesix non-authoritative protocol boundaries intact.
2. Makes 1k-fleet operations simple to integrate from existing Python tooling and CI.
3. Covers high-impact Mac, Jetson, and mixed-fleet use cases with auditable playbooks.
4. Runs deterministically in local `.venv` and Linux (G5g) environments.

## Non-Goals (Explicit)
- No new in-VM listeners, transports, or protocol verbs.
- No bypass of role/ticket/policy gates.
- No behavior drift from `/queen/*/ctl` and `/proc/*` schemas documented in `docs/INTERFACES.md`.
- No changes under `releases/` in this milestone.

## Deliverables
- Python orchestration core with typed schedule/lease/export/approval APIs mapped to existing control files.
- Environment-driven backend selection (`mock`, mounted filesystem, REST gateway, TCP console) with deterministic precedence.
- Host integration adapters for:
  - `systemd` service state,
  - Docker container inventory,
  - Kubernetes pod phase snapshots,
  - NVML/NVIDIA GPU telemetry,
  - PEFT/LoRA runtime package probes.
- Built-in high-impact playbooks covering:
  - 1000 Mac: release factory, private PEFT grid, endpoint compliance.
  - 1000 Jetson: traffic safety mesh, manufacturing safety + QA, critical infrastructure sensing.
  - Mixed fleets: closed-loop AI factory, medical edge AI, logistics digital twin.
- Frictionless CLI entrypoint (`cohesix-playbook`) plus example wrapper script.
- Updated documentation in `tools/cohesix-py/README.md` and `docs/PYTHON_SUPPORT.md`.
- Added Python tests for orchestration APIs, integration adapters, and playbook flows.

## Commands
- `source .venv/bin/activate`
- `python -m pip install -e 'tools/cohesix-py[integrations,ml,dev]'`
- `python -m pytest tools/cohesix-py/tests -q`
- `python -m cohesix.playbook_cli --list`
- `python -m cohesix.playbook_cli --playbook mixed-closed-loop-ai-factory --dry-run --mock`
- `python tools/cohesix-py/examples/use_case_playbook.py --playbook jetson-traffic-safety --dry-run --mock`
- Linux validation (G5g): run the same install/tests/playbook dry-run over SSH on the G5g host defined in `~/cohesix_dev.txt`.

## Checks (DoD)
- Python package installs cleanly into repo `.venv`.
- New tests pass with deterministic output in mock mode.
- Playbook CLI lists all built-in playbooks and executes dry-run reports without control writes.
- Live-compatible control writes remain bounded and map only to canonical control files.
- Host adapters degrade gracefully when optional host dependencies are unavailable.
- Linux validation on G5g passes with the same test suite and playbook dry-run commands.

## Task Breakdown
```
Title/ID: m25c-python-orchestration-core
Goal: Add typed, bounded orchestration APIs for schedule/lease/export/approval workflows.
Inputs: tools/cohesix-py/cohesix/client.py, docs/INTERFACES.md.
Changes:
  - tools/cohesix-py/cohesix/orchestration.py - typed request models, env backend discovery, /proc snapshot reads.
Commands:
  - python -m pytest tools/cohesix-py/tests/test_orchestration.py -q
Checks:
  - Control payloads are validated, bounded, and written only to canonical paths.
Deliverables:
  - Orchestration API module with deterministic tests.

Title/ID: m25c-python-host-integrations
Goal: Integrate systemd, docker, k8s, NVML, and PEFT probes with graceful fallback behavior.
Inputs: docs/HOST_TOOLS.md, docs/PYTHON_SUPPORT.md.
Changes:
  - tools/cohesix-py/cohesix/integrations.py - provider probes + NDJSON rendering for telemetry shipping.
  - tools/cohesix-py/tests/test_integrations.py - probe fallback/parse tests.
Commands:
  - python -m pytest tools/cohesix-py/tests/test_integrations.py -q
Checks:
  - Missing dependencies are reported as skipped/degraded, not hard failures.
Deliverables:
  - Provider adapter module and test coverage.

Title/ID: m25c-python-playbooks
Goal: Ship high-impact built-in playbooks for Mac, Jetson, and mixed 1k-fleet workflows.
Inputs: docs/USE_CASES.md, docs/PYTHON_SUPPORT.md.
Changes:
  - tools/cohesix-py/cohesix/playbooks.py - playbook catalog + execution helpers.
  - tools/cohesix-py/cohesix/playbook_cli.py - frictionless playbook CLI.
  - tools/cohesix-py/examples/use_case_playbook.py - example wrapper.
  - tools/cohesix-py/tests/test_playbooks.py - playbook catalog and execution tests.
Commands:
  - python -m pytest tools/cohesix-py/tests/test_playbooks.py -q
  - python -m cohesix.playbook_cli --list
Checks:
  - All nine use-case playbooks are discoverable and dry-run capable.
Deliverables:
  - Playbook SDK + CLI + tests.

Title/ID: m25c-python-docs-and-linux-validation
Goal: Document world-class Python UX and validate on local + G5g Linux.
Inputs: tools/cohesix-py/README.md, docs/PYTHON_SUPPORT.md, ~/cohesix_dev.txt.
Changes:
  - tools/cohesix-py/README.md - install/CLI/playbook UX updates.
  - docs/PYTHON_SUPPORT.md - orchestration + integration + playbook docs.
Commands:
  - source .venv/bin/activate
  - python -m pip install -e 'tools/cohesix-py[integrations,ml,dev]'
  - python -m pytest tools/cohesix-py/tests -q
  - ssh -i ~/.ssh/cohesix-cuda-builder.pem ubuntu@34.221.49.64 '<linux test commands>'
Checks:
  - Local and G5g test runs both pass; docs match as-built behavior.
Deliverables:
  - Updated docs + Linux validation evidence.
```

## Milestone 25d — REST Request-Auth Parity Across Host Tools (Gateway Capability Max) <a id="25d"></a>
[Milestones](#Milestones)

**Why now (gateway readiness at 1k scale):** `hive-gateway` now enforces request-auth at the REST edge for mutating paths. Any host tool that can route through REST must attach request-auth consistently or it becomes a brittle outlier in multi-tool, high-concurrency deployments.

**Status:** Complete - request-auth parity is implemented across REST-capable host tools, `coh mount` REST correctness fixes are landed, request-auth regression coverage is in place, and TCP console auth deadline behavior has been reviewed/aligned for remote tunnel usage.

## Goal
Deliver request-auth parity across all REST-capable host tools so gateway mode is predictable, secure, and low-friction:
1. Standardize token resolution order (CLI override + canonical env fallbacks).
2. Attach request-auth headers from every REST-capable host client path (including parallel fan-out paths).
3. Preserve single-console architecture (`hive-gateway` remains the only TCP console client in multiplexed mode).
4. Keep docs aligned with as-built CLI/env behavior.

## Non-Goals (Explicit)
- No new REST routes, RPC semantics, or control-plane verbs.
- No changes to in-VM console grammar (`ACK/ERR/END`) or NineDoor behavior.
- No changes to ticket authorization semantics.
- No changes under `releases/` in this milestone.

## Implementation Touchpoints
- `apps/coh/src/main.rs`, `apps/coh/src/rest.rs` - add REST request-auth token CLI/env resolution and attach headers for all REST access paths.
- `apps/coh/src/mount.rs` - ensure FUSE mount is a faithful projection of `LS`/`CAT`/`ECHO` semantics (non-zero file sizes, append-only offset discipline, and limited dynamic telemetry roots aligned with Milestone 21a).
- `apps/root-task/src/net/mod.rs`, `apps/root-task/src/net/console_srv.rs` - adjust TCP console unauthenticated auth deadlines to support remote tunnel usage without changing console grammar.
- `apps/cas-tool/src/main.rs` - add REST request-auth support for CAS uploads through gateway.
- `apps/gpu-bridge-host/src/main.rs` - add REST request-auth support for `/gpu/bridge/ctl` publish mode.
- `apps/swarmui/src-tauri/main.rs`, `apps/swarmui/src/lib.rs`, `apps/swarmui/src/hive.rs` - propagate request-auth token through REST transport bootstrap and parallel telemetry/status polling paths.
- `docs/HOST_TOOLS.md`, `docs/USERLAND_AND_CLI.md` - document canonical REST request-auth flags/env fallbacks for host operators.

## Commands
- `cargo test -p coh`
- `cargo test -p cas-tool`
- `cargo test -p gpu-bridge-host`
- `cargo test -p swarmui`
- `cargo check -p swarmui`

## Checks (DoD)
- All REST-capable host tools attach request-auth when provided via CLI/env.
- No REST-capable host tool requires ad-hoc or undocumented env variables.
- SwarmUI REST parallel fan-out paths (status + telemetry) preserve request-auth headers.
- Gateway-mode docs list the exact token flags/env keys operators must set.
- Existing TCP/mock flows remain unchanged.

## Task Breakdown
```
Title/ID: m25d-rest-auth-parity-host-tools
Goal: Ensure every REST-capable host tool resolves and attaches gateway request-auth headers.
Inputs: apps/coh/src/main.rs, apps/coh/src/rest.rs, apps/cas-tool/src/main.rs, apps/gpu-bridge-host/src/main.rs.
Changes:
  - apps/coh/src/main.rs, apps/coh/src/rest.rs - add `--rest-auth-token` support and env fallback resolution for REST mode.
  - apps/cas-tool/src/main.rs - add `--rest-auth-token` and env fallbacks for REST upload mode.
  - apps/gpu-bridge-host/src/main.rs - add `--rest-auth-token` and env fallbacks for REST publish mode.
Commands:
  - cargo test -p coh
  - cargo test -p cas-tool
  - cargo test -p gpu-bridge-host
Checks:
  - REST clients instantiate `GatewayClient` with request-auth when configured.
Deliverables:
  - Host-tool REST request-auth parity with deterministic fallback order.

Title/ID: m25d-swarmui-rest-auth-fanout
Goal: Propagate request-auth through SwarmUI REST boot and parallel fan-out reads/tails.
Inputs: apps/swarmui/src-tauri/main.rs, apps/swarmui/src/lib.rs, apps/swarmui/src/hive.rs.
Changes:
  - apps/swarmui/src-tauri/main.rs - resolve `SWARMUI_REST_AUTH_TOKEN` + canonical gateway env fallbacks.
  - apps/swarmui/src/lib.rs - carry request-auth token in REST parallel config and pass to all spawned transports.
  - apps/swarmui/src/hive.rs - attach request-auth token in parallel telemetry tail workers.
Commands:
  - cargo test -p swarmui
  - cargo check -p swarmui
Checks:
  - No SwarmUI REST code path creates `CohshRestTransport` without request-auth propagation.
Deliverables:
  - SwarmUI REST transport parity with gateway auth policy.

Title/ID: m25d-docs-as-built-rest-auth
Goal: Keep operator docs fully aligned with REST auth semantics after parity fixes.
Inputs: docs/HOST_TOOLS.md, docs/USERLAND_AND_CLI.md.
Changes:
  - docs/HOST_TOOLS.md - document per-tool `--rest-auth-token` and env fallback behavior.
  - docs/USERLAND_AND_CLI.md - reflect gateway request-auth requirements for `coh` and `swarmui` REST mode.
Commands:
  - scripts/ci/check_test_plan.sh
Checks:
  - Docs match shipped tool flags and env keys exactly.
Deliverables:
  - Updated host-tool operator guidance with no auth ambiguity.

Title/ID: m25d-coh-mount-rest-writable-and-bidir
Goal: Make `coh mount --rest-url` reliably readable/writable and suitable for safe bidirectional telemetry transfer (Milestone 21a semantics).
Inputs: apps/coh/src/mount.rs, apps/coh/src/main.rs, apps/coh/src/rest.rs, docs/OPERATOR_WALKTHROUGH.md, docs/TEST_PLAN.md.
Changes:
  - apps/coh/src/mount.rs - fix FUSE file attrs (non-zero file sizes), ensure `readdir` reports accurate directory vs file types (macOS uses the type eagerly), eliminate write-only “ghost” entries, allow host-side appends by avoiding POSIX-perm EPERM (kernel-level checks), and support limited dynamic telemetry roots under `coh.telemetry.root` without introducing generic POSIX create semantics.
  - apps/coh/src/main.rs, apps/coh/src/rest.rs - ensure REST request-auth token is accepted and attached for mount-backed `ECHO` paths so gateway mode stays writable when request-auth is enforced.
  - docs/OPERATOR_WALKTHROUGH.md - document FUSE mount prerequisites and a bidirectional telemetry transfer smoke using supported MIME types.
  - docs/TEST_PLAN.md - add a gateway-mode mount regression: REST mount can `cat` `/proc/*` and append to `/queen/telemetry/<device>/ctl` with request-auth enabled.
Commands:
  - cargo test -p coh
Checks:
  - REST mount `cat` shows non-empty content for bounded files like `/proc/lifecycle/state` and `/log/queen.log`.
  - REST mount can create a new telemetry device root by addressing `${coh.telemetry.root}/<device>/ctl`, then append records into the OS-named segment, and another host can read them via mount.
  - macOS REST mounts can append to `/queen/telemetry/<device>/ctl` without local EPERM/permission denials (errors must come from Cohesix bounds/policy, not host file mode bits).
  - No new generic create/mkdir/unlink/rename semantics are introduced; dynamic behavior is strictly scoped to telemetry ingest roots and remains OS-owned.
Deliverables:
  - Writable, bounded REST mount suitable for multi-host gateway ops with deterministic, policy-aligned behavior.

Title/ID: m25d-console-auth-timeout-remote-tcp-mount
Goal: Make remote TCP console attachments (and `coh mount --host/--port`) reliable over SSH tunnels/high-latency links without changing console grammar.
Inputs: apps/root-task/src/net/mod.rs, apps/root-task/src/net/console_srv.rs, apps/cohsh/src/transport/tcp.rs, docs/HOST_TOOLS.md, docs/OPERATOR_WALKTHROUGH.md.
Changes:
  - apps/root-task/src/net/mod.rs, apps/root-task/src/net/console_srv.rs - increase unauthenticated auth deadline (or implement a bounded auth window) so a client can deliver its token over WAN latency.
  - apps/cohsh/src/transport/tcp.rs - keep client-side retry/backoff bounded and emit a crisp error that points operators at gateway mode when auth times out.
  - docs/HOST_TOOLS.md, docs/OPERATOR_WALKTHROUGH.md - document that REST via `hive-gateway` is the default for remote multi-host ops; TCP console is best-effort over WAN until this is fixed.
Commands:
  - scripts/cohesix-build-run.sh --transport tcp --tcp-port 31337
  - (remote) cohsh/coh attach or mount over an SSH reverse tunnel
Checks:
  - g5g (AWS) can attach/mount over a reverse tunnel without `authentication timed out waiting for server response`.
Deliverables:
  - Remote-safe TCP auth window with unchanged console wire grammar and updated operator guidance.

Title/ID: m25d-macos-coh-fuse-default
Goal: Ship macOS `coh` builds with FUSE enabled so operators can use `coh mount` once MacFUSE is installed/approved.
Inputs: scripts/cohesix-build-run.sh, docs/HOST_TOOLS.md, docs/OPERATOR_WALKTHROUGH.md, docs/TEST_PLAN.md.
Changes:
  - scripts/cohesix-build-run.sh - build `coh` with `--features fuse` on macOS hosts.
  - docs/HOST_TOOLS.md, docs/OPERATOR_WALKTHROUGH.md, docs/TEST_PLAN.md - document MacFUSE prerequisites (`/dev/macfuse0`) and keep examples as-built.
Commands:
  - cargo build -p coh --release --features fuse
Checks:
  - macOS `coh mount` no longer reports “fuse support disabled”.
Deliverables:
  - macOS-ready `coh mount` binary and aligned operator docs.
```

----
**Release 0.8.0 alpha**
----

---

## Milestone 25e — Evidence Packs + Integration Kits (Audit-First Adoption) <a id="25e"></a>
[Milestones](#Milestones)

**Why now (buyability + integration):** Cohesix has a scale-capable, request-authenticated gateway path (25b–25d) and Python orchestration (25c). The highest leverage remaining adoption blocker is not new VM semantics; it is the absence of deterministic, auditor-friendly evidence artifacts and turnkey integration patterns that reuse existing control surfaces without introducing new protocols.

**Status:** Complete — full `docs/TEST_PLAN.md` pass (2026-02-16) and docs-as-built review complete.

## Goal
Deliver high-impact, low-risk adoption accelerators that remain host-side and strictly protocol-faithful:
1. Deterministic evidence packs for audits, due diligence, and incident review, sourced only from existing `/proc`, `/log`, `/audit`, `/replay`, and telemetry surfaces.
2. A correlated timeline view derived from evidence pack contents (no new runtime behavior).
3. Turnkey integration kits (CI + SIEM) that consume the REST/OpenAPI gateway and/or `coh`/Python tooling without introducing new control-plane semantics.
4. GPU lease receipts and chargeback-friendly exports derived from `/proc/lease/*`, `/audit/journal`, and `/gpu/*` status files (no changes to lease enforcement).

## Non-Goals (Explicit)
- No new in-VM listeners, protocols, transports, or control verbs.
- No changes to ACK/ERR/END grammar, NineDoor error semantics, or Secure9P bounds.
- No device identity enrollment or attestation work (covered by Milestone 26 and Milestone 28 `coh attest`).
- No changes under `releases/` in this milestone.

## Implementation Touchpoints
- `apps/coh/src/main.rs` + `apps/coh/src/lib.rs` — add evidence subcommands and shared helpers.
- `apps/coh/src/telemetry.rs` — reuse bounded telemetry pull implementation inside evidence packs.
- `crates/cohesix-rest/src/lib.rs` + `apps/hive-gateway` — consume `GET /v1/meta/bounds` and REST file projections for evidence reads.
- `apps/nine-door/src/host/audit.rs` + `apps/root-task/src/ninedoor.rs` — ensure evidence pack includes `/audit/export` and correlatable audit journal entries (read-only).
- `docs/HOST_TOOLS.md`, `docs/HOST_API.md`, `docs/OPERATOR_WALKTHROUGH.md` — document evidence pack CLI and integration recipes.

## Commands
- `cargo test -p coh`
- `cargo test -p hive-gateway`
- `python -m pytest tools/cohesix-py/tests -q`

## Checks (Definition of Done)
- `coh evidence pack` succeeds in `--mock` mode and in REST gateway mode with request-auth enabled, emitting a deterministic directory structure under `out/evidence/<id>/`.
- Evidence packs contain: manifest/policy fingerprint, bounds snapshot, `/proc` snapshots, `/replay/status`, a bounded `/log/queen.log` capture, and (optionally) downloaded telemetry segments. When `/audit/export` is present, packs include `/audit/export` plus redacted `/audit/journal` and `/audit/decisions`.
- `coh evidence timeline` produces stable, correlated output from an evidence pack without network access.
- GPU lease receipts can be emitted from `coh gpu lease` / `coh run` without changing VM control semantics, and include correlatable identifiers (`lease id`, `subject`, `resource`, `seq`) captured from `/proc/lease/*`.
- Integration kits run in mock mode end-to-end and contain no hardcoded secrets; all tokens/URLs are supplied via env vars.

## Task Breakdown
```
Title/ID: m25e-coh-evidence-pack
Goal: Add a deterministic evidence pack exporter that captures bounded, auditor-friendly system state without new semantics.
Inputs: apps/coh/src/main.rs, apps/coh/src/lib.rs, apps/coh/src/telemetry.rs, crates/cohesix-rest/src/lib.rs, docs/INTERFACES.md.
Changes:
  - apps/coh/src/main.rs - add `coh evidence pack` subcommand (supports `--mock`, `--rest-url`, and TCP console flows).
  - apps/coh/src/lib.rs + apps/coh/src/evidence.rs - implement bounded evidence reads for canonical nodes (`/proc/*`, `/log/queen.log`, `/audit/*`, `/replay/*`, `/proc/lease/*`, `/proc/schedule/*`).
  - apps/coh/src/evidence.rs - include manifest fingerprint + gateway bounds snapshot (`GET /v1/meta/bounds`) to bind evidence packs to a concrete as-built contract.
  - apps/coh/src/evidence.rs - optionally reuse `telemetry::pull` to download telemetry segments into the pack under manifest bounds.
  - docs/HOST_TOOLS.md + docs/OPERATOR_WALKTHROUGH.md - document evidence pack usage and output layout.
Commands:
  - cargo test -p coh
  - cargo run -p coh -- --mock evidence pack --out out/evidence/mock
Checks:
  - Pack includes all required nodes and never reads past manifest-bounded byte caps.
  - Pack output is stable for identical inputs (deterministic file names + ordering).
Deliverables:
  - `coh evidence pack` implementation + docs.

Title/ID: m25e-coh-evidence-timeline
Goal: Produce a correlated timeline view from evidence packs for incident review and audit traceability.
Inputs: apps/coh/src/evidence.rs (from m25e-coh-evidence-pack), apps/nine-door/src/host/audit.rs (journal format reference), docs/audit/CONTROL_TRACEABILITY.md.
Changes:
  - apps/coh/src/main.rs - add `coh evidence timeline --in <pack-dir>` that emits `timeline.ndjson` and `timeline.md`.
  - apps/coh/src/evidence_timeline.rs - parse `/audit/journal` JSONL + `/audit/decisions`, correlate with `/proc/lease/*` snapshots and `seq` fields, and emit stable, bounded output.
  - apps/coh/tests/evidence_timeline.rs - add fixture-driven tests ensuring stable ordering and robust handling of partial packs.
Commands:
  - cargo test -p coh
Checks:
  - Timeline output is deterministic for a fixed evidence pack and does not require network access.
Deliverables:
  - Timeline generator suitable for postmortems and due diligence artifacts.

Title/ID: m25e-integration-kits-ci-siem
Goal: Ship turnkey, protocol-faithful integration kits that reduce adoption friction in CI and SIEM pipelines.
Inputs: docs/HOST_API.md (OpenAPI), docs/HOST_TOOLS.md, tools/cohesix-py.
Changes:
  - docs/HOST_TOOLS.md - add a dedicated section: CI usage (generate evidence pack, upload artifacts) and SIEM export patterns (audit journal + decisions + lease snapshots).
  - tools/cohesix-py/examples/ci_evidence_pack.py - run `coh evidence pack` (or REST equivalents) in mock/dry-run, validate output layout, emit a machine-readable summary JSON.
  - tools/cohesix-py/examples/siem_export_ndjson.py - read an evidence pack and emit normalized NDJSON suitable for Splunk/Elastic ingestion (no network by default; optional `--post` with env-configured URL/token).
  - tools/cohesix-py/tests/test_examples_ci_siem.py - ensure examples run deterministically in mock mode and never require external connectivity.
Commands:
  - python -m pytest tools/cohesix-py/tests -q
Checks:
  - Examples run in mock mode on macOS and produce stable outputs with no secrets in logs.
Deliverables:
  - Integration kits that demonstrate “buyable” workflows without new control semantics.

Title/ID: m25e-gpu-lease-receipts
Goal: Add receipt-backed GPU leasing outputs for audit and chargeback without changing lease enforcement.
Inputs: apps/coh/src/gpu.rs, apps/coh/src/run.rs, docs/INTERFACES.md.
Changes:
  - apps/coh/src/gpu.rs - add optional `--receipt-out` that writes a JSON receipt including request parameters, ACK line, and a bounded `/proc/lease/*` snapshot captured immediately after the lease request.
  - apps/coh/src/run.rs - extend host command wrapper to emit receipts for lease-validated runs (breadcrumb correlation).
  - apps/coh/tests/receipts.rs - validate receipt schema, bounds, and determinism.
Commands:
  - cargo test -p coh
Checks:
  - Receipts never include secrets (tokens, raw tickets) and are stable for fixed inputs.
Deliverables:
  - Receipt artifacts suitable for audit and internal billing pipelines.
```

---

## Milestone 25f — Gateway Broker Refactor + Large Telemetry Reference Manifests (No-Retry Reliability Gate) <a id="25f"></a>
[Milestones](#Milestones)

**Why now (beta reliability + telemetry realism):** Strict no-retry gateway runs still surface backpressure and timeout failures under accelerated mixed control/telemetry load. The gateway currently multiplexes logical pool sessions over one locked TCP transport, and current harness coverage does not explicitly pressure MB/GB telemetry scenarios. This milestone addresses the root cause while preserving Cohesix red lines and single-console architecture.

## Goal
Deliver a Cohesix-aligned reliability and scale step that:
1. Replaces lock-contention request handling in `hive-gateway` with a bounded broker model (single wire owner, concurrent REST callers).
2. Allows many chunk references per telemetry manifest so MB/GB-class host artifacts can be represented without turning telemetry ingest into generic file transfer.
3. Expands the performance harness to explicit `1 MB`, `10 MB`, `100 MB`, and `1 GB` gateway scenarios with `--no-retries`, fast ramp, and strict error-budget gating.

## Non-Goals (Explicit)
- No new in-VM TCP listeners, RPC channels, or ad-hoc host/VM protocols.
- No change to ACK/ERR/END grammar, NineDoor error semantics, or Secure9P red lines (`msize <= 8192`, walk depth <= 8).
- No generic POSIX create/upload semantics under telemetry paths; segment naming/retention remains OS-owned and bounded.
- No hidden retry paths in benchmark mode; failures must remain visible and count against pass/fail.

## Implementation Touchpoints
- `apps/hive-gateway/src/main.rs` - brokerized request scheduler/dispatcher, bounded queueing, fairness between control and telemetry classes, and queue/backpressure observability.
- `apps/cohsh/src/transport/tcp.rs`, `apps/cohsh/src/session_pool.rs` - align transport/session abstractions with gateway broker ownership (single wire writer, no concurrent socket mutation).
- `tools/coh-rtc/src/ir.rs`, `configs/root_task.toml`, generated policy artifacts - introduce bounded telemetry reference-manifest limits (count/bytes) as manifest-driven controls.
- `apps/nine-door/src/host/telemetry.rs`, `apps/root-task/src/ninedoor.rs` - validate and enforce reference-manifest envelopes and quotas deterministically.
- `apps/cohsh/src/lib.rs`, `apps/coh/src/telemetry.rs`, `tools/cohesix-py` - support telemetry push/pull workflows that emit and consume chunk-reference manifests under manifest limits.
- `scripts/rest_perf_harness.py`, `tests/test_rest_perf_harness.py` - add explicit large-size scenario matrix, no-retry mode, fast-ramp presets, and strict error-budget exit behavior.
- `docs/INTERFACES.md`, `docs/HOST_TOOLS.md`, `docs/TEST_PLAN.md` - document as-built manifest-reference envelope semantics and benchmark gates.

## Commands
- `cargo test -p hive-gateway`
- `cargo test -p cohsh`
- `cargo test -p nine-door`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
- `.venv/bin/python -m pytest -q tests/test_rest_perf_harness.py`
- `python scripts/rest_perf_harness.py simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-1mb --error-budget-rate 0.01`
- `python scripts/rest_perf_harness.py simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-10mb --error-budget-rate 0.01`
- `python scripts/rest_perf_harness.py simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-100mb --error-budget-rate 0.01`
- `python scripts/rest_perf_harness.py simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-1gb --error-budget-rate 0.01`

## Checks (Definition of Done)
- Gateway REST handlers use a bounded broker queue/dispatcher instead of per-request lock contention on shared transport state; control-path latency remains protected under telemetry pressure.
- Backpressure is explicit and deterministic (`429` / gateway backpressure path), with queue depth and timeout counters exposed in status telemetry.
- Telemetry reference manifests accept many bounded chunk references per segment (manifest-driven limits), reject malformed/oversized entries deterministically, and never relax existing per-record Secure9P bounds.
- Large-file scenarios (`1 MB`, `10 MB`, `100 MB`, `1 GB`) run through gateway mode with `--no-retries` and fast ramp; heavy-ish beta gate is `error_rate <= 1.0%` (`--error-budget-rate 0.01`) and harness exits non-zero on violation.
- `docs/TEST_PLAN.md` includes mandatory commands and pass criteria for all four scenarios (no skip path).

## Task Breakdown
```
Title/ID: m25f-gateway-broker-refactor
Goal: Replace lock-contention transport access with a bounded broker model while preserving single-console wire ownership.
Inputs: apps/hive-gateway/src/main.rs, apps/cohsh/src/session_pool.rs, apps/cohsh/src/transport/tcp.rs, docs/HOST_TOOLS.md.
Changes:
  - apps/hive-gateway/src/main.rs - add broker request queues (control + telemetry), deterministic scheduling/fairness, and queue-aware backpressure responses.
  - apps/hive-gateway/src/main.rs - keep `hive-gateway` as sole console client; remove direct per-request competition on shared transport mutex.
  - apps/cohsh/src/session_pool.rs, apps/cohsh/src/transport/tcp.rs - align lease/session lifecycle with broker ownership and bounded shutdown/reconnect behavior.
Commands:
  - cargo test -p hive-gateway
  - cargo test -p cohsh
Checks:
  - Under concurrent REST load, checkout timeout/backpressure rates fall materially from baseline and control-plane operations remain serviceable.
Deliverables:
  - Brokerized gateway transport path with deterministic backpressure telemetry.

Title/ID: m25f-telemetry-manifest-chunk-references
Goal: Support MB/GB telemetry artifacts via many bounded chunk references rather than bulk inline payload transfer.
Inputs: tools/coh-rtc/src/ir.rs, configs/root_task.toml, apps/nine-door/src/host/telemetry.rs, apps/root-task/src/ninedoor.rs, docs/INTERFACES.md.
Changes:
  - tools/coh-rtc/src/ir.rs + configs/root_task.toml - add manifest-driven limits for telemetry reference manifests (max refs, max manifest bytes, max referenced bytes scope as required).
  - apps/nine-door/src/host/telemetry.rs + apps/root-task/src/ninedoor.rs - enforce deterministic validation for reference-manifest envelopes and bounded append semantics.
  - apps/cohsh/src/lib.rs + apps/coh/src/telemetry.rs - add/extend host tooling for emitting reference manifests and resolving latest segment IDs without new control verbs.
  - docs/INTERFACES.md - document reference-manifest schema and limits as-built.
Commands:
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json
  - cargo test -p nine-door
  - cargo test -p cohsh
Checks:
  - Valid manifests append successfully; malformed and over-limit manifests fail with deterministic ERR details.
Deliverables:
  - Manifest-reference telemetry ingest path suitable for real-world artifact sizes while preserving Cohesix control-plane boundaries.

Title/ID: m25f-harness-large-scenarios-no-retry
Goal: Make gateway reliability regressions visible with explicit no-retry large-size scenario gates.
Inputs: scripts/rest_perf_harness.py, tests/test_rest_perf_harness.py, docs/TEST_PLAN.md.
Changes:
  - scripts/rest_perf_harness.py - add explicit scenario presets (`telemetry-1mb`, `telemetry-10mb`, `telemetry-100mb`, `telemetry-1gb`) routed through gateway paths.
  - scripts/rest_perf_harness.py - enforce `--no-retries` mode, fast-ramp controls, and strict `--error-budget-rate` pass/fail behavior.
  - tests/test_rest_perf_harness.py - cover scenario wiring, no-retry semantics, and error-budget failure behavior.
  - docs/TEST_PLAN.md - add canonical commands and required outputs for all four scenarios.
Commands:
  - .venv/bin/python -m pytest -q tests/test_rest_perf_harness.py
  - python scripts/rest_perf_harness.py simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-1mb --error-budget-rate 0.01
  - python scripts/rest_perf_harness.py simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-10mb --error-budget-rate 0.01
  - python scripts/rest_perf_harness.py simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-100mb --error-budget-rate 0.01
  - python scripts/rest_perf_harness.py simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-1gb --error-budget-rate 0.01
Checks:
  - Each scenario emits deterministic summary artifacts and fails hard (exit code non-zero) when error rate exceeds budget.
Deliverables:
  - A repeatable large-object gateway reliability gate with no hidden retries.

Title/ID: m25f-docs-sync-and-operator-guidance
Goal: Keep docs-as-built alignment for broker behavior, large telemetry references, and no-retry gates.
Inputs: docs/INTERFACES.md, docs/HOST_TOOLS.md, docs/TEST_PLAN.md, docs/ARCHITECTURE.md.
Changes:
  - docs/INTERFACES.md - describe telemetry reference-manifest envelopes and manifest-driven limits.
  - docs/HOST_TOOLS.md - document gateway broker behavior, expected backpressure signals, and operator tuning knobs.
  - docs/TEST_PLAN.md - codify the mandatory 1MB/10MB/100MB/1GB no-retry fast-ramp matrix and pass/fail policy.
  - docs/ARCHITECTURE.md - update transport implementation notes to reflect brokerized gateway path while preserving single-console architecture.
Commands:
  - scripts/ci/check_test_plan.sh
Checks:
  - Documentation matches implemented flags, limits, and failure semantics exactly.
Deliverables:
  - Canonical operator and test documentation for Milestone 25f.
```

---

## Milestone 25g — Host Control Tickets via FUSE (GPU/PEFT + systemd/docker + K8s Coexistence) <a id="25g"></a>
[Milestones](#Milestones)

**Why now (high-value orchestration):** Cohesix already has bounded control surfaces for `/gpu/*`, `/host/*`, policy gates, and evidence packs, but host actions still require tool-specific flows. The highest-leverage next step is a unified, auditable host execution queue where Queen emits bounded JSON control tickets and host executors consume them through mounted file views without adding new protocols.

## Goal
Deliver a deterministic, policy-gated host control-ticket plane that:
1. Defines a manifest-driven ticket contract with idempotency and lifecycle states.
2. Adds a host ticket agent that watches mounted ticket files and executes bounded adapters.
3. Prioritizes high-value adapters: GPU lease/PEFT lifecycle, systemd remediation, Docker remediation, and Kubernetes coexistence translation.
4. Extends evidence/replay so every ticket decision is traceable and reproducible.

## Non-Goals (Explicit)
- No new in-VM TCP listeners, RPC channels, or host/VM sideband protocols.
- No change to ACK/ERR/END grammar, NineDoor error semantics, or Secure9P bounds.
- No in-VM CUDA/NVML, no in-VM container runtime, and no bypass of role/ticket authorization.
- No unbounded queueing, retries, or background mutation loops.

## Design Principles (Normative)
1. **Spec/status split** - ticket requests and ticket outcomes are append-only, separate streams.
2. **At-least-once + idempotency** - executors may re-read; outcomes are deduplicated by `id` + `idempotency_key`.
3. **Policy-first execution** - Queen gating and ticket scopes remain authoritative before host side effects.
4. **Replayability** - ticket state transitions are deterministic and captured by existing audit/evidence paths.

## Implementation Touchpoints
- `tools/coh-rtc/src/ir.rs`, `configs/root_task.toml`, generated artifacts - emit ticket schema/bounds (`host-ticket/v1`) and allowlisted action kinds.
- `apps/nine-door/src/host/*`, `apps/root-task/src/ninedoor.rs` - expose bounded ticket files under `/host/tickets/*` with append-only semantics.
- `apps/host-ticket-agent/*` (new host tool) - watch mounted ticket streams, claim work, execute adapters, and append status receipts.
- `apps/gpu-bridge-host/*`, `apps/coh/src/peft/*` - GPU/PEFT ticket executors.
- `apps/host-sidecar-bridge/*` - systemd/docker/K8s adapter execution and status projection.
- `tools/cohesix-py/*` - optional RBAC-to-ticket translation helpers for Kubernetes coexistence workflows.
- `docs/INTERFACES.md`, `docs/HOST_TOOLS.md`, `docs/USERLAND_AND_CLI.md`, `docs/USE_CASES.md`, `docs/TEST_PLAN.md`, `docs/ARCHITECTURE.md` - docs-as-built and operator/test alignment.

## Commands
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
- `scripts/check-generated.sh`
- `cargo test -p nine-door`
- `cargo test -p host-sidecar-bridge`
- `cargo test -p gpu-bridge-host`
- `cargo test -p coh --features mock`
- `cargo test -p tests --test shard_1k`
- `python3 -m pytest tools/cohesix-py/tests -q`
- `scripts/ci/check_test_plan.sh`

## Checks (Definition of Done)
- Ticket namespace is manifest-gated and bounded, with deterministic state transitions:
  - `queued` -> `claimed` -> `running` -> `succeeded|failed|expired`.
- Host ticket agent executes only allowlisted actions and writes bounded status receipts without leaking auth tokens or raw signing secrets.
- GPU/PEFT tickets drive `lease`, `import`, `activate`, and `rollback` flows using existing `/gpu/*` and `/queen/export/*` semantics.
- systemd/docker tickets perform bounded remediation (restart/stop/status verify) through host adapters and emit correlatable outcomes.
- Kubernetes coexistence flow translates RBAC-scoped intents into Cohesix tickets without introducing alternative control-plane semantics.
- Evidence packs and timeline tooling include ticket request/outcome correlation so incident replay and chargeback are deterministic.
- `docs/TEST_PLAN.md` includes mandatory ticket-flow runs (GPU/PEFT, systemd, docker, K8s coexistence, replay/evidence) with no skip path.

## Task Breakdown
```
Title/ID: m25g-ticket-ir-and-namespace
Goal: Define host control-ticket schema/bounds and expose append-only ticket streams in the namespace.
Inputs: tools/coh-rtc/src/ir.rs, configs/root_task.toml, apps/nine-door/src/host, apps/root-task/src/ninedoor.rs, docs/INTERFACES.md.
Changes:
  - tools/coh-rtc/src/ir.rs + configs/root_task.toml - add `host-ticket/v1` schema, action allowlist, byte caps, and lifecycle enums.
  - apps/nine-door/src/host/tickets.rs + apps/root-task/src/ninedoor.rs - add `/host/tickets/spec`, `/host/tickets/status`, `/host/tickets/deadletter`, and bounded snapshot nodes.
  - docs/INTERFACES.md - document ticket file paths, schema, and deterministic failure semantics.
Commands:
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json
  - scripts/check-generated.sh
  - cargo test -p nine-door
Checks:
  - Valid ticket lines append successfully; malformed/over-limit lines fail deterministically.
Deliverables:
  - Manifest-driven host ticket contract with generated artifacts and interface docs.

Title/ID: m25g-host-ticket-agent-core
Goal: Add a host-only ticket agent that watches mounted ticket streams and executes bounded work with idempotent claims.
Inputs: apps/coh/src/mount.rs, apps/host-ticket-agent (new), docs/HOST_TOOLS.md, docs/ARCHITECTURE.md.
Changes:
  - apps/host-ticket-agent/src/main.rs - tail/watch loop for `/host/tickets/spec` with deterministic cursor resume.
  - apps/host-ticket-agent/src/claim.rs - claim/idempotency handling keyed by `id` + `idempotency_key`.
  - apps/host-ticket-agent/src/status.rs - append bounded `host-ticket-result/v1` receipts to `/host/tickets/status`.
  - docs/HOST_TOOLS.md + docs/ARCHITECTURE.md - document runtime model, failure handling, and single-console constraints.
Commands:
  - cargo test -p host-ticket-agent
Checks:
  - Agent survives watcher interruptions, resumes from cursor, and avoids duplicate side effects for repeated tickets.
Deliverables:
  - Host ticket execution agent with deterministic claim/execute/report lifecycle.

Title/ID: m25g-gpu-peft-ticket-executors
Goal: Implement high-value GPU lease and PEFT lifecycle ticket actions.
Inputs: apps/gpu-bridge-host, apps/coh/src/peft, docs/GPU_NODES.md, docs/USE_CASES.md.
Changes:
  - apps/host-ticket-agent/src/executors/gpu.rs - execute `gpu.lease.grant|renew|release` and verify via `/gpu/<id>/lease`.
  - apps/host-ticket-agent/src/executors/peft.rs - execute `peft.import|activate|rollback` using existing host registry and `/gpu/models/*` flows.
  - docs/GPU_NODES.md + docs/USE_CASES.md - document ticket-driven GPU/PEFT orchestration for multi-tenant edge and model governance use cases.
Commands:
  - cargo test -p gpu-bridge-host
  - cargo test -p coh --features mock
Checks:
  - Ticketed GPU/PEFT flows are auditable, bounded, and preserve existing lease/model semantics.
Deliverables:
  - Ticket executors for the highest-value GPU/PEFT control loops.

Title/ID: m25g-systemd-docker-remediation
Goal: Add deterministic systemd/docker remediation ticket actions through host-side adapters.
Inputs: apps/host-sidecar-bridge, apps/host-ticket-agent, docs/HOST_TOOLS.md, docs/INTERFACES.md.
Changes:
  - apps/host-ticket-agent/src/executors/systemd.rs - support allowlisted `start|stop|restart|status-check` unit actions.
  - apps/host-ticket-agent/src/executors/docker.rs - support allowlisted container restart/stop/status actions with bounded output capture.
  - apps/host-sidecar-bridge/src/providers/* - normalize status verification for post-action receipts.
  - docs/HOST_TOOLS.md + docs/INTERFACES.md - document remediation action schemas and safeguards.
Commands:
  - cargo test -p host-sidecar-bridge
  - cargo test -p host-ticket-agent
Checks:
  - Remediation actions emit deterministic success/failure receipts and never execute non-allowlisted commands.
Deliverables:
  - Policy-bounded systemd/docker ticket remediation path.

Title/ID: m25g-k8s-rbac-ticket-translation
Goal: Preserve Kubernetes coexistence by translating RBAC-scoped intents into Cohesix host tickets.
Inputs: tools/cohesix-py, apps/host-ticket-agent, docs/USE_CASES.md, docs/PYTHON_SUPPORT.md.
Changes:
  - tools/cohesix-py/cohesix/orchestrator.py - add optional RBAC-to-ticket translation helpers for cordon/drain/lease workflows.
  - apps/host-ticket-agent/src/executors/k8s.rs - execute allowlisted K8s actions and emit bounded receipts.
  - docs/USE_CASES.md + docs/PYTHON_SUPPORT.md - document coexistence constraints, identity mapping, and ticket translation flow.
Commands:
  - python3 -m pytest tools/cohesix-py/tests -q
  - cargo test -p host-ticket-agent
Checks:
  - K8s coexistence remains out-of-band governance (no scheduler replacement) with deterministic ticket/audit linkage.
Deliverables:
  - RBAC-scoped K8s coexistence adapter over ticketed control semantics.

Title/ID: m25g-evidence-replay-and-testplan
Goal: Extend audit/evidence/replay and codify mandatory ticket-flow validation in the Test Plan.
Inputs: apps/coh/src/evidence.rs, apps/coh/src/evidence_timeline.rs, docs/TEST_PLAN.md, docs/SECURITY.md, docs/USERLAND_AND_CLI.md.
Changes:
  - apps/coh/src/evidence.rs + apps/coh/src/evidence_timeline.rs - correlate ticket `id`/`idempotency_key` across spec/status/audit/lease artifacts.
  - docs/TEST_PLAN.md - add mandatory control-ticket matrix: GPU/PEFT, systemd remediation, docker remediation, K8s coexistence translation, and replay/timeline validation.
  - docs/SECURITY.md + docs/USERLAND_AND_CLI.md - document token redaction, idempotency semantics, and operator procedures.
Commands:
  - cargo test -p coh
  - scripts/ci/check_test_plan.sh
Checks:
  - Evidence packs and timelines are deterministic and include ticket correlation for all mandatory ticket classes.
Deliverables:
  - Canonical docs and regression requirements for Milestone 25g ticketed orchestration.
```

---

## Milestone 25h — Multi-Hive Federation via Ticket Relay (Single-Writer Preserved, 10x1k Fleet Pattern) <a id="25h"></a>
[Milestones](#Milestones)

**Why now (fleet reality):** `docs/BENCHMARKS.md` validates a 1500 hard cap, with aggressive-profile reliability pressure around `~1000-1200` workers per hive due to gateway backpressure. The practical scale path is federation of many bounded hives, not active/active writes to one logical hive. `docs/FAILOVER.md` already codifies single-writer active/standby and host-orchestrated cutover; this milestone extends that model for interoperable multi-hive operations.

## Goal
Deliver a deterministic multi-hive interoperability layer that:
1. Scales operational control across many independent hives using host-side ticket relay.
2. Preserves single-writer semantics per hive and explicit split-brain fencing.
3. Enables pragmatic "10 queens -> 10,000 workers" orchestration patterns without adding new in-VM protocols.
4. Produces replayable, correlated evidence for cross-hive intents and outcomes.

## Non-Goals (Explicit)
- No active/active multi-queen writes to one logical hive namespace.
- No built-in cross-queen state replication or consensus protocol.
- No new in-VM TCP listeners, RPC channels, or shared-memory authority paths.
- No relaxation of Secure9P limits, ACK/ERR/END grammar, or existing policy gates.
- No dependence on REST-backed FUSE appends for mutation paths in federated mode; control writes remain REST `/v1/fs/echo`-driven.

## Design Principles (Normative)
1. **Hive as authority island** - each hive remains independently authoritative and bounded.
2. **Forward intents, not mutable state** - relay append-only tickets and receipts, never raw state replication.
3. **Idempotent by construction** - cross-hive actions correlate by `id + idempotency_key + source_hive + target_hive`.
4. **Host-owned federation logic** - keep routing, WAL, and retry policy host-side to preserve tiny VM TCB.
5. **Read-many, write-disciplined** - broad fleet reads are allowed; writes are fenced per-hive and explicit.

## Implementation Touchpoints
- `tools/coh-rtc/src/ir.rs`, `configs/root_task.toml`, generated artifacts - add manifest-gated federation config:
  - peer inventory, per-peer auth references, relay queue bounds, allowed cross-hive actions, and WAL bounds.
- `apps/host-ticket-agent/*` and/or new `apps/hive-federation-relay/*` - implement bounded cross-hive relay worker over existing REST gateway paths.
- `apps/hive-gateway/*` - expose relay-safe status counters (queue, dedupe, remote write failures) without adding control semantics.
- `apps/coh/src/evidence.rs`, `apps/coh/src/evidence_timeline.rs` - include cross-hive correlation fields in evidence/timeline exports.
- `scripts/failover_watchdog.py` - integrate federation-aware fencing hooks (pause relay during cutover, resume after health checks).
- `tools/cohesix-py/*` - optional fleet orchestrator helpers for fan-out by shard/hive policy while preserving ticket semantics.
- `docs/FAILOVER.md`, `docs/HOST_TOOLS.md`, `docs/INTERFACES.md`, `docs/ARCHITECTURE.md`, `docs/TEST_PLAN.md`, `docs/USE_CASES.md` - canonical as-built behavior and operator runbooks.

## Commands
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
- `scripts/check-generated.sh`
- `cargo test -p host-ticket-agent`
- `cargo test -p hive-gateway`
- `cargo test -p coh`
- `python3 -m pytest tools/cohesix-py/tests -q`
- `python3 scripts/failover_watchdog.py --help`
- `scripts/ci/check_test_plan.sh`

## Checks (Definition of Done)
- Per-hive single-writer invariant is preserved during normal operations, relay fan-out, planned failover, and unplanned cutover.
- Cross-hive relay is deterministic and idempotent: duplicate spec lines or relay restarts do not produce duplicate side effects.
- Federated writes use authenticated REST mutation paths; read-only FUSE mounts may be used for observability.
- Relay queueing is bounded with explicit backpressure/timeout counters and deterministic failure receipts.
- Evidence/timeline output correlates source hive intent -> target hive execution -> terminal receipt with stable IDs.
- A multi-hive validation matrix (minimum 3 hives; stretch 10 hives) passes with no split-brain writes and no grammar drift.
- `docs/TEST_PLAN.md` includes mandatory federation runs (relay success, relay failure, dedupe, failover pause/resume, evidence correlation) with no skip path.

## Task Breakdown
```
Title/ID: m25h-federation-ir-and-policy
Goal: Add manifest-driven federation config and bounds for cross-hive relay behavior.
Inputs: tools/coh-rtc/src/ir.rs, configs/root_task.toml, docs/INTERFACES.md, docs/ARCHITECTURE.md.
Changes:
  - tools/coh-rtc/src/ir.rs + configs/root_task.toml - add `federation` section (peers, allowlisted actions, queue/WAL bounds, auth references).
  - Generated artifacts/snippets - emit federation bounds for docs and host tooling.
  - docs/INTERFACES.md + docs/ARCHITECTURE.md - document federation envelope and invariants.
Commands:
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json
  - scripts/check-generated.sh
Checks:
  - Invalid peer config or over-limit bounds fail generation deterministically.
Deliverables:
  - Manifest-backed federation policy with generated docs snippets and hash-checked artifacts.

Title/ID: m25h-relay-agent-core
Goal: Implement bounded cross-hive ticket relay with WAL and deterministic cursor resume.
Inputs: apps/host-ticket-agent, apps/hive-gateway, docs/HOST_TOOLS.md, docs/FAILOVER.md.
Changes:
  - apps/host-ticket-agent/src/relay.rs (or new apps/hive-federation-relay) - watch source `/host/tickets/spec`, forward allowlisted intents to target `/host/tickets/spec`.
  - apps/host-ticket-agent/src/wal.rs - persist pending relay intents and replay unapplied entries after restart.
  - apps/hive-gateway - expose relay observability counters under existing status surfaces.
Commands:
  - cargo test -p host-ticket-agent
  - cargo test -p hive-gateway
Checks:
  - Relay restarts resume from cursor/WAL without duplicate side effects.
Deliverables:
  - Host-side relay engine with bounded queueing and restart-safe behavior.

Title/ID: m25h-federated-ticket-envelope
Goal: Define cross-hive ticket envelope and status correlation rules.
Inputs: apps/nine-door/src/host/tickets.rs, apps/root-task/src/ninedoor.rs, docs/INTERFACES.md.
Changes:
  - Ticket schema evolution (`host-ticket/v1` additive fields or `host-ticket-federated/v1`) including `source_hive`, `target_hive`, `relay_hop`, and correlation IDs.
  - Deterministic mapping from target status receipts back to source status/deadletter.
  - docs/INTERFACES.md - normative schema and lifecycle transitions for federated flow.
Commands:
  - cargo test -p nine-door
  - cargo test -p host-ticket-agent
Checks:
  - Malformed or unauthorized federated envelopes are rejected with deterministic ERR/receipts.
Deliverables:
  - Stable cross-hive envelope contract and receipt-correlation semantics.

Title/ID: m25h-fleet-read-model
Goal: Add operator-grade fleet read model without introducing global write semantics.
Inputs: apps/coh, apps/hive-gateway, docs/HOST_TOOLS.md.
Changes:
  - apps/coh/src/fleet.rs - read-only fan-in helpers (`coh fleet status`, `coh fleet lease-summary`, `coh fleet pressure`).
  - docs/HOST_TOOLS.md - document read-only fleet commands and write-fencing guidance.
  - Optional REST/FUSE examples for multi-hive observability trees.
Commands:
  - cargo test -p coh
Checks:
  - Fleet read commands produce deterministic merged output and never mutate hive state.
Deliverables:
  - Read-only fleet observability surface aligned with single-writer control semantics.

Title/ID: m25h-failover-fencing-integration
Goal: Integrate relay behavior with failover fencing and cutover runbooks.
Inputs: scripts/failover_watchdog.py, docs/FAILOVER.md, docs/TEST_PLAN.md.
Changes:
  - scripts/failover_watchdog.py - add relay pause/resume hooks tied to hold-down and health thresholds.
  - docs/FAILOVER.md - federation-aware runbook: freeze relay, cut over, replay WAL, resume relay.
  - docs/TEST_PLAN.md - mandatory failover + relay consistency checks.
Commands:
  - python3 scripts/failover_watchdog.py --help
  - scripts/ci/check_test_plan.sh
Checks:
  - Planned/unplanned cutovers do not create split-brain relay writes or duplicate downstream actions.
Deliverables:
  - Federation-aware failover automation and validated operator procedure.

Title/ID: m25h-federation-scale-gate
Goal: Validate pragmatic multi-hive scale targets with deterministic pass/fail criteria.
Inputs: scripts/rest_perf_harness.py, docs/BENCHMARKS.md, docs/TEST_PLAN.md, docs/USE_CASES.md.
Changes:
  - scripts/rest_perf_harness.py - add multi-hive scenario mode (N gateways, per-hive worker caps, relay load profile).
  - docs/BENCHMARKS.md - publish 3-hive baseline and optional 10-hive stretch results with first-failure mode analysis.
  - docs/USE_CASES.md + docs/TEST_PLAN.md - map federated scale to concrete use-case flows and mandatory gate commands.
Commands:
  - python scripts/rest_perf_harness.py simulate --multi-hive --hives 3 --workers-per-hive 1000 --no-retries --error-budget-rate 0.01
Checks:
  - Multi-hive scenarios hit target worker totals via federation without breaching per-hive invariants.
Deliverables:
  - Federation scale gate evidence and use-case-aligned capacity guidance.

Title/ID: m25h-evidence-and-timeline-correlation
Goal: Make cross-hive relay flows first-class in evidence packs and timelines.
Inputs: apps/coh/src/evidence.rs, apps/coh/src/evidence_timeline.rs, docs/HOST_TOOLS.md.
Changes:
  - apps/coh/src/evidence.rs - export source/target relay correlation fields and redacted auth metadata.
  - apps/coh/src/evidence_timeline.rs - stitch multi-hive ticket lifecycle into deterministic timeline ordering.
  - docs/HOST_TOOLS.md - operator guidance for multi-hive postmortems and chargeback.
Commands:
  - cargo test -p coh
Checks:
  - Evidence/timeline output is deterministic and sufficient to reconstruct federated control flow.
Deliverables:
  - Cross-hive evidence correlation integrated into existing audit-first tooling.
```

----
**Release 0.9.0 alpha**
----

---

The next planned releases target official Raspberry Pi 4 bare-metal boot (`U-Boot + binary image`) and AWS native boot via AMI.

---

## Milestone 26 — Official Pi 4 Bring-up (U-Boot + Binary Image) <a id="26"></a> 
[Milestones](#Milestones)

**Status:** In Progress — pivoted on February 23, 2026 from UEFI `BOOTAA64.EFI` bring-up to the official upstream seL4 Raspberry Pi 4 flow (`U-Boot + binary image`).

**Why now (context):**  
Upstream seL4 Pi 4 bring-up documentation and examples are built around loading the generated seL4 image from U-Boot (`fatload` + `go`) on `bcm2711`, not a UEFI handoff chain. Aligning Milestone 26 to this path reduces boot complexity, matches upstream behavior, and gives deterministic control at the U-Boot prompt for upcoming network milestones (26a/26b).

**Non-negotiable constraints:**  
- Boot chain for Pi 4 Milestone 26 is: `Pi firmware (start4/fixup) -> U-Boot -> seL4 image -> root-task`.
- Milestone 26 acceptance no longer depends on UEFI firmware settings or `BOOTAA64.EFI`.
- Backward compatibility remains mandatory: Pi 4 changes must not break existing QEMU workflows on macOS (`hvf`/`tcg`) or Linux (`kvm`/`tcg`) for `aarch64/virt`.
- Local diagnostics on Pi 4 must reuse the existing root-console parser and command semantics; no new shell grammar or in-VM protocol is permitted.
- Local diagnostics seat remains primitive: USB keyboard input + HDMI text output only; no compositor/windowing stack.
- Milestone 26 remains a strict no-NIC runtime baseline on Pi 4; root-task network bring-up and TCP console reachability on Pi 4 start in Milestone 26a.
- U-Boot may be used for pre-kernel network setup and diagnostics, but no new Cohesix protocol semantics may be introduced there.
- All post-seL4 hardware access (UART, NET, USB input, HDMI text output, TPM, RTC) must go through HAL-owned traits/drivers.

---

### Prerequisites (must be completed before Milestone 26 DoD)
**Upstream seL4 Pi 4 binary image support**
- Confirm and use upstream seL4 image output for `KernelPlatform=bcm2711` (for example `sel4test-driver-image-arm-bcm2711`).
- The generated image must preserve existing VM boot semantics once seL4 is entered.

**Upstream U-Boot Pi 4 support**
- Build Pi 4 U-Boot using `rpi_4_defconfig`.
- Ensure SD card firmware handoff is configured to load `u-boot.bin` as kernel payload.

---

### Goal
Deliver a **Pi firmware -> U-Boot -> seL4 image -> root-task** boot path on Raspberry Pi 4 that reaches the `cohesix>` prompt with primitive local diagnostics (USB keyboard input + HDMI text output) and deterministic no-NIC runtime evidence.

---

### Deliverables

- **Pi 4 U-Boot boot chain**
  - Standard FAT boot partition contains:
    - Raspberry Pi firmware assets (`start4.elf`, `fixup4.dat`, board DTBs/overlays as needed),
    - `u-boot.bin`,
    - seL4 generated image (`sel4test-driver-image-arm-bcm2711`),
    - Cohesix manifest artifacts used by root-task.
  - Root-task remains the first user process post-kernel boot.

- **U-Boot command path (authoritative for boot control)**
  - Document and standardize operator commands:
    - `fatls` to verify media,
    - `fatload` to load image into RAM,
    - `go` to transfer execution.
  - Define the environment conventions that 26a/26b will extend (`loadaddr`, `ipaddr`, `serverip`, `ethact`, `autoload`, `bootcmd`).

- **macOS debug harness for U-Boot scripts**
  - Add a reproducible QEMU U-Boot harness on macOS using `qemu_arm64_defconfig` and `qemu-system-aarch64 -machine virt`.
  - Use this harness to debug U-Boot env scripts and pre-boot network setup logic quickly.
  - Explicitly document that QEMU harness does not prove Pi 4 USB keyboard, HDMI, or GENET hardware behavior.

- **Identity & attestation**
  - Identity subsystem remains in root-task (TPM 2.0 or declared DICE fallback).
  - Capability ticket seeds are sealed only after successful attestation.
  - Attestation evidence bound to manifest fingerprint is appended to `/proc/boot` and exported via NineDoor.

- **Schema & validation**
  - Introduce profile naming aligned to boot reality (`pi4-uboot-aarch64`), with a compatibility alias from `uefi-aarch64` permitted only during migration.
  - Validate bounded hardware declarations (`uart`, `rtc`, local-seat, attestation policy/device requirements).
  - Enforce no-NIC runtime gate for Milestone 26 profile(s).

- **Local diagnostics seat (Pi 4, essential-only)**
  - Profile-gated local-seat path consumes USB keyboard input and routes bytes into the existing root console parser.
  - Primitive HDMI text sink mirrors root-console output lines with bounded memory and deterministic truncation.
  - If `hw.local_seat.required=true` and keyboard/display path cannot initialise, boot fails deterministically before ticket publication.

- **No-NIC baseline validation (Milestone 26 only)**
  - Root-task completes boot/attestation/local-seat bring-up without NIC initialisation.
  - `/proc/boot` emits deterministic evidence that networking is intentionally disabled in Milestone 26 runtime.

---

### Commands
- Build seL4 Pi 4 image:
  - `cmake --build seL4/build_UEFI --target images/sel4test-driver-image-arm-bcm2711`
- Build U-Boot for Pi 4:
  - `make -C third_party/u-boot rpi_4_defconfig`
  - `make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j$(sysctl -n hw.ncpu)`
- U-Boot boot commands on Pi 4:
  - `fatls mmc 0:1`
  - `fatload mmc 0:1 ${loadaddr} sel4test-driver-image-arm-bcm2711`
  - `go ${loadaddr}`
- U-Boot pre-boot networking setup commands (for 26a/26b preparation and diagnostics):
  - `setenv autoload no`
  - `setenv ipaddr <board-ip>`
  - `setenv serverip <host-ip>`
  - `dhcp`
  - `ping ${serverip}`
  - `saveenv`
- macOS QEMU U-Boot harness:
  - `make -C third_party/u-boot qemu_arm64_defconfig`
  - `make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j$(sysctl -n hw.ncpu)`
  - `qemu-system-aarch64 -machine virt -cpu cortex-a57 -m 2048 -nographic -bios third_party/u-boot/u-boot.bin`
- Runtime checks:
  - `cargo check -p root-task`
  - `cargo test -p root-task --features "kernel serial-console" local_seat`

---

### Checks (DoD)
- Pi 4 boots through U-Boot using `fatload` + `go` into seL4/root-task with deterministic log ordering.
- `cohesix>` prompt appears on HDMI text output and accepts USB keyboard commands (`help`, `bi`, `caps`, `ping`) with unchanged parser semantics.
- Command responses typed on USB keyboard are visible on HDMI and match serial transcript semantics.
- Manifest fingerprint is printed and matches packaged hash.
- If `hw.attestation.enabled=true`, attestation succeeds and evidence hash matches manifest fingerprint; if unavailable, boot aborts deterministically.
- Milestone 26 runtime on Pi 4 emits deterministic no-NIC evidence and does not require TCP console reachability.
- Root-task post-seL4 paths use HAL-only device access; no direct firmware-service assumptions in runtime code.
- macOS QEMU U-Boot harness can execute U-Boot env/network commands for script validation; Pi 4 hardware remains authoritative for USB/HDMI/NIC proof.
- Existing macOS/Linux QEMU regression scripts continue to pass unchanged unless explicitly profile-gated and documented.

---

### Compiler touchpoints
- `coh-rtc` emits hardware tables for selected profile(s) and docs import them into `docs/HARDWARE_BRINGUP.md` and `docs/ARCHITECTURE.md`.
- `coh-rtc` extends Pi 4 profile schema with bounded local-seat declarations (`enabled`, `required`, declared keyboard/display devices).
- `coh-rtc` enforces Milestone 26 no-NIC runtime gates while allowing U-Boot pre-boot network env declarations for future milestones.
- Migration guard: accept legacy `uefi-aarch64` manifests only through an explicit compatibility path and emit deterministic deprecation diagnostics.

---

## Task Breakdown
```
Title/ID: m26-uboot-bootchain
Goal: Boot Pi 4 via upstream U-Boot commands (`fatload` + `go`) into seL4/root-task with stable manifest fingerprint output.
Inputs: `seL4/build_UEFI/images/sel4test-driver-image-arm-bcm2711`, `third_party/u-boot`, Pi firmware boot partition files, profile manifest.
Changes:
  - `scripts/pi4-image-build.sh` — build deterministic Pi 4 FAT payload (`u-boot.bin` + seL4 image + manifest artifacts).
  - `docs/HARDWARE_BRINGUP.md` — document canonical Pi 4 U-Boot command flow and SD layout.
  - `apps/root-task` — preserve boot fingerprint line ordering relative to serial/local seat.
Commands:
  - `cmake --build seL4/build_UEFI --target images/sel4test-driver-image-arm-bcm2711`
  - `make -C third_party/u-boot rpi_4_defconfig`
  - `make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j$(sysctl -n hw.ncpu)`
Checks:
  - Pi 4 reaches root-task via `fatload` + `go`; missing/invalid manifest aborts before ticket publication.
Deliverables:
  - Reproducible Pi 4 U-Boot boot artifacts with documented hashes and commands.

---

Title/ID: m26-uboot-mac-debug-harness
Goal: Provide a fast macOS U-Boot/QEMU harness for debugging boot scripts and future network env setup.
Inputs: `third_party/u-boot` (`qemu_arm64_defconfig`), `qemu-system-aarch64`, docs updates.
Changes:
  - `scripts/uboot/qemu-uboot-smoke.sh` — launch U-Boot on QEMU `virt` with deterministic serial logging.
  - `docs/HARDWARE_BRINGUP.md` — list supported harness use cases and explicit non-goals (no Pi4 USB/HDMI/GENET fidelity).
Commands:
  - `make -C third_party/u-boot qemu_arm64_defconfig`
  - `make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j$(sysctl -n hw.ncpu)`
  - `qemu-system-aarch64 -machine virt -cpu cortex-a57 -m 2048 -nographic -bios third_party/u-boot/u-boot.bin`
Checks:
  - Harness reaches U-Boot prompt and can execute env and network setup commands deterministically.
Deliverables:
  - Faster pre-hardware debug loop for U-Boot setup logic.

---

Title/ID: m26-local-seat-minimal
Goal: Provide primitive local post-boot diagnostics on Pi 4 by wiring USB keyboard input and HDMI text output into existing root-console path.
Inputs: `apps/root-task/src/console/*`, `apps/root-task/src/userland/mod.rs`, `apps/root-task/src/event/*`, HAL mapping, `tools/coh-rtc`, profile manifest.
Changes:
  - HAL-bound USB keyboard input path feeding existing parser/dispatcher.
  - Primitive HDMI text sink mirroring root-console output lines with bounded memory and deterministic truncation.
  - `coh-rtc` schema/codegen for `hw.local_seat` (`enabled`, `required`, declared devices).
Commands:
  - `cargo check -p root-task`
  - `cargo test -p root-task --features "kernel serial-console" local_seat`
  - `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
Checks:
  - USB keyboard commands yield identical parser semantics to serial.
  - HDMI output mirrors root-console lines deterministically.
  - `hw.local_seat.required=true` fails fast when dependencies are unsatisfied.
Deliverables:
  - Pi 4 local-seat evidence proving `cohesix>` command loop on USB keyboard + HDMI.

---

Title/ID: m26-hal-boundary
Goal: Keep all post-seL4 runtime device access HAL-owned while allowing U-Boot-only pre-boot control.
Inputs: `apps/root-task/src/hal/*`, `apps/root-task/src/drivers/*`, `apps/root-task/src/kernel.rs`, `tools/coh-rtc/src/*`, docs.
Changes:
  - Ensure all Milestone 26 runtime paths (serial, local seat, attestation, future network handoff points) are represented through HAL interfaces.
  - Add deterministic checks and documentation language for bootloader-owned vs Cohesix-owned responsibilities.
Commands:
  - `cargo check -p root-task`
  - `cargo test -p root-task hal:: -- --nocapture`
  - `rg -n "EFI_|boot_services|runtime_services|uefi::" apps/root-task/src apps/nine-door/src tools/coh-rtc/src`
Checks:
  - No direct firmware-service assumptions in runtime code.
  - HAL remains sole runtime device access surface.
Deliverables:
  - Auditable bootloader/runtime boundary with CI-friendly guard commands and docs-as-built updates.
```
---

## Milestone 26a — Pi 4 Networking Baseline (GENETv5 + Static IPv4, U-Boot Configurable) <a id="26a"></a>
[Milestones](#Milestones)

**Status:** Complete — profile-gated GENETv5/static-IPv4 compiler and root-task wiring are in tree, QEMU regression coverage remains green, and the U-Boot smoke + Pi 4 validation command path is documented and reproducible.

**Why now (platform continuity):**  
Milestone 26 establishes Pi 4 U-Boot boot, identity, and a local diagnostics seat with an intentional no-NIC runtime baseline. Milestone 26a introduces the first Pi 4 native NIC path while preserving the existing root-task networking model and Cohesix control-plane semantics.

**Non-negotiable constraints:**
- No new in-VM listeners or protocols; the authenticated root-task TCP console remains the only in-VM TCP listener.
- Milestone 26a is the first milestone where Pi 4 TCP console reachability is expected; Milestone 26 must remain valid without NIC/TCP on Pi 4.
- DHCP is intentionally out of scope for 26a and is delivered in Milestone 26b; 26a uses static IPv4 only.
- Any VM vs Pi 4 networking differences must be profile-gated, manifest-defined, and documented.
- Driver implementation must remain HAL-bound with bounded queues and deterministic memory budgets.
- Backward compatibility is mandatory: 26a network changes must preserve existing macOS/Linux QEMU console and networking workflows unless explicitly profile-gated for Pi 4 `pi4-uboot-aarch64` (with transitional alias support for legacy `uefi-aarch64` manifests).

### Prerequisite
- Milestone **26** completed (U-Boot boot chain + device identity attestation + local diagnostics seat + bootloader/HAL boundary enforcement).
- Milestone 26 hardware evidence includes deterministic no-NIC boot transcripts for Pi 4 (`pi4-uboot-aarch64`).

### Goal
Add a production-safe, profile-gated NIC backend for Raspberry Pi 4 (`bcm2711` GENETv5) and wire `pi4-uboot-aarch64` static IPv4 configuration through manifest -> generated artifacts -> root-task net bring-up.

### Deliverables
- **GENETv5 NIC backend (Pi 4)**
  - Add a root-task driver backend for Broadcom GENETv5, implemented in pure Rust with HAL ownership for MMIO, IRQ, DMA, and cache maintenance.
  - Use this design-reference order for architecture review: Linux `bcmgenet` driver behavior (primary) -> Linux `bcm2711` Pi 4 DT bindings for GENET/MDIO/PHY wiring (secondary) -> U-Boot `bcmgenet` bring-up behavior (tertiary sanity reference).
  - References are design-only inputs; no direct code lift is permitted.
  - Integrate backend selection into existing `NetBackend` plumbing and keep QEMU backends (`rtl8139`/`virtio-net`) unchanged.

- **Profile-gated static IPv4 for `pi4-uboot-aarch64`**
  - Extend manifest IR and validation with bounded static IPv4 fields for Pi 4 U-Boot profile (interface IP, prefix length, optional gateway).
  - Generate root-task networking config from `coh-rtc` artifacts instead of hard-wired dev-virt defaults when `profile.name=pi4-uboot-aarch64` (or accepted legacy alias).
  - Reject invalid/static-zero network configs deterministically at compile time or early boot.

- **Docs-as-built alignment**
  - Update `docs/ARCHITECTURE.md`, `docs/INTERFACES.md`, and `docs/SECURITY.md` with:
    - profile-gated backend matrix (QEMU vs Pi 4),
    - static IPv4 configuration source of truth,
    - deterministic bounds and failure modes for GENETv5 bring-up.
  - Update `docs/HARDWARE_BRINGUP.md` with Pi 4 U-Boot network checklist and expected boot evidence lines.

### Commands
- `cargo check -p root-task`
- `cargo test -p root-task net:: -- --nocapture`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
- `scripts/pi4-image-build.sh --manifest out/manifests/root_task_resolved.json`
- `scripts/uboot/qemu-uboot-smoke.sh --net user`
- `cargo run -p cohsh --features tcp -- --transport tcp --host <STATIC_IP> --port 31337 --script scripts/cohsh/boot_v0.coh`

### Checks (DoD)
- Pi 4 U-Boot boot reaches root-task network init and reports `GENETv5` backend with static IPv4 from manifest-generated config.
- `cohsh --transport tcp` succeeds against the configured static address with no console grammar or ACK/ERR/END drift.
- Invalid Pi 4 static IPv4 manifest settings are rejected deterministically (compiler validation and/or early-boot fail-fast).
- Pi 4 26a validation explicitly demonstrates transition from Milestone 26 no-NIC baseline to 26a NIC-enabled boot using profile-gated configuration only.
- No DHCP client path is introduced in 26a; DHCP remains scoped to Milestone 26b.
- Existing macOS/Linux QEMU test and operator flows remain backward compatible with pre-26 behavior.
- Full regression pack remains green on QEMU; any profile-gated divergence is explicitly documented and fixture-backed.

### Compiler touchpoints
- `coh-rtc` emits profile-gated network config tables (backend selection + static IPv4 fields) into generated root-task artifacts.
- Manifest validation enforces:
  - static IPv4 required for `pi4-uboot-aarch64` network-enabled profile,
  - prefix bounds (`1..=32`),
  - backend/profile compatibility (`bcmgenet` only where declared in `hw.devices`).
- Docs snippet regeneration includes static IPv4 and backend mapping excerpts for Architecture/Interfaces docs.

### Task Breakdown
```
Title/ID: m26a-bcmgenet-driver
Goal: Add HAL-bound Broadcom GENETv5 NIC backend for Raspberry Pi 4 bring-up.
Inputs: apps/root-task/src/net/*, apps/root-task/src/drivers/*, apps/root-task/src/hal/*, docs/SECURITY.md, Linux `bcmgenet` + Linux `bcm2711` DT binding notes + U-Boot `bcmgenet` bring-up notes (reference-only).
Changes:
  - apps/root-task/src/drivers/bcmgenet.rs — GENETv5 register/ring/IRQ/PHY implementation with bounded queues and HAL-backed MMIO/IRQ/DMA/MDIO access only.
  - apps/root-task/src/drivers/mod.rs — expose bcmgenet backend.
  - apps/root-task/src/net/mod.rs + apps/root-task/src/net/stack.rs — backend selection and init wiring.
  - docs/ARCHITECTURE.md + docs/SECURITY.md — record GENET design-reference provenance and explicit no-code-lift policy.
Commands:
  - cargo check -p root-task
  - cargo test -p root-task --features "kernel net-console" bcmgenet
  - rg -n "EFI_|boot_services|runtime_services|uefi::" apps/root-task/src tools/coh-rtc/src
Checks:
  - Link-up, RX/TX smoke, and deterministic error paths are covered by unit/integration tests.
  - GENET runtime paths remain HAL-only; no direct MMIO/firmware-service usage appears outside HAL-owned modules.
Deliverables:
  - Pi 4 GENETv5 backend integrated behind existing net abstractions with documented source provenance order (Linux `bcmgenet` -> Linux `bcm2711` DT -> U-Boot `bcmgenet`) and reference-only compliance.

Title/ID: m26a-static-ipv4-profile-gate
Goal: Make Pi 4 U-Boot static IPv4 config manifest-authoritative and profile-gated.
Inputs: configs/root_task.toml, tools/coh-rtc, apps/root-task/src/generated, docs/INTERFACES.md.
Changes:
  - tools/coh-rtc/src/* — add IR fields + validation for `pi4-uboot-aarch64` static IPv4 network config.
  - apps/root-task/src/generated/* — regenerated network config outputs.
  - apps/root-task/src/net/mod.rs — consume generated profile config before dev-virt fallback defaults.
Commands:
  - cargo test -p coh-rtc
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json
Checks:
  - Invalid static IPv4 entries fail deterministically; valid configs produce stable generated artifacts.
Deliverables:
  - Manifest-driven static IPv4 for Pi 4 U-Boot profile with docs-as-built parity.

Title/ID: m26a-pi4-uboot-validation
Goal: Prove end-to-end TCP console reachability on Pi 4 using static IPv4 with no protocol drift.
Inputs: scripts/pi4-image-build.sh, docs/HARDWARE_BRINGUP.md, scripts/cohsh/boot_v0.coh.
Changes:
  - docs/HARDWARE_BRINGUP.md — Pi 4 checklist, expected boot lines, static IPv4 examples.
  - docs/ARCHITECTURE.md + docs/SECURITY.md — backend and threat-model updates.
Commands:
  - scripts/pi4-image-build.sh --manifest out/manifests/root_task_resolved.json
  - cargo run -p cohsh --features tcp -- --transport tcp --host <STATIC_IP> --port 31337 --script scripts/cohsh/boot_v0.coh
Checks:
  - Validation includes a before/after proof: Milestone 26 no-NIC transcript and Milestone 26a NIC-enabled transcript for the same board profile family.
  - Console attach/tail/test flows are unchanged except for profile-gated backend/address selection.
Deliverables:
  - Reproducible Pi 4 U-Boot network bring-up evidence and updated docs.

Title/ID: m26a-nettest-profile-gate
Goal: Extend `nettest` so the same command works on QEMU `dev-virt` and Pi 4 static IPv4 without changing console grammar.
Inputs: apps/root-task/src/net/*, apps/root-task/src/event/mod.rs, docs/INTERFACES.md, docs/USERLAND_AND_CLI.md.
Changes:
  - apps/root-task/src/net/stack.rs — remove `dev-virt`-only assumptions from self-test target selection and bind probe/echo targets to the active profile-gated interface config (including Pi 4 static IPv4/gateway paths).
  - apps/root-task/src/net/mod.rs — expose deterministic nettest target/report fields required by both QEMU and Pi 4 backends.
  - apps/root-task/src/event/mod.rs — update `help`/`netstats` lines so `nettest` no longer appears `dev-virt`-only when built for Pi 4 profiles.
  - docs/INTERFACES.md + docs/USERLAND_AND_CLI.md — document backend-agnostic `nettest` behavior and expected output fields.
Commands:
  - cargo check -p root-task
  - cargo test -p root-task net:: -- --nocapture
  - scripts/cohesix-build-run.sh --no-run --cargo-target aarch64-unknown-none
Checks:
  - `nettest` remains unchanged for QEMU (`127.0.0.1:{31338,31339}` hostfwd workflows still valid).
  - Pi 4 static IPv4 profile can run `nettest` and produce deterministic pass/fail evidence with no new in-VM listeners.
  - ACK/ERR/END console grammar and existing command names remain unchanged.
Deliverables:
  - Single `nettest` path that is profile-gated by backend/address config and validated on both QEMU and Pi 4 static IPv4.
```

---

## Milestone 26b — Pi 4 DHCP Baseline (NIC + Wi-Fi Policy, U-Boot Configurable) <a id="26b"></a>
[Milestones](#Milestones)

**Why now (operator continuity):**  
Milestone 26a brings up deterministic static IPv4 on Pi 4 wired NIC. Milestone 26b introduces a minimal DHCP path so operators can boot and diagnose in less-controlled edge environments without hand-editing static addresses, while preserving `no_std`, bounded behavior, and existing console/9P semantics.

**Non-negotiable constraints:**
- DHCP implementation must be pure Rust, `no_std`, and intentionally bare-bones (DHCPv4 only: DISCOVER/OFFER/REQUEST/ACK plus bounded timeout/retry logic).
- No new in-VM listeners or protocols are permitted; DHCP is only a client-side address acquisition path for existing network surfaces.
- Post-seL4 runtime must remain HAL-only; no direct bootloader/firmware service calls are allowed in root-task.
- NIC and Wi-Fi DHCP behavior must be policy-configurable for `pi4-uboot-aarch64` through compiler-validated profile settings sourced from U-Boot environment configuration when available; if bootloader policy inputs are absent, manifest defaults are authoritative.
- Wi-Fi scope is minimal diagnostics connectivity only (join + DHCP + existing TCP console path); no in-VM supplicant stack, roaming framework, or broad feature surface.
- Milestone 26b includes a new profile-gated Pi 4 CYW43xx Wi-Fi driver path; all Wi-Fi dataplane/control-plane access must be HAL-backed (SDIO, power/reset, IRQ/OOB, firmware handoff hooks) with no direct MMIO or firmware-service calls outside HAL.
- CYW43xx design references must be used in this order for architecture review: OpenBSD `bwfm` -> Zephyr/Infineon WHD HAL split -> Linux `brcmfmac` SDIO edge-case behavior. These are design references only; no source copy/paste is permitted.
- Backward compatibility is mandatory: Milestone 26b must not break existing macOS/Linux QEMU workflows, fixtures, or transport semantics.

### Prerequisite
- Milestone **26a** completed (Pi 4 GENETv5 + static IPv4 + profile-gated NIC bring-up).

### Goal
Add a deterministic `no_std` DHCP client core that can operate on Pi 4 wired NIC and profile-gated Wi-Fi paths, with bounded U-Boot-configurable network policy selection where available, while keeping all pre-existing QEMU flows backward compatible on macOS and Linux.

### Deliverables
- **Minimal DHCP core (`no_std`)**
  - Add a bounded DHCPv4 client core in root-task with strict packet validation, deterministic timers, bounded retransmits, and no dynamic protocol extensions beyond required lease fields.
  - Keep memory usage fixed and auditable; no unbounded allocations, no background worker threads, and no protocol parser ambiguity.

- **Interface binding: wired + Wi-Fi (profile-gated)**
  - Wire DHCP core to existing Pi 4 GENETv5 backend.
  - Add a profile-gated Wi-Fi association + link-up path for Pi 4 CYW43xx-class devices sufficient to reach DHCP (design remains HAL-bound and capability-scoped).
  - Introduce a dedicated CYW43xx Wi-Fi backend wired through HAL traits and existing `NetBackend` policy selection; no alternate ad-hoc control path is allowed.
  - Record implementation provenance in docs (OpenBSD `bwfm` design shape, Zephyr/WHD HAL layering, Linux `brcmfmac` recovery/link edge cases) and enforce reference-only usage.
  - Maintain a single network control-plane surface: existing root console + NineDoor behavior only.

- **U-Boot-configurable network policy**
  - Extend manifest/compiler schema with bounded policy fields for `pi4-uboot-aarch64`, including:
    - network mode (`off`, `static`, `dhcp`),
    - interface policy (`wired`, `wifi`, `auto`),
    - DHCP timing bounds and retry limits.
  - When bootloader setup can provide network policy inputs, capture them pre-handoff and normalize through manifest/DTB-generated structures validated by `coh-rtc`.
  - Preserve deterministic fallback behavior when U-Boot policy inputs are missing or invalid.

- **Backward-compatibility guardrails**
  - Keep QEMU `aarch64/virt` behavior for macOS/Linux unchanged by default (existing static/dev-virt flows must keep working with no required operator command changes).
  - Add explicit regression checks proving 26/26a/26b additions are profile-gated and do not alter prior QEMU console grammar, ACK/ERR/END behavior, or existing transport fixtures.

### Commands
- `cargo check -p root-task`
- `cargo test -p root-task net:: -- --nocapture`
- `cargo test -p coh-rtc`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
- `scripts/pi4-image-build.sh --manifest out/manifests/root_task_resolved.json`
- `scripts/uboot/qemu-uboot-smoke.sh --net user`
- `scripts/cohesix-build-run.sh --no-run --cargo-target aarch64-unknown-none`

### Checks (DoD)
- Pi 4 wired path acquires a DHCP lease within bounded retries/timeouts and exposes acquired config through existing diagnostics surfaces.
- Pi 4 Wi-Fi path (when enabled in profile and credentials are present) reaches link-up and acquires DHCP with deterministic failure modes and audited errors.
- Invalid DHCP options, malformed offers, lease overflows, and timeout exhaustion fail safely and deterministically.
- U-Boot-configurable network policy (where available) is honored through compiler-validated handoff structures; missing/unsupported bootloader policy cleanly falls back to manifest defaults.
- Existing macOS/Linux QEMU workflows remain backward compatible, including serial/TCP console behavior and existing regression fixtures.
- CYW43xx Wi-Fi path demonstrates HAL-only access in runtime code and tests; no direct MMIO/bootloader-service usage exists outside HAL-owned modules.
- Full regression pack remains green on QEMU; any profile-gated divergence is explicitly documented and fixture-backed.

### Compiler touchpoints
- `coh-rtc` adds bounded `pi4-uboot-aarch64` network policy fields for mode/interface selection and DHCP retry/timing limits.
- Validation enforces:
  - policy bounds and enum validity,
  - interface/profile compatibility (wired vs Wi-Fi declarations),
  - deterministic fallback policy when optional U-Boot-provided inputs are absent.
- Generated snippets in architecture/interfaces/security docs include the new policy and DHCP bounds so docs-as-built remains authoritative.

### Task Breakdown
```
Title/ID: m26b-dhcp-core-nostd
Goal: Implement bare-bones, bounded DHCPv4 client logic for root-task networking.
Inputs: apps/root-task/src/net/*, docs/INTERFACES.md, docs/SECURITY.md.
Changes:
  - apps/root-task/src/net/dhcp.rs — deterministic DHCP state machine and packet parser (DISCOVER/OFFER/REQUEST/ACK only).
  - apps/root-task/src/net/mod.rs — integrate DHCP mode into existing net bootstrap sequencing.
Commands:
  - cargo check -p root-task
  - cargo test -p root-task --features "kernel net-console" dhcp
Checks:
  - Lease acquisition and timeout paths are bounded and test-covered.
Deliverables:
  - `no_std` DHCP core integrated with deterministic diagnostics.

Title/ID: m26b-uboot-net-policy
Goal: Make wired/Wi-Fi DHCP policy compiler-authoritative with optional U-Boot-provided inputs.
Inputs: configs/root_task.toml, tools/coh-rtc, apps/root-task/src/generated, docs/HARDWARE_BRINGUP.md.
Changes:
  - tools/coh-rtc/src/* — add policy IR fields (`mode`, `interface`, DHCP bounds) and validation.
  - apps/root-task/src/generated/* — regenerated network policy constants/artifacts.
  - docs/HARDWARE_BRINGUP.md — document U-Boot policy source, fallback rules, and expected boot lines.
Commands:
  - cargo test -p coh-rtc
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json
Checks:
  - Invalid policy settings fail deterministically; valid settings generate stable artifacts.
Deliverables:
  - Manifest/U-Boot-policy-driven DHCP mode selection with docs-as-built parity.

Title/ID: m26b-pi4-wifi-dhcp-baseline
Goal: Add minimal Pi 4 Wi-Fi path sufficient for DHCP-backed diagnostics connectivity.
Inputs: apps/root-task/src/drivers/*, apps/root-task/src/hal/*, docs/ARCHITECTURE.md, docs/SECURITY.md.
Changes:
  - apps/root-task/src/drivers/* — add HAL-bound CYW43xx-class Wi-Fi bring-up path with bounded join/retry behavior and no direct MMIO outside HAL traits.
  - apps/root-task/src/net/* — bind Wi-Fi link state into shared DHCP/bootstrap flow.
  - docs/ARCHITECTURE.md + docs/SECURITY.md — update network backend matrix, bounds, threat notes, and design-reference provenance (`bwfm` -> Zephyr/WHD HAL split -> `brcmfmac` edge-case guidance; no code lift).
Commands:
  - cargo check -p root-task
  - cargo test -p root-task wifi:: -- --nocapture
  - rg -n "EFI_|boot_services|runtime_services|uefi::" apps/root-task/src tools/coh-rtc/src
Checks:
  - Wi-Fi join + DHCP works when enabled and declared; failure modes remain deterministic and auditable.
  - Wi-Fi runtime/device access remains HAL-only; any non-HAL direct access path fails review.
Deliverables:
  - Profile-gated CYW43xx Wi-Fi DHCP diagnostics path for Pi 4 with explicit reference-only provenance documentation.

Title/ID: m26b-qemu-compat-gate
Goal: Prove 26/26a/26b changes do not break macOS/Linux QEMU backward compatibility.
Inputs: scripts/qemu-run.sh, scripts/uboot/qemu-uboot-smoke.sh, regression fixtures, docs/TEST_PLAN.md.
Changes:
  - docs/TEST_PLAN.md — add explicit compatibility matrix checks for macOS/Linux QEMU after 26/26a/26b.
  - scripts/ci/* — add guard checks that fail on console/protocol drift for existing QEMU fixtures.
Commands:
  - scripts/cohesix-build-run.sh --no-run --cargo-target aarch64-unknown-none
  - scripts/uboot/qemu-uboot-smoke.sh --net user
Checks:
  - Existing QEMU fixtures on macOS/Linux remain passing with no operator-facing behavior regressions.
Deliverables:
  - Auditable compatibility evidence for the Pi 4 bare-metal rollout milestones.

Title/ID: m26b-nettest-interface-policy
Goal: Extend `nettest` to honor 26b network policy (`wired|wifi|auto`) while enforcing a single active control-plane interface at runtime.
Inputs: apps/root-task/src/net/*, tools/coh-rtc/src/*, apps/root-task/src/generated/*, docs/INTERFACES.md, docs/HARDWARE_BRINGUP.md.
Changes:
  - apps/root-task/src/net/mod.rs + apps/root-task/src/net/stack.rs — add policy-aware interface selection for self-test execution and report active/standby interface state in `netstats`.
  - apps/root-task/src/net/dhcp.rs — surface bounded lease/state signals needed by `nettest` diagnostics for DHCP paths.
  - tools/coh-rtc/src/* + apps/root-task/src/generated/* — emit and validate nettest-relevant policy fields (`mode`, `interface`, retry/timing bounds) for `pi4-uboot-aarch64`.
  - docs/INTERFACES.md + docs/HARDWARE_BRINGUP.md — codify deterministic single-active-interface behavior and failover evidence requirements for `auto`.
Commands:
  - cargo check -p root-task
  - cargo test -p root-task net:: -- --nocapture
  - cargo test -p coh-rtc
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json
Checks:
  - `nettest` on `wired` and `wifi` policies executes against only the selected interface.
  - `auto` policy uses deterministic priority/failover, with at most one active TCP console interface at any moment.
  - Existing QEMU `nettest` behavior and host workflows remain backward compatible with no required command changes.
Deliverables:
  - Policy-driven `nettest` behavior for Pi 4 DHCP milestones with explicit single-active-interface guarantees and compatibility evidence.
```

---

## Milestone 26c — Humanized Surface Cleanup + Markdown Audit (Zero-Regression) <a id="26c"></a>
[Milestones](#Milestones)

**Why now (reviewer trust):**
Milestones 25-26b establish technical capability, transport breadth, and Pi 4 bring-up evidence, but external reviewers still judge code quality from the visible authoring surface first. Milestone 26c removes template-heavy presentation, tightens comment/header policy, inventories every Markdown file, and widens the regression gate so cleanup is only complete when the full staged Test Plan passes on both QEMU and Pi 4.

**Non-negotiable constraints:**
- No protocol, namespace, ACK/ERR/END, telemetry, manifest, or release-behavior changes are permitted under a "humanizing" label.
- Comment/header/doc rewrites must not hide semantic changes. Behavior-changing refactors require their own tests and doc updates in the same change.
- Humanized documentation must remain auditable against the as-built system: generated snippets, manifest fingerprints, fixture outputs, release artifacts, and staged test evidence take precedence over prose.
- Generic restatement comments ("Defines the X module", "Provides the Y library surface", "CLI entry point") are prohibited once 26c lands; comments must explain invariants, authority boundaries, limits, operator-visible behavior, or rationale.
- Required file headers may be narrowed, reformatted, or reduced only by updating `AGENTS.md`, `CONTRIBUTING.md`, and `docs/CODING_GUIDELINES.md` in the same change.
- Large `root-task` and driver monoliths are not first-wave cleanup targets; extraction is allowed only when characterization tests already pin behavior.
- `docs/snippets/*.md`, release snapshot docs under `releases/**/*.md`, and other generated/derived documentation remain update-by-source artifacts; they are not to be hand-edited as a shortcut.
- Vendored/reference Markdown under `third_party/**/*.md` is inventory-only unless provenance, licensing, or linkage is wrong.
- Milestone closure is blocked until the full Test Plan succeeds on QEMU and Pi 4 with no `INCOMPLETE` markers.

### Prerequisite
- Milestone **26b** completed (Pi 4 DHCP baseline + QEMU compatibility guardrails).

### Goal
Humanize the authored Cohesix code and documentation surfaces without protocol or behavioral regression by:
1. replacing template-heavy comments and test naming with invariant-focused prose,
2. classifying and reviewing every `*.md` file in the repository,
3. auditing canonical documentation against generated artifacts, manifest outputs, and observed staged-run evidence before prose cleanup lands,
4. adding characterization/CI gates before touching riskier host-side cleanup targets, and
5. requiring the full staged Test Plan to pass on both QEMU and Pi 4 before closure.

### Markdown review scope (all `*.md` files)
Milestone 26c must inventory every Markdown file returned by:
`rg --files -g '*.md' | sort`

The inventory MUST include and disposition these path classes:
- root/project Markdown: `AGENTS.md`, `CONTRIBUTING.md`, `README.md`
- app and crate READMEs: `apps/**/README.md`, `crates/**/README.md`, `tools/cohesix-py/README.md`, `tests/integration/README.md`
- canonical product docs: `docs/*.md`
- audit docs: `docs/audit/*.md`, `docs/audit/checklists/*.md`, `docs/nist/*.md`
- generated or derived doc fragments: `docs/snippets/*.md`
- release snapshot docs: `releases/**/*.md`
- seL4/reference docs carried in-repo: `seL4/*.md`
- vendored/reference Markdown: `third_party/**/*.md`

For each inventory entry, 26c must record one of:
- human-edited canonical source,
- generated snippet / do not hand-edit,
- release snapshot / update only during release cut,
- vendored reference / review-only,
- external reference mirror / provenance and linkage only.

### Deliverables
- **Style charter and authoring rules**
  - Update repository guidance so comments explain invariants, limits, threat boundaries, operator-visible behavior, or rationale rather than file names or obvious syntax.
  - Replace blanket "library surface/module for/CLI entry point" phrasing with file-specific documentation or remove it when it adds no information.
  - Define which boilerplate is still mandatory, which is derived, and which is prohibited.

- **All-Markdown inventory and disposition**
  - Produce a checked-in Markdown inventory/disposition table covering every `*.md` file under the repository root.
  - Record which docs are canonical, generated, release-derived, reference-only, or vendored.
  - Ensure release and snippet docs are traced back to their source artifacts instead of receiving ad-hoc prose edits.

- **Docs-as-built audit before prose cleanup**
  - Audit canonical docs against current generated snippets, resolved manifests, committed fixtures, release artifacts, and staged test evidence before rewriting for tone or readability.
  - Record where documentation is sourced from compiler outputs, where it is manually maintained, and what exact commands prove alignment.
  - Reject any humanizing change that makes prose nicer but less faithful to the observed system.

- **Low-risk surface cleanup**
  - Clean up public crate surfaces, tiny modules, tests, READMEs, and operator docs first.
  - Prefer renaming tests, tightening doc comments, and deleting generic noise before any control-flow refactor.
  - Keep changes behavior-preserving and small enough to review mechanically.

- **Characterization and gate expansion**
  - Add or expand tests around crates currently most exposed to cleanup risk and currently weaker in CI coverage, especially `coh`, `host-ticket-agent`, and selected `root-task` helper modules.
  - Extend CI/test-plan execution so cleanup work in these areas is exercised automatically before merge.
  - Fail the gate on protocol drift, fixture drift, or undocumented output changes.

- **Selected structural cleanup**
  - Refactor repetitive validation and processing code in host-side crates only after characterization tests exist.
  - Focus on readability wins that reduce duplication or make invariants explicit, not aesthetic churn.
  - Defer monolithic kernel/driver rewrites until dedicated milestone work justifies the risk.

- **QEMU + Pi 4 full regression evidence**
  - Extend the staged Test Plan runner and docs so "PASS" can be asserted separately for QEMU and Pi 4 hardware runs using the same stage contract and evidence layout.
  - Require full successful completion of the Test Plan on QEMU and Pi 4 for milestone closure.

### Commands
- `rg --files -g '*.md' | sort > out/audit/m26c_markdown_inventory.txt`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
- `scripts/check-generated.sh`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p secure9p-core`
- `cargo test -p coh --features mock`
- `cargo test -p host-ticket-agent`
- `cargo test -p root-task --lib`
- `scripts/ci/check_test_plan.sh`
- `scripts/ci/test_plan_run.sh --state-dir out/test-plan/m26c-qemu`
- `COHESIX_TEST_TARGET=pi4 scripts/ci/test_plan_run.sh --state-dir out/test-plan/m26c-pi4`

### Checks (DoD)
- Every Markdown file returned by `rg --files -g '*.md'` is present in the checked-in 26c inventory with an explicit disposition.
- Cohesix-authored canonical docs are audited against generated snippets, manifest fingerprints, fixture outputs, release artifacts, and staged-run evidence before and after prose cleanup.
- Cohesix-authored canonical docs are rewritten to remove generic template prose and to describe the as-built system, invariants, or operator contract.
- Release snapshots, generated snippets, seL4 mirrors, and vendored docs are handled according to their disposition rules; no ad-hoc edits hide provenance.
- Low-risk code/comment cleanup lands with no console grammar, Secure9P semantics, manifest output, telemetry format, or release workflow drift.
- Cleanup candidates previously excluded or weakly covered in CI have characterization tests and staged-run coverage before structural refactors are accepted.
- QEMU full Test Plan run is `PASS` via the staged runner with `stage_01.done` through `stage_05.done` and no `*.incomplete` markers.
- Pi 4 full Test Plan run is `PASS` via the staged runner with the same stage completeness requirements and archived evidence/logs.
- Milestone closure evidence includes before/after reviewer-facing artifacts: representative code diffs, Markdown inventory/disposition, QEMU test-plan state dir, Pi 4 test-plan state dir, and due-diligence outputs.

### Compiler / docsystem touchpoints
- `coh-rtc` generated snippets referenced by `docs/snippets/*.md` remain authoritative; 26c may only change their source schemas or generators, never hand-edit the derived snippet text.
- Release snapshot documentation under `releases/**/*.md` remains derived from canonical docs and release packaging; if snapshot wording changes, the corresponding release-cut flow and notes must change in the same work.
- `docs/TEST_PLAN.md`, `scripts/ci/test_plan_run.sh`, and the stage scripts become authoritative for both QEMU and Pi 4 `PASS` semantics.

### Task Breakdown
```
Title/ID: m26c-authoring-charter-and-header-rules
Goal: Replace generic repository-wide authoring templates with explicit human-authored comment, header, and documentation rules.
Inputs: AGENTS.md, CONTRIBUTING.md, README.md, docs/CODING_GUIDELINES.md, docs/API_GUIDELINES.md, docs/BUILD_PLAN.md
Changes:
  - AGENTS.md — narrow file-header requirements to legal/provenance metadata plus rules for when comments are required.
  - CONTRIBUTING.md — codify review expectations for comment quality, diff shape, and no-regression cleanup sequencing.
  - README.md — align contributor-facing quality language with the new authoring policy.
  - docs/CODING_GUIDELINES.md — define acceptable doc-comment content, prohibited template phrasing, and behavior-preserving cleanup rules.
  - docs/API_GUIDELINES.md — require interface docs to describe contracts, invariants, and failure modes instead of file/module summaries.
Commands:
  - rg -n "Purpose:|Defines the|Provide(s)? the|CLI entry point|library and public module surface|module for" AGENTS.md CONTRIBUTING.md README.md docs/CODING_GUIDELINES.md docs/API_GUIDELINES.md
  - cargo fmt --all -- --check
Checks:
  - Repository guidance no longer instructs contributors to write generic file-summary boilerplate.
  - Header and comment rules are internally consistent across canonical contributor documents.
Deliverables:
  - Canonical authoring policy for human-readable code and docs without hidden behavior changes.

Title/ID: m26c-markdown-inventory-and-disposition
Goal: Inventory and disposition every Markdown file in the repository so canonical, generated, release-derived, and vendored docs are handled correctly.
Inputs: `rg --files -g '*.md' | sort`, root/project docs, apps/**/README.md, crates/**/README.md, docs/**/*.md, releases/**/*.md, seL4/*.md, tests/integration/README.md, tools/**/*.md, third_party/**/*.md
Changes:
  - docs/audit/M26C_MARKDOWN_INVENTORY.md — checked-in inventory/disposition table for every Markdown file returned by `rg --files -g '*.md'`.
  - docs/BUILD_PLAN.md — record milestone scope, disposition rules, and inventory obligations.
  - docs/REPO_LAYOUT.md — document Markdown path classes and which ones are canonical, derived, or vendored.
  - docs/SECURITY.md — capture documentation provenance rules for generated, release, and vendored Markdown when they influence audit evidence.
Commands:
  - rg --files -g '*.md' | sort > out/audit/m26c_markdown_inventory.txt
  - diff -u out/audit/m26c_markdown_inventory.txt <(awk 'NR>2 {print $1}' docs/audit/M26C_MARKDOWN_INVENTORY.md)
Checks:
  - Every repository Markdown file is accounted for exactly once in the checked-in inventory.
  - Each entry has an explicit disposition and an update rule.
Deliverables:
  - Auditable all-Markdown inventory and disposition table covering canonical docs, release snapshots, reference mirrors, and vendored material.

Title/ID: m26c-docs-as-built-audit
Goal: Audit canonical documentation against the actual generated and observed system before any humanizing prose cleanup lands.
Inputs: docs/ARCHITECTURE.md, docs/INTERFACES.md, docs/HOST_TOOLS.md, docs/SECURE9P.md, docs/SECURITY.md, docs/TEST_PLAN.md, docs/HARDWARE_BRINGUP.md, docs/USERLAND_AND_CLI.md, docs/BOOT_REFERENCE.md, docs/FAILURE_MODES.md, docs/OPERATOR_WALKTHROUGH.md, README.md, docs/snippets/*.md, apps/root-task/src/generated/*, out/manifests/root_task_resolved.json, release bundles under releases/**, staged test evidence
Changes:
  - docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md — trace each canonical doc surface to generated snippets, manifests, fixtures, release artifacts, or staged-run evidence.
  - docs/ARCHITECTURE.md + docs/INTERFACES.md + docs/HOST_TOOLS.md + docs/SECURE9P.md + docs/SECURITY.md + docs/TEST_PLAN.md + docs/HARDWARE_BRINGUP.md + docs/USERLAND_AND_CLI.md + docs/BOOT_REFERENCE.md + docs/FAILURE_MODES.md + docs/OPERATOR_WALKTHROUGH.md + README.md — correct any prose that does not match the current as-built system before tone cleanup begins.
  - docs/snippets/*.md + releases/**/*.md — verify provenance and regeneration paths; update only through the proper source or release-cut flow.
Commands:
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json
  - scripts/check-generated.sh
  - scripts/ci/check_test_plan.sh
Checks:
  - Canonical docs can be traced to current manifests, generated snippets, fixtures, release snapshots, or staged-run evidence.
  - Any docs drift is corrected before humanizing edits are accepted.
Deliverables:
  - Auditable docs-as-built report proving documentation truth before presentation cleanup.

Title/ID: m26c-low-risk-surface-cleanup
Goal: Humanize the highest-visibility low-risk code and doc surfaces after docs-as-built alignment is proven.
Inputs: apps/*/README.md, crates/**/README.md, tools/cohesix-py/README.md, tests/integration/README.md, public crate roots under apps/*/src/lib.rs, tests/**/*.rs, docs/OPERATOR_WALKTHROUGH.md, docs/QUICKSTART.md, docs/QUICKSTART_ALPHA.md, docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md
Changes:
  - apps/*/README.md + crates/**/README.md + tools/cohesix-py/README.md + tests/integration/README.md — replace template summaries with role-specific descriptions, assumptions, and operator-facing boundaries.
  - apps/*/src/lib.rs + apps/*/src/main.rs + crates/**/src/lib.rs — remove generic module-surface comments and rewrite doc comments around invariants or usage.
  - tests/**/*.rs — rename template-like test names/descriptions to scenario-driven names and tighten helper naming.
  - docs/OPERATOR_WALKTHROUGH.md + docs/QUICKSTART.md + docs/QUICKSTART_ALPHA.md — remove repetitive prose and keep operator sequences concrete.
Commands:
  - cargo test -p secure9p-core
  - cargo test -p cohsh-core
  - cargo test -p tests --quiet
Checks:
  - High-visibility surface files read as authored, file-specific documentation instead of generated scaffolding.
  - Humanized docs remain consistent with the audit evidence produced by `m26c-docs-as-built-audit`.
  - Test names and doc comments describe behaviors and scenarios, not file names.
Deliverables:
  - First-wave cleanup across low-risk surfaces with zero behavior changes and passing tests.

Title/ID: m26c-characterization-gates-before-refactor
Goal: Add characterization tests and merge gates around cleanup-sensitive crates before accepting structural refactors.
Inputs: .github/workflows/ci.yml, scripts/ci/test_plan_run.sh, scripts/ci/test_plan_stage_*.sh, apps/coh/src/mount.rs, apps/host-ticket-agent/src/*.rs, apps/root-task/src/net/stack.rs, docs/TEST_PLAN.md
Changes:
  - .github/workflows/ci.yml — stop excluding cleanup-sensitive crates without compensating gate coverage, or add explicit targeted jobs for them.
  - scripts/ci/test_plan_stage_02_host_fast.sh — exercise `coh`, `host-ticket-agent`, and other cleanup targets as first-class checks.
  - docs/TEST_PLAN.md — document the expanded cleanup-target coverage and required artifact retention.
  - apps/coh/src/mount.rs + apps/host-ticket-agent/src/*.rs + selected apps/root-task/src/* helpers — add characterization tests for current outputs, errors, and state transitions before refactor.
Commands:
  - cargo test -p coh --features mock
  - cargo test -p host-ticket-agent
  - cargo test -p root-task --lib
  - scripts/ci/check_test_plan.sh
Checks:
  - Cleanup-sensitive crates have deterministic tests that pin current operator-visible behavior.
  - CI and staged test-plan coverage run those tests automatically.
Deliverables:
  - Regression safety net expanded ahead of structural cleanup.

Title/ID: m26c-selected-host-structural-cleanup
Goal: Refactor repetitive host-side validation and processing flows once characterization tests are in place.
Inputs: apps/host-ticket-agent/src/lib.rs, apps/host-ticket-agent/src/claim.rs, apps/host-ticket-agent/src/status.rs, apps/coh/src/mount.rs, docs/ARCHITECTURE.md, docs/HOST_TOOLS.md, docs/INTERFACES.md
Changes:
  - apps/host-ticket-agent/src/lib.rs — factor manifest validation and ticket-processing branches into explicit invariant-bearing helpers without changing lifecycle semantics.
  - apps/host-ticket-agent/src/claim.rs + apps/host-ticket-agent/src/status.rs — tighten naming, error shaping, and test readability while preserving JSON/receipt behavior.
  - apps/coh/src/mount.rs — extract validation and append-only helpers into clearer units without changing FUSE or REST mount behavior.
  - docs/ARCHITECTURE.md + docs/HOST_TOOLS.md + docs/INTERFACES.md — update any as-built explanations needed to reflect clearer code structure while preserving external contracts.
Commands:
  - cargo test -p host-ticket-agent
  - cargo test -p coh --features mock
  - cargo clippy --workspace --all-targets -- -D warnings
Checks:
  - Host-side cleanup reduces repetition and makes invariants explicit without protocol or file-layout drift.
  - Characterization tests stay unchanged and passing across refactors.
Deliverables:
  - Human-readable host-side control-plane code with preserved external behavior.

Title/ID: m26c-full-test-plan-qemu-and-pi4
Goal: Make full staged Test Plan PASS on both QEMU and Pi 4 the explicit closure gate for 26c.
Inputs: docs/TEST_PLAN.md, docs/HARDWARE_BRINGUP.md, scripts/ci/test_plan_run.sh, scripts/ci/test_plan_stage_*.sh, scripts/pi4-image-build.sh, scripts/uboot/qemu-uboot-smoke.sh
Changes:
  - docs/TEST_PLAN.md — define QEMU and Pi 4 execution matrices, evidence paths, and PASS criteria using the same staged contract.
  - docs/HARDWARE_BRINGUP.md — document Pi 4 hardware setup, log capture, and operator prerequisites needed to run the full Test Plan.
  - scripts/ci/test_plan_run.sh + scripts/ci/test_plan_stage_*.sh — add target selection and evidence handling so the same runner can execute and archive QEMU and Pi 4 runs.
  - scripts/pi4-image-build.sh — produce deterministic artifacts consumed by Pi 4 staged test-plan runs.
Commands:
  - scripts/ci/test_plan_run.sh --state-dir out/test-plan/m26c-qemu
  - COHESIX_TEST_TARGET=pi4 scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml
  - COHESIX_TEST_TARGET=pi4 scripts/ci/test_plan_run.sh --state-dir out/test-plan/m26c-pi4
Checks:
  - The staged runner produces `stage_01.done` through `stage_05.done` for both QEMU and Pi 4 state dirs.
  - Neither state dir contains `*.incomplete` markers.
  - Pi 4 evidence includes serial logs, network/console validation, and due-diligence outputs matching the documented PASS contract.
Deliverables:
  - Full Test Plan PASS on QEMU and Pi 4 as the hard milestone exit criterion, with archived evidence for both targets.
```

---

## Milestone 27 — Pi 4 On-Device Spool Stores + Settings Persistence <a id="27"></a> 
[Milestones](#Milestones)

**Why now (resilience):** After Pi 4 U-Boot boot + identity (26), edge deployments need store/forward for telemetry and minimal settings that survive reboots and link outages without introducing a general filesystem or new protocols.

**Non-negotiable constraints**
- No changes to console grammar, 9P semantics, or TCP behavior vs VM unless profile‑gated and documented.
- No POSIX VFS; no general filesystem.
- Pure Rust userspace; no C‑FFI filesystems.
- Persistence is exposed only through NineDoor nodes (file‑shaped, bounded).

### Prerequisite
- Milestone **26** completed (Pi 4 U-Boot boot chain + device identity).

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
- Spool/settings metadata binds to the **manifest fingerprint** from 26 (e.g., recorded in `/proc/boot`), without introducing new trust roots.

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
- VM vs Pi 4 boot profile semantics remain byte‑stable unless explicitly profile‑gated.
- Regression pack passes unchanged; new tests are additive.

### Compiler touchpoints
- `coh-rtc` emits persistence limits (record size, max bytes, policy mode) into manifest IR; docs import the generated snippets.
- Manifest validation rejects persistence when storage devices are missing or mis‑declared for the Pi 4 boot profile.

### Task Breakdown
```
Title/ID: m27-spool-core
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

Title/ID: m27-spool-namespace
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

Title/ID: m27-settings-store
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

Title/ID: m27-spool-regressions
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


## Milestone 28 — Operator Utilities: Inspect, Trace, Bundle, Diff, Attest <a id="28"></a>
[Milestones](#Milestones)

**Why now (operator & adoption):**  
By this stage Cohesix is architecturally complete, SMP-aware, and Pi 4 bare-metal capable. What remains is operability: giving operators and integrators deterministic tools to understand, reproduce, compare, and prove system behavior without expanding the VM TCB or introducing new protocols.

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
  - Pi 4 boot profile (where applicable)

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
After Milestone 28:
- Cohesix is operable, not just correct
- Incidents are explainable and reproducible
- Operators can reason about state without guesswork
- Support and integration costs drop sharply
- The control plane remains small, auditable, and boring

## Milestone 28b — Authority Hardening: Delegated REST Identity, Fenced Failover, Idempotent Queen Intents <a id="28b"></a>
[Milestones](#Milestones)

**Why now (risk closure):**  
Milestones 25g and 25h delivered host tickets, federation relay, and bounded WAL behavior. As-built deployments still have two critical exposure points:
1) REST callers collapse into one gateway-attached Queen principal, and
2) failover correctness depends on external fencing discipline rather than deterministic writer fencing in the control plane path.

This milestone closes those gaps using existing as-built mechanisms (`hive-gateway`, `cohsh-core` ticket claims, `/host/tickets/*`, relay WAL, manifest compiler) without introducing new VM protocols or relaxing single-writer semantics.

---

## Goal
Strengthen authority and failover guarantees while preserving current transport and grammar boundaries:
1. Require caller-scoped delegated authority for mutating REST operations.
2. Make Queen control intents replay-safe with deterministic idempotency keys.
3. Add explicit writer-epoch fencing for failover and relay paths.
4. Eliminate fixture/bootstrap secret usage from release profiles.
5. Ship production profiles with audit/replay enabled and bounded by manifest limits.

---

## Non-Goals (Explicit)
- No active/active multi-queen writers for one logical hive.
- No in-VM HTTP services or new RPC channels.
- No changes to ACK/ERR/END grammar or Secure9P transport framing.
- No best-effort reconciliation loops or autonomous remediation behavior.

---

## Deliverables

### 1) Delegated REST Identity (No Shared Queen Principal for Writes)
**Purpose:** Preserve gateway multiplexing while restoring caller-level capability boundaries.

Implementation requirements:
- Mutating REST routes (`/v1/fs/echo` and equivalent write paths) require:
  - gateway request-auth token, and
  - delegated capability ticket header (`x-cohesix-ticket`), validated using existing ticket claims rules.
- Gateway maintains a bounded upstream session pool keyed by delegated ticket identity (hash), not a single shared writer session.
- Delegated ticket claims (`role`, `subject`, `mount scopes`, `budgets`) constrain what each REST caller can mutate.
- Read-only REST routes may remain gateway-role scoped in compatibility mode, but production profile defaults require delegated tickets for all mutating paths.

As-built leverage:
- Reuse `cohsh-core` ticket parsing/validation and existing ticket quota semantics.
- Reuse deterministic permission errors (`EPERM`, `ELIMIT`) and existing audit logging surfaces.

---

### 2) Idempotent Queen Control Grammar
**Purpose:** Ensure retries/replays never duplicate side effects on `/queen/ctl`.

Implementation requirements:
- Introduce strict envelope schema for `/queen/ctl` writes:
  - required: `schema`, `id`, `idempotency_key`, `issued_unix_ms`, `cmd`.
- Add bounded dedupe table in root-task authority path keyed by `id + idempotency_key`.
- Duplicate intent behavior is deterministic:
  - no side effects repeated,
  - stable acknowledgement path,
  - audit line records dedupe decision.
- Provide read-only introspection node for recent dedupe outcomes (bounded bytes/entries).

As-built leverage:
- Align with existing idempotency model used by `/host/tickets/spec`.
- Keep all writes append-only and routed through existing authority flow.

---

### 3) Writer-Epoch Fencing and Failover Determinism
**Purpose:** Replace implicit human/process fencing with explicit, verifiable writer epochs.

Implementation requirements:
- Add monotonic writer epoch to host control ticket flows and relay envelopes.
- Host-ticket-agent and relay pipeline reject stale-writer intents deterministically.
- Expose read-only writer-epoch/fence state for operator diagnostics and evidence correlation.
- Update failover runbook to require epoch promotion before standby becomes writable.

As-built leverage:
- Reuse existing `/host/tickets/{spec,status,deadletter}` lifecycle and relay WAL.
- Reuse existing failover active/standby model and single-writer policy.

---

### 4) Production Secret and Key Discipline
**Purpose:** Remove bootstrap/fixture secret assumptions from release-grade configurations.

Implementation requirements:
- Replace literal ticket secrets and fixture CAS signing key usage in production manifests with secret references.
- `coh-rtc` rejects release profiles that include:
  - fixture signing key paths,
  - bootstrap/default secret literals.
- Host tooling/docs define deterministic env/file resolution order for ticket secrets and CAS signing keys.
- Add rotation tests proving key rollover works without protocol changes.

As-built leverage:
- Extend existing manifest validation and due-diligence secret-hygiene checks from Milestone 25b.

---

### 5) Production Audit/Replay Baseline
**Purpose:** Make post-incident reconstruction and failover verification first-class in production profiles.

Implementation requirements:
- Production profile defaults:
  - `ecosystem.audit.enable = true`
  - `ecosystem.audit.replay_enable = true`
  - bounded retention values sized for fleet operations.
- Evidence packs include writer-epoch and dedupe state snapshots.
- Release gate fails if production profile disables audit/replay without explicit exception record.

As-built leverage:
- Use current `/audit/*`, `/replay/*`, `coh evidence pack`, and timeline tooling.

---

## Commands
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
- `cargo test -p hive-gateway`
- `cargo test -p coh`
- `cargo test -p cohsh`
- `cargo test -p root-task`
- `cargo test -p tests --test host_ticket_agent`
- `cargo test -p tests --test failover`
- `scripts/cohsh/run_regression_batch.sh`

---

## Checks (Definition of Done)
- Mutating REST request without delegated ticket is denied deterministically and audited.
- Delegated caller cannot exceed ticket scopes/quotas even when gateway is multiplexing many clients.
- Duplicate `/queen/ctl` intent (`id + idempotency_key`) never repeats side effects.
- Stale writer epoch is rejected deterministically across local and relayed host tickets.
- Production manifest/profile fails validation if fixture/default secrets are present.
- Production profile surfaces `/audit/*` and `/replay/*`; evidence packs include epoch and dedupe state.
- Regression pack passes unchanged; only additive fixtures are introduced.

---

## Compiler touchpoints
- `coh-rtc` schema/version update for:
  - delegated-ticket enforcement policy on REST mutating paths,
  - `/queen/ctl` envelope schema and dedupe bounds,
  - writer-epoch fencing policy and relay requirements,
  - production secret references,
  - audit/replay required defaults for release profiles.
- Generated snippets refreshed in:
  - `docs/INTERFACES.md`
  - `docs/ARCHITECTURE.md`
  - `docs/SECURITY.md`
  - `docs/FAILOVER.md`

---

## Task Breakdown
```
Title/ID: m28b-rest-delegated-identity
Goal: Require delegated capability tickets for mutating REST operations while preserving gateway multiplexing.
Inputs: apps/hive-gateway, apps/coh, apps/cohsh, docs/HOST_API.md, docs/API_GUIDELINES.md
Changes:
  - apps/hive-gateway/src/main.rs — require x-cohesix-ticket on mutating routes and enforce ticket-scoped upstream session routing.
  - apps/hive-gateway/src/auth.rs — delegated ticket validation + bounded cache keyed by ticket hash.
  - apps/coh/src/rest.rs — pass delegated ticket header for mutating REST calls.
  - apps/cohsh/src/transport/rest.rs — propagate delegated ticket on write paths.
Commands: cargo test -p hive-gateway && cargo test -p coh && cargo test -p cohsh
Checks: Writes without delegated ticket fail deterministically; writes with scoped ticket succeed only within claims.
Deliverables: Gateway no longer executes all writes as an undifferentiated shared Queen principal.

Title/ID: m28b-queen-ctl-idempotency
Goal: Add deterministic idempotency for /queen/ctl intents.
Inputs: apps/root-task, apps/nine-door, docs/INTERFACES.md
Changes:
  - apps/root-task/src/control/queen_ctl.rs — strict envelope parser with required id/idempotency_key and dedupe guard.
  - apps/root-task/src/control/dedupe.rs — bounded dedupe table with deterministic eviction and audit lines.
  - apps/nine-door/src/host/proc.rs — read-only dedupe status surface for operators.
Commands: cargo test -p root-task && cargo test -p nine-door
Checks: Duplicate intent never repeats side effects; deterministic audit and /proc visibility prove dedupe behavior.
Deliverables: Replay-safe Queen control grammar with bounded dedupe state.

Title/ID: m28b-failover-epoch-fencing
Goal: Enforce monotonic writer-epoch fencing across local and federated host ticket flows.
Inputs: apps/host-ticket-agent, docs/FAILOVER.md, docs/INTERFACES.md
Changes:
  - apps/host-ticket-agent/src/spec.rs — add writer_epoch validation to host-ticket schemas.
  - apps/host-ticket-agent/src/relay.rs — enforce stale epoch rejection and WAL replay ordering by epoch.
  - apps/host-ticket-agent/src/status.rs — expose bounded epoch/fence counters for evidence and diagnostics.
Commands: cargo test -p host-ticket-agent && cargo test -p tests --test failover
Checks: Stale writer epoch intents are rejected deterministically and deadlettered; promoted epoch resumes writes.
Deliverables: Explicit, verifiable writer fencing for active/standby failover.

Title/ID: m28b-production-secret-profile
Goal: Remove fixture/default secrets from release-grade manifests and enforce secret-reference policy.
Inputs: configs/root_task.toml, tools/coh-rtc, docs/SECURITY.md
Changes:
  - tools/coh-rtc/src/validate.rs — reject fixture key paths and bootstrap/default secret literals in production profiles.
  - configs/root_task.toml — introduce secret-reference fields for ticket and CAS signing material.
  - scripts/ci/due_diligence_gate.sh — extend hardcoded-secret checks to generated manifests/profiles.
Commands: cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json && scripts/ci/due_diligence_gate.sh
Checks: Production profile generation fails on fixture/default secrets and passes with secret references.
Deliverables: Release-manifest secret hygiene is compiler-enforced, not convention-only.

Title/ID: m28b-audit-replay-production-default
Goal: Ship production profile with audit/replay enabled and include fencing/dedupe state in evidence packs.
Inputs: configs/root_task.toml, apps/coh, docs/TEST_PLAN.md
Changes:
  - configs/root_task.toml — production profile audit/replay defaults and bounded retention values.
  - apps/coh/src/evidence.rs — include writer-epoch and dedupe status snapshots in evidence export.
  - docs/TEST_PLAN.md — add production-profile gate assertions for /audit and /replay availability.
Commands: cargo test -p coh && cargo test -p tests --test evidence && scripts/cohsh/run_regression_batch.sh
Checks: Evidence includes audit/replay plus fencing/dedupe state; regression pack remains byte-stable.
Deliverables: Audit-first production baseline with deterministic incident reconstruction inputs.
```

---

## Outcome
After Milestone 28b:
- REST multiplexing retains convenience without collapsing caller identity.
- Failover safety relies on deterministic writer fencing, not operator luck.
- Queen control retries are safe by construction.
- Release profiles enforce key hygiene and enable audit-grade reconstruction by default.

## Milestone 29 — Edge Local Status (Pi 4 Host Tool)  <a id="29"></a> 
[Milestones](#Milestones)

**Why now (compiler):** Field techs need offline status on edge devices using the same 9P grammar. Tool must respect Pi 4 boot profile semantics and attestation outputs.

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
- `coh-rtc` emits localhost binding guidance and attestation paths for Pi 4 boot profile into `docs/HARDWARE_BRINGUP.md` and `docs/USERLAND_AND_CLI.md`.

**Task Breakdown**
```
Title/ID: m29-status-tool
Goal: Build coh-status for offline/local status reads over 9P/TCP.
Inputs: apps/coh-status/, Pi 4 boot profile manifest outputs, attestation nodes.
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

Title/ID: m29-attest-verify
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

## Milestone 30 — AWS AMI (UEFI → Cohesix, ENA, Diskless 9door)  <a id="30"></a> 
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
- Reuse the Milestone 26b `no_std` DHCP core and add ENA binding + TCP/TLS client integration in root-task (post-seL4), with no firmware networking.
- Diskless bootstrap path **after seL4**: ENA → DHCP (26b core) → TCP → TLS → 9door mount.
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
Title/ID: m30-uefi-esp
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

Title/ID: m30-ena-adminq
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

Title/ID: m30-ena-io
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

Title/ID: m30-net-bootstrap
Goal: Network bootstrap to fabric (post-seL4, in root-task).
Inputs: apps/root-task net stack, TLS helpers, docs/AWS_AMI.md.
Changes:
- apps/root-task/src/net/dhcp.rs — adapt/reuse Milestone 26b DHCP core for ENA link and AWS-specific bounds.
- apps/root-task/src/net/tcp.rs — TCP bring-up for long-lived sessions.
- apps/root-task/src/net/tls.rs — fabric-auth TLS handshake.
- apps/root-task/src/net/bootstrap.rs — deterministic sequencing and retries.
Commands:
- cargo test -p root-task --test net_bootstrap
Checks:
- Network reaches "fabric-ready" state within defined bounds.
Deliverables:
- Bootstrap timing guarantees recorded.

Title/ID: m30-imdsv2-bootstrap
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

Title/ID: m30-fabric-mount
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

Title/ID: m30-ami-pipeline
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

----
**Tracked Activities**
----
## Activity — seL4 Build Artifact Prune (Repo Only)

**Status:** Planned.

**Purpose:** Reduce repo-local seL4 build trees to the minimal artifacts required for Cohesix builds while preserving kernel truth outputs under `seL4/build/`.

**Constraints**
- Repo-only pruning; upstream seL4 trees under `~/seL4` are untouched.
- Keep kernel truth outputs: `kernel/gen_headers/**`, `kernel/generated/**`, and config headers.
- Preserve build/run dependencies (`elfloader`, `kernel.elf`, `libsel4.a`, libsel4 headers, and config files).
- Cohesix must build and stage with `SEL4_BUILD_DIR` pointing at the pruned trees.

**Inputs**
- `seL4/build/`
- `seL4/SMP_build/`
- `scripts/cohesix-build-run.sh`
- `crates/sel4-sys/build.rs`
- `apps/root-task/build.rs`

**Runbook (repo only)**
1) Build allowlists for both trees and remove everything else.
2) Build Cohesix with `SEL4_BUILD_DIR` set to each tree.
3) Validate GIC detection against the pruned config headers.

**Checks**
- `SEL4_BUILD_DIR=... cargo build -p root-task --target aarch64-unknown-none` succeeds.
- `SEL4_BUILD_DIR=... scripts/cohesix-build-run.sh --no-run --cargo-target aarch64-unknown-none` succeeds.
- `scripts/lib/detect_gic_version.py <kernel/gen_config/kernel/gen_config.h>` returns a version.

**Deliverables**
- Repo-local seL4 trees pruned to the minimal allowlist.

## Activity — Security Evidence Demo (Post-M24, NIST 800-53 LOW)

**Status:** Complete.

**Purpose:** Demonstrate the evidence-based NIST 800-53 LOW mapping for Cohesix using the machine-checkable registry and guard scripts; no runtime behavior changes.

**Constraints**
- No code changes; run the demo against the current repo state and artifacts.
- Evidence is repo-local; no external URLs.
- This is a mapping + evidence guard, not a compliance claim.

**Inputs**
- `docs/SECURITY_NIST_800_53.md`
- `docs/nist/controls.toml`
- `tests/security/nist_evidence_smoke.sh`

**Runbook (host only)**
1) Validate registry and evidence links:
   - `cargo run -p security-nist -- check`
2) Generate the markdown summary table:
   - `cargo run -p security-nist -- report-md`
3) Assert documentation invariants:
   - `bash tests/security/nist_evidence_smoke.sh`

**Checks**
- `security-nist -- check` returns success with zero errors.
- `docs/nist/REPORT.md` is generated and reflects the registry.
- Smoke evidence script passes (Secure9P bounds, ACK/ERR ordering, role isolation).

**Deliverables**
- `docs/nist/REPORT.md` regenerated on demand from the registry.

## Activity — Operator-First Demo (Post-M24, No Code Changes)

**Status:** Complete.

**Purpose:** Demonstrate Cohesix as an operator-first control plane using shipped behavior only, with host tools as the primary action surface and SwarmUI as the trustable lens.

**Why host tools (sell the why)**
- They prove the control plane is real infrastructure: leases, telemetry, and PEFT flows are all file-driven and auditable.
- They let operators act without UI magic while SwarmUI verifies what actually happened.

**Constraints**
- No code changes; demo uses release bundle binaries and existing scripts only.
- SwarmUI is the primary surface. Use `cohsh` only when a required action is not available in SwarmUI, and quit SwarmUI before launching `cohsh` (per `docs/QUICKSTART.md`).
- Due to Mac port forwarding issues, run Queen and Worker VMs on Linux hosts for this demo. G5g runs host tools only.
- All ML/inference stays host-side; no CUDA/NVML in the VM.
- All actions use documented Secure9P/console commands and namespaces; no ad-hoc RPC.
- Live GPU bridge publish must be active for non-mock PEFT flows; the demo is blocked if `/gpu/models` is not exposed.

**Inputs**
- `docs/QUICKSTART.md`
- `docs/OPERATOR_WALKTHROUGH.md`
- `docs/GPU_NODES.md`

**VM placement note**
- This demo **does** exercise the Worker VM path described in `docs/GPU_NODES.md`. It aligns with the edge flow in `docs/NETWORK_CONFIG.md` (Jetson outbound connectivity, role-scoped tickets).

**Runbook (documented commands only; SwarmUI-first)**
0) Framing line: “Cohesix is not an ML system. It is a control-plane OS that decides when learning can change a system.”
1) Host readiness on the Mac queen host: `./bin/coh doctor --mock` (omit `--mock` to validate NVML/QEMU on a configured host).
2) Boot queen (QEMU) on a Linux host: `./qemu/run.sh`.
3) Launch SwarmUI on the same Linux host first (observational): `./bin/swarmui`.
   - Live Hive is read-only and reflects sessions/pressure/root-cut and worker activity.
   - Use the embedded Cohesix console prompt in SwarmUI for core verbs (demo it explicitly):
     - `help`
     - `ping`
     - `attach queen`
   - SwarmUI’s embedded console supports core verbs only; CLI-only commands must use `cohsh`.
4) When a required action is not available in SwarmUI, quit SwarmUI and switch to cohsh (host tools drive the story):
   - `./bin/cohsh --transport tcp --tcp-host <queen-host> --tcp-port 31337`
   - `attach queen`
   - `cat /proc/lifecycle/state` (optionally `/proc/lifecycle/reason`, `/proc/lifecycle/since`)
5) Bring up the Jetson Worker VM on a Linux host (architecture-complete path):
   - Boot the Jetson worker VM using the same release bundle runner (on the Jetson host):
     - `./qemu/run.sh`
   - Mint a worker ticket on the queen host (Linux) and pass it to Jetson:
     - `./bin/cohsh --mint-ticket --role worker-heartbeat --ticket-subject jetson-1`
     - (Alternative) `./bin/swarmui --mint-ticket --role worker-heartbeat --ticket-subject jetson-1`
   - On Jetson, attach as the worker role over TCP (outbound only per `docs/NETWORK_CONFIG.md`):
     - `./bin/cohsh --transport tcp --tcp-host <queen-host> --tcp-port 31337 --role worker-heartbeat --ticket "$WORKER_TICKET"`
   - In the Queen view (SwarmUI or cohsh), confirm workers appear under `/worker` before proceeding.
   - If `/worker` is empty, request a queen-side heartbeat spawn to seed a visible worker entry, then re-check:
     - `echo {"id":"spawn-1","target":"/queen/ctl","decision":"approve"} > /actions/queue`
     - `spawn heartbeat ticks=100`
     - `ls /worker`
6) Keep Live Hive active (optional):
   - `echo {"id":"spawn-2","target":"/queen/ctl","decision":"approve"} > /actions/queue`
   - `spawn heartbeat ticks=100`.
7) Host tools prove control-plane surface (Linux queen host or G5g, host tools only):
   - Live GPU bridge publish (required for `/gpu/models` and PEFT):
     - `./bin/gpu-bridge-host --publish --tcp-host <queen-host> --tcp-port 31337 --auth-token changeme --interval-ms 1000 --registry demo/peft_registry`
     - Optional sanity: `./bin/gpu-bridge-host --list`
   - GPU surface (live):
     - `./bin/coh --host <queen-host> --port 31337 gpu list`
     - `./bin/coh --host <queen-host> --port 31337 gpu lease --gpu GPU-0 --mem-mb 4096 --streams 1 --ttl-s 60`
   - Runtime breadcrumbs:
     - `./bin/coh --host <queen-host> --port 31337 run --gpu GPU-0 -- echo ok`
   - Telemetry export (pull):
     - `./bin/coh --host <queen-host> --port 31337 telemetry pull --out demo/telemetry/pull`
8) Telemetry ingest (queen surface; OS-named segments):
   - `telemetry push demo/telemetry/demo.txt --device device-1`
   - or (per walkthrough) `echo '{"new":"segment","mime":"text/plain"}' > /queen/telemetry/dev-1/ctl` then append to `/queen/telemetry/dev-1/seg/seg-000001`
9) Quit cohsh; relaunch SwarmUI to observe effects: `./bin/swarmui`.
   - Live Hive shows bounded telemetry text overlays (last N lines) and a details panel for a selected worker/source.
10) External PEFT (out-of-band): run training off-plane; produce adapter artifacts under `demo/peft_adapter/`.
11) Import + activate (host tool; no in-VM ML):
   - Verify `/gpu/models` is visible (live publish in step 7 must be running).
   - If the model already exists (previous demo run), remove it from the host registry before importing:
     - `rm -rf demo/peft_registry/available/qwen-edge-v1`
   - Live export (requires existing job under `/queen/export/lora_jobs/job_0001/`):
     - `./bin/coh --host <queen-host> --port 31337 peft export --job job_0001 --out demo/peft_export`
   - Live import + publish (refresh `/gpu/models` immediately after registry update):
     - `./bin/coh --host <queen-host> --port 31337 peft import --publish --model qwen-edge-v1 --from demo/peft_adapter --job job_0001 --export demo/peft_export --registry demo/peft_registry`
   - Live activate:
     - `./bin/coh --host <queen-host> --port 31337 peft activate --model qwen-edge-v1 --registry demo/peft_registry`
   - Adapter inputs: `demo/peft_adapter/adapter.safetensors`, `demo/peft_adapter/lora.json`, `demo/peft_adapter/metrics.json`.
   - Verify pointer via cohsh after closing SwarmUI: `ls /gpu/models/available` and `cat /gpu/models/active`
12) Rollback: `./bin/coh --host <queen-host> --port 31337 peft rollback --registry demo/peft_registry`
13) Optional lifecycle control (only when no outstanding leases/workers):
   - `ls /worker` (ensure empty) and confirm no active leases.
   - `lifecycle cordon`, `lifecycle drain`, `lifecycle resume`.

**Checks**
- `coh doctor` passes; QEMU boot ok; cohsh attaches and lifecycle reads return expected values.
- Telemetry segments appear under `/queen/telemetry/<device>/seg/`; ACK/ERR ordering remains deterministic.
- Live GPU bridge publish keeps `/gpu/models` visible without policy errors.
- PEFT import/activate/rollback update `/gpu/models/available` and `/gpu/models/active` per docs.
- Live Hive telemetry text overlays and details panel render bounded lines from live tails.
- No concurrent cohsh + SwarmUI usage; no new semantics introduced.

**Deliverables**
- Demo runbook, `demo/demo_runbook.coh`, and demo assets under `demo/` (no code or release artifact changes).

---

## Activity — LeJEPA Cloud/Edge Demo (Post-M24b, No Code Changes)

**Status:** Complete.

**Purpose:** Demonstrate LeJEPA’s heuristics-free training flow on g5g (ViT-S/16) with an edge-aligned ViT-Ti/16 deployment on Jetson, using Cohesix’s live GPU bridge publish + PEFT import/activate to close the loop without introducing new protocols.

**Constraints**
- No code changes; demo uses existing release bundle binaries and current repo artifacts only.
- All training/inference remains host-side; no CUDA/NVML in the VM.
- Live GPU bridge publish is required; the demo is blocked if `/gpu/models` is not visible.
- Use existing Secure9P/console semantics only; no ad-hoc RPC.
- SwarmUI must not run concurrently with cohsh (quit SwarmUI before cohsh).
- Due to Mac port forwarding issues, run Queen and Worker VMs on Linux hosts for this demo.

**Inputs**
- Models (already installed via Hugging Face):
  - g5g: `/home/models/vit-s16` (WinKawaks/vit-small-patch16-224)
  - Jetson: `/mnt/nvme/models/vit-ti16` (WinKawaks/vit-tiny-patch16-224)
- `docs/GPU_NODES.md`, `docs/HOST_TOOLS.md`, `docs/OPERATOR_WALKTHROUGH.md`
- Release bundle binaries on Mac (Queen host), Jetson (Worker VM), and g5g (host tools)

**Runbook (documented commands only)**
0) Verify model dirs (host-side only):
   - g5g: `ls /home/models/vit-s16`
   - Jetson: `ls /mnt/nvme/models/vit-ti16`
1) Boot Queen on a Linux host: `./qemu/run.sh`
2) Launch SwarmUI on the same Linux host: `./bin/swarmui`
   - Use the embedded console for `help`, `ping`, `attach queen`.
3) Start Live GPU Bridge publish on g5g (host tools only):
   - `./bin/gpu-bridge-host --publish --tcp-host <queen-host> --tcp-port 31337 --interval-ms 1000 --registry /home/models/peft_registry`
   - Sanity: `./bin/coh --host <queen-host> --port 31337 gpu list`
   - Confirm `/gpu/models` is visible (quit SwarmUI first if using cohsh):
     - `./bin/cohsh --transport tcp --tcp-host <queen-host> --tcp-port 31337`
     - `ls /gpu/models`
     - `ls /gpu/telemetry`
4) Bring up Jetson Worker VM (edge path):
   - Boot worker VM on Jetson: `./qemu/run.sh`
   - Mint ticket on Queen host (Mac):
     - `./bin/cohsh --mint-ticket --role worker-heartbeat --ticket-subject jetson-1`
   - Attach from Jetson (outbound only):
     - `./bin/cohsh --transport tcp --tcp-host <queen-host> --tcp-port 31337 --role worker-heartbeat --ticket "$WORKER_TICKET"`
   - Verify worker presence on Queen:
     - `ls /worker` (or `/shard/<label>/worker` if sharding enabled)
5) LeJEPA training (host-side, outside Cohesix):
   - Run your LeJEPA training harness on g5g using `/home/models/vit-s16` as the base.
   - Emit bounded telemetry records that conform to `gpu-telemetry/v1` via the bridge (no schema changes).
   - Produce adapter artifacts into `/home/models/lejepa/adapter/` (e.g., `adapter.safetensors`, `lora.json`, `metrics.json`).
6) Import + publish adapter (live refresh into `/gpu/models`):
   - If the model already exists (previous demo run), remove it from the host registry before importing:
     - `rm -rf /home/models/peft_registry/available/lejepa-edge-v1`
   - `./bin/coh --host <queen-host> --port 31337 peft import --publish --model lejepa-edge-v1 --from /home/models/lejepa/adapter --job job_0002 --export /home/models/lejepa/export --registry /home/models/peft_registry`
   - `./bin/coh --host <queen-host> --port 31337 peft activate --model lejepa-edge-v1 --registry /home/models/peft_registry`
   - Verify pointer (quit SwarmUI before cohsh):
     - `ls /gpu/models/available`
     - `cat /gpu/models/active`
7) Observe Live Hive overlays (SwarmUI):
   - Relaunch SwarmUI and confirm telemetry text overlays + details panel show bounded lines.
   - Confirm the active model id appears in the worker telemetry stream (per existing schema/labels).
8) Edge validation (Jetson host-side inference):
   - Load `/mnt/nvme/models/vit-ti16` and apply the newly published adapter (host-side only).
   - Confirm telemetry continues to flow into `/gpu/telemetry` and `/queen/telemetry`.

**Checks**
- `/gpu/models` is visible after publish and contains `lejepa-edge-v1`.
- `peft import` and `peft activate` update `/gpu/models/available` and `/gpu/models/active`.
- Live Hive overlays show bounded telemetry lines (no UI polling logic).
- All training/inference remains host-side; no in-VM GPU or new RPC.

**Deliverables**
- Demo notes and artifacts under `demo/` (no code changes, no release bundle changes).

---

## Activity — Jetson Orin Nano Gesture Language Demo (Post-M24d, OSS Only)

**Status:** Planned.

**Purpose:** Train and deploy an OSS-only gesture command language (10-20 commands) using a Jetson Orin Nano 8GB + webcam, while demonstrating Cohesix GPU leasing, telemetry, and `/gpu/models` publish/activate without introducing new protocols or VM-side ML.

**Constraints**
- No Cohesix code changes; demo uses existing release bundle binaries and current repo artifacts only.
- OSS-only stack; avoid NC/ND datasets or licenses that restrict derivative use.
- Training/inference runs host-side on Jetson; no CUDA/NVML in the VM.
- Use existing Secure9P/console semantics only; no ad-hoc RPC.
- Live GPU bridge publish must be active for `/gpu/models` and `/gpu/telemetry/schema.json` visibility.
- SwarmUI must not run concurrently with cohsh (quit SwarmUI before cohsh).

**Inputs**
- `docs/GPU_NODES.md`
- `docs/HOST_TOOLS.md`
- `docs/OPERATOR_WALKTHROUGH.md`
- OSS stack (example): MediaPipe (Apache-2.0), PyTorch (BSD), OpenCV (Apache-2.0), HaGRID (CC BY-SA) or ASL Alphabet (CC BY 4.0).

**Runbook (documented commands only; host-side training)**
0) Boot Queen on a Linux host: `./qemu/run.sh`
1) Start SwarmUI on the same Linux host: `./bin/swarmui`
2) On Jetson, start live GPU bridge publish to the Queen:
   - `./bin/gpu-bridge-host --publish --tcp-host <queen-host> --tcp-port 31337 --interval-ms 1000 --registry /mnt/nvme/models/gesture_registry`
3) On Jetson, lease the GPU for training:
   - `./bin/coh --host <queen-host> --port 31337 gpu lease --gpu GPU-0 --mem-mb 4096 --streams 1 --ttl-s 3600`
4) Capture dataset and train (host-side, OSS-only):
   - Use a hand-landmark pipeline (e.g., MediaPipe) to record sequences from the webcam into a local dataset.
   - Train a lightweight temporal classifier on landmark sequences; export artifacts under `/mnt/nvme/models/gesture/adapter/`.
5) Publish model registry and activate (Jetson host tools):
   - `./bin/coh --host <queen-host> --port 31337 peft import --publish --model gesture-ctl-v1 --from /mnt/nvme/models/gesture/adapter --job job_0003 --export /mnt/nvme/models/gesture/export --registry /mnt/nvme/models/gesture_registry`
   - `./bin/coh --host <queen-host> --port 31337 peft activate --model gesture-ctl-v1 --registry /mnt/nvme/models/gesture_registry`
6) Verify visibility (quit SwarmUI before cohsh):
   - `./bin/cohsh --transport tcp --tcp-host <queen-host> --tcp-port 31337`
   - `ls /gpu/models/available`
   - `cat /gpu/models/active`
7) Live inference loop (Jetson host-side):
   - Run the gesture recognizer against the webcam and emit bounded telemetry lines tagged with `model_id=gesture-ctl-v1` into `/queen/telemetry/*` via existing tools.

**Checks**
- `/gpu/models` is visible and contains `gesture-ctl-v1` after publish.
- Telemetry records conform to existing `gpu-telemetry/v1` schema; no new paths introduced.
- Training/inference remains host-side; no VM or Cohesix code changes.
- No concurrent cohsh + SwarmUI usage.

**Deliverables**
- Demo notes and artifacts under `demo/` (no code changes, no release bundle changes).
- Registry content under `/mnt/nvme/models/gesture_registry` (host-side only).

---

## Activity — SwarmUI UI Presentation Regression (Post-M24, Playwright)

**Status:** Complete.

**Purpose:** Add a UI-only regression layer for SwarmUI rendering, wiring, and transcript parity without changing control-plane behavior.

**Constraints**
- UI-only: no new verbs, no new protocols, no control-plane assertions.
- Replay-first determinism (UI must be driven from fixtures).
- Tests target the **latest SwarmUI release bundle** assets; no source-only assumptions.
- No SwarmUI runtime changes; no NineDoor/console semantics changes.
- Use Playwright (Node LTS) and keep browser binaries out of the repo.

**Inputs**
- Release bundle under `releases/` (latest macOS bundle).
- `tests/fixtures/traces/trace_v0.trace` and `tests/fixtures/traces/trace_v0.hive.cbor`.
- `docs/TEST_PLAN.md` (additive section).

**Commands**
- `cd tools/swarmui-ui-tests`
- `npm ci`
- `npx playwright install webkit`
- `SWARMUI_RELEASE_DIR=../releases/<latest> npm test`

**Checks**
- UI tests pass in replay mode without flake.
- Snapshot comparisons are deterministic and stable.
- Console transcript assertions match expected `OK/ERR/END` grammar.
- No changes to SwarmUI runtime logic or transport semantics.

**Deliverables**
- Playwright harness under `tools/swarmui-ui-tests/`.
- `docs/TEST_PLAN.md` updated with the SwarmUI Playwright section.
- Baseline screenshot snapshots committed.

---

## Activity — Warning Cleanup (Post-M24, No Behavior Changes)

**Status:** Complete.

**Purpose:** Remove compiler warnings without altering behavior or interfaces.

**Constraints**
- Warning-only cleanup; no behavioral or API changes.
- No changes to control-plane semantics or test fixtures.

**Inputs**
- `cargo check` output from macOS ARM64.

**Commands**
- `cargo check`

**Checks**
- No new warnings introduced.
- Existing tests and fixtures remain unchanged.

**Deliverables**
- Warning cleanups committed with no behavior changes.

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
