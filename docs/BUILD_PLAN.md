<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define Cohesix milestone scope, status, deliverables, and acceptance criteria. -->
<!-- Author: Lukas Bower -->

# Cohesix Build Plan (ARM64, Pure Rust Userspace)

This document is the normative authorization and status ledger for Cohesix
work. It records both active tasks and retained historical completion context;
historical measurements are not current product claims unless the active
milestone and [BENCHMARKS.md](BENCHMARKS.md) qualify them. Runtime contracts
belong in [ARCHITECTURE.md](ARCHITECTURE.md),
[INTERFACES.md](INTERFACES.md), and the other focused documents linked from
`README.md`. Generated values remain authoritative in the selected manifest,
resolved output, and `coh-rtc` artifacts.

- **Primary host:** macOS 26 on Apple Silicon.
- **Reference target:** QEMU `aarch64/virt` with GICv3.
- **Physical target:** Raspberry Pi 4 using Pi firmware, U-Boot, and the seL4
  binary-image handoff.
- **Kernel:** upstream seL4 from the selected external build directory.
- **Userspace:** pure-Rust `root-task` and NineDoor adapters, implemented
  `worker-heart`, `worker-gpu`, and `worker-lora` target loops,
  manifest-declared Pi 4 driver runtimes, and host-side operator and bridge
  tools.

Milestones build cumulatively. Work may advance only when the active task,
checks, evidence, and documentation agree. Cohesix uses a Queen/Worker control
model over bounded namespace and console operations; it does not introduce an
ad-hoc host RPC authority path.

Current terminology: a **shard** is a manifest-derived worker namespace bucket, and the canonical worker telemetry path is `/shard/<label>/worker/<id>/telemetry`. Older milestone records may mention the legacy `/worker/<id>/telemetry` alias; that alias is valid only when `sharding.legacy_worker_alias = true`.

## seL4 Reference Manual Alignment (v15.0.0)

We treat the official seL4 Reference Manual v15.0.0 ([PDF](https://sel4.systems/Info/Docs/seL4-manual-15.0.0.pdf)) as the authoritative description of kernel semantics. This plan
cross-checks each milestone against the relevant chapters to ensure we remain within the manual's constraints:
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
| [8d](#8d) | In-Session `test` Command + Preinstalled `.coh` Regression Scripts | Complete |
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
| [19](#19) | `cohsh-core` Extraction (Shared Grammar & Transport) | Complete |
| [20a](#20a) | `cohsh` as 9P Client Library | Complete |
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
| [26a](#26a) | Pi 4 Driver-Task Substrate + GENET/Serial/Display Isolation | Complete |
| [26b](#26b) | Pi 4 USB/Wi-Fi Driver Tasks + DHCP/Benchmark Concurrency | Reopened |
| [26c](#26c) | Regression-Gated Refactor + Surface Audit (Zero-Regression) | Complete |
| [26d](#26d) | seL4 15 Baseline Refresh + Reference/Performance Realignment | In Progress |
| [27](#27) | Bounded VM-Local Persistence: Spool Stores + Settings | Pending |
| [27b](#27b) | Formal Verification Baseline + Proof-Carrying Manifests | Pending |
| [27c](#27c) | Core-Local Service-Turn Scheduling (SMP Hot-Path Optimization) | Pending |
| [27d](#27d) | Operator-Lane Scheduler + Multi-Surface Responsiveness | Pending |
| [28](#28) | Operator Utilities: Inspect, Trace, Bundle, Diff, Attest | Pending |
| [28b](#28b) | Authority Hardening: Delegated REST Identity, Fenced Failover, Idempotent Queen Intents | Pending |
| [28b1](#28b1) | Provider Action Registry + Ecosystem Coexistence Conformance | Pending |
| [28c](#28c) | Host-Side AI Coexistence: Delegated Runs, Durable Context, Provider Receipts | Pending |
| [28d](#28d) | MCP/A2A Gateway Projection: Read-Only First, Ticketed Writes Later | Pending |
| [28e](#28e) | VM Cap-Bundle Authority + Structured Fault Lifecycle | Pending |
| [28f](#28f) | SwarmUI Desktop Workbench: Spectrum Shell + Live Hive Continuity | Pending |
| [29](#29) | Edge Local Status (Pi 4 Host Tool) | Pending |
| [29a](#29a) | Pi 4 Root-Shell Hardware Status (`hw-status`) | Pending |
| [29b](#29b) | AI-Native Namespace Surfaces (Control-Plane Only) | Pending |
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
## Milestone 7c — TCP transport parity while retaining existing flows <a id="7c"></a>
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

## Milestone 7d — ACK/ERR broadcast is implemented across serial and TCP <a id="7d"></a>
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
  - `configs/generated/root_task_resolved.json` — serialised IR with SHA-256 fingerprint stored alongside.
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
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json --cli-script scripts/cohsh/boot_v0.coh`
- `cargo check -p root-task --no-default-features --features kernel,net-console`
- `cargo test -p root-task`
- `cargo test -p tools/coh-rtc`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/boot_v0.coh`

**Checks (DoD)**
- Regeneration is deterministic: two consecutive runs of `cargo run -p coh-rtc …` produce identical Rust, JSON, and CLI artefacts (verified via hash comparison recorded in `configs/generated/root_task_resolved.json.sha256`).
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
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json --cli-script scripts/cohsh/boot_v0.coh`
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
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`
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
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
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
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`
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
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
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
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`
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

## Milestone 14 — Sharded Namespaces & Provider Split <a id="14"></a> 
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
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`

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
- Manifest IR v1.3: `client_policies.cohsh.pool`, `client_policies.retry`, `client_policies.heartbeat`. Compiler emits `configs/generated/cohsh_policy.toml` consumed at runtime (CLI loads it on start, failing if missing/out-of-sync).
- CLI regression `scripts/cohsh/session_pool.coh` demonstrating increased throughput under load and safe recovery from injected failures.
- Docs (`docs/USERLAND_AND_CLI.md`) describe new CLI flags/env overrides, referencing manifest-derived defaults.

**Status:** Complete — session pooling, retry policies, policy hashing, CLI regression coverage, and docs updates are in place; regression pack is green.

**Commands**
- `cargo test -p cohsh`
- `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/session_pool.coh`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`

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
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`
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
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`
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

**Status:** Complete. The bounded `m18-remove-hallucinated-lora-radio` defect
correction, discovered during the Milestone 26c documentation audit, closed on
2026-07-15. The implemented MODBUS and DNP3 sidecar contract remains complete.
Historical references that attached a radio subsystem to lowercase `lora`
identifiers are invalid: Cohesix does not implement or target that subsystem.
`LoRA`, `worker-lora`, and lowercase `lora` identifiers refer only to the AI
adapter/model lifecycle.

```text
Title/ID: m18-remove-hallucinated-lora-radio
Milestone: Milestone 18 — Field Bus & Low-Bandwidth Sidecars / reopened defect discovered by Milestone 26c README-linked documentation remediation
Goal: Remove the fabricated radio subsystem attached to lora identifiers while preserving the implemented MODBUS/DNP3 sidecars and AI LoRA receipt-only Worker.
Inputs: tools/coh-rtc/src/{ir.rs,codegen}, configs/root_task*.toml, apps/{sidecar-bus,worker-lora,nine-door,root-task}, scripts/cohsh/sidecar_integration.coh, README.md, docs/{GLOSSARY,INTERFACES,SECURITY,ROLES_AND_SCHEDULING,USE_CASES}.md, generated artifacts, current tests, immutable release snapshots.
Changes:
  - tools/coh-rtc, source manifests, and generated artifacts — remove sidecars.lora, the /lora radio namespace, regional-band, transmit, duty-cycle, payload, and radio-tamper schema and outputs; retain AI LoRA worker-role and PEFT schema.
  - apps/sidecar-bus, apps/worker-lora, apps/nine-door, and apps/root-task — remove radio-only APIs and namespace handlers; keep WorkerLora as a bounded AI lifecycle-receipt Worker with no training, inference, model loading, or GPU access in the VM.
  - scripts/cohsh and tests — remove radio assertions and add regressions that reject the retired manifest surface and preserve AI LoRA receipt behavior.
  - README.md and canonical docs — define LoRA once as low-rank adaptation, remove all current radio claims, and cross-reference the Queen export and host PEFT lifecycle contracts.
  - releases/ — leave published snapshots immutable; record the false historical radio claims as superseded current-documentation errata and exclude them from current as-built truth.
Commands:
  - cargo fmt --all -- --check
  - cargo test -p coh-rtc
  - cargo test -p worker-lora
  - cargo test -p sidecar-bus --features modbus,dnp3
  - cargo test -p nine-door
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib ninedoor::tests
  - cargo test -p cohsh --test script_catalog
  - cargo check -p worker-lora --target aarch64-unknown-none
  - cargo clippy --workspace --all-targets -- -D warnings
  - cargo check --workspace
  - cargo test --workspace -- --test-threads=1
  - SEL4_BUILD_DIR=seL4/SMP_build cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-qemu
  - SEL4_BUILD_DIR=seL4/build_UBOOT cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-pi4
  - scripts/cohesix-build-run.sh --no-run --cargo-target aarch64-unknown-none
  - scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml --sel4-kernel-source-dir "$HOME/seL4_15"
  - scripts/check-generated.sh
  - scripts/ci/check_test_plan.sh
  - scripts/ci/test_plan_run.sh --list
  - cargo audit
  - cargo deny check advisories
Checks:
  - Current source, manifests, generated artifacts, tests, scripts, and canonical docs contain no radio-specific region, transmission, duty-cycle, or radio-tamper behavior or claim under lora identifiers.
  - sidecars.lora is rejected as an unknown manifest field; MODBUS/DNP3 sidecars still pass their existing bounded-spool and authority tests.
  - WorkerLora remains an implemented receipt-only AI LoRA control-plane role, and /queen/export/lora_jobs plus host PEFT policy remain unchanged.
  - Generated-artifact drift, focused tests, workspace checks, local links, Markdown structure, and GitHub Mermaid validation pass.
Deliverables: Corrected compiler/runtime/docs suite, regenerated default-profile artifacts, regression evidence, and a release erratum without mutating historical release bundles.
```

**Closure evidence (2026-07-15):** The task commands pass. Compiler schema v1.6
rejects `sidecars.lora`; the canonical resolved-manifest SHA-256 is
`763ef148ed19f1250afdcc2e99611be1668369c6b7c375593af99e3420716f41`.
The QEMU package and stage-only Pi build regenerated their rootservers and
elfloader images; the retired radio paths and identifiers are absent from those
artifacts. Current source and canonical documentation scans contain only the AI
LoRA meaning, intentional negative tests, and this correction record.
Historical release snapshots remain unchanged and are explicitly superseded by
`docs/audit/M26C_DOC_DRIFT_LEDGER.md` entry `M26C-DOC-019`.

**Separate repository gate:** This correction does not close the active
Milestone 26d Pi packaging-size blocker. The regenerated
`seL4/build_UBOOT/elfloader/archive.archive.o.cpio` is 5,001,728 bytes against
the mandatory 4 MiB limit; the preceding checked-in artifact was 5,005,824
bytes. The correction therefore reduced the archive by 4 KiB and did not
introduce the breach, but `scripts/ci/size_guard.sh` remains red until the
Milestone 26d repository-gate task restores that independent invariant.

**Why now (context):** Remaining edge use cases depend on deterministic
adapters for industrial buses. Implementing them as sidecars preserves the
lean `no_std` core while meeting operational demands.

**Goal**
Deliver host-side adapters that bridge MODBUS and DNP3 into NineDoor namespaces,
driven by compiler-declared mounts and capability policies.

**Deliverables**
- Host-side sidecar framework (`apps/sidecar-bus`) offering async runtimes on macOS/Linux with feature gates to keep VM artefacts `no_std`. Sidecars communicate via Secure9P transports or serial overlays without embedding TCP servers in the VM.
- A recognized `worker-bus` template and generated `/bus/*` control/telemetry
  contract. Current selected profiles still mark the target Worker role as not
  implemented; the host-side provider remains the as-built integration path.
- Compiler schema v1.6 fields `sidecars.modbus` and `sidecars.dnp3` describing
  mounts, baud/link settings, and capability scopes; validation keeps resources
  within the event-pump budget.
- Documentation updates (`docs/ARCHITECTURE.md §12`, `docs/INTERFACES.md`) illustrating the sidecar pattern, security boundaries, and testing strategy.
- `scripts/cohsh/sidecar_integration.coh` integrated into the regression pack DoD.

**Historical completion boundary:** The MODBUS/DNP3 host-side framework,
compiler-declared `/bus` contract, CLI regression coverage, and documentation
remain accepted; Milestone 17 boundary remains
`3e6faa33410af58ed8d1942ce58ab701a276b882`. The invalid radio-specific `lora`
surface described above has been removed, so this milestone is closed again.

**Use-case alignment**
- Industrial IoT gateways (Edge §1) gain MODBUS integration without bloating the VM.
- Energy substations (Edge §2) receive DNP3 scheduling and signed config updates.

**Commands**
- `cargo test -p worker-bus`
- `cargo test -p sidecar-bus --features modbus,dnp3`
- `cohsh --script scripts/cohsh/sidecar_integration.coh`

**Checks (DoD)**
- Sidecars operate within declared capability scopes; attempts to access undeclared mounts are rejected and logged.
- Offline telemetry spooling validated for MODBUS/DNP3 adapters with manifest-driven limits.
- For this milestone, run the full Regression Pack both under QEMU and (where applicable) on the target hardware profile, and treat any divergence between the two as a bug unless explicitly documented.
- Sidecar mounts MUST NOT introduce new `/bus` names that collide with legacy namespaces; compiler must hash-prefix automatically if conflicts appear.
- Abuse case: unauthorized write into a sidecar mount returns `ERR` and an
  audit record.

**Compiler touchpoints**
- IR v1.6 ensures mounts, roles, and quotas for sidecars, generating documentation tables and manifest fragments consumed by host tooling.
- Validation prevents enabling sidecars without corresponding host dependencies or event-pump capacity.

**Task Breakdown**
```
Title/ID: m19-sidecar-framework
Goal: Build the host sidecar framework and bounded bus template.
Inputs: apps/sidecar-bus/, apps/worker-bus/, configs/root_task.toml sidecars.modbus and sidecars.dnp3.
Changes:
  - apps/sidecar-bus/src/lib.rs — capability-scoped adapters with feature gates for modbus/dnp3.
Commands:
  - cargo test -p sidecar-bus --features modbus,dnp3
  - cargo test -p worker-bus
Checks:
  - Unauthorized mount access returns ERR; bounded spool behavior is verified in tests.
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
- `configs/generated/cohsh_policy.toml` + generated policy artifacts (via `coh-rtc`).
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
  - configs/generated/cohsh_policy.toml — regenerated.
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
Inputs: apps/swarmui/src/hive.rs, configs/root_task.toml, configs/generated/cohsh_policy.toml.
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
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`
- `.venv/bin/python -m pytest -q tests/test_rest_perf_harness.py`
- `python scripts/rest_perf_harness.py --mode simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-1mb --error-budget-rate 0.01`
- `python scripts/rest_perf_harness.py --mode simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-10mb --error-budget-rate 0.01`
- `python scripts/rest_perf_harness.py --mode simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-100mb --error-budget-rate 0.01`
- `python scripts/rest_perf_harness.py --mode simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-1gb --error-budget-rate 0.01`

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
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
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
  - python scripts/rest_perf_harness.py --mode simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-1mb --error-budget-rate 0.01
  - python scripts/rest_perf_harness.py --mode simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-10mb --error-budget-rate 0.01
  - python scripts/rest_perf_harness.py --mode simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-100mb --error-budget-rate 0.01
  - python scripts/rest_perf_harness.py --mode simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-1gb --error-budget-rate 0.01
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
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`
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
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
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

**Why now (historical planning context):** At Milestone 25h planning time, an
earlier local benchmark snapshot was interpreted as a 1500-worker hard cap,
with gateway pressure observed around 1000-1200 workers per hive. That snapshot
is not a current qualified capacity claim; [BENCHMARKS.md](BENCHMARKS.md) owns
the active evidence and acceptance rules. The architectural conclusion remains:
scale across independently bounded hives instead of introducing active/active
writes to one logical hive. [FAILOVER.md](FAILOVER.md) defines the single-writer
active/standby and host-orchestrated cutover model extended by this milestone.

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
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`
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
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
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
  - python scripts/rest_perf_harness.py --mode simulate --multi-hive --hives 3 --workers-per-hive 1000 --no-retries --error-budget-rate 0.01
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
Upstream seL4 Pi 4 bring-up documentation originally used direct U-Boot image loading examples on `bcm2711`, not a UEFI handoff chain. Cohesix now follows the active staged U-Boot path: `scripts/pi4-image-build.sh` builds `seL4/build_UBOOT`, stages the seL4 binary image, driver-runtime CPIO, and padded DTB, then hands off with `bootm <image> <runtime-cpio> <dtb>`. This preserves deterministic control at the U-Boot prompt while matching the isolated runtime layout used by Milestone 26a/26b.

**Non-negotiable constraints:**  
- Boot chain for Pi 4 Milestone 26 is: `Pi firmware (start4/fixup) -> U-Boot -> seL4 image -> root-task`.
- Milestone 26 acceptance no longer depends on UEFI firmware settings or `BOOTAA64.EFI`.
- Backward compatibility remains mandatory: Pi 4 changes must not break existing QEMU workflows on macOS (`hvf`/`tcg`) or Linux (`kvm`/`tcg`) for `aarch64/virt`.
- Local diagnostics on Pi 4 must reuse the existing root-console parser and command semantics; no new shell grammar or in-VM protocol is permitted.
- Local diagnostics seat remains primitive: USB keyboard input + HDMI text output only; no compositor/windowing stack.
- Milestone 26 remains a strict no-NIC runtime baseline on Pi 4; root-task network bring-up and TCP console reachability on Pi 4 start in Milestone 26a.
- Milestone 26 has no Pi 4 NIC throughput or latency acceptance target; QEMU U-Boot harness timing is script/debug feedback only and cannot be used as Pi hardware latency or network-performance proof.
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
    - `fatload` to load the staged image, runtime CPIO, and DTB into RAM,
    - `bootm` to transfer execution through the staged U-Boot image contract.
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
  - If `hw.local_seat.required=true` and the local-seat declaration or schema cannot be admitted, boot fails deterministically before ticket publication. Linked USB/HDMI runtime backend attach failures keep the serial shell alive with red local-seat acceptance evidence so Pi 4 recovery remains possible.

- **No-NIC baseline validation (Milestone 26 only)**
  - Root-task completes boot/attestation/local-seat bring-up without NIC initialisation.
  - `/proc/boot` emits deterministic evidence that networking is intentionally disabled in Milestone 26 runtime.

---

### Commands
- Build seL4 Pi 4 image:
  - `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml`
- Build U-Boot for Pi 4:
  - `make -C third_party/u-boot rpi_4_defconfig`
  - `make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j$(sysctl -n hw.ncpu)`
- U-Boot boot commands on Pi 4:
  - `fatls mmc 0:1`
  - `fatload mmc 0:1 ${loadaddr} cohesix-image-arm-bcm2711`
  - `fatload mmc 0:1 0x15000000 cohesix-driver-runtimes.cpio.uimg`
  - `fatload mmc 0:1 0x14000000 bcm2711-rpi-4-b.dtb`
  - `bootm ${loadaddr} 0x15000000 0x14000000`
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
- Pi 4 boots through U-Boot using the staged `bootm` image + runtime CPIO + DTB handoff into seL4/root-task with deterministic log ordering.
- `cohesix>` prompt appears on HDMI text output and accepts USB keyboard commands (`help`, `bi`, `caps`, `ping`) with unchanged parser semantics.
- Command responses typed on USB keyboard are visible on HDMI and match serial transcript semantics.
- Manifest fingerprint is printed and matches packaged hash.
- If `hw.attestation.enabled=true`, attestation succeeds and evidence hash matches manifest fingerprint; if unavailable, boot aborts deterministically.
- Milestone 26 runtime on Pi 4 emits deterministic no-NIC evidence and does not require TCP console reachability.
- Root-task post-seL4 paths use HAL-only device access; no direct firmware-service assumptions in runtime code.
- macOS QEMU U-Boot harness can execute U-Boot env/network commands for script validation; Pi 4 hardware remains authoritative for USB/HDMI/NIC proof and QEMU timing is not accepted as Pi hardware latency evidence.
- Existing macOS/Linux QEMU regression scripts continue to pass unchanged unless explicitly profile-gated and documented.

---

### Compiler touchpoints
- `coh-rtc` emits hardware tables for selected profile(s) and docs import them into `docs/HARDWARE_BRINGUP.md` and `docs/ARCHITECTURE.md`.
- `coh-rtc` extends Pi 4 profile schema with bounded local-seat declarations (`enabled`, `required`, declared keyboard/display devices) and validates that required local-seat profiles name matching `required=true` keyboard/display device entries.
- `coh-rtc` enforces Milestone 26 no-NIC runtime gates while allowing U-Boot pre-boot network env declarations for future milestones.
- Migration guard: accept legacy `uefi-aarch64` manifests only through an explicit compatibility path and emit deterministic deprecation diagnostics.

---

## Task Breakdown
```
Title/ID: m26-uboot-bootchain
Goal: Boot Pi 4 via the staged U-Boot `bootm` handoff into seL4/root-task with stable manifest fingerprint output.
Inputs: `seL4/build_UBOOT/images/sel4test-driver-image-arm-bcm2711`, `third_party/u-boot`, Pi firmware boot partition files, profile manifest, staged driver-runtime CPIO, padded Pi 4 DTB.
Changes:
  - `scripts/pi4-image-build.sh` — build deterministic Pi 4 FAT payload (`u-boot.bin` + seL4 image + manifest artifacts).
  - `docs/HARDWARE_BRINGUP.md` — document canonical Pi 4 U-Boot command flow and SD layout.
  - `apps/root-task` — preserve boot fingerprint line ordering relative to serial/local seat.
Commands:
  - `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml`
  - `make -C third_party/u-boot rpi_4_defconfig`
  - `make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j$(sysctl -n hw.ncpu)`
Checks:
  - Pi 4 reaches root-task via staged `bootm <image> <runtime-cpio> <dtb>`; missing/invalid manifest or DTB policy aborts before ticket publication.
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
  - Pi 4 VL805 local-seat remains cold-boot-only: Linux/U-Boot captures may provide layout diagnostics, but root-port power readback, connected/enabled/speed state, and reset must be proven in the current Cohesix boot after command/event-ring proof.
Commands:
  - `cargo check -p root-task`
  - `cargo test -p root-task --features "kernel serial-console" local_seat`
  - `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`
Checks:
  - USB keyboard commands yield identical parser semantics to serial.
  - Pi 4 USB enumeration does not synthesize root-port state from Linux/U-Boot captures after command-ring proof.
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

## Milestone 26a — Pi 4 Driver-Task Substrate + GENET/Serial/Display Isolation <a id="26a"></a>
[Milestones](#Milestones)

**Status:** Complete — the Pi 4 driver-task substrate, serial/display isolation, GENET linked-runtime migration, preserved GENET compatibility behavior, and physical wired TCP proof are accepted as meeting the 26a definition of done. Fresh Genet runs retain clean TCP/cohsh reachability, bounded driver-task service evidence, and no root-owned steady-state physical-driver service path.

**Why now (platform continuity):**  
Milestone 25 established Cohesix's performance model: parallelism comes from isolated seL4 tasks, while root-task authority stays serialized. The first 26a implementation proved Pi 4 GENET reachability, but later Wi-Fi tuning exposed a system-level flaw: hardware drivers still share the cooperative root-task turn. The completed 26a closure converts HAL from a broad in-root provider into a driver admission layer with explicit scheduling contracts, then migrates low-risk hardware paths before CYW43.

**Non-negotiable constraints:**
- No new in-VM listeners or protocols; the authenticated root-task TCP console remains the only in-VM TCP listener.
- Milestone 26a is the first milestone where Pi 4 TCP console reachability is expected; Milestone 26 must remain valid without NIC/TCP on Pi 4.
- DHCP is intentionally out of scope for 26a and is delivered in Milestone 26b; 26a uses static IPv4 only.
- Any VM vs Pi 4 networking differences must be profile-gated, manifest-defined, and documented.
- Driver implementation must remain HAL-bound with bounded queues and deterministic memory budgets.
- Root-task remains the single authority for tickets, console grammar, namespace, policy, and replay; driver tasks own only device progress.
- Every hardware-facing service path must declare a HAL driver-task scheduling contract before it can be serviced. Missing, unbounded, or non-preemptible contracts fail closed.
- Driver tasks receive only their declared MMIO, DMA, IRQ notification, endpoint, fault, and ring capabilities. No driver task may receive Secure9P authority, ticket material, broad namespace state, or a catch-all `KernelHal`.
- Serial RX and USB/local-seat input are the highest-priority service class; HDMI/log flushing and network data are preemptible.
- Direct root-task driver paths are not a steady-state Pi 4 driver model. They may remain only for early/emergency serial output before the driver-task substrate exists and for QEMU/host compatibility profiles; physical Pi 4 steady-state driver service must fail closed until the corresponding independent ring-backed driver task owns the hardware state.
- Backward compatibility is mandatory: 26a network changes must preserve existing macOS/Linux QEMU console and networking workflows unless explicitly profile-gated for Pi 4 `pi4-uboot-aarch64` (with transitional alias support for legacy `uefi-aarch64` manifests).
- QEMU remains compatibility and transport-substrate evidence for 26a, not a Pi 4 wired-NIC latency baseline. Any 26a GENET latency reporting is diagnostic boundedness evidence for the physical static-IPv4 path; Milestone 26b owns same-harness benchmark comparison and the physical-network latency SLO.

### Prerequisite
- Milestone **26** completed (U-Boot boot chain + device identity attestation + local diagnostics seat + bootloader/HAL boundary enforcement).
- Milestone 26 hardware evidence includes deterministic no-NIC boot transcripts for Pi 4 (`pi4-uboot-aarch64`).

### Goal
Add the HAL-enforced driver-task substrate, migrate serial/display and GENET behind explicit scheduling contracts, and preserve the original Pi 4 GENET static IPv4 behavior as the compatibility target.

### Deliverables
- **HAL driver-task admission substrate**
  - Add a HAL-owned driver scheduling contract surface for serial, USB/local-seat, HDMI text, GENET, CYW43, SDIO host, PCIe root, and QEMU compatibility NICs.
  - Each contract declares role, service class, authority, isolation target, per-turn operation/byte/frame budget, queue depth, and blocking policy.
  - Runtime driver construction and polling fail closed when a hardware-facing path does not declare a valid, preemptible contract.
  - The initial compatibility implementation may still run selected drivers in-process, but the same contract is the admission record for the dedicated seL4 task that replaces it.

- **seL4 driver-task substrate**
  - Add root-owned wrappers for driver TCB creation, CSpace/VSpace setup, IPC-buffer installation, badged endpoints, notifications, IRQ binding, fault endpoints, scheduling-context parameters where available, and revocation.
  - As-built substrate work now includes non-MCS TCB priority/scheduling/resume/notification wrappers, CNode revoke, a remote-safe IPC-buffer bind helper, AArch64 VSpace root and boot-ASID-pool assignment wrappers, explicit page/page-table map wrappers for non-root VSpaces, bounded HAL driver-task command/completion rings, Pi 4 bootstrap-created driver TCB attempts for the manifest-selected subset of the seven acceptance-eligible Pi hardware runtime contracts, restricted child CSpaces, command endpoints, notifications, fault endpoint slots, stacks, manifest-selected per-driver SMP affinity through the same `seL4_TCB_SetAffinity` path used by NineDoor and worker TCBs, and tolerant handling for boot-seeded intermediate page tables while mapping driver IPC/stack frames. Physical Pi 4 owner-state boots must show `DRIVER_TASK_BOOT ... affinity_core=<manifest-core>` for the active selected contracts plus aggregate applied affinity before placement proof closes; any `DRIVER_TASK_AFFINITY_DEFERRED ... reason=pi4-child-tcb-affinity-boot-stall-guard` line is stale mitigation evidence and keeps affinity proof red. Physical Pi 4 owner-state boots still defer the optional early TCB-bound notification syscall and emit `DRIVER_TASK_NOTIFICATION_BIND_DEFERRED ... reason=pi4-early-tcb-notification-bind-boot-stall-guard`; endpoint-backed command-ring startup may continue, but notification lifecycle proof remains red until the bind path is reproved. QEMU compatibility smoke may still create all nine declared contracts, including RTL8139 and virtio-net, after virtio networking is ready.
    QEMU virtio compatibility builds keep the same contract set but publish an inactive `qemu-virtio-pre-net-resource-guard` report before network init so failed live-task bootstrap cannot exhaust resources needed by the virtio TCP regression path; with the explicit `qemu-driver-task-smoke` feature, they create all nine declared driver-task TCBs after virtio networking is ready, allocate per-driver VSpaces, assign ASIDs, map only a one-page driver trampoline plus stack/IPC/ring frames, complete a fixed-layout ring command without callback/context pointers, unmap root aliases for code/IPC/stack after the isolated task starts, and emit console-visible `DRIVER_TASK_BOOT_SMOKE phase=post-net-qemu status=summary ... vspace=isolated ipc_abi=shared-ring-command pointer_free_ipc=yes owner_state=not-proven` as QEMU transport-substrate proof.
    Physical Pi 4 builds now require the isolated VSpace constructor for normal driver bootstrap and reject virtual NIC trampoline work from the physical bootstrap set. For generated Pi 4 hot paths, root loads isolated `pi4-driver-*` runtime artifacts only from the raw driver-runtime CPIO embedded into the root-task image by `scripts/pi4-image-build.sh`; when an artifact is found by generated path, HAL maps every bounded `PT_LOAD` page from the runtime ELF plus stack/IPC/ring and declared MMIO/DMA/shared-buffer regions, stages a pointer-free `pi4-driver-abi` runtime-init descriptor containing physical page metadata, semantic resource ranges, bus-alias policy, optional IRQ descriptors, USB/PCIe and CYW43/SDIO bus-link descriptors, and framebuffer metadata, then starts the isolated fixed-ring runtime entry. Physical Pi pre-root bootstrap turns do not sample timer registers; later steady ring latency telemetry uses the EL0 virtual counter. Physical Pi ring turns use `seL4_Call` and isolated runtimes reply after publishing the primitive completion record, so lower-priority driver TCBs do not depend on `Yield` for service time. Runtime-init commands deliver topology and must be followed by hardware service turns before board proof can close.
    Per-driver runtime-image specs are compiler IR under `root_task.driver_images`, generated into the root-task manifest tables, and backed by separately isolated `pi4-driver-*` runtime image artifacts staged by both `scripts/cohesix-build-run.sh` and `scripts/pi4-image-build.sh`; `scripts/pi4-image-build.sh` packages the raw CPIO before the root-task build so the physical Pi image carries the required embedded runtime source. The staged U-Boot CPIO remains audit/packaging evidence and is not a runtime fallback for physical Pi owner-state boots. Generated `code-pages=128` covers the current multi-segment runtime images, including the observed 108-page runtime ELF spans, and the USB xHCI manifest aperture is now 16 pages instead of the earlier 2-page stub.
    Runtime DMA/shared budgets are descriptor-backed and sized to current runtime use: serial shared=4 pages, USB DMA/shared=128/32 pages, HDMI DMA/shared=0/16 pages plus framebuffer, GENET DMA/shared=64/32 pages for a 32-RX/32-TX ring shape, CYW43 DMA/shared=0/64 pages, SDIO DMA/shared=1/32 pages, and PCIe shared=16 pages. SDIO's single low, uncached DMA page is the private Pi firmware-mailbox request buffer for its typed WL_ON power sequence. The child VSpace layout keeps MMIO at `0x70200000`, DMA at `0x70800000`, and shared control at `0x70c00000`; semantic resource ranges carry aggregate page counts while per-page DMA descriptors cover every runtime-owned DMA page.
    Those artifacts implement fixed-ring production service engines rather than smoke-only stubs: serial handles bounded mini-UART init/RX/TX; HDMI renders into the mapped framebuffer; PCIe services primitive mapped-aperture read/write/flush operations; SDIO owns one HAL-declared SDHCI page, one noncontiguous firmware-mailbox page, one private low request page, the fixed Pi 4 WL_ON power contract, and fixed-layout CMD52/CMD53/POLL_IRQ service records; USB owns a direct-root-port xHCI boot-keyboard path with command/event/EP0/interrupt-IN rings; GENET owns MDIO/MAC setup plus bounded RX/TX descriptor-ring turns; and CYW43 owns the shared-control SDPCM command surface behind the pointer-free CYW43/SDIO bus-link descriptor without receiving direct SDHCI MMIO or a direct fallback. Seven generated runtime specs are acceptance-eligible (`root_context_required=false`, `hardware_state_migrated=true`), including `sdio-host` with exactly those two HAL-declared MMIO pages and one private DMA page, but fresh Pi boots activate the selected network role only: Wi-Fi boots select SDIO plus CYW43, while wired boots select GENET. Root hands firmware-mailbox authority to SDIO once only after PCIe/VL805 readiness proof; every later root mailbox call fails closed. An already preseeded mailbox page is transferred by copying its cached HAL-admitted frame capability into the child rather than requesting impossible fresh device-untyped coverage. SDIO retains the Linux-shaped GPIO129 GET-config/polarity, output/low, host power-off, 2 ms off, power-up-while-low, 10 ms settle, WL_ON high, startup-clock, and final 10 ms settle sequence as one-action service phases with virtual-counter deadlines. Firmware success requires the returned zero GPIO token, matching the Raspberry Pi expander ABI. Every property request carries Linux's zero request/response-size word. As in Linux, operation acceptance requires global property-transaction success plus the firmware-overwritten zero GPIO token; per-tag returned-length, tag, and end-marker metadata are not GPIO-consumer rejection predicates. The mailbox token and fixed mapped DMA page still bound the transaction, and protocol failures report reason bits for global status, GPIO token, retained cursor, and mailbox phase. Each firmware-property operation posts the proven Pi 4 VC address once and retains its DMA page while one reply sample is serviced per turn under a 500 ms virtual-counter deadline; root preserves the same request only for the exact mailbox-begin phases. No synchronous mailbox spin, alias replay, GPIO state-only path, confirm-read path, or root fallback remains. Pending phases retain the exact ring intake and publish no premature completion; the 20 ms per-turn contract remains intact. Missing exact SDIO IRQ/DPC proof fails closed, and old-generation CCCR responses cannot prevent physical reset. Fresh Pi hardware proof still has to show useful USB keyboard, HDMI, GENET/DHCP or Wi-Fi/DHCP, SDIO owner-state for Wi-Fi, and PCIe/VL805 behavior from those isolated runtimes before the implementation can be called board-proven.
  - QEMU/host compatibility profiles may still dispatch bounded service callbacks through live driver TCBs or root compatibility paths so existing virtual-device tests keep running. Those paths now enter through a single HAL callback-compatibility gate and a single HAL root-compatibility admission gate; physical Pi 4 builds compile out the callback slot state and both gates fail closed for steady-state hardware service. If the independent ring-backed path is unavailable, the service turn fails closed and acceptance remains red. The May 20 Pi 4 proof is not closure because every driver-task bootstrap failed with `seL4_DeleteFirst` and hot paths stayed root-task compatibility. CYW43, SDIO, and PCIe now have owner-ring service queues: SDIO records CMD52/CMD53/POLL_IRQ completion through the isolated runtime before owner-state can credit `sdio-host`, CYW43 firmware/NVRAM/SDPCM command records return from isolated runtime completions before root hardware execution, and PCIe port read/write/flush helpers return from isolated runtime completions before root hardware execution. Their current acceptance specs still need fresh Pi hardware proof to show the complete SDIO, CYW43, USB, and PCIe hardware state machines are live in the isolated runtimes.
  - The normal physical Pi 4 service path now uses linked-image command/completion rings where implemented and fails closed otherwise. Serial still keeps emergency boot logging alive, but normal UART runtime initialization now runs through the profile-selected `pi4-driver-serial` image and the event pump receives only a `driver-task-serial-client` after that command succeeds. Physical Pi network init selects ring-backed `GenetDriverTaskDevice` / `Cyw43DriverTaskDevice` clients; the isolated GENET image now owns MDIO/MAC plus bounded RX/TX descriptor-ring turns when wired is selected, and the isolated CYW43 image owns shared-control SDPCM command records while SDIO transport authority sits with the profile-selected `sdio-host` image when Wi-Fi is selected. Local-seat init has the same boundary: root has USB/HDMI ring-client shells, the isolated HDMI runtime can render with framebuffer metadata, and the isolated USB runtime owns the xHCI boot-keyboard path. The older `Pi4LocalSeat` root-resident selector handler and root-owned USB support crate have been removed and are not acceptance evidence. The physical Pi `KernelHal` no longer carries a direct `Pi4WifiState` slot and direct Wi-Fi HAL construction returns `pi4-wifi-driver-task-runtime-required`. The HAL still splits root-context diagnostic ring registration from pointer-free selector ring registration; QEMU/host compatibility can keep root-context diagnostic services, while physical Pi linked-image hardware service uses the fixed ring or fails closed. Shared-ring completions are tracked as `shared_ring_roles` and do not satisfy `hot_path=dedicated` or full acceptance; idle completions and zero-result progress completions cannot credit hot-path ownership. The proof layer also requires `DRIVER_TASK_OWNER_STATE_PROOF=yes` backed by per-active-hot-path `DRIVER_TASK_OWNER_STATE ... descriptor=present root_pointer=no` lines for the selected boot set, acceptance-eligible runtime-image specs, and the runtime transport mapping mask complete; `sdio-host` must prove the isolated SDIO runtime completed non-root-context hardware progress before it can credit Wi-Fi owner-state. Owner-state descriptor registration is rejected if the corresponding runtime-image spec is not acceptance-eligible. The QEMU smoke path proves the isolated trampoline, per-driver VSpace/ASID allocation, restricted mapping set, runtime-image declaration accounting, actual transport-region mapping, and pointer-free ring transport, but that is transport-substrate proof rather than Pi hardware hot-path ownership. Any callback-pointer, root-task compatibility, or root-resident selector service turn remains QEMU/host or migration evidence only and fails reopened Pi closure.
  - Root keeps authority and revocation; driver tasks receive only compiler-declared caps and bounded shared rings.
  - seL4 scheduling-context fields are profile-qualified: MCS builds bind explicit scheduling contexts, while non-MCS builds enforce the same contract with TCB priority/domain plus bounded IPC/poll budgets.
  - The substrate must not introduce POSIX threads, implicit async runtimes, unbounded queues, or a second listener/protocol.

- **Serial/display driver-task migration**
  - Move normal UART service behind a `driver-serial` contract and preserve emergency early-boot debug UART fallback only until handoff.
  - Move HDMI/text flushing behind a `driver-display` contract so display refresh cannot block serial input, USB keyboard input, or network control progress.
  - Prove serial command echo and HDMI mirror behavior under synthetic GENET traffic.

- **GENETv5 NIC backend (Pi 4)**
  - Add a root-task driver backend for Broadcom GENETv5, implemented in pure Rust with HAL ownership for MMIO, IRQ, DMA, and cache maintenance.
  - Use this design-reference order for architecture review: Linux `bcmgenet` driver behavior (primary) -> Linux `bcm2711` Pi 4 DT bindings for GENET/MDIO/PHY wiring (secondary) -> U-Boot `bcmgenet` bring-up behavior (tertiary sanity reference).
  - References are design-only inputs; no direct code lift is permitted.
  - Integrate backend selection into existing `NetBackend` plumbing and keep QEMU backends (`rtl8139`/`virtio-net`) unchanged.
  - Promote GENET from in-root compatibility path to `driver-genet` once the frame-driver ABI, DMA ring grants, IRQ notification, and bounded frame IPC pass QEMU and Pi 4 checks.

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
- `cargo test -p sel4-sys`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::driver_task`
- `cargo test -p root-task net:: -- --nocapture`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`
- `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml`
- `scripts/uboot/qemu-uboot-smoke.sh --net user`
- `cargo run -p cohsh --features tcp -- --transport tcp --tcp-host <STATIC_IP> --tcp-port 31337 --script scripts/cohsh/boot_v0.coh`

### Checks (DoD)
- HAL driver-task contracts exist and validate for serial, USB/local-seat, HDMI text, GENET, CYW43, SDIO host, PCIe root, RTL8139, and virtio-net.
- Network stack construction rejects missing or invalid driver scheduling contracts before device initialisation.
- Pi 4 and QEMU driver-task acceptance must distinguish contract declaration from isolation: root-task compatibility service turns are useful diagnostic evidence only, while reopened physical Pi closure requires the manifest-selected active driver-task set to report `DRIVER_TASK_COMPATIBILITY=0`, `DRIVER_TASK_DEDICATED_READY=yes`, role-specific `DRIVER_TASK_SERIAL_DEDICATED=yes`, `DRIVER_TASK_USB_DEDICATED=yes`, `DRIVER_TASK_DISPLAY_DEDICATED=yes`, `DRIVER_TASK_NET_DEDICATED=yes`, selected-role `DRIVER_TASK_SDIO_DEDICATED=yes` for Wi-Fi, and `DRIVER_TASK_PCIE_DEDICATED=yes`, plus `DRIVER_TASK_SUBSTRATE_READY=yes`, `DRIVER_TASK_FAILED_COUNT=0`, `DRIVER_TASK_CAPSET_PROOF=yes`, `DRIVER_TASK_FAULT_PROOF=yes`, `DRIVER_TASK_REVOKE_PROOF=yes`, `DRIVER_TASK_SCHED_PROOF=yes`, `DRIVER_TASK_AFFINITY_PROOF=yes`, selected active affinity configured/applied proof, `DRIVER_TASK_AFFINITY_MANIFEST_PROOF=yes`, `DRIVER_TASK_VSPACE_PROOF=yes`, `DRIVER_TASK_POINTER_FREE_IPC_PROOF=yes`, `DRIVER_TASK_OWNER_STATE_PROOF=yes`, and active-net identity proof under `scripts/pi4_gate_proof.sh --require-driver-task-proof`; QEMU compatibility smoke may still prove all nine declared contracts as transport-substrate evidence only. `DRIVER_TASK_OWNER_STATE_PROOF=yes` requires concrete descriptor proof for `serial-console`, `usb-keyboard`, `hdmi-text`, `pcie-root`, and the selected network role (`genet-nic` for wired, or `cyw43-wifi` plus `sdio-host` for Wi-Fi); SDIO only credits after a non-root-context isolated runtime CMD52/CMD53/POLL_IRQ completion proves hardware progress. Aggregate `owner_state=driver-owned` text is not closure. A `DRIVER_TASK_AFFINITY_DEFERRED ... reason=pi4-child-tcb-affinity-boot-stall-guard` line is stale mitigation evidence, not affinity proof; it must keep `DRIVER_TASK_AFFINITY_PROOF` and `DRIVER_TASK_AFFINITY_MANIFEST_PROOF` red. A `DRIVER_TASK_NOTIFICATION_BIND_DEFERRED ... reason=pi4-early-tcb-notification-bind-boot-stall-guard` line likewise is current mitigation evidence, not notification lifecycle proof. Host tests must keep the pointer-free ring records fixed-layout and primitive-only, prove the physical Pi profile cuts over to driver-task client shells instead of root-owned hardware constructors, and cover the Pi 4 hot-path command catalog for serial console, USB keyboard, HDMI text, GENET RX/TX, CYW43 RX/TX, SDIO host, and PCIe root service. Callback-pointer service turns, including live-TCB callback turns, remain compatibility evidence until the driver state boundary is owned by a ring-backed task. The Pi 4 manifest defaults assign both `bcmgenet-v5` and `cyw43455` to core `3`; boot evidence must show `affinity_core=3` for the selected contract before claiming fourth-core execution.
- Root-owned driver-task substrate can create, monitor, fault-report, and revoke at least one non-authority driver task without changing console grammar.
- Serial and HDMI service remain responsive while synthetic GENET traffic consumes its full allowed budget.
- Pi 4 U-Boot boot reaches root-task network init and reports `GENETv5` backend with static IPv4 from manifest-generated config.
- `cohsh --transport tcp` succeeds against the configured static address with no console grammar or ACK/ERR/END drift.
- Any 26a GENET latency evidence is recorded as physical static-IPv4 responsiveness diagnostics and must not be compared against QEMU loopback latency as a closure gate.
- Invalid Pi 4 static IPv4 manifest settings are rejected deterministically (compiler validation and/or early-boot fail-fast).
- Pi 4 26a validation explicitly demonstrates transition from Milestone 26 no-NIC baseline to 26a NIC-enabled boot using profile-gated configuration only.
- No DHCP client path is introduced in 26a; DHCP remains scoped to Milestone 26b.
- Existing macOS/Linux QEMU test and operator flows remain backward compatible with pre-26 behavior.
- Full regression pack remains green on QEMU; any profile-gated divergence is explicitly documented and fixture-backed.

### Compiler touchpoints
- `coh-rtc` emits driver-task contract tables for profile-selected hardware roles, including task id, service class, priority band, period/budget target, IRQ badge, DMA/ring bounds, queue depth, and shutdown/revoke behavior.
- `coh-rtc` emits profile-gated network config tables (backend selection + static IPv4 fields) into generated root-task artifacts.
- Manifest validation enforces:
  - static IPv4 required for `pi4-uboot-aarch64` network-enabled profile,
  - prefix bounds (`1..=32`),
  - backend/profile compatibility (`bcmgenet` only where declared in `hw.devices`).
- Docs snippet regeneration includes static IPv4 and backend mapping excerpts for Architecture/Interfaces docs.

### Task Breakdown
```
Title/ID: m26a-driver-task-hal-contracts
Goal: Make hardware driver service admissible only through HAL-declared scheduling contracts.
Inputs: apps/root-task/src/hal/*, apps/root-task/src/net/*, apps/root-task/src/serial/*, apps/root-task/src/local_seat.rs, docs/DRIVERS.md, docs/TEST_PLAN.md.
Changes:
  - apps/root-task/src/hal/* — add driver-task contract types, built-in role contracts, and per-turn budget validation.
  - apps/root-task/src/net/mod.rs + apps/root-task/src/net/stack.rs — require valid NIC driver contracts before network device construction and expose active contract diagnostics.
  - apps/root-task/src/serial/mod.rs + apps/root-task/src/local_seat.rs — expose serial and local-seat contracts for event-loop admission.
Commands:
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::driver_task
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib net::tests::network_driver_task_contracts_match_backend_labels
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::poll_io_obeys_driver_task_budget
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests::serial_input_skips_ready_network_data_poll_for_driver_task_turn
Checks:
  - Missing, zero-budget, blocking, or non-preemptible driver contracts fail deterministically before service.
  - Built-in contracts preserve the priority order: serial/USB input before network control, network control before network data, diagnostics/display last.
Deliverables:
  - HAL scheduling-contract ratchet for current compatibility drivers and future dedicated seL4 driver tasks.

Title/ID: m26a-smp-activity-diagnostic
Goal: Add `smp activity` as a bounded root-console diagnostic for cross-core activity evidence without enabling seL4 kernel benchmark builds.
Inputs: crates/cohsh-core/src/command.rs, crates/cohsh-core/src/verb.rs, apps/root-task/src/event/mod.rs, apps/root-task/src/console/mod.rs, apps/root-task/src/local_seat.rs, apps/root-task/src/net/mod.rs, apps/root-task/src/hal/driver_task.rs, apps/root-task/src/affinity.rs, docs/USERLAND_AND_CLI.md.
Changes:
  - crates/cohsh-core/src/command.rs + crates/cohsh-core/src/verb.rs — extend the canonical grammar from `smp` to `smp [activity]`, reject unknown pseudo-profile arguments, and keep plain `smp` mapped to the existing debug-kernel scheduler snapshot.
  - apps/root-task/src/affinity.rs + apps/root-task/src/event/mod.rs — implement `smp activity` from bounded userspace telemetry: event-pump command/timer counters, serial backpressure, local-seat keyboard/HDMI mirror counters, network status/counters when present, HAL driver-task contracts, driver-task runtime proof masks, manifest affinity assignments, and repeated-sample per-core counter-delta rows; keep the original plain `smp` debug path on the same multi-assignment core bucket formatter.
  - apps/root-task/src/console/mod.rs — keep early-console behavior explicit when the event-pump telemetry source is unavailable.
  - docs/USERLAND_AND_CLI.md + docs/snippets/cohsh_grammar.md — document that `smp activity` is not a cycle-accurate profiler, does not require kernel benchmark builds, and is mirrored to HDMI through the local-seat path while raw seL4 debug dump text remains UART-only.
Commands:
  - cargo test -p cohsh-core smp
  - cargo test -p root-task smp_activity
  - cargo test -p root-task --features net-console smp_activity
Checks:
  - `smp activity` never depends on `CONFIG_BENCHMARK_TRACK_KERNEL_ENTRIES`, debug-kernel benchmark syscalls, PMU counters, or unbounded sampling.
  - Output is line-bounded and useful for Pi 4 diagnostics: it distinguishes parser/event-loop progress, serial pressure, HDMI/local-seat mirroring, attached network progress, driver-task compatibility vs dedicated proof, and configured role/driver affinity.
  - The htop-ish core rows are assignment buckets, not exclusive CPU owners: when multiple roles/drivers map to the same core, `tasks=` lists all of them and the rate fields aggregate only safe userspace counter deltas for that bucket while keeping `cpu_pct=unavailable`.
  - Plain debug-kernel `smp` also treats core rows as assignment buckets: before each UART-only seL4 scheduler/CPU dump, the probe line includes every role/driver allocated to that core instead of dropping secondary assignments.
  - HDMI display receives the same event-pump `smp activity` lines when local-seat mirroring is active; raw kernel debug output from plain `smp` remains serial-only.
  - Unknown arguments such as `smp profile` fail grammar validation instead of silently aliasing to plain `smp`.
Deliverables:
  - Root-console `smp activity` diagnostic with parser, event-pump, HDMI mirror, and feature-scoped network test coverage.

Title/ID: m26a-driver-task-kernel-substrate
Goal: Add root-owned seL4 task/capability substrate for hardware driver tasks without changing authority semantics.
Inputs: apps/root-task/src/kernel.rs, apps/root-task/src/hal/*, apps/root-task/src/cspace*, configs/root_task.toml, docs/ROLES_AND_SCHEDULING.md.
Changes:
  - crates/sel4-sys/src/lib.rs + apps/root-task/src/sel4.rs — add driver TCB scheduling/resume/notification wrappers, CNode revoke, and remote-safe IPC buffer install.
  - apps/root-task/src/hal/* + apps/root-task/src/kernel.rs — add driver-task handles/rings for TCB creation, CSpace/VSpace setup, scheduling attributes, per-driver SMP affinity, notification binding, fault endpoint badges, and revocation.
  - configs/root_task.toml + coh-rtc outputs — add profile-gated driver-task specs, per-driver affinity, and bounds.
Commands:
  - cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-pi4
  - cargo test -p sel4-sys
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::driver_task
Checks:
  - Root retains cap ownership and can fault-report/revoke a driver task.
  - Driver tasks receive only declared device grants and bounded ring frames.
Deliverables:
  - Dedicated driver-task creation substrate ready for serial/display, GENET, USB, CYW43, SDIO, and PCIe migration.

Title/ID: m26a-serial-display-driver-tasks
Goal: Move normal serial and HDMI display service behind dedicated driver-task contracts while preserving emergency debug fallback.
Inputs: apps/root-task/src/serial/*, apps/root-task/src/local_seat.rs, apps/root-task/src/event/*, docs/DRIVERS.md.
Changes:
  - apps/root-task/src/serial/* — replace blocking normal-output paths with bounded driver-task IPC after early boot handoff.
  - apps/root-task/src/local_seat.rs — route HDMI text refresh through a bounded display-sink task.
  - apps/root-task/src/event/* — preserve serial/USB priority over network data and display flushing.
Commands:
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat::tests
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests
Checks:
  - Serial echo and HDMI mirror remain fluid while GENET consumes its full data budget.
  - Emergency debug UART remains available only for boot/fault diagnostics, not normal console output.
Deliverables:
  - Serial/display task isolation proof with no ACK/ERR/END grammar drift.

Title/ID: m26a-genet-driver-task
Goal: Promote GENET from in-root compatibility driver to isolated frame driver task.
Inputs: apps/root-task/src/drivers/driver_task_net.rs, apps/root-task/src/net/*, apps/pi4-driver-runtime/src/lib.rs.
Changes:
  - apps/root-task/src/drivers/driver_task_net.rs — keep root as the GENET ring client only and fail closed when the isolated runtime has not returned owner progress.
  - apps/pi4-driver-runtime/src/lib.rs — keep DMA ring ownership, TX reclaim, RX refill, and service turns in the isolated `driver-genet` runtime.
  - apps/root-task/src/net/* — consume bounded Ethernet-frame IPC from the GENET task while keeping smoltcp/listener semantics in root.
Commands:
  - scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml
Checks:
  - GENET static IPv4 reachability is preserved, ring pressure is observable, and serial/HDMI remain responsive under wired load.
Deliverables:
  - First production dataplane driver task and preserved 26a static IPv4 behavior.

Compatibility baseline tasks below are retained because they describe the original 26a GENET/static-IPv4 behavior that must keep working. They do not close reopened 26a by themselves; closure now requires both the baseline behavior and the driver-task substrate/migration checks above.

Title/ID: m26a-bcmgenet-driver
Goal: Preserve HAL-declared Broadcom GENETv5 NIC behavior through the isolated Pi driver runtime.
Inputs: apps/root-task/src/net/*, apps/root-task/src/drivers/driver_task_net.rs, apps/pi4-driver-runtime/src/lib.rs, crates/pi4-driver-abi/src/lib.rs, apps/root-task/src/hal/*, docs/SECURITY.md, Linux `bcmgenet` + Linux `bcm2711` DT binding notes + U-Boot `bcmgenet` bring-up notes (reference-only).
Changes:
  - apps/root-task/src/drivers/driver_task_net.rs — keep root as the GENET ring client only and fail closed when isolated runtime owner-state proof is missing.
  - apps/pi4-driver-runtime/src/lib.rs — carry the GENETv5 register/ring/IRQ/PHY implementation inside the isolated `driver-genet` runtime with HAL-declared MMIO/IRQ/DMA/MDIO resources only.
  - apps/root-task/src/net/mod.rs + apps/root-task/src/net/stack.rs — backend selection and ring-client init wiring.
  - docs/ARCHITECTURE.md + docs/SECURITY.md — record GENET design-reference provenance and explicit no-code-lift policy.
Commands:
  - cargo check -p root-task
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib drivers::driver_task_net
  - cargo test -p pi4-driver-runtime
  - rg -n "EFI_|boot_services|runtime_services|uefi::" apps/root-task/src tools/coh-rtc/src
Checks:
  - Link-up, RX/TX smoke, and deterministic error paths are covered by unit/integration tests.
  - GENET hardware service remains in the isolated runtime with HAL-declared resources; root exposes only the bounded ring client.
Deliverables:
  - Pi 4 GENETv5 isolated runtime backend integrated behind existing net abstractions with documented source provenance order (Linux `bcmgenet` -> Linux `bcm2711` DT -> U-Boot `bcmgenet`) and reference-only compliance.

Title/ID: m26a-static-ipv4-profile-gate
Goal: Make Pi 4 U-Boot static IPv4 config manifest-authoritative and profile-gated.
Inputs: configs/root_task.toml, tools/coh-rtc, apps/root-task/src/generated, docs/INTERFACES.md.
Changes:
  - tools/coh-rtc/src/* — add IR fields + validation for `pi4-uboot-aarch64` static IPv4 network config.
  - apps/root-task/src/generated/* — regenerated network config outputs.
  - apps/root-task/src/net/mod.rs — consume generated profile config before dev-virt fallback defaults.
Commands:
  - cargo test -p coh-rtc
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
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
  - scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml
  - cargo run -p cohsh --features tcp -- --transport tcp --tcp-host <STATIC_IP> --tcp-port 31337 --script scripts/cohsh/boot_v0.coh
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

## Milestone 26b — Pi 4 USB/Wi-Fi Driver Tasks + DHCP/Benchmark Concurrency <a id="26b"></a>
[Milestones](#Milestones)

**Status:** Reopened — the prior bounded DHCP, USB/local-seat, QEMU compatibility, and production wired/GENET evidence remains accepted. Milestone 26d Wi-Fi boot investigation exposed one pre-existing 26b defect: `m26b-wifi-driver-task` and `m26b-sdio-host-driver-task-graduation` used poll/yield service instead of the notification-driven CYW43/SDIO DPC boundary required by their original root-no-wait contract. The reciprocal generated notification/IRQ topology, bounded pointer-free DPC event ring, and isolated-runtime IRQ/DPC service have since landed under `m26b-wifi-sdio-notification-dpc-closure`. Milestone 26b remains reopened until repeated current-image Wi-Fi DHCP, raw TCP/`cohsh`, ordered RX, clean DPC counters, and unchanged operator-input gates close the functional proof requirement. This scope adds no operator protocol, namespace, system authority, production-Wi-Fi parity claim, or unrelated 26d system-model change.

**Why now (operator continuity):**  
Milestone 26b depends on completed 26a driver-task substrate and wired/serial/display isolation. Milestone 26b applies that model to the two paths that exposed the regression: USB keyboard/local-seat and CYW43 Wi-Fi. Wi-Fi and selected-network performance must improve by moving SDIO/firmware/RX/TX or GENET RX/TX progress onto bounded manifest-declared isolated driver runtimes, not by extending the root event-loop turn.

**Non-negotiable constraints:**
- DHCP implementation must be pure Rust, `no_std`, and intentionally bare-bones (DHCPv4 only: DISCOVER/OFFER/REQUEST/ACK plus bounded timeout/retry logic).
- No new in-VM listeners or protocols are permitted; DHCP is only a client-side address acquisition path for existing network surfaces.
- Post-seL4 runtime must remain HAL-only; no direct bootloader/firmware service calls are allowed in root-task.
- NIC and Wi-Fi DHCP behavior must be policy-configurable for `pi4-uboot-aarch64` through compiler-validated profile settings sourced from U-Boot environment configuration when available; if bootloader policy inputs are absent, the Pi 4 manifest defaults are authoritative and select DHCP/`auto` so GENET DHCP is the no-credential path and Wi-Fi DHCP is selected by explicit Wi-Fi policy or credentials.
- Wi-Fi scope is minimal diagnostics connectivity only (join + DHCP + existing TCP console path); no in-VM supplicant stack, roaming framework, or broad feature surface.
- Wi-Fi is a research, diagnostics, and degraded-link bring-up transport. Production Pi 4 deployments that require high worker concurrency or predictable REST latency should use the wired GENET path unless a site-specific Wi-Fi envelope is freshly proven and documented.
- Milestone 26b includes a new profile-gated Pi 4 CYW43xx Wi-Fi driver path; all Wi-Fi dataplane/control-plane access must be HAL-backed (SDIO, power/reset, IRQ/OOB, firmware handoff hooks) with no direct MMIO or firmware-service calls outside HAL.
- CYW43xx design references must be used in this order for architecture review: OpenBSD `bwfm` -> Zephyr/Infineon WHD HAL split -> Linux `brcmfmac` SDIO edge-case behavior. These are design references only; no source copy/paste is permitted.
- USB/xHCI/HID owns a higher-priority driver-task contract than Wi-Fi data. Keyboard first-byte and fast-typing proof are hard gates before Wi-Fi performance claims.
- CYW43/SDIO runs under separate network-control and network-data budgets. EAPOL, DHCP, ARP, and TCP ACK progress may preempt Wi-Fi bulk data, but neither class may preempt USB/local-seat or serial input.
- Runtime RX aggregation/glom may be enabled only after bounded superframe storage, capped deaggregation work, drop counters, and recovery gates are implemented inside the Wi-Fi driver task.
- Performance claims use normalized parity: raw `cohsh` over Pi Wi-Fi is measured first, REST gateway overhead is measured separately, QEMU remains a compatibility/capacity reference by itself, and production Pi throughput-parity claims must come from a fresh same-harness wired/GENET Pi run that meets or exceeds the selected best-QEMU throughput reference.
- Wi-Fi performance claims use the documented research envelope instead of QEMU parity: the safe sustained 26b Wi-Fi worker cap is `120` workers, short exploratory pressure may be run up to `300` workers, and `1500` workers is a fault-injection/stress ceiling only. Wi-Fi runs must pass the accepted error budget, preserve raw TCP/cohsh reachability, keep CYW43/SDIO counters free of drops/overflows/faults, and report latency separately as physical-link evidence.
- The isolated runtime throughput verdict excludes QEMU-latency parity only after recording latency fields in the artifacts and showing Pi 4 wired NIC and Wi-Fi latency is explained and aligned with the selected transport's documented physical-network expectations. Throughput, successful operation count, error rate, and bounded-backpressure behavior remain verdict inputs for production wired/GENET. Persistent second-scale stalls, unbounded queue growth, drops, or timeout-driven success remain blockers for production throughput claims even when exploratory Wi-Fi stress passes its error budget.
- Backward compatibility is mandatory: Milestone 26b must not break existing macOS/Linux QEMU workflows, fixtures, or transport semantics.

### Prerequisite
- Milestone **26a** is complete before 26b closure, including the HAL driver-task admission substrate, serial/display isolation, GENET driver-task migration, and preserved Pi 4 GENETv5/static-IPv4 compatibility behavior.

### Goal
Move USB/xHCI/HID and CYW43/SDIO Wi-Fi onto dedicated, HAL-admitted driver-task contracts, preserve DHCP and single-interface policy behavior, prove Wi-Fi/network load cannot degrade USB keyboard, serial console, or HDMI responsiveness, and close same-harness isolated runtime throughput parity for the production wired/GENET Pi path against the accepted best-QEMU reference. Wi-Fi closure proves bounded research/diagnostic transport behavior at the documented worker envelope and reports latency as physical-link evidence instead of QEMU-loopback parity.

### Deliverables
- **USB/xHCI/HID driver task**
  - Promote Pi 4 VL805/xHCI/HID keyboard service from in-root compatibility polling to a `driver-usb` task with a realtime-input scheduling contract.
  - Preserve the poll-only xHCI lane until hardware proves interrupt delivery; timer IRQ 27 remains timer-only evidence, not USB evidence.
  - Preserve the 128 armed keyboard interrupt-IN runway, bounded HID event draining, bounded EP0 `GET_REPORT` input fallback, and Gate 10 first-byte/first-printable-byte acceptance criteria.
  - Preserve the May 18-19 old-good hub-keyboard order as an additive isolated runtime replay contract. Final USB closure requires both `USB_GATE=10` / `USB_BLOCKER=none` and `USB_OLDGOOD_REPLAY=yes` / `USB_OLDGOOD_MISSING=none`; late Gate 10 text without controller, hub, interrupt-IN, first-report, and first-byte sequence evidence is not closure.
  - Root consumes only bounded local-seat byte events; it does not own xHCI registers, rings, DMA, or HID polling after driver-task handoff.

- **CYW43/SDIO Wi-Fi driver task**
  - Promote SDIO host I/O, CYW43 firmware/control plane, EAPOL admission, RX/TX queues, credit handling, and optional glom/deaggregation into `driver-wifi`.
  - Graduate standalone `sdio-host` into the 26b acceptance set with one HAL-declared SDHCI MMIO page, fixed-layout CMD52/CMD53/POLL_IRQ service records, atomic card-reset/pending-generation/commit records, and required owner-state proof.
  - Split Wi-Fi work into network-control and network-data budgets so EAPOL/DHCP/TCP ACK progress cannot be starved by bulk RX/TX, while USB/serial still preempt both.
  - Preserve the May 18-19 Linux-shaped host-EAPOL order as an additive isolated runtime replay contract. Final Wi-Fi closure requires both `WIFI_GATE=10` / `WIFI_BLOCKER=none` and `WIFI_OLDGOOD_REPLAY=yes` / `WIFI_OLDGOOD_MISSING=none`; DHCP/netstats and condensed `join complete` summaries cannot substitute for ordered SDIO/CYW43 owner-state, control setup, association/link, M1/M2/M3/M4, PTK/GTK, secure release, DHCP, nettest, and final netstats proof.
  - Root receives Ethernet frames through bounded IPC/rings and never waits synchronously on SDIO credit, CMD52/CMD53 loops, firmware replies, or glom deaggregation.
  - CYW43 startup preserves Linux's `bus:rxglom=1` through `event_msgs_ext` and `WLC_UP`; aggregated RX is bounded in the driver task, and any future runtime disable/superframe expansion must run only after secure carrier proof with capped work and recovery gates.
  - Runtime Wi-Fi data/glom RX is separated from control-plane reply reads: control replies retain the conservative Linux first-read/remainder shape, while runtime RX uses one 512-byte block-aligned Function 2 read into an 8192-byte bounded buffer and caps deaggregation at 16 subframes plus a 16-entry ready queue.

- **Normalized benchmark and isolated runtime parity gates**
  - Add benchmark reporting that separates raw `cohsh` over Pi Wi-Fi, REST cold gateway overhead, and REST hot/cache projection.
  - Define the accepted best-QEMU reference by artifact path, harness command, worker count, request suite, gateway/auth mode, QEMU SMP topology, pressure settings, and error-budget policy.
  - Compare QEMU and Pi isolated runtime artifacts with the same harness and matched workload/provenance. QEMU results alone remain semantic/capacity references; production Pi hardware parity requires fresh wired/GENET evidence.
  - Treat Wi-Fi as a separate research/diagnostic lane: the routine sustained Wi-Fi gate is capped at `120` workers, exploratory pressure is capped at `300` workers, and `1500` workers is retained only as a stress ceiling for driver fault discovery.
  - Exclude QEMU-latency parity from pass/fail only after preserving latency fields in JSON and human summaries and verifying Pi 4 wired NIC/Wi-Fi latency remains within the documented physical-network expectations for the selected transport; throughput, successful operation count, error rate, and bounded-backpressure behavior decide the production wired/GENET verdict.
  - Pi 4 performance acceptance requires zero USB/serial/HDMI responsiveness regression during Wi-Fi/NIC load and no root-owned steady-state physical-driver service path.

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
    - bounded static IPv4 override fields (`ip`, `prefix_len`, optional `gateway`) for explicit `mode=static`,
    - DHCP timing bounds and retry limits.
  - When bootloader setup can provide network policy inputs, capture them pre-handoff and normalize through manifest/DTB-generated structures validated by `coh-rtc`.
  - Preserve deterministic fallback behavior when U-Boot policy inputs are missing or invalid.
  - Stage an interactive Pi 4 U-Boot wizard that offers a Linux-style default action (`Continue with existing config` when saved Cohesix policy exists, otherwise `Boot with manifest defaults`), bounded DHCP/static selection, bounded wired/Wi-Fi selection, serial/USB-keyboard-safe numbered prompts, and optional HDMI logo display without introducing any new Cohesix protocol semantics.

- **Backward-compatibility guardrails**
  - Keep QEMU `aarch64/virt` behavior for macOS/Linux unchanged by default (existing static/dev-virt flows must keep working with no required operator command changes).
  - Add explicit regression checks proving 26/26a/26b additions are profile-gated and do not alter prior QEMU console grammar, ACK/ERR/END behavior, or existing transport fixtures.

### Commands
- `cargo check -p root-task`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::driver_task`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat::tests`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests`
- `cargo test -p root-task net:: -- --nocapture`
- `cargo test -p pi4-driver-abi`
- `cargo test -p pi4-driver-runtime`
- `cargo test -p coh-rtc`
- `python3 -m pytest tests/test_rest_perf_harness.py tests/test_pi4_compare_driver_models.py`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`
- `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml`
- `scripts/uboot/qemu-uboot-smoke.sh --net user`
- `scripts/cohesix-build-run.sh --no-run --cargo-target aarch64-unknown-none`
- `python3 scripts/rest_perf_harness.py --mode perf --suite all --runs 5 --log-dir out/bench --log-prefix m26b-qemu-reference`
- `python3 scripts/rest_perf_harness.py --mode perf --suite all --runs 5 --no-qemu --no-gateway --rest-url http://<pi4-gateway-host>:<port> --log-dir out/bench --log-prefix m26b-pi4-linked-runtime`

### Checks (DoD)
- `driver-usb` proves `USB_GATE=10`, `USB_BLOCKER=none`, first-byte/first-printable-byte evidence, and fast typing remains fluid while the manifest-selected network driver consumes its data budget.
- Serial command echo p50/p95 and HDMI mirror progress remain bounded while Wi-Fi or wired network tasks consume their data budgets; GENET/CYW43 pre-root runtime-init may defer before the root shell when pointer-free ring proof is incomplete, and that deferral must not be counted as selected-NIC proof.
- `driver-wifi` reports bounded control/data service counters: SDIO ops, bytes, RX/TX frames, credit waits, budget exhaustion/yield count, queued frames, drops, and max service latency.
- `driver-sdio` reports dedicated-role and owner-state proof from non-root-context isolated runtime SDIO hardware progress before `sdio-host` can count toward Pi 4 acceptance.
- Wi-Fi accepts load only through the driver-task frame ABI; root never spins on SDIO credit, CMD52/CMD53 loops, firmware replies, or glom deaggregation.
- Pi 4 wired path acquires a DHCP lease within bounded retries/timeouts and exposes acquired config through existing diagnostics surfaces.
- Pi 4 Wi-Fi path (when enabled in profile and credentials are present) reaches link-up and acquires DHCP with deterministic failure modes and audited errors.
- Invalid DHCP options, malformed offers, lease overflows, and timeout exhaustion fail safely and deterministically.
- U-Boot-configurable network policy (where available) is honored through compiler-validated handoff structures; missing/unsupported bootloader policy cleanly falls back to manifest defaults.
- Existing macOS/Linux QEMU workflows remain backward compatible, including serial/TCP console behavior and existing regression fixtures.
- CYW43xx Wi-Fi path demonstrates HAL-only access in runtime code and tests; no direct MMIO/bootloader-service usage exists outside HAL-owned modules.
- Raw Pi `cohsh` Wi-Fi latency, REST cold gateway overhead, and REST hot projection latency are reported separately; QEMU benchmark results are not used as Pi Wi-Fi hardware proof without a matched fresh Pi run.
- Wi-Fi DoD uses the research envelope: a fresh `120`-worker sustained run must pass the accepted error budget, retain post-load raw `cohsh` reachability, and show clean CYW43/SDIO counters (`tx_submit == tx_complete`, no RX drops/overflows, no trace faults/retries outside explicitly recorded recovery). `300`-worker Wi-Fi runs are useful exploratory pressure, and `1500`-worker Wi-Fi runs are stress diagnostics rather than production or closure requirements.
- A comparator identifies the selected best-QEMU benchmark artifact and the matched Pi isolated runtime artifact, rejects workload/provenance mismatches, excludes QEMU-latency parity from pass/fail, reports Pi 4 wired NIC/Wi-Fi latency against documented physical-network expectations, and reports a deterministic production PASS only when wired/GENET Pi throughput meets or exceeds QEMU with the accepted error budget.
- Isolated runtime hot-path changes remain bounded and preserve active-slot, fingerprint, completion, and busy-on-conflict invariants; no root-owned physical driver service path is added or re-enabled to win the benchmark.
- Pi benchmark evidence is fresh, non-empty, normalized, and separated from flash, shell, USB/local-seat, HDMI, and serial-responsiveness proof lanes.
- Full regression pack remains green on QEMU; any profile-gated divergence is explicitly documented and fixture-backed.

### Compiler touchpoints
- `coh-rtc` adds bounded `pi4-uboot-aarch64` network policy fields for mode/interface selection and DHCP retry/timing limits.
- `coh-rtc` extends driver-task contract emission for USB, CYW43/SDIO, and network-control/network-data priority classes.
- Benchmark or counter fields that become generated artifacts must be covered by `scripts/check-generated.sh` before 26b closes.
- `docs/DRIVERS.md`, `docs/BENCHMARKS.md`, `docs/TEST_PLAN.md`, and `scripts/ci/test_plan_run.sh` must agree on which evidence is benchmark proof, board-acceptance proof, and compatibility-only proof.
- Validation enforces:
  - policy bounds and enum validity,
  - interface/profile compatibility (wired vs Wi-Fi declarations),
  - deterministic fallback policy when optional U-Boot-provided inputs are absent.
- Generated snippets in architecture/interfaces/security docs include the new policy and DHCP bounds so docs-as-built remains authoritative.

### Task Breakdown
```
Title/ID: m26b-usb-driver-task
Goal: Move Pi 4 xHCI/HID keyboard service behind the realtime USB driver-task contract.
Inputs: apps/root-task/src/local_seat.rs, apps/pi4-driver-runtime/src/lib.rs, crates/pi4-driver-abi/src/lib.rs, apps/root-task/src/event/*, scripts/pi4_trace_normalize.py, tests/test_pi4_trace_normalize.py, docs/DRIVERS.md.
Changes:
  - apps/root-task/src/local_seat.rs — route local-seat keyboard-byte consumption through the isolated `driver-usb` runtime.
  - apps/pi4-driver-runtime/src/lib.rs — own the direct-root-port xHCI command/event/EP0/interrupt-IN rings, root-port reset, slot/address/configuration, HID boot-protocol setup, and DMA report polling in the isolated USB runtime.
  - apps/root-task/src/local_seat.rs — keep root-task as a ring-client/prompt consumer only; no root-owned USB implementation crate remains.
  - apps/root-task/src/event/* — consume bounded local-seat byte events and preserve USB priority over serial dispatch and all network work.
Commands:
  - cargo test -p pi4-driver-runtime --lib -- --test-threads=1
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat::tests
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests
Checks:
  - USB first-byte and first-printable-byte proof remain Gate 10.
  - Fast typing remains fluid while network data queues are saturated.
Deliverables:
  - Isolated USB keyboard task path with existing local-seat console semantics preserved.

Title/ID: m26b-wifi-driver-task
Goal: Move CYW43/SDIO Wi-Fi service behind a dedicated driver task with bounded control/data budgets.
Inputs: apps/root-task/src/drivers/driver_task_net.rs, apps/root-task/src/hal/pi4_wifi.rs, apps/root-task/src/hal/driver_task.rs, apps/pi4-driver-runtime/src/lib.rs, crates/pi4-driver-abi/src/lib.rs, apps/root-task/src/net/*, scripts/pi4_trace_normalize.py, scripts/pi4_compare_driver_models.py, tests/test_pi4_trace_normalize.py, tests/test_pi4_compare_driver_models.py, docs/DRIVERS.md.
Changes:
  - apps/root-task/src/hal/pi4_wifi.rs — grant only SDIO, fixed Pi 4 WL_ON power/reset, firmware-bundle, IRQ/OOB, and DMA/ring resources declared for `driver-wifi`; `sdio-host` owns the noncontiguous firmware-mailbox page and one private low request page after a one-way root bootstrap handoff.
  - apps/root-task/src/drivers/driver_task_net.rs — keep root as the CYW43 ring client only and fail closed when isolated SDIO/CYW43 service has not returned owner progress.
  - crates/pi4-driver-abi/src/lib.rs + apps/pi4-driver-runtime/src/lib.rs — define fixed-layout SDIO CMD52/CMD53/POLL_IRQ plus atomic card-reset/pending-generation/commit command records and service them in the isolated SDIO runtime as the bus-owner ABI that CYW43 must use.
  - apps/root-task/src/net/* — consume bounded Ethernet-frame IPC without changing authenticated TCP console semantics.
Commands:
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_wifi
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib net::
  - scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml
Checks:
  - Wi-Fi control progress yields on budget exhaustion instead of spinning.
  - Runtime CYW43 data TX admits smoltcp TX tokens only when the firmware SDPCM credit window is open; no-credit data TX must return/yield without a credit-spin loop in the root service turn.
  - Runtime CYW43 TX staging lets smoltcp fill the bounded SDPCM/BDC transmit frame directly instead of copying through an extra stack frame buffer.
  - CYW43 startup does not write `bus:rxglom=0` before `event_msgs_ext`/`WLC_UP`; aggregated RX is bounded, counted, and recoverable inside the Wi-Fi driver task.
Deliverables:
  - CYW43/SDIO driver task that can make Wi-Fi progress without stealing root-task USB/serial turns.

Title/ID: m26b-sdio-host-driver-task-graduation
Goal: Graduate standalone SDIO host service from diagnostic runtime to 26b acceptance-eligible driver task.
Inputs: configs/root_task*.toml, apps/root-task/src/hal/{mod.rs,driver_task.rs,pi4_wifi.rs}, apps/pi4-driver-runtime/src/lib.rs, crates/pi4-driver-abi/src/lib.rs, scripts/pi4_gate_proof.sh, scripts/pi4_trace_normalize.py, docs/DRIVERS.md, docs/TEST_PLAN.md.
Changes:
  - configs/root_task*.toml + generated manifest artifacts — declare `sdio-host` with one HAL-declared SDHCI MMIO page, DMA/shared budgets, and `hardware_state_migrated=true`.
  - apps/root-task/src/hal/* — map SDHCI into the SDIO isolated runtime resource set, keep runtime commands pointer-free, and register `sdio-host` owner-state only after non-root-context CMD52/CMD53/POLL_IRQ hardware progress.
  - scripts/pi4_gate_proof.sh + scripts/pi4_trace_normalize.py — require SDIO dedicated-role and owner-state evidence as part of reopened 26a/26b Pi 4 acceptance.
  - docs/BUILD_PLAN.md + docs/DRIVERS.md + docs/TEST_PLAN.md + docs/HARDWARE_BRINGUP.md — record SDIO as a 26b acceptance hot path rather than a future diagnostic guard.
Commands:
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::driver_task
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_wifi
  - cargo test -p pi4-driver-runtime
  - python3 -m pytest -q tests/test_pi4_gate_proof.py tests/test_pi4_trace_normalize.py
  - scripts/check-generated.sh
  - scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml
Checks:
  - Generated Pi 4 specs report `sdio-host root_context_required=false hardware_state_migrated=true mmio_pages=1`.
  - `scripts/pi4_gate_proof.sh --require-driver-task-proof` requires `DRIVER_TASK_SDIO_DEDICATED=yes` and a `DRIVER_TASK_OWNER_STATE ... hot_path=sdio-host ... root_pointer=no` descriptor.
  - SDIO production ring commands do not carry `DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE`; root-context diagnostic turns remain explicitly non-acceptance.
Deliverables:
  - SDIO runtime acceptance promotion with generated, code, proof-gate, and docs-as-built alignment.

Title/ID: m26b-wifi-sdio-notification-dpc-closure
Goal: Restore the original 26b root-no-wait CYW43/SDIO contract with a generated reciprocal notification topology and a bounded Linux-shaped isolated-runtime DPC.
Inputs: configs/root_task*.toml, tools/coh-rtc/src/{ir.rs,codegen/**}, apps/root-task/src/generated/**, crates/pi4-driver-abi/src/lib.rs, apps/root-task/src/hal/{mod.rs,driver_task.rs}, apps/pi4-driver-runtime/src/lib.rs, scripts/pi4_trace_normalize.py, scripts/pi4_gate_proof.sh, docs/DRIVERS.md, fresh Pi serial/pcap evidence gathered during Milestone 26d.
Changes:
  - configs/root_task*.toml + tools/coh-rtc/src/** + generated artifacts — declare and validate SDIO IRQ 158, reciprocal CYW43/SDIO notification slots, the existing shared owner window, and a fixed four-entry DPC event ring as compiler-owned topology.
  - crates/pi4-driver-abi/src/lib.rs — version the pointer-free reciprocal bus-link descriptor and define bounded sequence-stamped DPC event-ring metadata without changing console or network protocol grammar.
  - apps/root-task/src/hal/{mod.rs,driver_task.rs} — admit only generated caps/topology, bind the selected runtime notifications, and keep root as a bounded ring client with no steady SDIO service loop.
  - apps/pi4-driver-runtime/src/lib.rs — let SDIO own IRQ capture/clear/ack and wake CYW43; let CYW43 retain bounded software-pending DPC state, preserve wire order, and yield/resignal on budget exhaustion.
  - scripts/pi4_trace_normalize.py + scripts/pi4_gate_proof.sh + docs/DRIVERS.md — keep notification deferral red and require bounded IRQ/DPC, ordered RX, multi-boot TCP/cohsh, and unchanged USB/serial proof.
Commands:
  - cargo test -p coh-rtc
  - cargo test -p pi4-driver-abi
  - cargo test -p pi4-driver-runtime
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::driver_task
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
  - scripts/check-generated.sh
  - scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml
Checks:
  - Generated topology contains exactly one level-triggered SDIO IRQ 158 owner and one reciprocal `cyw43-sdio` link with bounded notification slots and four-entry event ring; malformed, overlapping, unbounded, or unknown topology fails validation.
  - Root never waits synchronously on SDIO credit, CMD52/CMD53, firmware replies, or glom work; notification/DPC progress remains inside the isolated SDIO/CYW43 runtimes and preserves pointer-free active-slot bounds.
  - Fresh repeated Wi-Fi boots show no `DRIVER_TASK_NOTIFICATION_BIND_DEFERRED`, no DPC-ring overrun/drop, ordered control/event/data delivery, working DHCP/TCP/cohsh, and unchanged serial/USB/local-seat gates.
Deliverables:
  - Generated reciprocal CYW43/SDIO notification contract, fixed bounded DPC event-ring ABI, isolated-runtime implementation, proof gates, docs-as-built alignment, and repeatable Wi-Fi TCP evidence.

Title/ID: m26b-net-control-priority
Goal: Split network-control and network-data work so EAPOL/DHCP/TCP ACK progress is prioritized without starving physical input.
Inputs: apps/root-task/src/net/*, apps/root-task/src/drivers/driver_task_net.rs, apps/pi4-driver-runtime/src/lib.rs, apps/root-task/src/event/*.
Changes:
  - apps/root-task/src/net/* — add diagnostics and scheduling hooks for network-control vs network-data service classes.
  - apps/root-task/src/drivers/driver_task_net.rs + apps/pi4-driver-runtime/src/lib.rs — expose ring-client/runtime counters for EAPOL/DHCP/control progress and bulk RX/TX data.
  - apps/root-task/src/event/* — preserve USB/serial priority before either network class.
Commands:
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib net::
Checks:
  - Synthetic Wi-Fi data backlog cannot delay keyboard bytes or serial command echo.
  - Control traffic is not blocked behind bulk data queue pressure.
Deliverables:
  - Observable network-control/data scheduling contract for Pi 4 Wi-Fi and future high-performance drivers.

Title/ID: m26b-rest-normalized-parity
Goal: Reframe Wi-Fi performance proof as raw Pi Wi-Fi latency plus measured REST gateway overhead.
Inputs: scripts/rest_perf_harness.py, docs/BENCHMARKS.md, apps/hive-gateway/src/main.rs, apps/cohsh/src/transport/tcp.rs.
Changes:
  - scripts/rest_perf_harness.py — report raw/cold/hot modes separately and preserve no-retry failure accounting.
  - docs/BENCHMARKS.md — distinguish QEMU loopback capacity from Pi Wi-Fi physical proof.
  - apps/hive-gateway/src/main.rs — preserve cache/coalescing as host-side projection only; no write bypass.
Commands:
  - .venv/bin/python -m pytest -q tests/test_rest_perf_harness.py
  - cargo run -p cohsh --features tcp -- --transport tcp --tcp-host <wifi-lease-ip> --tcp-port 31337 --auth-token <token> --script scripts/cohsh/boot_v0.coh
  - python3 scripts/rest_perf_harness.py --mode perf --rest-url <pi4-rest-url> --suite status --runs 20 --no-transient-retries
Checks:
  - REST cold p95 is reported as raw `cohsh` p95 plus gateway overhead.
  - Hot projection results are labeled separately and cannot satisfy raw Wi-Fi hardware gates.
Deliverables:
  - Honest Pi Wi-Fi benchmark model that optimizes toward QEMU semantics without claiming impossible physical loopback parity.

Title/ID: m26b-benchmark-comparator
Goal: Define and enforce same-harness QEMU-vs-Pi isolated runtime throughput parity while evaluating Pi latency against documented physical-network norms instead of QEMU loopback parity.
Inputs: scripts/rest_perf_harness.py, tests/test_rest_perf_harness.py, tests/test_pi4_compare_driver_models.py, docs/BENCHMARKS.md, existing out/bench QEMU and Pi summaries.
Changes:
  - scripts/rest_perf_harness.py or scripts/pi4_compare_driver_models.py — emit/compare benchmark provenance, reject mismatched workloads, and calculate pass/fail from throughput/error metrics only.
  - tests/test_rest_perf_harness.py and tests/test_pi4_compare_driver_models.py — cover matched PASS, throughput FAIL, workload mismatch, stale-artifact rejection, and latency-excluded verdict behavior.
  - docs/BENCHMARKS.md — document the best-QEMU selection rule, Pi artifact requirements, and latency-exclusion rule.
Commands:
  - python3 -m pytest tests/test_rest_perf_harness.py tests/test_pi4_compare_driver_models.py
Checks: comparator rejects stale or mismatched artifacts and produces deterministic PASS/FAIL for matched QEMU/Pi runs without using QEMU latency parity as a verdict metric; it still flags Pi wired NIC/Wi-Fi latency outside documented industry-normal physical LAN/control-plane ranges.
Deliverables: benchmark comparator, tests, and documented parity rule.

Title/ID: m26b-linked-runtime-counters
Goal: Add bounded counters required to explain isolated runtime benchmark misses without UART spam.
Inputs: crates/pi4-driver-abi, apps/pi4-driver-runtime, apps/root-task/src/hal/driver_task.rs, apps/root-task/src/drivers/driver_task_net.rs.
Changes:
  - crates/pi4-driver-abi/src/** — fixed-layout counter fields for turns, drained descriptors, staged bytes, cache work, busy/backpressure, and overruns where existing records are insufficient.
  - apps/pi4-driver-runtime/src/** — update counters inside GENET, CYW43, SDIO, USB, HDMI, serial, and PCIe service loops.
  - apps/root-task/src/hal/driver_task.rs and apps/root-task/src/drivers/driver_task_net.rs — expose bounded, activity-gated counter snapshots through existing diagnostics/evidence surfaces.
  - scripts/pi4_trace_normalize.py + tests/test_pi4_trace_normalize.py — normalize `DRIVER_TASK_COUNTER_*` fields, reject empty or truncated counter lines through `DRIVER_TASK_COUNTER_INVALID`, and keep counters diagnostic-only.
Commands:
  - cargo test -p pi4-driver-abi
  - cargo test -p pi4-driver-runtime
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib drivers::driver_task_net
  - python3 -m pytest -q tests/test_pi4_trace_normalize.py
Checks: counters are fixed-layout, bounded, non-authority-bearing, activity-gated, not updated from polling-loop atomics, normalized with `DRIVER_TASK_COUNTER_INVALID=0`, and do not change console grammar or Secure9P semantics.
Deliverables: counter ABI, runtime updates, root-side evidence snapshots, normalizer guard fields, and tests.

Title/ID: m26b-linked-driver-hotpath-closure
Goal: Remove avoidable single-frame or single-descriptor overhead while preserving isolated runtime active-slot invariants.
Inputs: apps/pi4-driver-runtime, crates/pi4-driver-abi, apps/root-task/src/hal/driver_task.rs, docs/DRIVERS.md.
Changes:
  - apps/pi4-driver-runtime/src/** — bounded local batching for descriptor drain, cache maintenance, completion publication, and telemetry counters where benchmark evidence shows contention.
  - crates/pi4-driver-abi/src/** — fixed burst bounds and max-turn evidence where ABI fields are needed.
  - apps/root-task/src/hal/driver_task.rs — preserve staged active-slot submit, range validation, busy-on-conflict, and completion publication under batching.
Commands:
  - cargo test -p pi4-driver-abi
  - cargo test -p pi4-driver-runtime
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib
Checks: batching is bounded, cannot overwrite active payload-bearing turns, and does not add root-owned physical-driver service.
Deliverables: lower-contention isolated runtime hot paths with contract-local backpressure evidence.

Title/ID: m26b-target-benchmark-proof
Goal: Produce fresh same-harness QEMU and Pi 4 isolated runtime benchmark evidence before 26b closes.
Inputs: scripts/rest_perf_harness.py, selected QEMU reference, Pi 4 serial capture, active wired or Wi-Fi profile, docs/TEST_PLAN.md.
Changes:
  - out/bench/m26b-* — archived QEMU reference, Pi isolated runtime run, comparator output, and normalized serial evidence.
  - docs/BENCHMARKS.md — update benchmark verdict and artifact table after proof is produced.
  - docs/TEST_PLAN.md and scripts/ci/test_plan_run.sh — add 26b QEMU/Pi benchmark stages if not already covered by target-qualified runner configuration.
Commands:
  - python3 scripts/rest_perf_harness.py --mode perf --suite all --runs 5 --log-dir out/bench --log-prefix m26b-qemu-reference
  - python3 scripts/rest_perf_harness.py --mode perf --suite all --runs 5 --no-qemu --no-gateway --rest-url http://<pi4-gateway-host>:<port> --log-dir out/bench --log-prefix m26b-pi4-linked-runtime
Checks: Pi isolated runtime throughput meets or exceeds the selected QEMU reference under matched workload/provenance, latency remains recorded and checked against documented industry-normal physical LAN/control-plane ranges but excluded from the QEMU-throughput parity verdict, and side proof lanes stay separate.
Deliverables: fresh benchmark artifacts, comparator PASS/FAIL, normalized Pi proof, and updated benchmark docs.

Title/ID: m26b-authenticated-cohsh-reboot
Goal: Add an authenticated `cohsh reboot` command that works over TCP and local-seat without introducing a new in-VM listener or bypassing existing Queen secrets.
Inputs: crates/cohsh-core/src/{verb.rs,command.rs,help.rs}, apps/cohsh/src/lib.rs, apps/cohsh/src/transport/tcp.rs, apps/root-task/src/{event/mod.rs,reboot.rs,kernel.rs}, scripts/pi4-image-build.sh, third_party/u-boot/configs/rpi_4_defconfig, docs/{HARDWARE_BRINGUP.md,INTERFACES.md,USERLAND_AND_CLI.md}.
Changes:
  - crates/cohsh-core/src/* — add the shared `reboot` console verb and `REBOOT` ACK label so serial, TCP, cohsh, and local-seat parse the same command.
  - apps/root-task/src/reboot.rs + apps/root-task/src/kernel.rs — register the Pi 4 BCM2711 watchdog reset backend through HAL-owned device mapping, best-effort mark authenticated Pi resets as one-shot fast boots through a Cohesix-specific BCM2711 PM RSTS high marker before watchdog arming, report the retained watchdog software-reset status bit as diagnostics only, follow the BCM2711 PM WDOG/RSTC restart sequence for reset, and fail closed when that reset path is unavailable.
  - apps/root-task/src/event/mod.rs — require a secret-backed Queen session, emit `OK REBOOT detail=scheduled`, then defer the hardware reboot request long enough to flush the acknowledgement.
  - apps/cohsh/src/* — expose `cohsh reboot` through the existing authenticated TCP console flow and stop waiting for trailing lines once `OK REBOOT` is received.
  - scripts/pi4-image-build.sh + third_party/u-boot/configs/rpi_4_defconfig — generate a Pi 4 U-Boot script that loads saved `cohesix.env` policy, consumes and clears the authenticated reboot marker when visible, reports software-reset status as diagnostics only, enters the interactive Cohesix menu by default, and rejects stale U-Boot binaries that still use generic `bootflow scan`.
  - docs/USERLAND_AND_CLI.md + docs/snippets/cohsh_grammar.md — document reboot grammar and the TCP/local-seat authorization model.
  - docs/HARDWARE_BRINGUP.md + docs/INTERFACES.md — document the authenticated reboot fast-boot handoff and saved-policy behavior.
Commands:
  - cargo test -p cohsh-core
  - cargo test -p cohsh --lib network_console_verbs_forward_to_transport
  - cargo test -p root-task reboot --lib
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests::local_seat_ticket_backed_reboot_schedules_backend_request
  - scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml
Checks: Bare `reboot` without an authenticated session fails; physical console/local-seat reboot requires a Queen ticket minted from the existing Queen secret; TCP reboot remains gated by the existing TCP auth token plus Queen session; Pi 4 reset access uses only HAL-mapped PM/watchdog registers; an authenticated Pi reset best-effort sets the Cohesix high RSTS marker without making marker readback a policy gate; the watchdog reset path follows the BCM2711 PM WDOG/RSTC restart sequence and fails closed if those reset-critical accesses are unavailable; the next generated U-Boot script load consumes high marker state before menu/input setup when present, reports the software-reset bit as diagnostics only, and enters the interactive Cohesix menu whose first action continues with saved or manifest settings; the Pi 4 U-Boot default command sources `boot.scr.uimg` directly rather than entering EFI/bootflow scan.
Deliverables: Shared reboot verb, host CLI forwarding, authenticated root-task scheduling, HAL-backed Pi 4 reboot backend, one-shot authenticated reboot marker diagnostics, preserved interactive splash/menu behavior, generated-script validation, and tests for denial/success paths.

Title/ID: m26b-queen-log-evidence-dump
Goal: Make `/log/queen.log` a bounded, useful Pi 4 benchmark evidence surface and expose a host-side dump workflow through `cohsh` and SwarmUI without changing console or Secure9P semantics.
Inputs: apps/root-task/src/{log_buffer.rs,event/mod.rs,ninedoor.rs,bootstrap/log.rs}, apps/cohsh/src/lib.rs, apps/swarmui/src/{lib.rs,transport.rs}, apps/swarmui/frontend/{app.js,index.html,styles.css}, scripts/rest_perf_harness.py, docs/{HOST_TOOLS.md,USERLAND_AND_CLI.md,INTERFACES.md,TEST_PLAN.md}.
Changes:
  - apps/root-task/src/log_buffer.rs — raise live `/log/queen.log` retention to 2048 lines, add sequence/eviction metadata if needed for evidence integrity, and preserve bounded line storage.
  - apps/root-task/src/event/mod.rs + apps/root-task/src/ninedoor.rs — export the retained log through chunked, non-lossy streaming so `cat`/`tail`/`log` do not allocate or copy a 2048-line snapshot in one pending buffer and `END` is emitted only after the final retained line.
  - apps/root-task/src/bootstrap/log.rs + selected audit call sites — keep high-rate driver chatter out of the log while adding curated benchmark evidence markers: boot/build/session identity, log retention metadata, benchmark start/end, host write summaries, policy/lifecycle denials, telemetry wrap/quota, and bounded backpressure/error summaries.
  - apps/cohsh/src/lib.rs — add `log dump <local-file.txt>` as a host-only command that reads `/log/queen.log` through the existing transport, writes only payload lines to the destination, refuses existing files unless an explicit force form is documented, and preserves ACK/ERR/END grammar on the wire.
  - apps/swarmui/src/** + apps/swarmui/frontend/** — expose the same bounded log dump/read workflow as a SwarmUI projection over existing console/REST transport semantics, with no UI-owned evidence serializer, no second session in live console mode, and no hidden polling.
  - docs and tests — document the 2048-line bounded retention/export behavior, diagnostic contents, `cohsh log dump` usage after REST harness runs, SwarmUI workflow, and performance constraints.
Commands:
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib log_buffer event::tests
  - cargo test -p cohsh
  - cargo test -p swarmui
  - python3 -m pytest -q tests/test_rest_perf_harness.py
Checks: `/log/queen.log` retains up to 2048 curated lines with explicit oldest-line eviction, host/UI dumps preserve every retained line without truncation, generic command streaming remains bounded per turn, Pi 4 benchmark hot paths do not gain high-rate log writes, `cohsh log dump` and SwarmUI use existing read semantics only, and REST harness benchmark evidence can identify the run, transport/session, recent control failures, backpressure, and retention window without relying on UART spam.
Deliverables: 2048-line bounded queen log, non-lossy chunked dump path, curated benchmark evidence contents, `cohsh log dump` command, SwarmUI log dump/read projection, docs, and focused regression coverage.

Compatibility baseline tasks below are retained because they describe the original 26b DHCP, U-Boot policy, Wi-Fi, and QEMU guardrails that must not regress. They do not close reopened 26b by themselves; closure now requires the USB/Wi-Fi driver-task and concurrency/benchmark gates above plus the retained baseline checks below.

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
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
Checks:
  - Invalid policy settings fail deterministically; valid settings generate stable artifacts.
Deliverables:
  - Manifest/U-Boot-policy-driven DHCP mode selection with docs-as-built parity.

Title/ID: m26b-uboot-network-wizard
Goal: Add an interactive Pi 4 U-Boot network wizard with saved-config continue flow, bounded static IPv4 prompts, and optional logo display.
Inputs: scripts/pi4-image-build.sh, third_party/u-boot/configs/rpi_4_defconfig, docs/HARDWARE_BRINGUP.md, docs/INTERFACES.md, docs/COHESIX_LOGO.png.
Changes:
  - scripts/pi4-image-build.sh — generate an `askenv`-driven Pi 4 boot script that can continue with saved policy, choose DHCP vs static, choose wired vs Wi-Fi, collect bounded static IPv4 fields, persist only Cohesix policy to a dedicated FAT-side file, and optionally render a staged BMP logo on HDMI.
  - third_party/u-boot/configs/rpi_4_defconfig — enable the `askenv` and `bmp display` support required by the wizard while keeping the seL4-recommended Pi 4 USB baseline.
  - docs/HARDWARE_BRINGUP.md + docs/INTERFACES.md — document the wizard flow, persisted `coh_*` variables, bounded `/chosen/cohesix,*` static IPv4 properties, and the saved-config fast path.
Commands:
  - bash -n scripts/pi4-image-build.sh
  - make -C third_party/u-boot rpi_4_defconfig
  - make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j$(sysctl -n hw.ncpu)
  - scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml
Checks:
  - The first wizard action continues with saved Cohesix settings when any persisted override exists, otherwise it boots manifest defaults by default.
  - `DHCP OFF` collects bounded static IPv4 fields and mirrors them into DTB `/chosen/cohesix,static-*` properties before handoff.
  - The HDMI logo is optional: it renders when enabled and available, and boot still proceeds cleanly when it is disabled or cannot be staged.
Deliverables:
  - Pi 4 U-Boot network wizard with deterministic saved-config continuation and optional logo display.

Title/ID: m26b-pi4-wifi-dhcp-baseline
Goal: Add minimal Pi 4 Wi-Fi path sufficient for DHCP-backed diagnostics connectivity.
Inputs: apps/root-task/src/drivers/*, apps/root-task/src/hal/*, docs/ARCHITECTURE.md, docs/SECURITY.md.
Changes:
  - apps/root-task/src/hal/* — add Pi 4 Wi-Fi HAL traits for SDIO host I/O, power/reset control, OOB/host-wake observation, and firmware/NVRAM/CLM handoff; default implementations must fail deterministically with `unsupported`.
  - apps/root-task/src/drivers/* — add a dedicated CYW43455/CYW43xx-class driver path that follows the reference shape `bwfm` -> Zephyr/WHD HAL split -> Linux `brcmfmac` (`sdio.c`, `firmware.c`, `cfg80211.c`) without lifting source.
  - apps/root-task/src/drivers/* — implement the Pi 4 SDIO transport path first (function-0 direct I/O, function-1/2 enable, mailbox/interrupt handling, bounded firmware download, NVRAM/CLM staging) before join/auth logic.
  - apps/root-task/src/net/* — bind Wi-Fi link state and Ethernet frame ingress/egress into the shared smoltcp bootstrap/DHCP flow without introducing any new in-VM listener surface.
  - docs/ARCHITECTURE.md + docs/SECURITY.md — update the network backend matrix, bounds, threat notes, silicon identification (Pi 4B uses CYW43455 over SDIO), and reference-only provenance (`bwfm` -> Zephyr/WHD HAL split -> `brcmfmac` edge-case guidance; no code lift).
Commands:
  - cargo check -p root-task
  - cargo test -p root-task wifi:: -- --nocapture
  - rg -n "EFI_|boot_services|runtime_services|uefi::" apps/root-task/src tools/coh-rtc/src
Checks:
  - Wi-Fi join + DHCP works when enabled and declared; failure modes remain deterministic and auditable.
  - Wi-Fi runtime/device access remains HAL-only; any non-HAL direct access path fails review.
Deliverables:
  - Profile-gated CYW43xx Wi-Fi DHCP diagnostics path for Pi 4 with explicit reference-only provenance documentation.

Title/ID: m26b-pi4-wifi-hal-foundation
Goal: Add the HAL surface required to support Pi 4 Wi-Fi without any direct MMIO or firmware-service bypass in drivers.
Inputs: apps/root-task/src/hal/*, apps/root-task/src/kernel.rs, docs/ARCHITECTURE.md.
Changes:
  - apps/root-task/src/hal/mod.rs — add bounded SDIO, Wi-Fi control GPIO, OOB/host-wake, and firmware bundle traits/types with deterministic `unsupported` defaults.
  - apps/root-task/src/kernel.rs — keep Wi-Fi runtime rejection paths explicit until HAL-backed implementations are present.
  - docs/ARCHITECTURE.md — document HAL ownership of Pi 4 Wi-Fi transport/control resources.
Commands:
  - cargo check -p root-task --no-default-features --features net-console
  - cargo test -p root-task hal:: -- --nocapture
Checks:
  - All Wi-Fi transport/control operations are represented in HAL before any driver code depends on them.
  - Non-Pi4 or pre-implementation HALs fail deterministically with `unsupported`.
Deliverables:
  - Auditable Wi-Fi HAL contract for subsequent 26b tasks.

Title/ID: m26b-pi4-sdio-host-bringup
Goal: Implement the Pi 4 SDIO host path needed by CYW43455.
Inputs: apps/root-task/src/hal/*, apps/root-task/src/drivers/*, Pi 4 DTB/U-Boot handoff artifacts.
Changes:
  - apps/root-task/src/hal/* — add Pi 4 SDIO host implementation for the BCM2835/BCM2711 SDIO controller used by the on-board Wi-Fi device.
  - apps/root-task/src/drivers/* — add bounded command/data path helpers for function-0 direct I/O and function-1/2 CMD53 transfers.
  - docs/HARDWARE_BRINGUP.md — record the Pi 4 SDIO host node/pin assumptions and bring-up evidence.
Commands:
  - cargo check -p root-task
  - cargo test -p root-task sdio:: -- --nocapture
Checks:
  - SDIO host init, clocking, bus-width changes, and direct/extended transfers are HAL-only and test-covered.
Deliverables:
  - Pi 4 SDIO transport substrate for CYW43455.

Title/ID: m26b-cyw43455-firmware-transport
Goal: Bring the CYW43455 to a firmware-ready state with bounded mailbox and firmware/NVRAM/CLM handling.
Inputs: apps/root-task/src/drivers/*, out/pi4-sd/*, docs/SECURITY.md.
Changes:
  - apps/root-task/src/drivers/* — add CYW43455 reset/attach sequencing, core discovery, mailbox handling, firmware download, NVRAM application, and CLM load.
  - docs/SECURITY.md — document firmware provenance, bounds, and runtime trust assumptions for the bundled Pi 4 Wi-Fi blobs.
Commands:
  - cargo check -p root-task
  - cargo test -p root-task cyw43:: -- --nocapture
Checks:
  - Firmware bundle validation is bounded and deterministic.
  - Driver reaches a ready-for-join state without bypassing HAL.
Deliverables:
  - CYW43455 transport/firmware path ready for association work.

Title/ID: m26b-cyw43455-join-dhcp
Goal: Complete Pi 4 Wi-Fi diagnostics connectivity by joining a network and binding the shared DHCP/bootstrap flow.
Inputs: apps/root-task/src/net/*, apps/root-task/src/drivers/*, docs/INTERFACES.md, docs/HARDWARE_BRINGUP.md.
Changes:
  - apps/root-task/src/drivers/* — add bounded open-network/WPA2-PSK join sequencing, link-state reporting, and audited failure reasons.
  - apps/root-task/src/net/* — route `wifi` and `auto` policy through the CYW43455 path while preserving single-active-interface guarantees.
  - docs/INTERFACES.md + docs/HARDWARE_BRINGUP.md — document required credentials handoff, join breadcrumbs, DHCP evidence, and operator workflow.
Commands:
  - cargo check -p root-task
  - cargo test -p root-task wifi:: -- --nocapture
  - scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml
Checks:
  - `wifi` reaches link-up + DHCP when credentials are present.
  - `auto` prefers Wi-Fi only when the CYW43455 path is healthy and still exposes at most one active control-plane interface.
Deliverables:
  - End-to-end Pi 4 Wi-Fi diagnostics path with DHCP and console reachability.

Title/ID: m26b-pi4-wifi-hardware-validation
Goal: Capture Pi 4 on-device evidence for CYW43455 join, DHCP, and deterministic historical `auto` compatibility behavior before closing the retained 26b compatibility baseline.
Inputs: flashed Pi 4 SD image, serial capture, Linux known-good brcmfmac/MMC/SDHCI capture, U-Boot USB handoff trace, Wi-Fi credentials, direct-link/compatibility host setup, docs/HARDWARE_BRINGUP.md, docs/TEST_PLAN.md.
Changes:
  - scripts/pi4_trace_normalize.py — normalize Pi 4 USB/Wi-Fi serial traces into JSONL/summary artifacts for boot-to-boot comparison.
  - docs/HARDWARE_BRINGUP.md — record the exact serial breadcrumbs, host commands, and observed IP/console evidence for a successful Wi-Fi boot.
  - docs/TEST_PLAN.md — add the final Pi 4 Wi-Fi validation transcript and `auto` fallback proof.
Commands:
  - scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml
  - minicom -D /dev/cu.usbserial-0001 -b 115200 -o -C pi4-serial.log
  - python3 scripts/pi4_trace_normalize.py pi4-serial.log --summary
  - python3 scripts/pi4_trace_normalize.py pi4-serial.log --gate-summary --expect USB_GATE=10 --expect USB_BLOCKER=none --expect WIFI_GATE=10 --expect WIFI_BLOCKER=none --expect WIFI_EXACT=none
  - python3 scripts/pi4_trace_normalize.py pi4-serial.log --gate-summary --expect USB_BOOTLOADER_HANDOFF_SEEN=no --expect USB_COLD_BOOT_SEEN=yes --expect BOOT_HALTED=no --expect TIMER_IRQ27_SEEN=no
  - python3 scripts/pi4_trace_normalize.py pi4-linux-known-good.log --domain wifi --summary
  - cargo run -p cohsh --features tcp -- --transport tcp --tcp-host <wifi-lease-ip> --tcp-port 31337 --auth-token bootstrap
Checks:
  - Normalized Cohesix, U-Boot, and Linux comparison traces preserve USB/Wi-Fi stages, blockers, and latest verdicts as machine-readable evidence.
  - Final Pi 4 USB acceptance requires `USB_COLD_BOOT_SEEN=yes`, command/event-ring proof before live Cohesix-owned root-port power assertion/readback/sampling/reset, and no Linux/U-Boot captured root-port enumeration.
  - Serial shows CYW43455 attach/join breadcrumbs followed by `[dhcp] lease bound ...`.
  - `netstats` / `netstatus` report the Wi-Fi lease on `policy=wifi`.
  - `auto` proves single-active-interface behavior; historical compatibility profiles may exercise absent-device wired fallback, while the physical driver-task profile treats selected-CYW43 attach/join/runtime failure as fatal driver evidence.
Deliverables:
  - Pi 4 hardware validation evidence, normalized trace summaries, and known-good Linux/U-Boot comparison artifacts required to preserve the historical Milestone 26b compatibility baseline.

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
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
Checks:
  - `nettest` on `wired` and `wifi` policies executes against only the selected interface.
  - `auto` policy uses deterministic priority/failover, with at most one active TCP console interface at any moment.
  - Existing QEMU `nettest` behavior and host workflows remain backward compatible with no required command changes.
Deliverables:
  - Policy-driven `nettest` behavior for Pi 4 DHCP milestones with explicit single-active-interface guarantees and compatibility evidence.

Title/ID: m26b-swarmui-tauri2-host-upgrade
Goal: Keep SwarmUI on the latest stable Tauri desktop line without changing Cohesix transport or UI semantics during the Pi 4 DHCP rollout.
Inputs: apps/swarmui/*, scripts/setup_environment.sh, scripts/linux_host_tools_sync.sh, docs/HOST_TOOLS.md.
Changes:
  - apps/swarmui/Cargo.toml + apps/swarmui/src-tauri/main.rs + apps/swarmui/tauri.conf.json — migrate SwarmUI from Tauri 1 to Tauri 2 while preserving the existing command surface, replay/bootstrap flow, CSP, and `window.__TAURI__` frontend bridge.
  - apps/swarmui/src-tauri/capabilities/default.json — add the minimum desktop capability set required by the Tauri 2 permission model.
  - apps/swarmui/tests/no_http_deps.rs — scope dependency-policy checks to the active desktop target so Tauri 2 mobile-only transitive crates do not create false failures.
  - scripts/setup_environment.sh + scripts/linux_host_tools_sync.sh + docs/HOST_TOOLS.md — align Linux runtime/build prerequisites to the Tauri 2 WebKitGTK 4.1 package line.
Commands:
  - cargo check -p swarmui
  - cargo test -p swarmui
Checks:
  - SwarmUI keeps the existing host-side command/invoke surface and passes its replay, cache, security, transcript, and console-parity tests after the Tauri 2 migration.
  - Linux host setup/build scripts install WebKitGTK 4.1 packages required by Tauri 2.
  - No new Cohesix protocol verbs, transports, or UI authority are introduced.
Deliverables:
  - SwarmUI upgraded to the current Tauri 2 desktop line with host-tool packaging and test coverage kept in sync.

Title/ID: m26b-swarmui-spectrum-shell
Goal: Rebuild the SwarmUI desktop shell around Spectrum Web Components while preserving Cohesix transport semantics, replay behavior, and Live Hive rendering.
Inputs: apps/swarmui/frontend/*, tools/swarmui-ui-tests/*, docs/USERLAND_AND_CLI.md.
Changes:
  - apps/swarmui/frontend/index.html + app.js + components/console.js + styles/* — adopt a vendored Spectrum-based shell (`sp-theme`, `sp-button`, `sp-textfield`, `sp-picker`, `sp-divider`) for operator controls while preserving all current IDs, panel outputs, and Pixi-backed Live Hive behavior.
  - apps/swarmui/frontend/vendor/spectrum.bundle.js — self-host the exact Spectrum Web Components bundle needed by SwarmUI so Tauri/release builds remain offline-safe and CDN-free.
  - tools/swarmui-ui-tests/tests/swarmui.spec.js + screenshots — update the Playwright harness for Spectrum-backed text fields/buttons, keep replay/canvas coverage intact, and add shell regression checks for ticket minting and control wiring.
  - docs/USERLAND_AND_CLI.md — record that SwarmUI ships a vendored Spectrum shell while remaining a presentation-only frontend over existing Cohesix transports.
Commands:
  - cargo check -p swarmui --all-targets
  - cargo clippy -p swarmui --all-targets -- -D warnings
  - cargo test -p swarmui
  - cd tools/swarmui-ui-tests && npm test
Checks:
  - SwarmUI keeps the same Tauri command surface, replay/bootstrap flow, and Live Hive polling semantics after the Spectrum shell migration.
  - The shell remains self-hosted for Tauri/release bundles with no CDN requirement and no new transport authority.
  - Playwright coverage exercises the Spectrum-backed controls without weakening existing Live Hive and transcript assertions.
Deliverables:
  - World-class SwarmUI shell using vendored Spectrum Web Components with replay-safe UI regression coverage and warning-free SwarmUI Rust builds.
```

---

## Milestone 26c — Regression-Gated Refactor + Surface Audit (Zero-Regression) <a id="26c"></a>
[Milestones](#Milestones)

**Why now (reviewer trust):**
Milestones 25-26b establish technical capability, transport breadth, Pi 4 bring-up evidence, and isolated runtime benchmark closure, but the implementation has accumulated visible scaffolding, duplicated validation paths, long runtime modules, and uneven characterization coverage. Milestone 26c is the aggressive refactor window after isolated runtime benchmark closure and before seL4 15 realignment: it inventories tracked Markdown authoring surfaces, records docs-as-built truth, expands characterization and boundary gates, and then permits broad behavior-preserving refactors across Cohesix-authored host tools, root-task adapters, HAL-facing network code, tests, and public documentation. Cleanup is complete only when the target-qualified staged Test Plan passes on both QEMU and Pi 4 with evidence that external behavior did not drift.

**Current planning status:** Complete. The documentation-only task
`m26c-readme-linked-doc-suite-remediation`, including its preservation and
newcomer-glossary follow-ups, completed on 15 July 2026. The later correction
that removed fabricated LoRa-radio semantics was authorized and closed
separately under Milestone 18; it does not leave Milestone 26c open. Historical
release snapshots remain immutable, and their superseded wording is recorded
in `docs/audit/M26C_DOC_DRIFT_LEDGER.md`.

The original QEMU and Pi 4 closure remains accepted. QEMU closure is anchored
by `out/test-plan/m26c-qemu` and Stage 05 due-diligence root
`out/audit/gate/20260628T015332Z`. Pi 4 closure is anchored by the final wired
GENET boot in `/Users/lukasbower/pi4-serial-20260629-135454.log`, paired
USB-Ethernet capture `/Users/lukasbower/tcpdump-usb-eth-20260629-135504.pcap`,
runtime/DMA proof bundle
`out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env`, direct TCP
proof `out/test-plan/m26c-pi4-live/cohsh-tcp-proof-genet-latest.txt`, and Stage
05 due-diligence root `out/audit/gate/20260629T061204Z`. That accepted proof is
wired GENET at `192.168.10.50` with `PI4_RUNTIME_DMA_PROOF=fresh-pi`,
`PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`,
`TIMER_BACKEND=arch-counter`, `TIMER_CLOCK_HZ=54000000`,
`TIMER_EL0_COUNTER=vct`, DHCP-bound TCP `cohsh`, and REST/gateway validation.
It does not substitute for current-image Milestone 26d Pi 4 revalidation.

**Non-negotiable constraints:**
- No protocol, namespace, ACK/ERR/END, telemetry, manifest, console grammar, Secure9P, or release-behavior drift may hide under a "refactor", "cleanup", or "humanizing" label.
- Refactors are behavior-preserving only: extraction, decomposition, deduplication, typed-error cleanup, naming cleanup, and invariant documentation after characterization coverage exists for the touched surface.
- The only planned 26c behavior changes are the named worker-architecture, phase-1 cap-backed worker endpoint, notification-backed lifecycle, and profile-qualified MCS evidence tasks. They close public Queen/Worker documentation drift without adding new roles, protocols, namespace grammar, or host-only authority paths.
- The named worker/cap/notification/MCS lanes are not refactors. They must land as authorized behavior-changing tasks, then freeze a post-behavior external-contract baseline before any Phase 4 cleanup wave starts.
- The target-qualified staged runner is an enabling blocker, not merely closure polish. `scripts/ci/test_plan_run.sh --target qemu|pi4` and `scripts/ci/check_test_plan.sh` must exist and agree before cleanup or structural decomposition can start.
- Humanization is evidence-first and mostly subtractive: delete generic boilerplate before rewriting, keep comments only for invariants/bounds/authority/failure behavior, and make the AI-fingerprint audit an independent gate.
- Simplicity must be measured, not asserted. Cleanup/refactor waves must show reduced duplication, clearer ownership, fewer generic comments, smaller or better-named hot modules where practical, and no new abstraction unless it removes real repetition or isolates authority.
- Every cleanup/refactor wave must be bisectable: one owned surface, one preserved-contract list, one characterization artifact, one scorecard entry, one targeted test subset, and final QEMU/Pi staged-run compatibility evidence before milestone closure.
- Generated snippets, generated reports, tracked release snapshots, append-only audit evidence, seL4/reference mirrors, and vendored Markdown are update-by-source or inventory-only surfaces. Do not hand-edit them for style.
- VM-side Cohesix remains `no_std`; shared semantic helpers must be `no_std` by construction and must not pull host transport, filesystem, process, network, or provider crates into VM closure profiles.
- HAL remains the only path for device authority, MMIO, DMA publication, IRQ binding, physical-address handling, firmware-service handoff, and cache-maintenance policy. SDIO/USB seams are extraction-only and must not introduce new hardware support or a parallel driver framework.
- Pi 4 DMA truth is `bounded_no_iommu`: bounded HAL-managed DMA ownership and evidence, not malicious-device DMA confinement or SMMU/IOMMU isolation.
- Linked Pi 4 runtime-image specs, `pi4-driver-abi`, runtime CPIO staging, owner-state descriptors, the 26b SDIO acceptance state, and fresh-Pi hardware proof remain separate evidence states; generated eligibility or QEMU proof is not board closure. Milestone 26c may audit and preserve this evidence split, but SDIO graduation belongs to 26b.
- Isolated runtime performance evidence is counter-qualified: valid `DRIVER_TASK_COUNTER` snapshots, owner-state proof, and same-harness Pi benchmark evidence do not close latency claims unless the Pi profile proves `timers-arch-counter` with `TIMER_BACKEND=arch-counter`, `TIMER_CLOCK_HZ=54000000`, `TIMER_EL0_COUNTER=vct`, and `DUMMY_TIMER_SEEN=no`. Dummy-timer captures, stale serial logs, or physical-counter/timer-control exports leave performance proof red.
- Multi-agent execution is mandatory. Each lane must leave a checked-in or attached handoff with inputs, files touched or intentionally skipped, commands, artifact paths, blockers, deferrals, residual gaps, and PASS/FAIL status.
- Milestone closure requires a clear/deferred blocker ledger, complete lane handoffs, green task gates, and full target-qualified staged Test Plan PASS on QEMU and Pi 4 with no `INCOMPLETE` markers.

### Prerequisite
- Reopened Milestones **26a** and **26b** completed, including checked-in Pi 4 USB/serial/HDMI responsiveness evidence under wired and Wi-Fi load, driver-task scheduling evidence, arch-counter timer-backend proof, GENET/static compatibility evidence, DHCP/Wi-Fi compatibility evidence, QEMU compatibility evidence, regenerated profile-specific manifest evidence, isolated `pi4-driver-*` runtime image/ABI evidence, and explicit closure or deferral of any runtime-image owner-state proof boundary.

### Goal
Humanize and simplify Cohesix-authored code and documentation without losing behavioral proof. The milestone succeeds only if the audit artifacts, characterization gates, targeted worker/runtime additions, cleanup refactors, generated outputs, and QEMU/Pi 4 staged evidence all agree on the as-built system, and the final scorecard shows the main authored surfaces are easier to read, review, and maintain than the starting point.

### Execution Order
Later phases may start only when earlier phase gates are complete for the touched surface or explicitly deferred in `docs/audit/M26C_AS_BUILT_BLOCKERS.md` with dependency impact.

| Phase | Purpose | Tasks | Gate |
| --- | --- | --- | --- |
| 0 | Scope, ownership, blockers | `m26c-as-built-blocker-ledger`, `m26c-target-qualified-runner-baseline`, `m26c-refactor-map-and-risk-ratchet`, lane setup in `M26C_AGENT_HANDOFFS.md` | No unowned files, hidden blockers, missing target-qualified runner contract, missing sub-agent lanes, or unmeasured simplification targets. |
| 1 | Documentation and provenance truth | `m26c-authoring-charter-and-header-rules`, `m26c-ai-fingerprint-authorship-review`, `m26c-markdown-inventory-and-disposition`, `m26c-mermaid-as-built-diagram-audit`, `m26c-docs-as-built-audit`, `m26c-runtime-boundary-and-semantic-parity-audit` | Inventories, drift ledger, AI-fingerprint audit, parity matrix, and generated-source dispositions are complete. |
| 2 | Required implementation additions | `m26c-pi-runtime-dma-proof-closure`, `m26c-dma-protection-profile-truth`, `m26c-worker-architecture-implementation`, `m26c-cap-backed-worker-endpoints`, `m26c-notification-backed-worker-lifecycle`, `m26c-worker-driver-mcs-budget-evidence`, `m26c-post-behavior-baseline-freeze` | New behavior is compiler-declared, tested, documented as-built, does not create new protocol/namespace grammar, and is frozen into the post-behavior baseline before refactor work starts. |
| 3 | Refactor safety gates | `m26c-characterization-gates-before-refactor`, `m26c-no-std-boundary-gates` | Cleanup-sensitive behavior and VM dependency closures are pinned before structural edits. |
| 4 | Humanization and structural cleanup | `m26c-low-risk-surface-cleanup`, `m26c-host-tool-structural-cleanup`, `m26c-root-task-runtime-decomposition`, `m26c-hal-network-and-local-seat-decomposition` | Each cleanup wave is bisectable and starts only after its characterization artifact, preserved-contract list, owner, scorecard entry, and target test subset are recorded; risk-ratchet does not regress and HAL/no_std/protocol boundaries hold. |
| 5 | Closure | `m26c-full-test-plan-qemu-and-pi4` | QEMU and Pi 4 staged Test Plan runs pass with target-qualified artifacts and no incomplete markers. |

### Documentation Scope
The Markdown and Mermaid lanes cover only tracked Markdown returned by `git ls-files '*.md' | sort`. Ignored build output, local evidence folders, caches, virtualenvs, and nested external checkouts are excluded unless the main Cohesix repository tracks them.

The inventory must cover root docs, app/crate/tool READMEs, `docs/**/*.md`, audit registers, dated audit snapshots, checklists, generated compliance reports, snippets, release snapshots, tracked `seL4/**/*.md`, and tracked `third_party/**/*.md`. Each entry must be classified as one of: human-edited canonical source, live audit register, append-only audit evidence, generated report, generated snippet, release snapshot, vendored reference, or external reference mirror. Mermaid blocks inherit the disposition of their containing file unless the Mermaid inventory records a stricter update rule.

### Required Artifacts
Task blocks below own the exact file changes, commands, checks, and deliverables. At milestone level, closure requires these artifact families:

- Audit control: `docs/audit/M26C_AS_BUILT_BLOCKERS.md`, `M26C_AGENT_HANDOFFS.md`, `M26C_TARGET_RUNNER_BASELINE.md`, `M26C_REFACTOR_MAP.md`, `M26C_REFACTOR_RISK_RATCHET.csv`, `M26C_REFACTOR_OWNERSHIP.md`, `M26C_SIMPLICITY_SCORECARD.md`, `M26C_POST_BEHAVIOR_BASELINE.md`.
- Documentation truth: `M26C_MARKDOWN_INVENTORY.{csv,md}`, `M26C_MERMAID_INVENTORY.csv`, `M26C_MERMAID_GITHUB_RENDER_AUDIT.md`, `M26C_DOCS_AS_BUILT_AUDIT.md`, `M26C_DOC_DRIFT_LEDGER.md`, `M26C_AI_FINGERPRINT_AUDIT.md`.
- Runtime boundaries: `M26C_RUNTIME_BOUNDARY_AUDIT.md`, `M26C_NINEDOOR_PARITY_MATRIX.md`, VM-target dependency trees, generated manifest/snippet hashes, Pi 4 runtime/DMA proof classification, arch-counter timer-backend proof, and DMA protection profile evidence.
- Worker/runtime implementation evidence: worker architecture tests, cap-backed endpoint negative tests, notification lifecycle tests, MCS/non-MCS scheduling evidence, isolated runtime ABI tests, and fresh-source provenance for staged Pi runtime artifacts.
- Cleanup evidence: before/after characterization for each refactor wave, risk-ratchet review, no-std/HAL boundary evidence, simplification scorecard entries, and authoring cleanup decisions tied back to as-built facts.
- Closure evidence: QEMU and Pi 4 state dirs with `stage_01.done` through `stage_05.done`, required target artifacts present before `stage_05.done`, no incomplete markers, due-diligence outputs, and a final planner reconciliation of all sub-agent handoffs.

### Commands
- `git ls-files '*.md' | sort > out/audit/m26c_markdown_inventory.txt`
- `cut -d, -f1 docs/audit/M26C_MARKDOWN_INVENTORY.csv | sed '1d' | sort > out/audit/m26c_markdown_inventory_checked_in.txt`
- `diff -u out/audit/m26c_markdown_inventory.txt out/audit/m26c_markdown_inventory_checked_in.txt`
- `scripts/ci/mermaid_inventory.py --markdown-list out/audit/m26c_markdown_inventory.txt --out docs/audit/M26C_MERMAID_INVENTORY.csv`
- `scripts/ci/check_mermaid_github.sh --markdown-list out/audit/m26c_markdown_inventory.txt`
- `scripts/ci/render_mermaid_github.sh --markdown-list out/audit/m26c_markdown_inventory.txt --out out/audit/m26c-mermaid-rendered`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`
- `scripts/check-generated.sh`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo audit`
- `cargo deny check advisories`
- `rg -n "unsafe|unwrap\\(|expect\\(|panic!" apps crates tools`
- `rg -n "robust|flexible|seamless|world-class|comprehensive|easy to use|powerful|simple and intuitive|AI-generated|generated by" README.md CONTRIBUTING.md AGENTS.md docs apps crates tools`
- `rg -n "Defines the|Provides the|CLI entry point|library surface|module for|This module" apps crates tools docs README.md CONTRIBUTING.md AGENTS.md`
- `cargo test -p secure9p-core`
- `cargo test -p nine-door --test integration`
- `cargo test -p coh --features mock`
- `cargo test -p cohsh`
- `cargo test -p cohsh-core`
- `cargo test -p host-ticket-agent`
- `cargo test -p gpu-bridge-host`
- `cargo test -p hive-gateway`
- `python3 -m pytest tools/cohesix-py/tests -q`
- `cargo test -p root-task --tests`
- `cargo test -p root-task --lib`
- `cargo test -p worker-heart`
- `cargo test -p worker-gpu`
- `cargo test -p worker-lora`
- `cargo test -p pi4-driver-abi`
- `cargo test -p pi4-driver-runtime`
- `cargo check -p root-task --target aarch64-unknown-none --no-default-features --features "cohesix-dev"`
- `cargo tree -p root-task --target aarch64-unknown-none -e normal --no-default-features --features "cohesix-dev" > out/audit/m26c_root_task_tree_qemu.txt`
- `cargo check -p root-task --target aarch64-unknown-none --no-default-features --features "kernel bootstrap-trace serial-console net-console"`
- `cargo tree -p root-task --target aarch64-unknown-none -e normal --no-default-features --features "kernel bootstrap-trace serial-console net-console" > out/audit/m26c_root_task_tree_pi4.txt`
- `cargo check -p pi4-driver-runtime --target aarch64-unknown-none`
- `scripts/ci/check_test_plan.sh`
- `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml`
- `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m26c-qemu`
- `scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m26c-pi4`

### Closure Gates
- Every task block below satisfies its own Commands, Checks, and Deliverables; any skipped command has a documented environment blocker and remediation owner in the 26c handoff log.
- The target-qualified runner contract is implemented and checked before Phase 4 begins; `--target qemu|pi4` is not allowed to remain a milestone-closing TODO while cleanup is underway.
- The blocker ledger is clear or explicitly deferred to named later milestones, and no deferred item is used as satisfied evidence.
- Markdown, Mermaid, docs-as-built, AI-fingerprint, runtime-boundary, NineDoor parity, refactor-map, risk-ratchet, and sub-agent handoff artifacts all exist and agree with generated/source truth.
- Pi 4 runtime/DMA evidence separates generated eligibility, target compile, QEMU proof, fresh hardware proof, 26b SDIO owner-state evidence, and full board closure; Pi 4 DMA wording remains `bounded_no_iommu` and never implies SMMU/IOMMU isolation. Milestone 26c does not reopen SDIO graduation.
- Pi 4 performance evidence separates valid `DRIVER_TASK_COUNTER` snapshots, owner-state proof, same-harness benchmark evidence, and timer-backend proof; any missing or dummy timer backend keeps latency proof red even when runtime/DMA proof is otherwise present.
- Worker-heart, worker-gpu, and worker-lora have real VM-side worker loops; phase-1 VM worker authority requires matching badged endpoint caps; lifecycle delivery uses generated notifications where applicable; MCS claims are profile-qualified.
- `docs/audit/M26C_POST_BEHAVIOR_BASELINE.md` records the external behavior snapshot after authorized worker/cap/notification/MCS additions and before cleanup begins; later refactor waves compare against that baseline rather than the pre-26c placeholder behavior.
- Cleanup/refactor waves have before/after characterization, preserve console/Secure9P/manifest/telemetry/release behavior, keep HAL-only device authority, keep VM builds `no_std`, and do not increase non-test risk indicators without approved exceptions.
- Each cleanup/refactor wave is small enough to revert independently and records owner, touched surface, preserved contracts, characterization artifact, targeted test subset, before/after result, and scorecard delta.
- `M26C_SIMPLICITY_SCORECARD.md` records before/after evidence for touched high-value surfaces: boilerplate removed, duplicated logic collapsed, module/function boundaries improved, docs shortened or clarified, and any net complexity increase justified by required worker/runtime behavior or tests.
- Workspace baseline checks (`fmt`, `clippy -D warnings`, `check`, `test`, `cargo audit`, and `cargo deny check advisories`) pass or have scoped blockers recorded before closure.
- QEMU and Pi 4 staged Test Plan runs both pass through `stage_05.done`, contain required target artifacts, and contain no incomplete markers.
- Final closure evidence includes representative before/after diffs, inventories, audits, generated-artifact proof, QEMU/Pi 4 state dirs, due-diligence outputs, and a planner reconciliation of every sub-agent lane.

### Compiler / docsystem touchpoints
- `coh-rtc` generated snippets referenced by `docs/snippets/*.md` remain authoritative; 26c may only change their source schemas or generators, never hand-edit the derived snippet text.
- `docs/nist/REPORT.md` remains generated from its source registry; 26c may only update it through the proper generator flow.
- Mermaid diagrams inside generated, release-derived, vendored, or reference-only Markdown follow the same disposition rules as their containing file; 26c may audit and report them, but edits must occur through the proper source, release-cut, or provenance flow.
- Tracked release-cut documentation under `releases/RELEASE_NOTES-*.md`, `releases/*/{README.md,QUICKSTART.md,RELEASE_NOTES.md}`, `releases/*/docs/**/*.md`, and `releases/*/python/**/README.md` remains derived from canonical docs and release packaging; if snapshot wording changes, the corresponding release-cut flow and notes must change in the same work. Release-local validation outputs under `releases/**/out/**`, embedded virtualenv content under `releases/**/.venv*/**`, and other untracked byproducts are provenance inputs only and are not part of the 26c inventory.
- `docs/TEST_PLAN.md`, `scripts/ci/test_plan_run.sh`, `scripts/ci/check_test_plan.sh`, and the stage scripts become authoritative for both QEMU and Pi 4 `PASS` semantics.
- Generated `root_task.driver_images` records, `crates/pi4-driver-abi`, isolated `pi4-driver-*` runtime image artifacts, and driver-runtime CPIO packaging are authoritative boundary artifacts for Pi 4 driver-task cleanup; 26c may audit, test, and refactor around them only without changing acceptance status or hand-editing generated descriptors.
- Pi 4 runtime/DMA proof fields, descriptor resource totals, owner-state evidence, and source artifact freshness are 26c audit/test-plan surfaces; later operator tooling may project them only after this milestone defines the evidence semantics.
- Pi 4 timer-backend fields are 26c audit/test-plan surfaces for performance claims. The accepted Pi profile uses read-only EL0 virtual-counter telemetry only when `KernelArmExportVCNTUser` / `CONFIG_EXPORT_VCNT_USER` is enabled, scales elapsed-time proof from `TIMER_CLOCK_HZ`, and rejects physical counter or EL0 timer-control exports for isolated runtime latency evidence.
- DMA protection profile fields are compiler-owned surfaces. Pi 4 profiles must resolve to bounded no-IOMMU discipline, while SMMU-backed profiles are future hardware targets that require generated per-device DMA-domain state before code or docs may claim hardware-enforced isolation.
- Any future shared semantic helpers introduced to reduce host/VM drift must remain explicitly `no_std`-safe, must not import host-side capabilities, transports, or provider crates into the VM build, and must be reviewed against the archived per-profile dependency trees.
- Worker role/task manifests, ticket scopes, lease bindings, telemetry paths, and lifecycle gates are compiler-owned surfaces; any new worker implementation fields must enter `coh-rtc` IR and generated docs before code depends on them.
- Cap-backed worker endpoint-ticket fields are compiler-owned surfaces in 26c. Generated role state must describe which badged endpoint caps are minted for attach/control/telemetry/revoke-sensitive paths and which stronger cap-bundle fields remain deferred.
- Notification lifecycle fields are compiler-owned surfaces in 26c. Generated role state must describe notification badges for revoke, shutdown, lease, telemetry-pressure, and IRQ events where applicable, and must state which notification authority moves into the later full cap-bundle profile.
- Worker and driver scheduling-context fields are compiler-owned and profile-qualified. MCS profiles must generate budget/period, timeout endpoint, and consumed-budget evidence fields; non-MCS profiles must generate the priority/domain and bounded service-turn fallback state instead.
- Refactor-generated module boundaries must not become new public interfaces unless `docs/INTERFACES.md`, `docs/HOST_TOOLS.md`, or the relevant canonical doc is updated in the same change.
- HAL/network decompositions must retain generated manifest authority for boot policy defaults and must not hand-code policy values that already belong in `root_task.toml` or `coh-rtc` outputs.

### Task Breakdown
```
Title/ID: m26c-as-built-blocker-ledger
Goal: Record and gate current as-built mismatches before 26c cleanup or downstream milestones claim alignment.
Inputs: docs/BUILD_PLAN.md, AGENTS.md, docs/SECURE9P.md, docs/ARCHITECTURE.md, docs/INTERFACES.md, docs/SECURITY.md, docs/SECURITY_NIST_800_53.md, docs/TEST_PLAN.md, docs/HARDWARE_BRINGUP.md, configs/root_task_pi4_uboot_aarch64.toml, scripts/ci/test_plan_run.sh, apps/root-task/src/net/**, apps/root-task/src/drivers/**, apps/root-task/src/hal/**, apps/root-task/src/ninedoor.rs, apps/pi4-driver-runtime/**, crates/pi4-driver-abi/**, apps/worker-heart/src/kernel.rs, apps/worker-gpu/src/kernel.rs, scripts/release_bundle.sh
Changes:
  - docs/audit/M26C_AS_BUILT_BLOCKERS.md — checked-in blocker ledger with owner, severity, dependency impact, evidence command, and closure/defer decision for each initial blocker.
  - docs/BUILD_PLAN.md — keep 26c and later milestones synchronized with blocker disposition changes.
  - docs/TEST_PLAN.md — reference target-qualified evidence expectations once the runner is implemented.
Commands:
  - rg -n "TCP_SMOKE_PORT|31339" apps/root-task scripts docs
  - rg -n "read_volatile|write_volatile|from_exposed_addr|phys|MMIO" apps/root-task/src/drivers apps/root-task/src/serial
  - cargo test -p secure9p-codec
  - scripts/ci/check_test_plan.sh
Checks:
  - Initial blocker entries cover 26b evidence, isolated Pi 4 runtime-image acceptance gaps, Pi 4 network IR/docs validation drift, target-qualified runner, Secure9P path validation, non-console in-VM TCP, HAL MMIO ownership, generated-doc drift, fixture/default secrets, placeholder auth defaults, worker-task docs/implementation mismatch, and later-milestone as-built overclaims for persistence, `coh-status`, delegated REST identity, writer-epoch fencing, AI namespace roots, and AWS/ENA/IMDS support.
  - No structural cleanup task starts until blockers are fixed or explicitly deferred with dependency impact.
Deliverables:
  - Auditable 26c blocker ledger that prevents downstream milestones from inheriting unresolved as-built drift silently.

Title/ID: m26c-target-qualified-runner-baseline
Goal: Make target-qualified staged-run semantics real before any cleanup or structural decomposition depends on them.
Inputs: docs/TEST_PLAN.md, docs/HARDWARE_BRINGUP.md, scripts/ci/test_plan_run.sh, scripts/ci/test_plan_stage_*.sh, scripts/ci/check_test_plan.sh, scripts/pi4-image-build.sh, scripts/uboot/qemu-uboot-smoke.sh
Changes:
  - scripts/ci/test_plan_run.sh — add explicit `--target qemu|pi4` selection, write target metadata into the shared state dir, pass the selected target to every stage, and reject unsupported target/stage combinations before a stage can write misleading evidence.
  - scripts/ci/test_plan_run.sh + scripts/ci/test_plan_common.sh — add explicit `--iteration` rerun semantics and per-stage input fingerprints so focused reruns cannot be mistaken for target-qualified PASS and stale earlier evidence is detected before later-stage reuse.
  - scripts/ci/test_plan_stage_*.sh — consume the selected target and write target-qualified stage markers, logs, iteration markers, input fingerprints, and incomplete records without treating QEMU artifacts as Pi 4 proof or Pi 4 hardware logs as QEMU proof.
  - scripts/ci/test_plan_stage_05_due_diligence.sh + scripts/ci/due_diligence_gate.sh — permit Stage 05 to validate and reuse fresh Stage 03 regression evidence from the same state dir while leaving standalone due-diligence exhaustive by default.
  - scripts/ci/check_test_plan.sh — enforce that docs, runner usage, stage scripts, and target matrix language agree.
  - docs/TEST_PLAN.md + docs/HARDWARE_BRINGUP.md — document the QEMU/Pi 4 target matrix, target-specific prerequisites, allowed stage combinations, state-dir metadata, and PASS/INCOMPLETE interpretation.
  - docs/audit/M26C_TARGET_RUNNER_BASELINE.md — checked-in evidence for runner syntax, target matrix, smoke commands, intentionally unsupported combinations, and the rule that Phase 4 cleanup cannot start without this gate.
Commands:
  - scripts/ci/test_plan_run.sh --list
  - scripts/ci/test_plan_run.sh --target qemu --stage 1 --state-dir out/test-plan/m26c-runner-qemu-smoke
  - scripts/ci/test_plan_run.sh --target pi4 --stage 1 --state-dir out/test-plan/m26c-runner-pi4-smoke
  - scripts/ci/check_test_plan.sh
Checks:
  - `--target qemu|pi4` is accepted by the runner, recorded in the state dir, and visible to every stage script.
  - Unsupported target/stage combinations fail before producing `stage_*.done` or target-qualified PASS evidence.
  - Focused `--iteration` runs write only `stage_*.iteration` / `stage_*.<target>.iteration` plus input fingerprints, never PASS markers.
  - `COHSH_BATCH_GROUPS` subsets are iteration-only; final Stage 03 and due-diligence runs require the full regression batch or verified reuse of a fresh full Stage 03 batch.
  - The checker fails if docs mention runner commands, stage matrices, or closure criteria that the scripts do not implement.
  - No Phase 4 cleanup or structural decomposition task may start until this runner baseline is present and PASS.
Deliverables:
  - Target-aware staged-runner contract and smoke evidence available early enough to guard all later cleanup.

Title/ID: m26c-authoring-charter-and-header-rules
Goal: Replace generic repository-wide authoring templates with explicit human-authored comment, header, and documentation rules.
Inputs: AGENTS.md, CONTRIBUTING.md, README.md, docs/CODING_GUIDELINES.md, docs/API_GUIDELINES.md, docs/BUILD_PLAN.md
Changes:
  - AGENTS.md — narrow file-header requirements to legal/provenance metadata plus rules for when comments are required.
  - CONTRIBUTING.md — codify review expectations for comment quality, diff shape, subtractive cleanup, and no-regression sequencing.
  - README.md — align contributor-facing quality language with the new authoring policy.
  - docs/CODING_GUIDELINES.md — define acceptable doc-comment content, prohibited template phrasing, when generic comments should be deleted rather than rewritten, and behavior-preserving cleanup rules.
  - docs/API_GUIDELINES.md — require interface docs to describe contracts, invariants, and failure modes instead of file/module summaries.
Commands:
  - rg -n "Purpose:|Defines the|Provide(s)? the|CLI entry point|library and public module surface|module for" AGENTS.md CONTRIBUTING.md README.md docs/CODING_GUIDELINES.md docs/API_GUIDELINES.md
  - cargo fmt --all -- --check
Checks:
  - Repository guidance no longer instructs contributors to write generic file-summary boilerplate.
  - Header and comment rules are internally consistent across canonical contributor documents.
Deliverables:
  - Canonical authoring policy for human-readable code and docs without hidden behavior changes.

Title/ID: m26c-ai-fingerprint-authorship-review
Goal: Make authored code, docs, tests, and examples read as repository-native work rather than template or model-generated filler.
Inputs: README.md, CONTRIBUTING.md, AGENTS.md, docs/**/*.md, apps/**/README.md, crates/**/README.md, tools/**/*.md, apps/**/src/**/*.rs, crates/**/src/**/*.rs, tools/**/*.rs, tools/cohesix-py/**/*.py, tests/**/*.rs, docs/audit/M26C_MARKDOWN_INVENTORY.csv, docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md, docs/audit/M26C_REFACTOR_MAP.md
Changes:
  - docs/audit/M26C_AI_FINGERPRINT_AUDIT.md — checked-in audit of generic phrasing, template comments, repetitive cadence, inflated adjectives, placeholder examples, and non-specific test names, with delete/rewrite/defer decisions and evidence links.
  - docs/audit/M26C_AGENT_HANDOFFS.md — record the independent authorship-review lane, reviewer, inspected surfaces, commands, residual gaps, and PASS/FAIL decision.
  - README.md + CONTRIBUTING.md + docs/CODING_GUIDELINES.md + docs/API_GUIDELINES.md — document authorship expectations and the difference between required provenance metadata and low-value boilerplate.
  - Cohesix-authored docs, comments, tests, and examples classified as low-risk by `M26C_REFACTOR_MAP.md` — delete or rewrite AI-fingerprint findings without changing behavior, grammar, fixture outputs, generated snippets, release snapshots, vendored references, or append-only audit evidence.
Commands:
  - rg -n "robust|flexible|seamless|world-class|comprehensive|easy to use|powerful|simple and intuitive|AI-generated|generated by" README.md CONTRIBUTING.md AGENTS.md docs apps crates tools
  - rg -n "Defines the|Provides the|CLI entry point|library surface|module for|This module" apps crates tools docs README.md CONTRIBUTING.md AGENTS.md
  - cargo fmt --all -- --check
  - scripts/check-generated.sh
Checks:
  - Every finding is classified as delete, rewrite, acceptable-as-specific, generated/do-not-edit, release-derived, vendored/reference-only, append-only, or deferred with rationale.
  - Rewritten prose names the actual Cohesix contract, invariant, failure mode, evidence path, or operator-visible behavior instead of using generic praise or template structure.
  - Generated snippets, release snapshots, vendored references, and append-only audit evidence are not hand-edited to satisfy style.
  - An independent reviewer lane signs off in `docs/audit/M26C_AGENT_HANDOFFS.md` before low-risk cleanup or public documentation polish is marked complete.
Deliverables:
  - Checked-in AI-fingerprint/authorship audit and cleanup evidence that make first-read code and documentation feel specific, technical, and maintained.

Title/ID: m26c-markdown-inventory-and-disposition
Goal: Inventory and disposition every tracked Markdown file in the main Cohesix repository so canonical, generated, release-derived, append-only, and vendored docs are handled correctly.
Inputs: `git ls-files '*.md' | sort`, root/project docs, apps/**/README.md, crates/**/README.md, docs/**/*.md, tracked release-cut docs, seL4/**/*.md, tests/integration/README.md, tools/**/*.md, third_party/**/*.md
Changes:
  - docs/audit/M26C_MARKDOWN_INVENTORY.csv — authoritative checked-in inventory/disposition source for every tracked Markdown file returned by `git ls-files '*.md'`.
  - docs/audit/M26C_MARKDOWN_INVENTORY.md — rendered human-readable report derived from the checked-in inventory source.
  - docs/BUILD_PLAN.md — record milestone scope, disposition rules, and inventory obligations.
  - docs/REPO_LAYOUT.md — document Markdown path classes and which ones are canonical, derived, or vendored.
  - docs/SECURITY.md — capture documentation provenance rules for generated, release, and vendored Markdown when they influence audit evidence.
Commands:
  - git ls-files '*.md' | sort > out/audit/m26c_markdown_inventory.txt
  - cut -d, -f1 docs/audit/M26C_MARKDOWN_INVENTORY.csv | sed '1d' | sort > out/audit/m26c_markdown_inventory_checked_in.txt
  - diff -u out/audit/m26c_markdown_inventory.txt out/audit/m26c_markdown_inventory_checked_in.txt
Checks:
  - Every tracked repository Markdown file is accounted for exactly once in the checked-in inventory.
  - Each entry has an explicit disposition and an update rule.
  - The rendered report is mechanically derived from the machine-readable source.
Deliverables:
  - Auditable Markdown inventory and disposition source/report pair covering canonical docs, release snapshots, audit evidence, reference mirrors, and vendored material.

Title/ID: m26c-mermaid-as-built-diagram-audit
Goal: Inventory, validate, and refine every Mermaid diagram in tracked Markdown so Cohesix visual documentation is as-built, review-grade, and 100% GitHub-online compatible.
Inputs: `git ls-files '*.md' | sort`, README.md, docs/ARCHITECTURE.md, docs/INTERFACES.md, docs/HOST_TOOLS.md, docs/USE_CASES.md, docs/FAILOVER.md, docs/GPU_NODES.md, docs/NETWORK_CONFIG.md, docs/SECURE9P.md, docs/HARDWARE_BRINGUP.md, docs/TEST_PLAN.md, docs/audit/M26C_MARKDOWN_INVENTORY.csv, docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md, apps/root-task/src/hal/**, apps/root-task/src/net/**, apps/root-task/src/event/**, apps/root-task/src/console/**, apps/nine-door/src/host/**, configs/root_task*.toml
Changes:
  - docs/audit/M26C_MERMAID_INVENTORY.csv — checked-in inventory of every Mermaid block in tracked Markdown with file, heading, diagram type, disposition, owner, as-built evidence source, GitHub compatibility status, and update rule.
  - docs/audit/M26C_MERMAID_GITHUB_RENDER_AUDIT.md — render evidence for every edited or newly added diagram, including GitHub-online proof or the exact compatibility check path used.
  - scripts/ci/mermaid_inventory.py — produce the Mermaid inventory from the tracked Markdown list without scanning ignored build outputs or local evidence directories.
  - scripts/ci/check_mermaid_github.sh + scripts/ci/render_mermaid_github.sh — fail on unsupported GitHub Mermaid syntax and render all accepted diagrams into 26c audit evidence.
  - README.md + docs/*.md + apps/**/README.md + crates/**/README.md + tools/**/*.md — refine stale or underspecified Mermaid diagrams; add new diagrams where canonical docs lack visual explanation of as-built contracts.
  - docs/ARCHITECTURE.md — add or refine a HAL architecture Mermaid diagram if the as-built audit shows HAL ownership, driver boundaries, or host/VM capability separation is not clear enough for external review.
Commands:
  - git ls-files '*.md' | sort > out/audit/m26c_markdown_inventory.txt
  - scripts/ci/mermaid_inventory.py --markdown-list out/audit/m26c_markdown_inventory.txt --out docs/audit/M26C_MERMAID_INVENTORY.csv
  - scripts/ci/check_mermaid_github.sh --markdown-list out/audit/m26c_markdown_inventory.txt
  - scripts/ci/render_mermaid_github.sh --markdown-list out/audit/m26c_markdown_inventory.txt --out out/audit/m26c-mermaid-rendered
Checks:
  - Every Mermaid block in tracked Markdown is inventoried exactly once and classified as canonical human-edited, generated/regenerate-only, release-derived, vendored/reference-only, append-only audit evidence, or external reference mirror.
  - Edited and newly added diagrams render correctly in GitHub online and avoid unsupported Mermaid features such as custom init directives, raw HTML labels, external assets, theme CSS, local renderer extensions, or experimental diagram syntax unless GitHub-online proof is archived.
  - Canonical diagrams describe the as-built system with enough detail to show authority boundaries, HAL ownership, host/VM separation, generated-manifest authority, Secure9P paths, worker roles, Pi 4 boot/network flows, and operator-visible control/data paths.
  - Diagram changes do not contradict generated snippets, manifests, fixtures, test-plan evidence, or the 26c blocker ledger.
Deliverables:
  - Review-grade GitHub-renderable Mermaid diagram set with inventory, compatibility proof, and as-built traceability for every tracked Markdown diagram.

Title/ID: m26c-refactor-map-and-risk-ratchet
Goal: Freeze the aggressive refactor scope, ownership, preserved contracts, simplicity targets, and risk-ratchet baseline before structural edits begin.
Inputs: docs/BUILD_PLAN.md, docs/audit/EXCEPTIONS.md, docs/audit/findings.csv, apps/**/src/**/*.rs, crates/**/src/**/*.rs, tools/**/src/**/*.rs, tools/cohesix-py/**/*.py, scripts/ci/*.sh
Changes:
  - docs/audit/M26C_REFACTOR_MAP.md — classify each Cohesix-authored candidate as no-touch, low-risk cleanup, characterization-first refactor, boundary-sensitive refactor, or deferred, and assign each accepted wave a single owned surface, preserved-contract list, characterization artifact, target test subset, and rollback-sized scope.
  - docs/audit/M26C_SIMPLICITY_SCORECARD.md — record before/after simplification targets for high-value authored surfaces: duplicate branches/helpers to collapse, generic comments to delete, large modules/functions to split or explicitly defer, docs to shorten, abstractions that must justify their existence, and the post-wave scorecard delta.
  - docs/audit/M26C_REFACTOR_RISK_RATCHET.csv — record the non-test `unsafe`, `unwrap`, `expect`, and `panic!` baseline plus allowed exceptions and reviewer sign-off requirements.
  - docs/audit/M26C_REFACTOR_OWNERSHIP.md — assign disjoint ownership for host tools, root-task adapters, HAL/network, docs/audit, and staged-run plumbing.
  - docs/BUILD_PLAN.md — keep 26c scope synchronized with the checked-in map if refactor lanes are added or deferred.
Commands:
  - rg -n "unsafe|unwrap\\(|expect\\(|panic!" apps crates tools
  - rg -n "Defines the|Provides the|CLI entry point|library surface|module for|This module|robust|flexible|seamless" apps crates tools docs README.md CONTRIBUTING.md AGENTS.md
  - cargo fmt --all -- --check
  - cargo clippy --workspace --all-targets -- -D warnings
Checks:
  - Every structural refactor target has a named owner, preserved external contracts, required characterization evidence, and an explicit rollback-sized scope.
  - Every accepted Phase 4 wave is independently reviewable and revertible, with its own target test subset and baseline comparison path.
  - Every cleanup/refactor target has a concrete simplicity target or a written deferral reason; "cleaned up" without before/after evidence is not acceptable closure.
  - New abstractions are accepted only when the scorecard shows they remove meaningful duplication, isolate authority, reduce module pressure, or make an invariant easier to audit.
  - Non-test risk indicators do not increase unless an approved exception is recorded before merge.
  - Deferred high-risk refactors have a concrete reason instead of being silently skipped.
Deliverables:
  - Checked-in refactor map, simplicity scorecard, ownership plan, and risk-ratchet baseline that make aggressive cleanup auditable.

Title/ID: m26c-docs-as-built-audit
Goal: Audit canonical documentation against the actual generated and observed system before any humanizing prose cleanup lands.
Inputs: docs/ARCHITECTURE.md, docs/INTERFACES.md, docs/HOST_TOOLS.md, docs/SECURE9P.md, docs/SECURITY.md, docs/TEST_PLAN.md, docs/HARDWARE_BRINGUP.md, docs/USERLAND_AND_CLI.md, docs/BOOT_REFERENCE.md, docs/FAILURE_MODES.md, docs/OPERATOR_WALKTHROUGH.md, README.md, docs/snippets/*.md, apps/root-task/src/generated/*, configs/generated/root_task_resolved.json, tracked release-cut docs under releases/**, staged test evidence, scripts/ci/check_test_plan.sh
Changes:
  - docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md — trace each canonical doc surface to generated snippets, manifests, fixtures, release artifacts, or staged-run evidence.
  - docs/audit/M26C_DOC_DRIFT_LEDGER.md — record docs/script drift discovered during the audit and the closure evidence for each item.
  - docs/ARCHITECTURE.md + docs/INTERFACES.md + docs/HOST_TOOLS.md + docs/SECURE9P.md + docs/SECURITY.md + docs/TEST_PLAN.md + docs/HARDWARE_BRINGUP.md + docs/USERLAND_AND_CLI.md + docs/BOOT_REFERENCE.md + docs/FAILURE_MODES.md + docs/OPERATOR_WALKTHROUGH.md + README.md — correct any prose that does not match the current as-built system before tone cleanup begins.
  - docs/snippets/*.md + tracked release-cut docs under releases/** — verify provenance and regeneration paths; update only through the proper source or release-cut flow.
  - scripts/ci/check_test_plan.sh — update whenever 26c changes authoritative test-plan commands, stage semantics, or target-qualified PASS requirements.
Commands:
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
  - scripts/check-generated.sh
  - scripts/ci/check_test_plan.sh
Checks:
  - Canonical docs can be traced to current manifests, generated snippets, fixtures, release snapshots, script behavior, or staged-run evidence.
  - Any docs/script drift is corrected or explicitly deferred with scope-safe rationale before humanizing edits are accepted.
Deliverables:
  - Auditable docs-as-built report and drift ledger proving documentation truth before presentation cleanup.

Title/ID: m26c-readme-linked-doc-suite-remediation
Milestone: Milestone 26c — Regression-Gated Refactor + Surface Audit / documentation-closure defects discovered during Milestone 26d
Goal: Maintain the human-authored documents linked directly or transitively from `README.md` as one concise, production-grade, as-built suite without changing system behavior or generated truth.
Inputs: README.md, its directly linked human-authored Markdown documents, historical glossary-bearing revisions of docs/HOST_TOOLS.md and docs/USERLAND_AND_CLI.md, AGENTS.md, docs/BUILD_PLAN.md, docs/HOST_API.md, resources/openapi/hive-gateway.yaml, docs/audit/M26C_MARKDOWN_INVENTORY.csv, docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md, docs/audit/M26C_DOC_DRIFT_LEDGER.md, docs/audit/M26C_MERMAID_INVENTORY.csv, configs/root_task*.toml, configs/generated/root_task_resolved.json, docs/snippets/*.md, current CLI help, source/tests, current target-qualified evidence, and the Milestone 26d Pi 4 proof boundary.
Changes:
  - README.md — present current scope, evidence status, supported targets, safe getting-started paths, and a categorized documentation map without release or hardware overclaiming.
  - docs/ARCHITECTURE.md + docs/INTERFACES.md + docs/SECURE9P.md + docs/ROLES_AND_SCHEDULING.md + docs/DRIVERS.md — establish non-overlapping ownership for architecture, external contracts, protocol invariants, role/scheduling policy, and physical-driver methodology.
  - docs/USERLAND_AND_CLI.md + docs/HOST_TOOLS.md + docs/API_GUIDELINES.md + docs/PYTHON_SUPPORT.md + docs/FAILURE_MODES.md + docs/OPERATOR_WALKTHROUGH.md — establish one operator journey, one tool catalogue, and cross-referenced API, Python, and recovery guidance grounded in current command help and fixtures.
  - resources/openapi/hive-gateway.yaml + docs/HOST_API.md — correct the machine-readable placement of the existing optional `TAIL lines` query and replace the stale duplicated schema mirror with a concise link-backed operator reference, without changing gateway behavior.
  - docs/USE_CASES.md + docs/HARDWARE_BRINGUP.md + docs/BOOT_REFERENCE.md + docs/GPU_NODES.md + docs/BENCHMARKS.md — separate implemented capability, current proof, historical evidence, candidate deployment patterns, and planned work; keep flash, current-image boot, transport, and benchmark proof lanes distinct.
  - docs/BUILD_PLAN.md — record this restoration-only scope and return 26c to `Complete` only after every check below passes.
  - docs/OPERATOR_RECIPES.md — when needed, own task-oriented evidence, mount, lifecycle, PEFT, and federation workflows so the canonical walkthrough remains one ordered journey.
  - docs/GLOSSARY.md — own plain-language definitions for Cohesix-specific terms and the foundational OS, security, protocol, AI, hardware, and proof concepts needed to understand them; link to owning contract documents rather than restating full schemas or limits.
  - CONTRIBUTING.md + docs/QUICKSTART.md + docs/SECURITY.md + docs/TOOLCHAIN_MAC_ARM64.md — keep the transitive onboarding and project-governance perimeter consistent with current security, validation, release, and seL4 15 requirements.
  - Canonical Mermaid blocks — retain only diagrams that materially clarify an as-built boundary or sequence; validate both syntax and semantic ownership against code, manifests, and current evidence.
  - docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md + docs/audit/M26C_DOC_DRIFT_LEDGER.md + docs/audit/M26C_MERMAID_INVENTORY.csv + docs/audit/M26C_MERMAID_GITHUB_RENDER_AUDIT.md + docs/audit/M26C_AGENT_HANDOFFS.md + docs/audit/M26C_SIMPLICITY_SCORECARD.md — refresh the live audit records with source anchors, before/after size and ownership evidence, lane handoffs, and the final Mermaid inventory/render result.
Commands:
  - git ls-files '*.md' | sort > out/audit/m26c-doc-remediation-markdown.txt
  - scripts/ci/mermaid_inventory.py --markdown-list out/audit/m26c-doc-remediation-markdown.txt --out docs/audit/M26C_MERMAID_INVENTORY.csv
  - scripts/ci/check_mermaid_github.sh --markdown-list out/audit/m26c-doc-remediation-markdown.txt
  - scripts/ci/render_mermaid_github.sh --markdown-list out/audit/m26c-doc-remediation-markdown.txt --out out/audit/m26c-doc-remediation-mermaid-rendered
  - scripts/check-generated.sh
  - scripts/ci/check_test_plan.sh
  - scripts/ci/test_plan_run.sh --list
  - python3 scripts/ci/check_driver_test_coverage.py
  - cargo check --workspace
  - cargo test -p hive-gateway
  - git diff --check
Checks:
  - Every directly linked canonical document has one stated purpose, an explicit as-built or planning boundary, retained 2026 Lukas Bower metadata, and useful cross-references instead of copied contracts.
  - Every transitive onboarding, contribution, security, and toolchain guide reached from the suite is current, secret-safe, reproducible, and consistent with the charter.
  - Durable public namespace, record, derivation, evidence-pack, driver-profile, and simulation contracts remain documented even when generated mirrors and historical debug prose are removed.
  - Every Cohesix-specific term used by the README documentation map is defined for a newcomer or points to a more specific canonical definition; collision-prone terms such as LoRA prose versus lowercase `lora` identifiers, NineDoor/NineDoorBridge, host/target, mock/QEMU/hardware, and source/resolved manifest are explicitly distinguished.
  - Selected-profile manifests, resolved output, generated snippets, current source/tests, and target-qualified evidence remain authoritative; generated snippets are linked or reproduced only through their generator and are not hand-edited.
  - Current QEMU evidence, accepted historical Pi 4 evidence, current-image Pi 4 revalidation, Wi-Fi research status, host-only integrations, and future milestones are never presented as interchangeable proof.
  - All local links and referenced anchors resolve, code fences are balanced, Markdown is structurally consistent, and every active Mermaid block passes the GitHub compatibility checker plus an available CLI render pass.
  - No runtime command, endpoint implementation, namespace, ACK/ERR/END behavior, authority path, release snapshot, or hardware acceptance state changes under the documentation rewrite; the OpenAPI correction describes existing handler behavior.
Deliverables:
  - A reviewable README-linked documentation suite with clear information ownership, materially less repetition, complete GitHub-compatible diagrams, and validation evidence.

Title/ID: m26c-runtime-boundary-and-semantic-parity-audit
Goal: Document the intentional host `std` / VM `no_std` runtime split and prove overlapping NineDoor semantics remain aligned without runtime convergence.
Inputs: apps/root-task/src/ninedoor.rs, apps/root-task/src/event/**, apps/root-task/src/console/**, apps/root-task/src/log_buffer.rs, apps/root-task/src/lib.rs, apps/root-task/src/net/**, apps/root-task/src/hal/**, apps/root-task/src/local_seat.rs, apps/root-task/tests/**, apps/nine-door/src/host/*.rs, apps/nine-door/tests/*.rs, docs/ARCHITECTURE.md, docs/INTERFACES.md, docs/SECURE9P.md, docs/SECURITY.md, docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md, docs/audit/M26C_REFACTOR_MAP.md
Changes:
  - docs/audit/M26C_RUNTIME_BOUNDARY_AUDIT.md — record VM TCB boundaries, forbidden host-side capabilities in the VM, and adapter ownership of host/VM surfaces.
  - docs/audit/M26C_NINEDOOR_PARITY_MATRIX.md — enumerate overlapping host/VM semantics, expected outputs, and evidence paths for each parity claim.
  - docs/ARCHITECTURE.md + docs/INTERFACES.md + docs/SECURE9P.md + docs/SECURITY.md — replace any ambiguous "eventual merge" language with explicit boundary language: separate adapters, shared contracts only.
  - apps/nine-door/tests/*.rs + apps/root-task/tests/** and host-mode root-task bridge tests — add or extend parity coverage for overlapping operator-visible surfaces without changing protocol grammar.
Commands:
  - cargo test -p nine-door --test integration
  - cargo test -p root-task --tests
  - cargo test -p root-task --lib
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
Checks:
  - Checked-in audit artifacts state clearly that VM-side Cohesix remains `no_std` and that host `std` capability is not a valid convergence target.
  - Every overlapping host/VM semantic surface in the parity matrix has either passing test evidence from the relevant `apps/nine-door` and `root-task` test suites or an explicit host-only/VM-only disposition.
  - Canonical docs describe the runtime split as intentional architecture rather than an accidental temporary inconsistency.
Deliverables:
  - Auditable runtime-boundary report and semantic-parity matrix that establish the proof surface required before boundary-sensitive runtime refactors.

Title/ID: m26c-pi-runtime-dma-proof-closure
Goal: Define and prove Pi 4 isolated runtime descriptor, DMA, owner-state, and artifact-freshness evidence before 26c cleanup or later operator tooling cites the runtime path as closed.
Inputs: apps/root-task/src/hal/**, apps/root-task/src/local_seat.rs, apps/root-task/src/generated/**, apps/pi4-driver-runtime/src/**, crates/pi4-driver-abi/src/**, configs/root_task_pi4_uboot_aarch64.toml, scripts/pi4-image-build.sh, scripts/ci/test_plan_run.sh, docs/TEST_PLAN.md, docs/HARDWARE_BRINGUP.md, docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md
Changes:
  - crates/pi4-driver-abi/src/** — add or tighten descriptor-resource tests for runtime MMIO, DMA, shared-buffer, IRQ, bus-alias, and framebuffer ranges emitted by generated Pi 4 driver-image records.
  - apps/pi4-driver-runtime/src/** — add runtime tests that classify DMA reservation/allocation and owner-state evidence without relying on root-owned fallback paths.
  - apps/root-task/src/hal/** + apps/root-task/src/local_seat.rs — surface bounded evidence for DMA arena reservation, bus-address publication, cache maintenance, runtime-init descriptor delivery, and hardware-state handoff while preserving HAL-only authority.
  - docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md + docs/TEST_PLAN.md + docs/HARDWARE_BRINGUP.md — define absent, stale, generated-only, target-build, QEMU, fresh Pi hardware proof, valid counter snapshot, and arch-counter timer-backend proof states and require source artifact provenance for each claim.
  - scripts/ci/test_plan_run.sh + stage checks where needed — fail 26c Pi closure if runtime/DMA proof is absent, stale, or inferred from generated eligibility alone.
Commands:
  - cargo test -p pi4-driver-abi
  - cargo test -p pi4-driver-runtime
  - cargo test -p root-task --tests
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
  - scripts/check-generated.sh
  - scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml
  - scripts/pi4_gate_proof.sh --require-driver-task-proof
  - scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m26c-pi4
Checks:
  - Runtime-image eligibility, descriptor resource totals, DMA reservation/allocation evidence, owner-state fields, and source artifact freshness are reported separately.
  - Missing DMA proof is a blocking absent-evidence state, not inferred success from manifest generation, target compilation, QEMU smoke output, or stale serial logs.
  - Isolated runtime performance proof requires valid `DRIVER_TASK_COUNTER` evidence plus Pi arch-counter proof (`TIMER_BACKEND=arch-counter`, `TIMER_CLOCK_HZ=54000000`, `TIMER_EL0_COUNTER=vct`, `DUMMY_TIMER_SEEN=no`); counters alone remain diagnostic.
  - DMA ownership, cache maintenance, bus-address publication, runtime-init descriptor delivery, and hardware handoff remain behind HAL/generated interfaces with no direct MMIO or ad-hoc physical-address path.
  - Fresh Pi evidence, when claimed, names the exact serial log, manifest hash, image build, and test-plan state directory used for the claim.
Deliverables:
  - 26c-owned Pi 4 runtime/DMA proof semantics and evidence gates strong enough for later operator tools to project read-only summaries without defining acceptance.

Title/ID: m26c-dma-protection-profile-truth
Goal: Add compiler-owned DMA protection profiles that state Pi 4 as bounded no-IOMMU DMA discipline and reserve SMMU-backed isolation claims for future hardware profiles.
Inputs: tools/coh-rtc/src/ir.rs, tools/coh-rtc/src/validate.rs, configs/root_task_pi4_uboot_aarch64.toml, configs/root_task.toml, apps/root-task/src/hal/**, docs/ARCHITECTURE.md, docs/HARDWARE_BRINGUP.md, docs/SECURITY.md, docs/TEST_PLAN.md, docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md
Changes:
  - tools/coh-rtc/src/ir.rs + tools/coh-rtc/src/validate.rs — add a generated DMA protection profile enum such as `none`, `bounded_no_iommu`, `smmu_v2`, and `smmu_v3`, with Pi 4 profiles required to resolve to `bounded_no_iommu`.
  - configs/root_task_pi4_uboot_aarch64.toml + generated manifests/snippets — record Pi 4 DMA as HAL-mediated bounded DMA ownership, not hardware-enforced IOMMU/SMMU isolation.
  - apps/root-task/src/hal/** — expose profile-specific evidence fields for bounded arenas, descriptor validation, bus-address publication, cache maintenance, buffer quarantine, and absent SMMU capability.
  - docs/ARCHITECTURE.md + docs/HARDWARE_BRINGUP.md + docs/SECURITY.md + docs/TEST_PLAN.md — document the Pi 4 limitation, prohibit SMMU/IOMMU isolation wording for BCM2711, and define the future SMMU hardware contract separately.
  - docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md — add a docs-as-built check that rejects diagrams, prose, or evidence claims that imply Pi 4 malicious-device DMA confinement.
Commands:
  - cargo test -p coh-rtc dma_protection
  - cargo test -p root-task --tests dma
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
  - scripts/check-generated.sh
  - rg -n "IOMMU|SMMU|hardware-enforced DMA|DMA isolation" docs README.md AGENTS.md
Checks:
  - Pi 4 manifests and docs resolve to bounded no-IOMMU DMA discipline and explicitly refuse hardware-enforced SMMU/IOMMU isolation claims.
  - Bounded Pi 4 DMA evidence covers per-driver arenas, descriptor validation, bus-address publication, cache maintenance, quarantine/reuse policy, and fresh proof provenance.
  - Future `smmu_v2` or `smmu_v3` profiles cannot be enabled unless generated per-device DMA-domain state, StreamID/context-bank or equivalent IO-space mapping policy, fault evidence, and revoke/unmap semantics are present.
  - No public doc, Mermaid diagram, evidence pack, or operator output describes BCM2711/Pi 4 as providing malicious-device DMA confinement.
Deliverables:
  - Review-grade DMA protection vocabulary that keeps Pi 4 claims honest while preserving a clean path to real SMMU-backed targets.

Title/ID: m26c-worker-architecture-implementation
Goal: Replace placeholder kernel-side worker GPU/LoRA paths with real seL4 worker ticket, lease, telemetry, and revocation loops matching public Queen/Worker documentation.
Inputs: apps/worker-heart/src/**, apps/worker-gpu/src/**, apps/worker-lora/src/**, apps/root-task/src/ninedoor.rs, apps/root-task/src/event/**, apps/root-task/src/lifecycle.rs, apps/root-task/src/generated/**, tools/coh-rtc/src/**, crates/cohesix-ticket/**, docs/ARCHITECTURE.md, docs/INTERFACES.md, docs/GPU_NODES.md, docs/WORKER_TICKETS.md, docs/ROLES_AND_SCHEDULING.md, docs/SECURITY.md, docs/TEST_PLAN.md
Changes:
  - apps/worker-heart/src/kernel.rs — implement the VM-side heartbeat worker loop that attaches with a scoped worker ticket, emits bounded telemetry to the documented worker path, observes lifecycle/revocation state, and exits deterministically on shutdown.
  - apps/worker-gpu/src/kernel.rs + apps/worker-gpu/src/lib.rs — replace placeholder spin/stub behavior with a no_std worker loop that consumes WorkerGpu tickets, reads lease/model pointers through the documented namespace, emits GPU lease/job telemetry receipts, and never touches CUDA/NVML or raw GPU hardware.
  - apps/worker-lora/src/lib.rs + apps/worker-lora/src/common.rs — implement the VM-side LoRA worker control/telemetry loop for ticket-scoped adapter/model lifecycle receipts while keeping training, TensorRT, CUDA, and PEFT execution host-side.
  - apps/root-task/src/ninedoor.rs + apps/root-task/src/event/** + apps/root-task/src/lifecycle.rs — publish worker namespace entries, ticket-gated attach state, lease/lifecycle revocation signals, and bounded telemetry paths needed by the VM workers without changing Secure9P grammar.
  - tools/coh-rtc/src/ir.rs + tools/coh-rtc/src/validate.rs + generated snippets — add worker-role implementation state, ticket scopes, lease paths, telemetry bounds, shutdown/revoke policy, and docs-as-built validation so public worker claims are compiler-aligned.
  - docs/ARCHITECTURE.md + docs/INTERFACES.md + docs/GPU_NODES.md + docs/WORKER_TICKETS.md + docs/ROLES_AND_SCHEDULING.md + docs/SECURITY.md + docs/TEST_PLAN.md — update public Queen/Worker behavior to match the implemented seL4 worker loops and host-only GPU/PEFT boundaries.
Commands:
  - cargo test -p worker-heart
  - cargo test -p worker-gpu
  - cargo test -p worker-lora
  - cargo test -p root-task --tests worker
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
  - scripts/check-generated.sh
  - cargo check -p worker-heart --target aarch64-unknown-none
  - cargo check -p worker-gpu --target aarch64-unknown-none
  - cargo check -p worker-lora --target aarch64-unknown-none
Checks:
  - Worker-heart, worker-gpu, and worker-lora no longer rely on placeholder kernel spin loops for public VM worker semantics.
  - Each worker loop uses scoped tickets and documented namespace paths, emits bounded telemetry/receipts, and handles lease expiry, lifecycle cut, revocation, and shutdown deterministically.
  - GPU and LoRA workers remain control-plane workers only: CUDA, NVML, TensorRT, PEFT training/import execution, and raw hardware access stay host-side.
  - Generated manifests and docs agree on which worker roles are implemented, which paths they may access, and which telemetry/lease bounds apply.
  - Existing console grammar, Secure9P framing, ACK/ERR/END behavior, and host-side worker helper APIs do not drift.
Deliverables:
  - Real seL4 worker architecture matching public Queen/Worker docs, with host/VM boundaries and tests strong enough for 26c closure.

Title/ID: m26c-cap-backed-worker-endpoints
Goal: Make first-phase VM worker tickets cap-backed by requiring badged seL4 endpoint capabilities for worker attach, telemetry, lease renewal, and revocation-sensitive paths.
Inputs: apps/root-task/src/lifecycle.rs, apps/root-task/src/ninedoor.rs, apps/root-task/src/event/**, apps/root-task/src/generated/**, apps/worker-heart/src/**, apps/worker-gpu/src/**, apps/worker-lora/src/**, tools/coh-rtc/src/**, crates/cohesix-ticket/**, docs/WORKER_TICKETS.md, docs/ROLES_AND_SCHEDULING.md, docs/SECURITY.md, docs/INTERFACES.md
Changes:
  - apps/root-task/src/lifecycle.rs + apps/root-task/src/event/** — mint role/lease/epoch-badged endpoint caps for worker attach, control, telemetry, and revoke-sensitive receipt paths; retain the parent cap path needed to revoke derived lease caps.
  - apps/root-task/src/ninedoor.rs — reject worker attach, telemetry append, lease-renewal, and receipt paths when only ticket metadata is present and no matching badged endpoint cap was invoked.
  - apps/worker-heart/src/** + apps/worker-gpu/src/** + apps/worker-lora/src/** — replace metadata-only ticket presentation with endpoint-cap invocation in the worker loops while preserving bounded no_std behavior.
  - tools/coh-rtc/src/** + generated snippets — add phase-1 cap-backed-ticket fields for endpoint badges, lease epochs, role scopes, revoke behavior, and explicit deferred full-cap-bundle status.
  - docs/WORKER_TICKETS.md + docs/ROLES_AND_SCHEDULING.md + docs/SECURITY.md + docs/INTERFACES.md — document Cohesix tickets as audit records backed by badged endpoint caps for VM worker authority.
Commands:
  - cargo test -p root-task --tests worker
  - cargo test -p worker-heart
  - cargo test -p worker-gpu
  - cargo test -p worker-lora
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
  - scripts/check-generated.sh
  - cargo check -p worker-heart --target aarch64-unknown-none
  - cargo check -p worker-gpu --target aarch64-unknown-none
  - cargo check -p worker-lora --target aarch64-unknown-none
Checks:
  - Forged ticket strings or stale ticket metadata without the corresponding badged endpoint cap cannot attach as a worker, renew a lease, emit accepted telemetry, or publish a valid receipt.
  - Revocation deletes or invalidates the derived endpoint caps and late invocations from the old lease epoch fail deterministically.
  - Generated manifests and docs distinguish phase-1 endpoint-cap-backed tickets from the later full cap-bundle work.
  - Existing Secure9P grammar, console ACK/ERR/END behavior, and host-ticket schemas do not drift.
Deliverables:
  - First-phase cap-backed VM worker ticket authority with concrete seL4 endpoint caps and negative tests, without overclaiming full frame/notification/DMA cap-bundle isolation.

Title/ID: m26c-notification-backed-worker-lifecycle
Goal: Replace lifecycle polling dependency with generated seL4 notification objects for worker revoke, shutdown, lease expiry, telemetry pressure, and driver IRQ wakeups.
Inputs: apps/root-task/src/lifecycle.rs, apps/root-task/src/event/**, apps/root-task/src/hal/**, apps/root-task/src/generated/**, apps/worker-heart/src/**, apps/worker-gpu/src/**, apps/worker-lora/src/**, apps/pi4-driver-runtime/src/**, tools/coh-rtc/src/**, docs/ROLES_AND_SCHEDULING.md, docs/SECURITY.md, docs/INTERFACES.md, docs/TEST_PLAN.md
Changes:
  - tools/coh-rtc/src/** — add generated notification lifecycle fields and badge classes for revoke, shutdown, lease-expiry, telemetry-pressure, and IRQ events where applicable.
  - apps/root-task/src/lifecycle.rs + apps/root-task/src/event/** — publish notification caps/badges to worker loops and signal lifecycle changes without introducing new protocols or namespace grammar.
  - apps/root-task/src/hal/** + apps/pi4-driver-runtime/src/** — keep driver IRQ delivery notification-backed and expose bounded evidence for notification badges and acknowledgement behavior.
  - apps/worker-heart/src/** + apps/worker-gpu/src/** + apps/worker-lora/src/** — wait on endpoint IPC plus notification delivery for lifecycle changes, and remove unbounded lifecycle polling from worker loops.
  - docs/ROLES_AND_SCHEDULING.md + docs/SECURITY.md + docs/INTERFACES.md + docs/TEST_PLAN.md — document notification-backed lifecycle delivery, badge meanings, and the boundary between 26c lifecycle notifications and 28b full cap-bundle notification authority.
Commands:
  - cargo test -p root-task --tests worker
  - cargo test -p root-task --tests notification
  - cargo test -p worker-heart
  - cargo test -p worker-gpu
  - cargo test -p worker-lora
  - cargo test -p pi4-driver-runtime
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
  - scripts/check-generated.sh
Checks:
  - Revoke, shutdown, lease-expiry, telemetry-pressure, and applicable IRQ events are delivered through generated notification caps/badges rather than unbounded lifecycle polling.
  - Worker loops block or yield deterministically while waiting for endpoint IPC or notification events and still handle lease expiry, revoke, telemetry backpressure, and shutdown.
  - Driver IRQ notification behavior remains HAL-owned and does not create a new polling path or direct IRQ bypass outside generated descriptors.
  - Generated docs distinguish 26c notification-backed lifecycle signaling from 28b full notification cap-bundle authority.
Deliverables:
  - Event-driven worker and driver lifecycle signaling with seL4 notification objects, bounded tests, and no new Cohesix protocol surface.

Title/ID: m26c-worker-driver-mcs-budget-evidence
Goal: Bind worker and manifest-declared isolated driver runtime bounded-execution claims to generated scheduling-context evidence on MCS profiles while preserving explicit non-MCS fallback evidence.
Inputs: apps/root-task/src/lifecycle.rs, apps/root-task/src/hal/**, apps/root-task/src/generated/**, apps/worker-heart/src/**, apps/worker-gpu/src/**, apps/worker-lora/src/**, apps/pi4-driver-runtime/src/**, tools/coh-rtc/src/**, docs/ROLES_AND_SCHEDULING.md, docs/SECURITY.md, docs/TEST_PLAN.md, docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md
Changes:
  - tools/coh-rtc/src/** — add profile-qualified worker/driver scheduling fields for MCS budget, period, timeout endpoint, consumed-budget reporting, and non-MCS priority/domain fallback state.
  - apps/root-task/src/lifecycle.rs + apps/root-task/src/hal/** — bind worker and driver TCBs to generated scheduling contexts on MCS profiles; on non-MCS profiles, emit the documented priority/domain and bounded service-turn evidence instead.
  - apps/worker-heart/src/** + apps/worker-gpu/src/** + apps/worker-lora/src/** + apps/pi4-driver-runtime/src/** — surface bounded-loop progress, timeout, and shutdown evidence without changing protocol grammar or namespace layout.
  - docs/ROLES_AND_SCHEDULING.md + docs/SECURITY.md + docs/TEST_PLAN.md — document profile-qualified MCS enforcement, timeout-fault behavior, consumed-budget evidence, and non-MCS fallback wording.
  - docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md — reject prose that claims kernel-enforced CPU budgets on profiles that only provide priority/domain and bounded service turns.
Commands:
  - cargo test -p root-task --tests scheduling
  - cargo test -p worker-heart
  - cargo test -p worker-gpu
  - cargo test -p worker-lora
  - cargo test -p pi4-driver-runtime
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
  - scripts/check-generated.sh
Checks:
  - MCS profiles prove generated scheduling-context budget/period binding, timeout endpoint routing, and consumed-budget evidence for worker and driver tasks.
  - Non-MCS profiles continue to pass with explicit priority/domain and bounded service-turn evidence and never claim MCS enforcement.
  - Worker and driver loop tests cover budget exhaustion or service-turn exhaustion, telemetry backpressure, lease expiry, revoke, and shutdown without unbounded spin.
  - Public docs, generated snippets, and test-plan gates distinguish MCS enforcement from non-MCS compatibility.
Deliverables:
  - Profile-qualified scheduling evidence strong enough for seL4 reviewers to audit bounded worker and driver execution claims.

Title/ID: m26c-post-behavior-baseline-freeze
Goal: Freeze the external behavior snapshot after the authorized 26c worker/runtime additions and before cleanup refactors begin.
Inputs: docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md, docs/audit/M26C_RUNTIME_BOUNDARY_AUDIT.md, docs/audit/M26C_NINEDOOR_PARITY_MATRIX.md, docs/audit/M26C_TARGET_RUNNER_BASELINE.md, docs/audit/M26C_REFACTOR_MAP.md, apps/root-task/tests/**, apps/nine-door/tests/**, worker test suites, generated snippets/manifests, scripts/ci/test_plan_run.sh, scripts/ci/check_test_plan.sh
Changes:
  - docs/audit/M26C_POST_BEHAVIOR_BASELINE.md — record the post-Phase-2 external contract snapshot: console grammar, Secure9P/NineDoor namespace semantics, manifest and generated-snippet hashes, worker ticket/lease/telemetry/lifecycle behavior, cap-backed endpoint evidence, notification evidence, MCS/non-MCS scheduling evidence, Pi runtime/DMA evidence classifications, and known deferred blockers.
  - docs/audit/M26C_REFACTOR_MAP.md — link every Phase 4 cleanup candidate to the relevant post-behavior baseline section and preserved-contract list.
  - docs/audit/M26C_AGENT_HANDOFFS.md — record the freeze decision, commands, skipped commands, residual blockers, and PASS/FAIL status before any cleanup lane starts.
  - docs/TEST_PLAN.md — state that Phase 4 refactors compare against the post-behavior baseline rather than pre-26c placeholder worker behavior.
Commands:
  - scripts/check-generated.sh
  - cargo test -p nine-door --test integration
  - cargo test -p root-task --tests
  - cargo test -p worker-heart
  - cargo test -p worker-gpu
  - cargo test -p worker-lora
  - scripts/ci/check_test_plan.sh
  - scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m26c-post-behavior-qemu
  - scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m26c-post-behavior-pi4
Checks:
  - The baseline is recorded only after all authorized Phase 2 behavior-changing tasks are complete or explicitly deferred without being used as satisfied evidence.
  - Refactor waves use this post-behavior snapshot as their preserved external contract, so cleanup does not accidentally revert or mutate the new intended worker/cap/notification/MCS behavior.
  - Any remaining Pi 4 or runner blocker is recorded with dependency impact, and no Phase 4 task cites blocked evidence as proof.
  - The target-qualified QEMU/Pi 4 state dirs and command outputs named in the baseline match the runner contract from `m26c-target-qualified-runner-baseline`.
Deliverables:
  - Hard baseline separating authorized 26c behavior changes from later behavior-preserving cleanup.

Title/ID: m26c-characterization-gates-before-refactor
Goal: Add characterization tests and merge gates around cleanup-sensitive crates before accepting structural refactors.
Inputs: .github/workflows/ci.yml, scripts/ci/test_plan_run.sh, scripts/ci/test_plan_stage_*.sh, scripts/ci/check_test_plan.sh, apps/coh/src/mount.rs, apps/cohsh/src/**, crates/cohsh-core/**, apps/host-ticket-agent/src/*.rs, apps/gpu-bridge-host/src/**, apps/root-task/src/net/**, apps/root-task/src/hal/**, apps/root-task/src/local_seat.rs, apps/root-task/src/ninedoor.rs, apps/root-task/src/event/**, apps/root-task/src/console/**, apps/pi4-driver-runtime/**, crates/pi4-driver-abi/**, apps/nine-door/tests/*.rs, docs/TEST_PLAN.md, docs/audit/M26C_REFACTOR_MAP.md
Changes:
  - .github/workflows/ci.yml — stop excluding cleanup-sensitive crates without compensating gate coverage, or add explicit targeted jobs for `coh`, `cohsh`, `cohsh-core`, `host-ticket-agent`, `gpu-bridge-host`, `root-task`, `pi4-driver-abi`, `pi4-driver-runtime`, and `nine-door` parity surfaces.
  - scripts/ci/test_plan_stage_02_host_fast.sh — exercise `coh`, `cohsh`, `cohsh-core`, `host-ticket-agent`, `gpu-bridge-host`, `nine-door` integration coverage, `root-task --tests`, `root-task --lib`, `pi4-driver-abi`, `pi4-driver-runtime`, and VM-target boundary checks as first-class checks.
  - scripts/ci/check_test_plan.sh — enforce the updated target-aware staged-run contract and authoritative command matrix.
  - docs/TEST_PLAN.md — document the expanded cleanup-target coverage, VM boundary evidence, target-qualified artifacts, post-behavior baseline comparison, and required artifact retention.
  - apps/coh/src/mount.rs + apps/cohsh/src/** + crates/cohsh-core/** + apps/host-ticket-agent/src/*.rs + apps/gpu-bridge-host/src/** + selected apps/root-task/src/* helpers + selected `apps/nine-door` and root-task parity surfaces — add characterization tests for current outputs, errors, and state transitions before refactor.
Commands:
  - cargo test -p coh --features mock
  - cargo test -p cohsh
  - cargo test -p cohsh-core
  - cargo test -p host-ticket-agent
  - cargo test -p gpu-bridge-host
  - cargo test -p nine-door --test integration
  - cargo test -p root-task --tests
  - cargo test -p root-task --lib
  - cargo test -p pi4-driver-abi
  - cargo test -p pi4-driver-runtime
  - cargo check -p pi4-driver-runtime --target aarch64-unknown-none
  - rg -n "unsafe|unwrap\\(|expect\\(|panic!" apps crates tools
  - scripts/ci/check_test_plan.sh
Checks:
  - Cleanup-sensitive crates have deterministic tests that pin current operator-visible behavior.
  - Characterization artifacts name the post-behavior baseline section they preserve, so behavior-changing Phase 2 work and behavior-preserving Phase 4 cleanup cannot be conflated.
  - Driver runtime ABI descriptors, isolated runtime service turns, acceptance-eligibility fixtures, and board-proof boundaries remain pinned before cleanup touches driver-task code.
  - CI and staged test-plan coverage run those tests automatically and retain root-task VM-boundary evidence and target-qualified staged-run artifacts.
  - Risk-ratchet output is attached to refactor reviews and does not drift without an approved exception.
Deliverables:
  - Regression safety net expanded ahead of structural cleanup.

Title/ID: m26c-no-std-boundary-gates
Goal: Add explicit CI and audit gates that fail if host-side capability leaks into the VM build across the closure profiles used by 26c.
Inputs: .github/workflows/ci.yml, scripts/ci/test_plan_stage_02_host_fast.sh, apps/root-task/Cargo.toml, apps/nine-door/Cargo.toml, apps/pi4-driver-runtime/Cargo.toml, crates/pi4-driver-abi/Cargo.toml, configs/root_task.toml, configs/root_task_pi4_uboot_aarch64.toml, docs/TEST_PLAN.md, docs/ARCHITECTURE.md
Changes:
  - .github/workflows/ci.yml — add root-task VM-profile and `pi4-driver-runtime` `no_std` checks and archive dependency-tree artifacts for the QEMU `cohesix-dev` and Pi 4 U-Boot closure profiles at minimum.
  - scripts/ci/test_plan_stage_02_host_fast.sh — emit `out/audit/m26c_root_task_tree_qemu.txt`, `out/audit/m26c_root_task_tree_pi4.txt`, and driver-runtime VM-target dependency evidence (or equivalent state-dir artifacts) from the VM-target dependency tree as part of cleanup gating.
  - docs/TEST_PLAN.md + docs/ARCHITECTURE.md — document the VM boundary gate, expected artifacts, and interpretation rules per closure profile.
  - docs/BUILD_PLAN.md — record that shared semantic helpers are allowed only when they remain `no_std`-safe and do not drag host-side crates into the VM build.
Commands:
  - cargo check -p root-task --target aarch64-unknown-none --no-default-features --features "cohesix-dev"
  - cargo tree -p root-task --target aarch64-unknown-none -e normal --no-default-features --features "cohesix-dev" > out/audit/m26c_root_task_tree_qemu.txt
  - cargo check -p root-task --target aarch64-unknown-none --no-default-features --features "kernel bootstrap-trace serial-console net-console"
  - cargo tree -p root-task --target aarch64-unknown-none -e normal --no-default-features --features "kernel bootstrap-trace serial-console net-console" > out/audit/m26c_root_task_tree_pi4.txt
  - cargo check -p pi4-driver-runtime --target aarch64-unknown-none
  - cargo tree -p pi4-driver-runtime --target aarch64-unknown-none -e normal > out/audit/m26c_pi4_driver_runtime_tree.txt
  - scripts/ci/check_test_plan.sh
Checks:
  - The VM-target root-task build and Pi 4 isolated driver runtime build remain `no_std` and succeed without host-side operator/tooling dependencies across the closure profiles.
  - The dependency-tree artifacts are reviewed as part of 26c evidence and show no accidental pull-in of host-only surfaces.
  - Cleanup work cannot merge if boundary evidence is missing or contradicts the documented VM/host split.
Deliverables:
  - Explicit per-profile `no_std` boundary gate for cleanup-era work, reducing drift risk without touching runtime behavior.

Title/ID: m26c-low-risk-surface-cleanup
Goal: Humanize the highest-visibility low-risk code and doc surfaces after docs-as-built, AI-fingerprint, characterization, and no-std gates cover the touched surface.
Inputs: apps/*/README.md, crates/**/README.md, tools/cohesix-py/README.md, tests/integration/README.md, public crate roots under apps/*/src/lib.rs, tests/**/*.rs, docs/OPERATOR_WALKTHROUGH.md, docs/QUICKSTART.md, docs/QUICKSTART_ALPHA.md, docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md, docs/audit/M26C_AI_FINGERPRINT_AUDIT.md
Changes:
  - apps/*/README.md + crates/**/README.md + tools/cohesix-py/README.md + tests/integration/README.md — replace template summaries with role-specific descriptions, assumptions, and operator-facing boundaries.
  - apps/*/src/lib.rs + apps/*/src/main.rs + crates/**/src/lib.rs — remove generic module-surface comments and rewrite surviving doc comments around invariants or usage.
  - tests/**/*.rs — rename template-like test names/descriptions to scenario-driven names and tighten helper naming.
  - docs/OPERATOR_WALKTHROUGH.md + docs/QUICKSTART.md + docs/QUICKSTART_ALPHA.md — remove repetitive prose and keep operator sequences concrete.
Commands:
  - cargo test -p secure9p-core
  - cargo test -p cohsh-core
  - cargo test -p tests --quiet
Checks:
  - High-visibility surface files read as authored, file-specific documentation instead of generated scaffolding.
  - Humanized docs remain consistent with the audit evidence produced by `m26c-docs-as-built-audit`.
  - AI-fingerprint findings for touched low-risk surfaces are closed or explicitly deferred in `docs/audit/M26C_AI_FINGERPRINT_AUDIT.md`.
  - Each cleanup batch maps to one accepted wave in `M26C_REFACTOR_MAP.md`, with a preserved-contract list, targeted tests, and before/after scorecard evidence.
  - The simplicity scorecard shows net clarity improvement for touched surfaces; cleanup-only changes that add more prose, wrappers, or concepts than they remove require explicit reviewer justification.
  - Test names and doc comments describe behaviors and scenarios, not file names.
Deliverables:
  - First-wave cleanup across low-risk surfaces with zero behavior changes and passing tests.

Title/ID: m26c-host-tool-structural-cleanup
Goal: Refactor repetitive host-side validation, transport, and ticket-processing flows once characterization tests are in place.
Inputs: apps/coh/src/**, apps/cohsh/src/**, crates/cohsh-core/**, apps/host-ticket-agent/src/**, apps/gpu-bridge-host/src/**, apps/hive-gateway/src/**, tools/cohesix-py/cohesix/**, docs/ARCHITECTURE.md, docs/HOST_TOOLS.md, docs/INTERFACES.md, docs/API_GUIDELINES.md
Changes:
  - apps/host-ticket-agent/src/** — factor manifest validation, ticket-processing branches, executor selection, and receipt handling into explicit invariant-bearing helpers without changing lifecycle semantics.
  - apps/coh/src/** + apps/cohsh/src/** + crates/cohsh-core/** — consolidate duplicated path, grammar, request-auth, mount, and transport validation while preserving ACK/ERR/END, FUSE, REST, and TCP behavior.
  - apps/gpu-bridge-host/src/** + apps/hive-gateway/src/** — tighten host-only boundary checks, error shaping, and telemetry/report formatting without changing documented host-side contracts.
  - tools/cohesix-py/cohesix/** — align Python-side validation and examples with the same documented host API contracts where characterization coverage exists.
  - docs/ARCHITECTURE.md + docs/HOST_TOOLS.md + docs/INTERFACES.md + docs/API_GUIDELINES.md — update any as-built explanations needed to reflect clearer code structure while preserving external contracts.
Commands:
  - cargo test -p host-ticket-agent
  - cargo test -p coh --features mock
  - cargo test -p cohsh
  - cargo test -p cohsh-core
  - cargo test -p gpu-bridge-host
  - cargo test -p hive-gateway
  - python3 -m pytest tools/cohesix-py/tests -q
  - cargo clippy --workspace --all-targets -- -D warnings
Checks:
  - Host-side cleanup reduces repetition and makes invariants explicit without protocol, file-layout, receipt, or request-auth drift.
  - Each host-tool refactor wave is independently reviewable and revertible, with one owner, one preserved-contract list, one characterization artifact, and one targeted test subset recorded before edits.
  - Characterization tests stay unchanged and passing across refactors.
Deliverables:
  - Human-readable host-side control-plane code with preserved external behavior and tighter shared validation boundaries.

Title/ID: m26c-root-task-runtime-decomposition
Goal: Decompose root-task runtime adapter code into smaller invariant-bearing modules after parity and `no_std` gates pin behavior.
Inputs: apps/root-task/src/lib.rs, apps/root-task/src/ninedoor.rs, apps/root-task/src/event/**, apps/root-task/src/console/**, apps/root-task/src/log_buffer.rs, apps/root-task/src/audit/**, apps/root-task/src/bootstrap/**, apps/root-task/src/hal/driver_task.rs, apps/root-task/tests/**, apps/pi4-driver-runtime/**, crates/pi4-driver-abi/**, configs/root_task*.toml, docs/ARCHITECTURE.md, docs/INTERFACES.md, docs/SECURE9P.md, docs/TEST_PLAN.md, docs/audit/M26C_RUNTIME_BOUNDARY_AUDIT.md, docs/audit/M26C_NINEDOOR_PARITY_MATRIX.md
Changes:
  - apps/root-task/src/ninedoor.rs + apps/root-task/src/event/** + apps/root-task/src/console/** — split parsing, dispatch, event pumping, completion formatting, and permission-denial paths into narrowly named modules without changing console grammar or Secure9P namespace semantics.
  - apps/root-task/src/log_buffer.rs + selected `/proc` emitters — extract append-only, cursor, and evidence formatting helpers while preserving current output fixtures.
  - apps/root-task/src/lib.rs + apps/root-task/src/bootstrap/** + apps/root-task/src/hal/driver_task.rs — clarify initialization sequencing, generated-manifest handoff boundaries, and manifest-declared isolated driver runtime descriptor ownership without moving authority out of compiler-generated artifacts.
  - apps/pi4-driver-runtime/** + crates/pi4-driver-abi/** — keep ABI and runtime changes behavior-preserving, pointer-free, and aligned with generated `root_task.driver_images` records.
  - apps/root-task/tests/** + apps/nine-door/tests/*.rs — preserve and extend parity fixtures before and after the decomposition.
  - docs/ARCHITECTURE.md + docs/INTERFACES.md + docs/SECURE9P.md + docs/TEST_PLAN.md — update as-built module ownership only when public contracts or evidence paths become clearer.
Commands:
  - cargo test -p nine-door --test integration
  - cargo test -p root-task --tests
  - cargo test -p root-task --lib
  - cargo test -p pi4-driver-abi
  - cargo test -p pi4-driver-runtime
  - cargo check -p root-task --target aarch64-unknown-none --no-default-features --features "cohesix-dev"
  - cargo check -p root-task --target aarch64-unknown-none --no-default-features --features "kernel bootstrap-trace serial-console net-console"
  - cargo check -p pi4-driver-runtime --target aarch64-unknown-none
  - cargo clippy --workspace --all-targets -- -D warnings
Checks:
  - Root-task decomposition preserves ACK/ERR/END grammar, namespace layout, `/proc` output shapes, event ordering, append-only behavior, and generated-manifest authority.
  - Linked driver-runtime descriptor handling, fixed-ring command/completion ABI, and acceptance-eligibility classification remain unchanged unless a reopened 26a/26b acceptance task changes them with hardware evidence.
  - VM-target builds remain `no_std` across QEMU and Pi 4 closure profiles.
  - Each root-task decomposition wave has a baseline-linked preserved-contract list and can be reverted without undoing unrelated worker/runtime behavior.
  - No new `unsafe`, `unwrap`, `expect`, or `panic!` appears in non-test root-task code without an approved exception.
Deliverables:
  - Smaller root-task runtime modules with preserved operator-visible behavior and explicit boundary evidence.

Title/ID: m26c-hal-network-and-local-seat-decomposition
Goal: Refactor Pi 4 HAL-facing network, Wi-Fi, and local-seat code into clearer bounded units and narrow SDIO/USB platform seams without changing boot policy, transcripts, or device authority.
Inputs: apps/root-task/src/hal/**, apps/root-task/src/net/**, apps/root-task/src/drivers/**, apps/root-task/src/local_seat.rs, apps/root-task/src/generated/**, apps/pi4-driver-runtime/**, crates/pi4-driver-abi/**, configs/root_task_pi4_uboot_aarch64.toml, docs/HARDWARE_BRINGUP.md, docs/NETWORK_CONFIG.md, docs/BOOT_REFERENCE.md, docs/TEST_PLAN.md
Changes:
  - apps/root-task/src/net/** — separate policy parsing, DHCP state, static IPv4 validation, active/standby interface selection, and evidence formatting while preserving manifest and DTB override semantics.
  - apps/root-task/src/hal/** + apps/root-task/src/drivers/** + apps/pi4-driver-runtime/** + crates/pi4-driver-abi/** — keep all device access behind HAL traits, split device-state and firmware/handoff bookkeeping from transcript emission, preserve isolated runtime command ownership, extract current-behavior-only SDIO host and USB platform/DMA seams after characterization tests exist, and remove duplicated bounds checks under the same evidence gate.
  - apps/root-task/src/local_seat.rs — isolate local-seat input/display policy from boot evidence and network policy handoff code without changing first-boot operator behavior.
  - docs/HARDWARE_BRINGUP.md + docs/NETWORK_CONFIG.md + docs/BOOT_REFERENCE.md + docs/TEST_PLAN.md — update as-built explanations and evidence commands for any clearer module boundaries.
Commands:
  - cargo test -p root-task --tests
  - cargo test -p root-task --lib
  - cargo test -p pi4-driver-abi
  - cargo test -p pi4-driver-runtime
  - cargo check -p root-task --target aarch64-unknown-none --no-default-features --features "kernel bootstrap-trace serial-console net-console"
  - cargo check -p pi4-driver-runtime --target aarch64-unknown-none
  - scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml
  - scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m26c-pi4
Checks:
  - HAL access remains centralized behind HAL traits; no direct MMIO, physical address, DMA publication, IRQ binding, or firmware-service shortcuts are introduced outside HAL.
  - New SDIO/USB seams are extraction-only, have before/after characterization evidence, and do not add new hardware support or generic future-driver behavior.
  - Each HAL/network/local-seat wave is isolated to one preserved behavior surface and records the exact tests plus Pi 4/QEMU evidence needed to prove no transcript or authority drift.
  - Isolated runtime acceptance eligibility and the separate fresh-Pi board-proof boundary remain explicit until owner-state proof and fresh Pi hardware evidence close them.
  - Boot transcripts, `netstats`, `netstatus`, DHCP/static policy evidence, Wi-Fi fallback semantics, and no-NIC compatibility remain unchanged unless a separately approved breaking-change path is followed.
  - Pi 4 staged evidence proves the refactor did not regress local-seat, network-policy, or U-Boot handoff behavior.
Deliverables:
  - Clearer Pi 4 HAL/network/local-seat modules with preserved hardware bring-up semantics and explicit SDIO/USB platform seams ready for later milestone evaluation.

Title/ID: m26c-full-test-plan-qemu-and-pi4
Goal: Make full staged Test Plan PASS on both QEMU and Pi 4 the explicit closure gate for 26c.
Inputs: docs/TEST_PLAN.md, docs/HARDWARE_BRINGUP.md, docs/audit/M26C_TARGET_RUNNER_BASELINE.md, docs/audit/M26C_POST_BEHAVIOR_BASELINE.md, scripts/ci/test_plan_run.sh, scripts/ci/test_plan_stage_*.sh, scripts/ci/check_test_plan.sh, scripts/pi4-image-build.sh, scripts/uboot/qemu-uboot-smoke.sh
Changes:
  - docs/TEST_PLAN.md — verify QEMU and Pi 4 execution matrices, evidence paths, PASS criteria, and target-qualified artifact requirements still match the early runner baseline and post-behavior baseline.
  - docs/HARDWARE_BRINGUP.md — verify Pi 4 hardware setup, log capture, and operator prerequisites remain sufficient to run the full Test Plan.
  - scripts/ci/test_plan_run.sh + scripts/ci/test_plan_stage_*.sh — use the already implemented `--target qemu|pi4` contract to archive final target-qualified evidence without pretending QEMU and Pi 4 evidence are identical.
  - scripts/ci/check_test_plan.sh — enforce final alignment between target-aware docs, stage scripts, baseline artifacts, and authoritative commands.
  - scripts/pi4-image-build.sh — produce deterministic artifacts consumed by Pi 4 staged test-plan runs.
Commands:
  - scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m26c-qemu
  - scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml
  - scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m26c-pi4
Checks:
  - The staged runner produces `stage_01.done` through `stage_05.done` for both QEMU and Pi 4 state dirs.
  - Neither state dir contains `*.incomplete` markers.
  - Stage 05 verifies the required target-qualified artifacts exist before writing `stage_05.done`.
  - Pi 4 evidence includes serial logs, network or console validation, and due-diligence outputs matching the documented PASS contract.
Deliverables:
  - Full Test Plan PASS on QEMU and Pi 4 as the hard milestone exit criterion, with archived evidence for both targets.
```

---

## Milestone 26d — seL4 15 Baseline Refresh + Reference/Performance Realignment <a id="26d"></a>
[Milestones](#Milestones)

**Why now (kernel truth):**
Milestone 26c makes the docs-as-built audit, target-qualified Test Plan, host/VM boundary evidence, and regression-gated refactor baseline explicit. Milestone 26d refreshes the external kernel baseline and canonical references to seL4 15.0.0, anchors manual alignment to the official seL4 Reference Manual v15.0.0 ([PDF](https://sel4.systems/Info/Docs/seL4-manual-15.0.0.pdf)), proves the reopened 26a/26b driver-task model still holds on QEMU and Pi 4, and closes kernel-version drift before later feature work builds on stale assumptions. Because a kernel refresh changes scheduler, syscall, timer, cache, and generated-artifact behavior that can move measured latency or throughput, 26d also owns a bounded benchmark revalidation and regression-tuning lane: compare historical-best, accepted 26b, first seL4 15, and post-tuning evidence before later milestones rely on the refreshed baseline.

**Non-negotiable constraints:**
- No further system-model change beyond the reopened 26a/26b driver-task baseline. Cohesix remains an upstream seL4, pure-Rust root-task authority system with hardware driver tasks; Microkit, CAmkES, and capDL loader adoption are explicitly out of scope for 26d.
- No new operator-visible protocol, namespace, ACK/ERR/END, telemetry, manifest, or release-behavior changes are permitted under a kernel-refresh label.
- `rust-sel4` adoption is out of scope. Cohesix may audit upstream Rust support for compatibility reference, but 26d must preserve the current Cohesix-owned `sel4-sys` / `sel4-runtime` / root-task bootstrap stack unless a separate milestone authorizes replacement.
- Canonical kernel/manual provenance must be updated with specific versions and, where available, upstream commit identifiers for QEMU, SMP, and Pi 4/U-Boot build flows.
- Older manual/reference mentions are known 26d blockers, not acceptable post-26d residue. Later milestones must not cite seL4 15 alignment until `m26d-kernel-provenance-refresh` updates or explicitly retires those references.
- Any seL4 build configuration that still depends on legacy `KernelDomainSchedule` / `domain_schedule.c` handling must be either removed when semantically unused or migrated/documented consistently with seL4 15 behavior; one-domain configurations must not retain hidden schedule-file dependencies.
- The seL4 15 refresh must preserve the Pi 4 hardware-counter contract used by isolated runtime performance proof: `release-pi4` / `timers-arch-counter` builds expose only the read-only EL0 virtual counter (`KernelArmExportVCNTUser` / `CONFIG_EXPORT_VCNT_USER`), keep physical counter and EL0 timer-control exports disabled, and derive elapsed-time proof from refreshed `TIMER_CLOCK_HZ=54000000` generated headers. Kernel refresh work must not reclassify dummy-timer or physical-counter captures as valid latency evidence.
- Performance tuning is permitted in 26d only when tied to a measured same-harness regression, regression-risk, or drift exposed by the seL4 15 refresh. Allowed tuning includes bounded scheduler/budget constants, driver-runtime service-turn cadence, cache-maintenance batching thresholds, TCP/REST gateway timeout plumbing, harness provenance/reporting fixes, and comparator hygiene. Tuning must preserve Secure9P semantics, ACK/ERR/END grammar, manifest authority, HAL-only physical-device authority, root/driver-task ownership boundaries, no-retry benchmark accounting, and the documented Pi 4 wired/GENET versus Wi-Fi proof split.
- 26d tuning must not become service-bucket/core-local redesign, new protocol work, new namespace or telemetry grammar, relaxed error budgets, retry masking, root-owned physical-driver shortcuts, larger unbounded queues, or a reclassification of Wi-Fi stress diagnostics as production parity. Those remain reopened 26b, 27c, or later scoped work.
- QEMU and Pi 4 target-qualified evidence must be regenerated on the refreshed kernel baseline before 26d can close.

### Prerequisite
- The original Milestone **26c** runtime/refactor closure is accepted
  (target-qualified test-plan runner, docs-as-built audit, refactor map,
  risk-ratchet baseline, and no-std boundary evidence available). Its later
  documentation-only remediation may run concurrently and does not invalidate
  those accepted prerequisite artifacts.
- Reopened Milestones **26a** and **26b** completed or explicitly scoped where their driver-task and benchmark artifacts are inputs to the kernel refresh; no isolated runtime performance assumption may be rewritten under the kernel-refresh label.

### Goal
Upgrade Cohesix's external seL4 baseline and normative references to seL4 15.0.0 while preserving root-task authority plus the reopened 26a/26b hardware driver-task model, prove zero operator-visible drift across QEMU and Pi 4, and preserve or recover the accepted 26b REST/driver-runtime benchmark envelope when the refreshed kernel baseline exposes a bounded performance regression.

### Deliverables
- **Kernel baseline refresh**
  - Refresh external seL4 build inputs used by Cohesix bring-up, CI, and Pi 4 image flows to seL4 15.0.0.
  - Record the exact upstream seL4 version/commit accepted for Cohesix in canonical docs and test evidence.
  - Record refreshed Pi 4 counter-export evidence for `KernelArmExportVCNTUser` / `CONFIG_EXPORT_VCNT_USER`, forbidden physical-counter/timer-control exports, and `TIMER_CLOCK_HZ`.

- **Reference-manual and docs realignment**
  - Update `docs/BUILD_PLAN.md`, `docs/TOOLCHAIN_MAC_ARM64.md`, `README.md`, and other canonical docs that currently describe the normative seL4 manual/baseline so they align with seL4 15.0.0 as built and link the official seL4 Reference Manual v15.0.0 PDF (`https://sel4.systems/Info/Docs/seL4-manual-15.0.0.pdf`).
  - Refresh the carried seL4 manual mirror under `seL4/` if the repo continues to store it as a tracked reference artifact.
  - Record any seL4 15 changes that are intentionally irrelevant to Cohesix today, especially domain-schedule features tied to Microkit/CAmkES/capDL workflows.

- **Direct-API compatibility audit and fixes**
  - Audit `crates/sel4-sys`, `crates/sel4-runtime`, `crates/pi4-driver-abi`, profile-selected `apps/pi4-driver-runtime` images, generated `root_task.driver_images` descriptors, and root-task bootstrap code against seL4 15 headers and generated metadata.
  - Preserve Cohesix’s explicit TLS and IPC-buffer installation path unless a compatibility fix requires adjustment.
  - Keep SMP affinity, boot-info handling, CSpace assumptions, and debug-syscall guards aligned with seL4 15 generated headers for both single-core and SMP builds.
  - Keep root-task and linked-driver-runtime timer build guards aligned with seL4 15 generated `TIMER_CLOCK_HZ` and VCNT export truth; compatibility fixes may not introduce `CNTPCT_EL0`, EL0 timer-control register use, or raw CPU-speed spin loops for elapsed time.

- **Domain-schedule debt removal**
  - Audit build caches and Pi 4/U-Boot kernel configuration for stale `KernelDomainSchedule` / `domain_schedule.c` assumptions.
  - For configurations with `KernelNumDomains=1`, remove or document any stale schedule-file dependency so Cohesix does not inherit `sel4test` domain-schedule defaults as an accidental build requirement.
  - If any Cohesix-owned configuration genuinely uses domains later, migrate that path to seL4 15 runtime/domain-schedule semantics in a scoped follow-on task before enabling it.

- **Regression and evidence refresh**
  - Re-run generated-artifact guards, root-task checks/tests, QEMU bring-up, Pi 4 image build, and the full target-qualified Test Plan on the seL4 15 baseline.
  - Publish refreshed audit artifacts proving that operator-visible semantics remain unchanged, that the VM build remains `no_std`, and that Pi 4 latency/performance proof still uses the virtual-counter backend rather than dummy timers or physical-counter exports.

- **Benchmark revalidation and bounded tuning**
  - Archive a before/after benchmark ledger with four explicit lanes: historical top benchmark, accepted 26b baseline, first seL4 15 baseline before tuning, and final seL4 15 evidence after any tuning.
  - Re-run the REST performance harness in the same-harness QEMU/Pi shape used by 26b, keeping raw direct `cohsh`/TCP proof, REST gateway overhead, QEMU semantic/capacity reference, wired/GENET production parity, and Wi-Fi research/diagnostic evidence separate.
  - Permit only bounded tuning that preserves existing authority and protocol semantics; every performance fix must identify the moved layer as kernel/generated-artifact drift, root-task scheduling cadence, isolated driver-runtime service cadence, gateway/harness behavior, physical-target variance, or pre-existing 26b debt.
  - Update `docs/BENCHMARKS.md` only for refreshed artifact indexes, verdict changes, or explicit non-blocking debt. Do not erase historical-best evidence or convert uncommitted local diagnostics into canonical proof without archived artifacts.

### Commands
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`
- `scripts/check-generated.sh`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo test -p pi4-driver-abi`
- `cargo test -p pi4-driver-runtime`
- `cargo check -p root-task --target aarch64-unknown-none --no-default-features --features "cohesix-dev"`
- `cargo check -p root-task --target aarch64-unknown-none --no-default-features --features "kernel bootstrap-trace serial-console net-console"`
- `cargo check -p pi4-driver-runtime --target aarch64-unknown-none`
- `scripts/cohesix-build-run.sh --sel4-build seL4/build --no-run --cargo-target aarch64-unknown-none --profile release --root-task-features cohesix-dev`
- `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml --sel4-build-dir seL4/build_UBOOT`
- `python3 -m pytest -q tests/test_rest_perf_harness.py tests/test_pi4_compare_driver_models.py`
- `python3 scripts/rest_perf_harness.py --mode perf --suite all --runs 5 --log-dir out/bench --log-prefix m26d-qemu-sel4-15-initial`
- `python3 scripts/rest_perf_harness.py --mode perf --suite all --runs 5 --no-qemu --no-gateway --rest-url http://<pi4-gateway-host>:<port> --log-dir out/bench --log-prefix m26d-pi4-sel4-15-initial`
- `python3 scripts/rest_perf_harness.py --mode perf --suite all --runs 5 --log-dir out/bench --log-prefix m26d-qemu-sel4-15-final`
- `python3 scripts/rest_perf_harness.py --mode perf --suite all --runs 5 --no-qemu --no-gateway --rest-url http://<pi4-gateway-host>:<port> --log-dir out/bench --log-prefix m26d-pi4-sel4-15-final`
- `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m26d-qemu`
- `scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m26d-pi4`

### Checks (DoD)
- Canonical docs link the official seL4 Reference Manual v15.0.0 PDF and explicitly align to the accepted seL4 15.0.0 baseline, remaining consistent with the refreshed as-built evidence.
- `crates/sel4-sys`, `crates/sel4-runtime`, `crates/pi4-driver-abi`, `apps/pi4-driver-runtime`, generated driver-image descriptors, driver-runtime CPIO packaging, and root-task bootstrap code build cleanly against seL4 15 generated headers/artifacts for the QEMU baseline, SMP baseline, and Pi 4 U-Boot baseline used by Cohesix.
- QEMU and Pi 4 target-qualified Test Plan runs pass on the refreshed kernel baseline with no `*.incomplete` markers and no undocumented operator-visible output drift.
- Secure9P bounds, console grammar, manifest outputs, release semantics, and host/VM runtime boundaries remain unchanged unless separately documented as defects fixed in the same change.
- Build artifacts and documentation no longer rely on an accidental `sel4test`-provided `domain_schedule.c` dependency for one-domain Cohesix configurations; any remaining dependency is explicit, justified, and documented.
- Pi 4 refreshed evidence reports `TIMER_BACKEND=arch-counter`, `TIMER_CLOCK_HZ=54000000`, `TIMER_EL0_COUNTER=vct`, and `DUMMY_TIMER_SEEN=no`; any missing/mismatched counter export leaves isolated runtime latency proof red until fixed or explicitly scoped back to reopened 26a/26b acceptance.
- 26d benchmark evidence includes the historical-best, accepted 26b, first seL4 15, and final seL4 15 lanes; any tuning is explained by layer, bounded by existing authority/protocol rules, and rechecked with the REST performance harness without retry masking or relaxed error budgets.
- If final seL4 15 benchmark evidence remains below the accepted 26b envelope, the closure record classifies the gap as blocking regression, physical-target variance with evidence, pre-existing 26b debt, or explicitly deferred later-milestone work. Deferred work cannot be counted as 26d closure evidence for downstream milestones.
- The carried seL4 manual/reference artifacts, if tracked in-repo, match the accepted kernel baseline or are explicitly relinked to the authoritative upstream 15.0.0 source.

### Compiler / docsystem touchpoints
- `coh-rtc` outputs, docs snippets, and manifest fingerprints remain authoritative; 26d may update generators or schemas only when required by the seL4 15 baseline refresh, and any such change must be reflected in the same evidence set.
- Pi 4 manifest-declared isolated driver runtime ABI/descriptors/images remain authoritative for driver-task bootstrap evidence during the kernel refresh; 26d may adapt them only for seL4 15 compatibility and must not reclassify runtime-spec acceptance or board-proof boundaries without reopened 26a/26b hardware acceptance evidence.
- Root-task and linked-driver-runtime hardware-counter guards remain authoritative for performance proof during the kernel refresh. Generated seL4 headers and CMake/cache config must agree before `timers-arch-counter` evidence can satisfy 26d closure.
- REST harness output, benchmark provenance fields, and `docs/BENCHMARKS.md` artifact indexes remain the source of truth for same-harness performance claims during the refresh. Any harness change made in 26d must improve provenance, strictness, or failure classification without changing the workload contract used for comparison.
- `docs/TEST_PLAN.md`, `scripts/ci/test_plan_run.sh`, and target-qualified state-dir evidence remain the source of truth for QEMU/Pi 4 pass semantics during the kernel refresh.

### Atomic tasks
```
Title/ID: m26d-kernel-provenance-refresh
Goal: Refresh Cohesix seL4 baseline inputs and record accepted seL4 15.0.0 provenance.
Inputs: external seL4 source/build trees, seL4 15.0.0 release notes, official seL4 Reference Manual v15.0.0 PDF (`https://sel4.systems/Info/Docs/seL4-manual-15.0.0.pdf`), docs/TOOLCHAIN_MAC_ARM64.md, README.md.
Changes:
  - docs/TOOLCHAIN_MAC_ARM64.md — record the accepted seL4 15.0.0 baseline and external-build expectations.
  - README.md — update high-level kernel/manual baseline references if they mention stale versions or assumptions.
  - seL4/ — refresh tracked manual/reference artifacts only if the repo continues carrying them as canonical mirrors.
Commands: cargo check --workspace
Checks: canonical docs and carried references identify the accepted seL4 15.0.0 baseline with no stale older-manual pin left in normative guidance.
Deliverables: updated docs/reference provenance and a checked-in note of the accepted upstream version/commit.
```

```
Title/ID: m26d-sel4-api-compat-audit
Goal: Bring Cohesix-owned seL4 bindings/runtime/bootstrap code into clean alignment with seL4 15 generated artifacts.
Inputs: crates/sel4-sys, crates/sel4-runtime, crates/pi4-driver-abi, apps/pi4-driver-runtime, apps/root-task, configs/root_task*.toml, seL4/build, seL4/SMP_build, seL4/build_UBOOT.
Changes:
  - crates/sel4-sys — compatibility fixes required by seL4 15 headers/generated metadata.
  - crates/sel4-runtime — compatibility fixes required by seL4 15 root-task startup behavior.
  - crates/pi4-driver-abi + apps/pi4-driver-runtime — compatibility fixes required by seL4 15 runtime-init descriptor layout, pointer-free ring transport, or target build rules.
  - apps/root-task — bootstrap and generated driver-image handoff adjustments only where required by seL4 15 semantics.
Commands:
  - cargo check -p root-task --target aarch64-unknown-none --no-default-features --features "cohesix-dev"
  - cargo test -p pi4-driver-abi
  - cargo test -p pi4-driver-runtime
  - cargo check -p pi4-driver-runtime --target aarch64-unknown-none
Checks: direct-seL4 Cohesix builds compile on all supported kernel profiles, and manifest-declared isolated driver runtime ABI/build evidence stays green, without changing system model or operator-visible semantics.
Deliverables: passing workspace/build-target checks and refreshed low-level compatibility evidence.
```

```
Title/ID: m26d-pi4-counter-contract-refresh
Goal: Reprove the Pi 4 virtual-counter timer contract on the seL4 15 baseline before accepting isolated runtime latency or performance evidence.
Inputs: seL4/build_UBOOT/CMakeCache.txt, seL4/build_UBOOT/kernel/gen_headers/plat/platform_gen.h, apps/root-task/build.rs, apps/root-task/src/arch/aarch64/timer.rs, apps/pi4-driver-runtime/build.rs, apps/pi4-driver-runtime/src/lib.rs, scripts/pi4-image-build.sh, scripts/pi4_gate_proof.sh, scripts/pi4_trace_normalize.py, docs/HARDWARE_BRINGUP.md, docs/TEST_PLAN.md.
Changes:
  - scripts/pi4-image-build.sh — keep seL4 15 Pi 4 staging blocked unless VCNT export is enabled, physical counter/timer-control exports are disabled, and generated `TIMER_CLOCK_HZ` matches the accepted Pi profile.
  - apps/root-task/build.rs + apps/root-task/src/arch/aarch64/timer.rs — preserve root-task build/runtime checks that use `CNTVCT_EL0` only under the `timers-arch-counter` profile and scale elapsed-time proof from generated frequency.
  - apps/pi4-driver-runtime/build.rs + apps/pi4-driver-runtime/src/lib.rs — preserve isolated runtime build/runtime checks and `RuntimeDeadline` conversion from legacy retry counts to counter-backed deadlines.
  - scripts/pi4_gate_proof.sh + scripts/pi4_trace_normalize.py + docs/HARDWARE_BRINGUP.md + docs/TEST_PLAN.md — ensure refreshed Pi 4 proof gates require `TIMER_BACKEND=arch-counter`, `TIMER_CLOCK_HZ=54000000`, `TIMER_EL0_COUNTER=vct`, and `DUMMY_TIMER_SEEN=no` before latency proof is accepted.
Commands:
  - scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml --sel4-build-dir seL4/build_UBOOT
  - cargo check -p root-task --target aarch64-unknown-none --no-default-features --features "kernel bootstrap-trace serial-console net-console"
  - cargo check -p pi4-driver-runtime --target aarch64-unknown-none
  - scripts/pi4_gate_proof.sh --require-driver-task-proof
Checks:
  - Refreshed seL4 15 Pi 4 generated headers and cache expose only the accepted virtual-counter path for EL0 timing.
  - Root-task and isolated runtime builds fail on missing VCNT export, nonzero physical-counter/timer-control export, or missing/nonzero-invalid `TIMER_CLOCK_HZ`.
  - Runtime latency telemetry and proof normalizer output reject dummy-timer captures, stale logs, physical-counter exports, and missing timer frequency as performance evidence.
Deliverables:
  - Checked-in 26d evidence tying seL4 15 provenance, Pi 4 counter configuration, isolated runtime deadline behavior, and target-qualified performance proof into one refreshed contract.
```

```
Title/ID: m26d-benchmark-revalidation-and-tuning
Goal: Revalidate and, where needed, recover the accepted 26b REST/driver-runtime benchmark envelope on the seL4 15 baseline.
Inputs: scripts/rest_perf_harness.py, tests/test_rest_perf_harness.py, tests/test_pi4_compare_driver_models.py, docs/BENCHMARKS.md, out/bench/m26b-* artifacts, refreshed seL4 15 QEMU/Pi build artifacts, fresh Pi serial/pcap proof for the selected transport.
Changes:
  - docs/BENCHMARKS.md — record refreshed artifact indexes, before/after verdicts, and any explicitly classified non-blocking debt.
  - scripts/rest_perf_harness.py + tests/test_rest_perf_harness.py — provenance/reporting or strictness fixes only when needed to compare the seL4 15 run against the accepted 26b workload without changing the workload contract.
  - apps/root-task/src/**, apps/pi4-driver-runtime/src/**, crates/pi4-driver-abi/src/**, or apps/hive-gateway/src/** — bounded tuning only where same-harness evidence points to a moved layer caused or exposed by the seL4 15 refresh.
Commands:
  - python3 -m pytest -q tests/test_rest_perf_harness.py tests/test_pi4_compare_driver_models.py
  - python3 scripts/rest_perf_harness.py --mode perf --suite all --runs 5 --log-dir out/bench --log-prefix m26d-qemu-sel4-15-initial
  - python3 scripts/rest_perf_harness.py --mode perf --suite all --runs 5 --no-qemu --no-gateway --rest-url http://<pi4-gateway-host>:<port> --log-dir out/bench --log-prefix m26d-pi4-sel4-15-initial
  - python3 scripts/rest_perf_harness.py --mode perf --suite all --runs 5 --log-dir out/bench --log-prefix m26d-qemu-sel4-15-final
  - python3 scripts/rest_perf_harness.py --mode perf --suite all --runs 5 --no-qemu --no-gateway --rest-url http://<pi4-gateway-host>:<port> --log-dir out/bench --log-prefix m26d-pi4-sel4-15-final
Checks:
  - Historical top, accepted 26b, first seL4 15, and final seL4 15 lanes are all recorded with artifact paths, workload parameters, tool version/provenance, selected seL4 build, gateway/auth mode, QEMU SMP topology, target transport, and error-budget policy.
  - QEMU/Pi comparisons reject stale or mismatched artifacts before any verdict; Pi evidence remains fresh, timer-qualified, and paired with raw TCP/cohsh proof for the selected transport.
  - Tuning preserves Secure9P bounds, console grammar, manifest authority, no-retry failure accounting, HAL/driver-task ownership, and root-task no-std constraints.
  - Wi-Fi remains a research/diagnostic lane with the documented worker envelope; production throughput parity still requires fresh wired/GENET evidence.
Deliverables:
  - Benchmark ledger and refreshed artifacts that make the seL4 15 performance delta reviewable before later milestones depend on the refreshed baseline.
```

```
Title/ID: m26d-domain-schedule-debt-removal
Goal: Remove or explicitly resolve stale legacy domain-schedule dependencies from Cohesix seL4 build configurations.
Inputs: seL4/build_UBOOT/CMakeCache.txt, seL4/build*/generated artifacts, Pi 4 build scripts, seL4 15.0.0 upgrade notes.
Changes:
  - scripts/pi4-image-build.sh and related build docs — ensure Cohesix-owned Pi 4 flows do not silently depend on stale `domain_schedule.c` defaults when domains are not in use.
  - docs/HARDWARE_BRINGUP.md and docs/BUILD_PLAN.md — document the resolved seL4 15 domain-schedule posture for Cohesix.
Commands: scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml --sel4-build-dir seL4/build_UBOOT
Checks: one-domain Cohesix builds no longer inherit accidental `sel4test` schedule-file assumptions.
Deliverables: documented and verified domain-schedule posture for Cohesix seL4 15 builds.
```

```
Title/ID: m26d-repository-gate-closure
Milestone: Milestone 26d — seL4 15 Baseline Refresh + Reference/Performance Realignment / repository-wide regression gate closure
Goal: Restore every mandatory repository-wide source, dependency, packaging, and staged-regression gate exposed while preparing the refreshed QEMU and Pi 4 evidence, without changing operator-visible semantics or hardware authority.
Inputs: Cargo.toml, Cargo.lock, deny.toml, apps/root-task/build.rs, apps/root-task/build_support.rs, apps/root-task/src, scripts/cohesix-build-run.sh, scripts/ci/test_plan_*.sh, scripts/ci/due_diligence_gate.sh, .github/workflows/*.yml, README.md, docs/TEST_PLAN.md, docs/audit risk registers, current M26b/M26d source and build evidence.
Changes:
  - Cargo.toml + Cargo.lock + CI — update vulnerable or yanked transitive dependency selections to supported compatible versions without widening VM dependency closure, track the lockfile, and reject stale resolution before build/test commands.
  - apps/root-task/build.rs + apps/root-task/build_support.rs + apps/root-task/src — correct fresh-checkout generated-artifact validation, deterministic test-contract drift, and strict lint failures exposed by the canonical Pi 4 Stage 02 feature set.
  - scripts/cohesix-build-run.sh and focused tests/docs — keep the QEMU rootfs below the 4 MiB guard while preserving all manifest-declared runtime artifact names, bytes, and isolated-runtime lookup semantics.
  - scripts/ci, tools/rust-risk-audit, README.md, and canonical docs — make Python selection exact and recoverable, count cfg-test Rust separately from production risk, record the real linked-runtime unsafe delta, and align as-built packaging explanations with the canonical QEMU path.
  - .github/workflows/*.yml — audit every trigger and job, consolidate required pull-request, main-branch, manual, and scheduled checks into the smallest auditable workflow set, retire stubs and jobs that invoke removed tooling, preserve mandatory Rust, generated-artifact, staged-plan, VM-boundary, and dependency gates, and use least-privilege permissions with deterministic toolchain/action pins and retained failure evidence.
Commands:
  - go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.12 .github/workflows/*.yml
  - cargo metadata --locked --no-deps
  - cargo fmt --all -- --check
  - cargo clippy --workspace --all-targets -- -D warnings
  - cargo check --workspace
  - cargo test --workspace -- --test-threads=1
  - cargo test -p rust-risk-audit
  - cargo run --quiet --locked -p rust-risk-audit -- --baseline docs/audit/rust_risk_baseline.toml
  - cargo audit
  - cargo deny check advisories
  - scripts/check-generated.sh
  - scripts/cohesix-build-run.sh --no-run --cargo-target aarch64-unknown-none
  - scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m26d-repository-gates-qemu
  - scripts/ci/test_plan_run.sh --target pi4 --stage 1 --state-dir out/test-plan/m26d-repository-gates-pi4
  - scripts/ci/test_plan_run.sh --target pi4 --stage 2 --state-dir out/test-plan/m26d-repository-gates-pi4
Checks: mandatory lint, workspace, dependency, generated-artifact, QEMU packaging, cfg-aware risk-ratchet, and offline staged-regression gates pass with no newly ignored advisory, forged marker, skipped stage, rootfs-size exception, operator grammar drift, or loss of manifest-declared driver-runtime artifact identity; accepted production unsafe growth is explicit and expiry-bounded; live Pi stages remain separately hardware-gated; every retained workflow parses cleanly, references only tracked commands, preserves required event coverage, and has no redundant or no-op job.
Deliverables: reviewable dependency resolution and risk register, deterministic root-task/Python regression coverage, sub-4-MiB QEMU payload, clean QEMU plus offline Pi target-qualified gate evidence, and one coherent GitHub Actions surface with obsolete workflow files removed.
```

```
Title/ID: m26d-full-regression-refresh
Goal: Prove the seL4 15 baseline refresh preserves operator-visible behavior and accepted benchmark envelopes across QEMU and Pi 4.
Inputs: refreshed generated artifacts, seL4 15 build trees, docs/TEST_PLAN.md, scripts/ci/test_plan_run.sh, m26d benchmark ledger.
Changes:
  - docs/TEST_PLAN.md — update kernel-baseline references only where needed to match the refreshed evidence.
  - out/test-plan/m26d-qemu and out/test-plan/m26d-pi4 — target-qualified PASS evidence on the refreshed baseline.
Commands:
  - scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m26d-qemu
  - scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m26d-pi4
Checks: QEMU and Pi 4 staged Test Plan runs are PASS with no undocumented drift, and benchmark revalidation has either recovered the accepted 26b envelope or recorded a scoped blocker/defer decision that downstream milestones cannot count as satisfied evidence.
Deliverables: target-qualified refreshed evidence proving seL4 15 upgrade safety and performance continuity for Cohesix.
```

---

## Post-26d Benchmark Cadence (Milestones 27+) <a id="post-26d-benchmark-cadence"></a>
[Milestones](#Milestones)

26d establishes the refreshed seL4 15 performance baseline. Later milestones must use that evidence as the rolling comparison point, but they must not turn every feature into a full hardware benchmark gate. Benchmarking after 26d is tiered:

- **Full same-harness benchmark gate:** required only when a milestone changes runtime scheduling, physical driver/runtime service, hot network paths, gateway broker behavior used by the canonical REST harness, or a new physical target. The run must compare against the latest accepted rolling baseline and archive QEMU/Pi or target-specific artifacts in `out/bench/` or `docs/bench/` as appropriate.
- **Targeted microbenchmark gate:** required when a milestone adds bounded storage drains, namespace roots, read/write authorization checks, protocol projections, exporters, UI render loops, or evidence-pack processing that can add measurable overhead but does not change the physical network/runtime path.
- **No runtime benchmark gate by default:** applies to documentation-only, verification-only, schema-only, read-only inspection, and pure conformance milestones. These milestones may measure CI/proof/tool runtime or bounded command latency, but they do not rerun Pi/QEMU throughput unless their changes land in a runtime or gateway hot path.

Benchmark evidence must keep proof lanes separate: raw direct `cohsh`/TCP, REST/gateway overhead, QEMU semantic/capacity reference, Pi wired/GENET production parity, Pi Wi-Fi research/diagnostic evidence, storage/spool pressure, UI render cadence, and target-specific ENA/AWS proof are not interchangeable. Any claimed optimization must identify the moved layer and preserve Secure9P bounds, ACK/ERR/END grammar, manifest authority, HAL-only physical-device authority, no-retry accounting, and bounded queues.

Cadence by milestone family:
- **27 persistence:** targeted spool/settings pressure plus a small REST/status sanity benchmark only for profiles with persistence enabled.
- **27b verification:** no runtime benchmark; track verification-gate runtime and proof reproducibility only.
- **27c core-local scheduling:** full same-harness QEMU/Pi benchmark gate with service-bucket counters and fresh target evidence.
- **27d operator lanes:** full latency/fairness gate under mixed serial, USB local-seat, TCP console, HDMI, diagnostics, network, telemetry, and persistence pressure; throughput claims remain tied to 26d/27c baselines and must not collapse separated proof lanes.
- **28 read-only utilities:** no full benchmark; require bounded command latency for inspect/diff/attest over representative evidence packs.
- **28b/28d gateway authority/protocol work:** gateway-focused latency/backpressure benchmarks for REST delegated writes, MCP resources/tools, and A2A task flows; full Pi hardware benchmark only if gateway results expose runtime-path regression.
- **28b1/28c/28e:** targeted provider/exporter, AI-run-cost, or fault-recovery timing where the milestone changes those paths; no full Pi/QEMU throughput gate by default.
- **28f UI workbench:** UI render/backlog benchmark gate for Live Hive, replay, and inactive-view polling behavior.
- **29/29a field status:** bounded command-latency checks only.
- **29b AI namespace:** namespace-scale microbenchmarks for high-churn job/run roots; no full hardware benchmark unless the namespace provider changes hot runtime paths.
- **30 AWS/ENA:** new-target benchmark gates; single-queue first-link evidence is not peak performance, and peak ENA claims require archived EC2 evidence tied to generated queue and service-bucket policy.

---

## Milestone 27 — Bounded VM-Local Persistence: Spool Stores + Settings <a id="27"></a>
[Milestones](#Milestones)

**Why now (resilience):** After Pi 4 U-Boot boot + identity (26), edge deployments need store/forward for telemetry and minimal settings that survive reboots and link outages without introducing a general filesystem or new protocols.

**As-built alignment note:** Current code has host-side sidecar buffering, telemetry ring snapshot helpers, and U-Boot-owned network/Wi-Fi persistence, but it does **not** have VM-local persistent spool/settings storage, `persistence.*` manifest IR, isolated storage-runtime block service, root-task persistent-store client plumbing, or `/proc/spool/*` and `/queen/spool/*` providers. Milestone 27 introduces those surfaces; older docs or code comments that mention persistent spool as already available are drift unless backed by this milestone's acceptance evidence.

**Non-negotiable constraints**
- No changes to console grammar, 9P semantics, or TCP behavior vs VM unless profile‑gated and documented.
- No POSIX VFS; no general filesystem.
- Pure Rust userspace; no C‑FFI filesystems.
- Persistence is exposed only through NineDoor nodes (file‑shaped, bounded).
- `/proc` remains read-only observability. Milestone 27 must not introduce write-only or append-only controls under `/proc`; mutating spool/settings controls live under explicit role-scoped control roots.
- Storage selection is profile-gated and role-selected. The default Pi 4 hardware profile uses a manifest-declared raw SD/MMC block region on the boot microSD card, separate from the FAT boot partition and separate from U-Boot-owned `cohesix.env` policy storage.
- Persistence-enabled Pi 4 SD cards use a two-partition default layout: partition 1 is a 1 GiB FAT32 boot partition labeled by the flash script's `--disk-label` value (default `COHESIX`), and partition 2 is an unformatted raw M27 persistence region. The raw region has no filesystem label; host tooling may name or tag the partition `COHESIX-PERSIST` for operator visibility only. Flash tooling must check target SD size before erasing, fail closed when the card is too small, and leave any media beyond the manifest-declared persistence range unallocated/reserved unless a later profile explicitly opts into using the remainder.
- Physical Pi 4 block-device service must use a manifest-declared isolated storage runtime. Root-task may admit HAL resources, publish region descriptors, submit bounded block service turns, validate records, and expose NineDoor state, but it must not contain a root-owned SD/MMC storage driver or direct steady-state SD/MMC MMIO path.
- The Pi 4 CYW43 SDIO Wi-Fi transport is not a persistence backend. Its `sdio-host` role remains a CYW43 bus role, not a generic SD-card block role.
- USB mass storage is not the Milestone 27 default path. It may be added only as a later optional removable-media profile after USB local-seat/xHCI ownership is hardware-proved and cannot be a boot-critical dependency.

### Prerequisite
- Reopened Milestones **26a** and **26b**, plus Milestones **26c** and **26d**, completed where they are dependencies for the selected profile: Pi 4 driver-task concurrency evidence is available, the 26b benchmark ledger is closed or explicitly non-blocking for the selected persistence profile, the 26c blocker ledger is clear or scoped, and the seL4 baseline used by the persistence profile is current.

### Goal
Provide **bounded, crash‑resilient on‑device persistence** for:
1) telemetry store/forward (append‑only ring log), and  
2) minimal settings (A/B committed pages),
exposed through NineDoor without expanding the TCB.

This milestone is **not** an extension of the existing host-side sidecar spool mounted at `/bus/<adapter>/spool`. That spool remains an in-memory, nonpersistent sidecar facility. Milestone 27 introduces a distinct **VM-local persistent spool** with read-only observability under `/proc/spool/*` and mutating control files under role-scoped spool control roots.

### Deliverables

#### A) Compiler + manifest admission
- New `persistence.*` IR fields in `coh-rtc` for:
  - spool bounds (`max_bytes`, `max_record_bytes`, `mode`)
  - settings bounds and page sizing
  - storage device/region declaration for profiles that enable persistence
- Storage roles are explicit:
  - `virtio-blk` for QEMU and CI parity.
  - `pi4-sdmmc-raw` for the default Pi 4 hardware profile, backed by a fixed raw block region/partition on the boot microSD card and serviced by an isolated storage runtime over the fixed driver-task ABI.
  - `usb-mass-storage` is optional/future and must be rejected unless that profile explicitly admits removable media.
- Pi 4 validation rejects persistence if the profile attempts to bind storage to `cyw43455`, the CYW43 `sdio-host` bus role, `usb-local-seat`, root-task SD/MMC MMIO, or any FAT-path/U-Boot policy file. Runtime persistence must not read or write `cohesix.env`.
- Persistence config is **separate** from existing `sidecars.*.adapters[].spool`; the names must not be reused or overloaded.
- Generated docs snippets and manifest validation reject persistence when the selected boot profile does not declare a compatible storage region.

#### B) Storage plumbing (hardware + QEMU parity)
- Block-device abstraction in HAL (role-selected devices, not model-selected) plus a fixed-layout block-service ABI for physical storage runtimes.
- Root-task persistent-store semantics and NineDoor namespace are authoritative for the Pi 4 / seL4 path; physical block I/O is performed only by the isolated storage runtime, and host `nine-door` may mirror the same semantics for tests but does not define them.
- QEMU reference uses `virtio-blk`; the Pi 4 default hardware path uses `pi4-sdmmc-raw`, a bounded SD/MMC block role for a manifest-declared raw region and an isolated `driver-storage` runtime image.
- `pi4-sdmmc-raw` is a new storage role and must not reuse the CYW43 SDIO transport APIs, Wi-Fi SDIO runtime image, Wi-Fi SDHCI proof markers, or a root-owned SD/MMC compatibility driver as block-device proof.
- The Pi 4 raw region must be outside the FAT boot assets used by firmware/U-Boot and outside any U-Boot environment/policy file. Root-task must access it only as a ring client through bounded block service turns, not a FAT parser or direct SD/MMC driver.
- USB mass storage support, if later admitted, must be a separate optional profile with explicit removal/error semantics, lower priority than USB keyboard/local-seat service, and tests proving keyboard responsiveness under storage I/O.
- No `std` dependencies, no POSIX VFS, no general filesystem.

#### C) Telemetry spool store (append‑only ring log)
- Backing: fixed-size block region/partition serviced by the profile-selected block backend; on Pi 4 hardware that backend is the isolated storage runtime, not root-task SD/MMC code.
- Record format (versioned, bounded): `magic | version | kind | seq | ts | len | crc | payload`.
- Crash rule: a record is valid only if header + checksum validate; partial tail records are ignored.
- Bounded behavior:
  - max record size and deterministic scan budget.
  - explicit policy: **refuse when full** or **overwrite oldest only when acked**.
- NineDoor exposure (names must align with `ARCHITECTURE.md`):
  - `/proc/spool/status` (read-only summary)
  - `/proc/spool/read` (read-only bounded stream)
  - `/queen/spool/append` (queen-only append control, one record per write)
  - `/queen/spool/ack` (queen-only cursor-advance control)
  - Worker-origin telemetry continues through existing role-scoped telemetry paths unless a later milestone explicitly adds a worker-owned spool append path.
- Existing `/bus/<adapter>/spool` semantics remain unchanged and continue to describe host-side sidecar buffering only.

#### D) Settings store (A/B committed pages)
- Two fixed pages/blocks with `generation + checksum`.
- Update semantics: write inactive page fully, validate checksum, then commit by generation.
- Bounded settings size; strict UTF‑8 validation and max key/value lengths (if KV).
- Settings are limited to **runtime-owned local settings**. They explicitly exclude:
  - network mode/interface/static IP/Wi-Fi credentials already persisted by the Pi 4 U-Boot flow in `cohesix.env`
  - manifest-authored defaults or other boot-authoritative policy values already mirrored via `/chosen/cohesix,*`

#### E) Identity binding
- Spool/settings metadata binds to the **manifest fingerprint** from 26 (e.g., recorded in `/proc/boot`), without introducing new trust roots.

#### F) Testing + regression hardening
- Crash‑fault simulation tests for both stores (power loss at every write boundary).
- Fuzz record decoder with strict size limits; reject malformed frames.
- Golden fixture: known block image → expected `status/read/ack` behavior.
- Targeted performance guard for persistence-enabled profiles: measure spool append/read/ack pressure, settings roundtrip latency, and a small REST/status sanity run before and after enabling VM-local persistence. This is a microbenchmark gate only; it must not be treated as a fresh 26b/26d same-harness hardware parity run unless persistence changes the active physical network/runtime path.
- Regression pack additions:
  - `scripts/cohsh/spool_roundtrip.coh`
  - `scripts/cohsh/settings_roundtrip.coh`
- Security and compliance docs must update in the same change to describe VM-local data at rest, retention, erase/rekey behavior, manifest-fingerprint binding, and why persistence does not become a general filesystem.

### Commands
- `cargo test -p root-task`
- `cargo test -p nine-door`
- `cargo test -p pi4-driver-abi`
- `cargo test -p pi4-driver-runtime`
- `cohsh --script scripts/cohsh/spool_roundtrip.coh`
- `cohsh --script scripts/cohsh/settings_roundtrip.coh`
- `python3 scripts/rest_perf_harness.py --mode perf --suite status --runs 5 --log-dir out/bench --log-prefix m27-persistence-status-sanity`

### Checks (DoD)
- Spool append/read/ack semantics are deterministic and bounded; invalid tail records after crash are ignored.
- Store/forward works offline and resumes correctly after reboot.
- Settings updates are atomic across power loss (A/B semantics).
- Runtime settings do not duplicate or override the Pi 4 U-Boot-owned network/Wi-Fi persistence contract.
- Pi 4 default persistence uses only the manifest-declared raw SD/MMC region through the isolated storage runtime. CYW43 SDIO, USB keyboard/local-seat, FAT boot files, `cohesix.env`, and root-owned SD/MMC/MMIO paths are rejected as storage backends.
- No general filesystem or POSIX surface introduced.
- `/proc` remains read-only; spool append/ack writes are accepted only through documented role-scoped control paths.
- VM vs Pi 4 boot profile semantics remain byte‑stable unless explicitly profile‑gated.
- Regression pack passes unchanged; new tests are additive.
- Persistence-enabled benchmark evidence shows bounded spool/settings latency and no material status-read regression against the accepted 26d rolling baseline; any full hardware throughput rerun is required only if the active network/runtime hot path changed.

### Compiler touchpoints
- `coh-rtc` emits persistence limits (record size, max bytes, policy mode), settings bounds, profile storage declarations, and physical storage-runtime descriptors into manifest IR; docs import the generated snippets.
- Manifest validation rejects persistence when storage devices or required isolated storage-runtime descriptors are missing or mis-declared for the selected boot profile, including attempts to bind Pi 4 runtime persistence to CYW43 SDIO, USB local-seat, FAT boot files, U-Boot policy files, or root-owned SD/MMC MMIO.
- Persistence IR is distinct from existing sidecar spool IR; docs and generated clients must keep those surfaces disambiguated.

### Task Breakdown
```
Title/ID: m27-persistence-ir
Goal: Admit persistent spool/settings in compiler IR without overloading existing sidecar spool semantics.
Inputs: tools/coh-rtc, docs/ARCHITECTURE.md, docs/INTERFACES.md.
Changes:
  - tools/coh-rtc/src/ir.rs — `persistence.*` schema, validation, and profile gating.
  - tools/coh-rtc/src/codegen/{docs,rust,cohsh}.rs — generated limits and snippet updates.
Commands:
  - cargo test -p coh-rtc
Checks:
  - Persistence and sidecar spool configs are distinct; invalid storage declarations are rejected.
  - Pi 4 admits `pi4-sdmmc-raw` only when a bounded raw region and isolated storage-runtime descriptor are declared, and rejects CYW43 SDIO, USB local-seat, FAT, root-task SD/MMC, or `cohesix.env` storage bindings.
Deliverables:
  - Compiler-enforced persistence admission with docs snippets refreshed.

Title/ID: m27-pi4-sd-partitioning
Goal: Teach Pi 4 flash tooling to create the M27 boot-plus-raw-persistence SD layout without mounting or formatting the persistence region.
Inputs: scripts/pi4-image-build.sh, docs/HARDWARE_BRINGUP.md, docs/BOOT_REFERENCE.md, persistence IR storage declarations.
Changes:
  - scripts/pi4-image-build.sh — add a persistence-aware partition planner and flasher path that works on macOS and Linux: inspect the target block-device size before any destructive operation, reject undersized media, create the default Pi-compatible partition table with partition 1 as a 1 GiB FAT32 `COHESIX` boot partition and partition 2 as an unformatted raw `COHESIX-PERSIST` role/partition sized from the M27 manifest declaration or conservative default cap, leave any tail outside the declared region unallocated/reserved, copy staged boot assets only to partition 1, and verify both partition geometry and boot-file hashes after flashing.
  - scripts/pi4-image-build.sh — keep existing stage-only behavior unchanged and add a non-destructive dry-run/planner mode for CI that reports the selected partition table, start LBAs, sizes, labels/tags, host tool path (`diskutil` on macOS, `sfdisk`/`parted` plus `mkfs.vfat` on Linux), and the exact reason an SD target would be rejected.
  - docs/HARDWARE_BRINGUP.md + docs/BOOT_REFERENCE.md — document the M27 SD layout, minimum-card-size policy, Mac/Linux host requirements, the fact that partition 2 has no filesystem, and the rule that U-Boot continues to load only from `mmc 0:1` / the FAT boot partition.
  - tests/test_pi4_image_build_partitioning.py or an equivalent shell test — cover the dry-run planner for representative 4 GiB, 8 GiB, 16 GiB, and 32 GiB card sizes on macOS/Linux command profiles without touching real block devices.
Commands:
  - bash -n scripts/pi4-image-build.sh
  - python3 -m pytest tests/test_pi4_image_build_partitioning.py
Checks:
  - Flash tooling never erases before the SD size, selected OS backend, FAT boot geometry, and raw persistence geometry have been validated.
  - Partition 1 remains the only mounted/formatted partition and contains the staged boot assets with verified hashes.
  - Partition 2 is never formatted as FAT/exFAT/ext/ext4, is never used for `cohesix.env`, and is emitted only as the manifest-declared raw region for `pi4-sdmmc-raw`.
  - macOS and Linux dry-run planner output matches for the same byte-size inputs, apart from host command syntax.
Deliverables:
  - Cross-platform Pi 4 SD partitioning/flashing plan and implementation ready for M27 raw persistence admission.

Title/ID: m27-block-hal
Goal: Add bounded block-device plumbing for persistent regions without reintroducing root-owned physical storage drivers.
Inputs: apps/root-task/src/hal/, crates/pi4-driver-abi, apps/pi4-driver-runtime, docs/ARCHITECTURE.md.
Changes:
  - apps/root-task/src/hal/block.rs — block traits, role-selected storage admission, and root-side ring-client binding.
  - apps/root-task/src/storage/layout.rs — persistent region selection and bounds.
  - crates/pi4-driver-abi/src/** — fixed-layout block read/write/flush records and bounded completion evidence for physical storage runtimes.
  - apps/pi4-driver-runtime/src/** — isolated `driver-storage` service for Pi 4 raw SD/MMC block-region access, separate from CYW43 SDIO and U-Boot FAT policy storage.
Commands:
  - cargo test -p root-task --test spool
  - cargo test -p pi4-driver-abi
  - cargo test -p pi4-driver-runtime
Checks:
  - QEMU `virtio-blk` path and Pi 4 isolated runtime `pi4-sdmmc-raw` path resolve the same bounded block contract.
  - Physical Pi 4 tests fail closed if persistence attempts direct root-task SD/MMC MMIO or a missing isolated storage-runtime descriptor.
  - USB mass storage is absent from the default Pi 4 persistence profile and cannot preempt USB keyboard/local-seat service.
Deliverables:
  - HAL storage admission plus isolated runtime block service plumbing for the persistent spool/settings layers.

Title/ID: m27-root-spool-namespace
Goal: Implement persistent spool semantics in root-task and expose read-only `/proc/spool/*` plus role-scoped spool controls via the in-VM NineDoor bridge.
Inputs: apps/root-task/src/ninedoor.rs, docs/ARCHITECTURE.md, docs/INTERFACES.md.
Changes:
  - apps/root-task/src/storage/spool.rs — ring log + checksum validation.
  - apps/root-task/src/ninedoor.rs — `/proc/spool/{status,read}` provider plus `/queen/spool/{append,ack}` policy enforcement.
Commands:
  - cargo test -p root-task --test spool
Checks:
  - Partial tail records are ignored; bounded scan time and ack semantics are enforced in the VM path; `/proc` has no write endpoints.
Deliverables:
  - Authoritative seL4/root-task persistent spool namespace.

Title/ID: m27-host-spool-mirror
Goal: Mirror persistent spool semantics in host `nine-door` for host-mode tests without changing the VM contract owner.
Inputs: apps/nine-door, root-task spool semantics, docs/INTERFACES.md.
Changes:
  - apps/nine-door/src/host/spool.rs — host-mode spool provider.
  - apps/nine-door/src/host/namespace.rs — mount persistent spool provider for tests.
Commands:
  - cargo test -p nine-door --test spool
Checks:
  - Host-mode provider matches root-task spool semantics byte-for-byte for canonical fixtures.
Deliverables:
  - Test mirror of the persistent spool namespace.

Title/ID: m27-settings-store
Goal: Implement A/B settings persistence for runtime-owned local settings only.
Inputs: HAL block traits, docs/ARCHITECTURE.md.
Changes:
  - apps/root-task/src/storage/settings.rs — A/B pages + checksum.
  - docs/ARCHITECTURE.md / docs/INTERFACES.md / docs/SECURITY.md / docs/SECURITY_NIST_800_53.md — explicit data-at-rest posture plus exclusion of U-Boot-owned network/Wi-Fi settings.
Commands:
  - cargo test -p root-task --test settings
Checks:
  - Power‑loss simulations yield either old or new state, never corruption.
  - Settings keys exclude U-Boot-owned network/Wi-Fi fields and manifest-authored boot policy.
Deliverables:
  - Settings store with atomic semantics and a single-source-of-truth boundary.

Title/ID: m27-persistence-regressions
Goal: Add deterministic regression scripts and fixtures.
Inputs: scripts/cohsh/, tests/fixtures/.
Changes:
  - scripts/cohsh/spool_roundtrip.coh — append/read/ack sequence.
  - scripts/cohsh/settings_roundtrip.coh — set/get + A/B markers.
  - docs/BENCHMARKS.md — record persistence-enabled status/spool/settings microbenchmark artifacts without claiming fresh hardware throughput parity.
Commands:
  - cohsh --script scripts/cohsh/spool_roundtrip.coh
  - cohsh --script scripts/cohsh/settings_roundtrip.coh
  - python3 scripts/rest_perf_harness.py --mode perf --suite status --runs 5 --log-dir out/bench --log-prefix m27-persistence-status-sanity
Checks:
  - Scripts pass unchanged; transcripts stable.
  - Status/spool/settings latency stays bounded against the accepted 26d rolling baseline for persistence-enabled profiles, or the delta is classified before 27 closes.
Deliverables:
  - Regression fixtures and targeted persistence benchmark artifacts committed or archived and referenced in docs/TEST_PLAN.md.
```

## Milestone 27b — Formal Verification Baseline + Proof-Carrying Manifests <a id="27b"></a>
[Milestones](#Milestones)

**Why now (assurance):** After Milestone 27 gives Cohesix a persistent, manifest-bound Pi 4/QEMU substrate, later operator, production-authority, AI, and AWS milestones need stronger assurance than regression tests and evidence packs alone. Milestone 27b establishes the first formal-verification baseline around the actual Cohesix authority model: generated manifests, Secure9P/NineDoor bounds, HAL admission, driver-task ABI/resource grants, ticket/namespace confinement, and replayable state transitions. It does not claim full end-to-end formal verification of the whole OS or physical hardware behavior.

**As-built alignment note:** Cohesix already has strong verification hooks: compiler-generated manifests, Secure9P red lines, no-unsafe protocol crates, HAL ownership rules, staged regression plans, evidence packs, and seL4 as the upstream kernel proof base. It does **not** yet have machine-checked Cohesix-specific proof artifacts, proof-carrying manifest witnesses, TLA+/PlusCal state models, Kani/Miri proof jobs, or a CI verification gate that binds those artifacts to generated Rust and docs. Milestone 27b introduces those surfaces; older prose must not describe Cohesix as formally verified until the claims in this milestone have passing evidence.

**Non-negotiable constraints**
- No proof claim may exceed the checked artifact. QEMU proof, Pi 4 hardware evidence, static analysis, bounded model checking, TLA+ state exploration, and inherited seL4 kernel assumptions must remain separate.
- Do not claim full seL4-style refinement proof from Cohesix spec to binary unless that proof exists. The accepted claim for this milestone is bounded, machine-checked verification of selected Cohesix contracts plus explicit assumptions.
- Later milestones may cite 27b only through named claim ids and evidence classes. A downstream production, AI, MCP/A2A, or AWS task must state whether it depends on inherited seL4 assumptions, Cohesix static/generated checks, bounded model checks, state-model exploration, QEMU/staged evidence, fresh Pi 4 evidence, or target-specific AWS evidence.
- Formal models must describe generated/as-built interfaces, not desired future behavior. If a model and generated manifest disagree, the fix is IR/codegen/docs alignment, not weakening the model.
- Verification tooling must be deterministic on macOS ARM64 and CI-friendly; heavyweight tools may be optional locally only if CI has a bounded equivalent or checked artifact.
- No new runtime protocols, namespace roots, ticket semantics, or HAL bypasses are permitted under the banner of verification.
- Verification witnesses are generated artifacts or generated-adjacent evidence; they must not become a hand-maintained second source of truth.
- Physical Pi 4 hardware behavior remains evidence-based. Formal verification can prove the admission/resource/ABI contracts that precede hardware execution, not that a device actually behaves correctly on the board.

### Prerequisite
- Milestone **27** completed for the selected profile, including manifest-bound persistence, generated docs alignment, and deterministic regression evidence.
- Reopened Milestones **26a** and **26b**, plus Milestones **26c** and **26d**, completed or explicitly scoped where their artifacts are inputs to the proof surface: driver-task substrate, HAL admission, isolated runtime descriptors, 26b benchmark parity evidence, target-qualified tests, and refreshed seL4 baseline evidence.

### Goal
Establish a machine-checkable verification baseline for the highest-value Cohesix contracts:
1. manifest/compiler invariants and generated proof witnesses,
2. Secure9P codec/session/path/fid/append-only bounds,
3. ticket, namespace, and role confinement,
4. HAL-only hardware authority and driver-task resource admission,
5. pointer-free, bounded Pi 4 driver-task ABI descriptors,
6. selected Queen/Worker, policy, audit, replay, and persistence state machines,
7. explicit proof assumptions, residual gaps, and non-claims,
8. a claim ladder that downstream milestones can cite without collapsing static proof, model checking, QEMU evidence, Pi 4 evidence, and AWS target evidence into one assurance label.

### Deliverables

#### A) Verification claim register
- Add `docs/FORMAL_VERIFICATION.md` as the canonical register of checked claims, proof assumptions, trusted bases, non-claims, and residual gaps.
- Distinguish:
  - inherited upstream seL4 assumptions,
  - Rust type/memory-safety assumptions,
  - Cohesix static checks,
  - bounded model-checking results,
  - state-model exploration results,
  - QEMU/staged evidence,
  - fresh Pi 4 hardware evidence.
- Define allowed external wording for Cohesix assurance claims so release docs, audit reports, and operator utilities do not overstate proof scope.
- Define a stable claim-ladder vocabulary and claim-id format. Each claim records `claim_id`, `evidence_class`, `trusted_base`, `assumption_ref`, `artifact_ref`, `target_profile`, and `non_claim` fields so later milestones can cite exactly what was proven and what remains evidence-based.

#### B) Proof-carrying manifest witnesses
- Extend `coh-rtc` to emit a deterministic verification witness for each resolved manifest profile, including:
  - Secure9P `msize`, walk-depth, path, and fid constraints,
  - namespace roots, mutability, role permissions, and append-only controls,
  - ticket inventory and role/path/verb authority matrix,
  - HAL storage, MMIO, DMA, IRQ, and driver-task resource grants,
  - driver-image ABI bounds, runtime resource windows, and non-overlap checks,
  - persistence bounds and data-at-rest identity binding,
  - claim ids and evidence-class tags consumed by downstream production, AI, MCP/A2A, Pi 4, and AWS milestone gates.
- Add a verifier that checks the witness against the resolved manifest and generated Rust tables, failing closed on drift.
- Witnesses must be regenerated from IR; hand-editing witness output is invalid.

#### C) Secure9P formal and bounded verification
- Add bounded proofs for `secure9p-codec` and `secure9p-core` covering:
  - length-prefix and `msize` handling,
  - walk depth and path rejection (`..`, invalid UTF-8, NUL where rejected),
  - fid creation, clunk retirement, and no reuse after clunk,
  - tag/window/queue limits,
  - append-only read/write bound helpers,
  - deterministic error mapping for malformed frames.
- Use Kani or an equivalent bounded model checker where practical, plus fuzz corpus regression for parser edge cases.
- Keep existing Rust tests as regression evidence; proofs do not replace fixtures.

#### D) HAL and driver-task authority checks
- Add a static authority checker that rejects direct physical-address discovery, device-untyped retyping, DMA allocation/publish, IRQ binding, cache maintenance, or ad-hoc MMIO outside approved HAL modules.
- Check that driver-task bootstrap grants only declared CSpace, VSpace, endpoint, notification, fault endpoint, stack, IPC, ring, MMIO, DMA, shared-buffer, and IRQ resources from generated manifests.
- Check that driver tasks do not receive Secure9P authority, broad namespace state, ticket secrets, or catch-all `KernelHal` authority.
- Verify `pi4-driver-abi` descriptors are pointer-free, layout-stable, bounded, and incapable of representing overlapping undeclared resource windows.

#### E) State-machine models
- Add small TLA+/PlusCal or equivalent models for:
  - Secure9P session/fid lifecycle,
  - ticket issuance, use, revoke, and denial,
  - Queen/Worker lifecycle and namespace visibility,
  - policy apply/rollback and audit/replay ordering,
  - persistence spool append/read/ack crash behavior,
  - driver-task admission and fail-closed service turns.
- Model outputs must map back to generated manifest fields and checked Rust fixtures. Models that cannot be tied to as-built fields are design notes, not verification closure.

#### F) CI verification gate and evidence archive
- Add `scripts/ci/verification_gate.sh` to run the deterministic verification subset:
  - generated-artifact drift guard,
  - proof-witness generation and verification,
  - Secure9P bounded proofs or CI-approved bounded substitutes,
  - state-model checks at documented bounds,
  - HAL/driver-task authority checker,
  - fuzz corpus regression,
  - existing Rust tests for touched proof surfaces.
- Emit evidence under `out/verification/<run-id>/` with machine-readable summaries and human-readable logs.
- Update `docs/TEST_PLAN.md` so formal verification augments, but does not replace, the staged Test Plan and hardware proof gates.

### Commands
- `scripts/check-generated.sh`
- `cargo test -p coh-rtc`
- `cargo test -p secure9p-codec`
- `cargo test -p secure9p-core`
- `cargo test -p pi4-driver-abi`
- `scripts/ci/verification_gate.sh`

### Checks (DoD)
- `coh-rtc` emits verification witnesses for active QEMU and Pi 4 profiles, and the verifier proves they match generated Rust and resolved manifests.
- Secure9P codec/core proofs or bounded checks pass for the documented contract set.
- HAL/static authority checker has no bypass findings outside approved HAL modules and documented test fixtures.
- Driver-task ABI/resource checks prove no undeclared resource windows, pointer-bearing descriptors, overlapping arenas, or broad authority grants.
- State models run at documented bounds and any counterexample is either fixed or recorded as a blocker with a named later milestone.
- `docs/FORMAL_VERIFICATION.md` states exact claims, assumptions, and non-claims, including that Pi 4 hardware behavior still requires fresh board evidence.
- Verification evidence is reproducible and archived under `out/verification/<run-id>/`.

### Compiler touchpoints
- `coh-rtc` emits proof witnesses from the same IR used for generated Rust, docs snippets, policies, and manifests.
- Manifest schema changes that affect authority, namespace layout, persistence, Secure9P bounds, or driver resources must update the witness schema and verifier in the same change.
- Generated docs may summarize witness contents, but canonical proof truth is the resolved manifest plus generated witness plus verifier output.

### Task Breakdown
```
Title/ID: m27b-claim-register
Goal: Define Cohesix formal-verification claims, assumptions, and non-claims before adding proof tooling.
Inputs: AGENTS.md, docs/BUILD_PLAN.md, docs/ARCHITECTURE.md, docs/SECURITY.md, docs/TEST_PLAN.md.
Changes:
  - docs/FORMAL_VERIFICATION.md — proof scope, trusted bases, checked claims, non-claims, residual gaps, and acceptable release wording.
Commands:
  - git diff --check docs/FORMAL_VERIFICATION.md docs/BUILD_PLAN.md
Checks:
  - The document separates inherited seL4 assumptions, Cohesix machine checks, staged evidence, and Pi 4 hardware proof.
Deliverables:
  - Canonical assurance-claim register for later verification tasks.

Title/ID: m27b-proof-witness-ir
Goal: Generate and verify proof-carrying manifest witnesses from compiler IR.
Inputs: tools/coh-rtc, configs/root_task*.toml, apps/root-task/src/generated, configs/generated/.
Changes:
  - tools/coh-rtc/src/verify.rs — witness schema and verifier.
  - tools/coh-rtc/src/codegen/* — witness emission beside resolved manifests.
  - docs/snippets/* — generated witness summaries where appropriate.
Commands:
  - cargo test -p coh-rtc
  - scripts/check-generated.sh
Checks:
  - Witnesses match resolved manifests and generated Rust; hand-edited or stale witnesses fail closed.
Deliverables:
  - Proof-carrying manifest witness pipeline for QEMU and Pi 4 profiles.

Title/ID: m27b-secure9p-proofs
Goal: Add bounded machine checks for Secure9P codec/session invariants.
Inputs: crates/secure9p-codec, crates/secure9p-core, scripts/cohsh/*, docs/USERLAND_AND_CLI.md.
Changes:
  - crates/secure9p-codec/proofs/ — bounded frame/decoder proof harnesses.
  - crates/secure9p-core/proofs/ — fid/session/window/append-only proof harnesses.
  - docs/FORMAL_VERIFICATION.md — Secure9P proof claim updates.
Commands:
  - cargo test -p secure9p-codec
  - cargo test -p secure9p-core
  - scripts/ci/verification_gate.sh --secure9p-only
Checks:
  - Secure9P red lines are machine-checked and regression fixtures still pass unchanged.
Deliverables:
  - Reproducible Secure9P proof evidence.

Title/ID: m27b-hal-authority-checker
Goal: Enforce HAL-only device authority and driver-task resource confinement statically.
Inputs: apps/root-task/src/hal, apps/root-task/src/cspace, apps/root-task/src/kernel.rs, crates/pi4-driver-abi.
Changes:
  - tools/verify-cohesix/src/hal_authority.rs — static scanner/checker for HAL bypass and broad authority grants.
  - crates/pi4-driver-abi/proofs/ — ABI layout/bounds checks.
Commands:
  - cargo test -p pi4-driver-abi
  - cargo run -p verify-cohesix -- hal-authority --manifest configs/generated/root_task_resolved.json
Checks:
  - MMIO/DMA/IRQ/resource grants appear only through approved HAL paths; driver descriptors are pointer-free and bounded.
Deliverables:
  - HAL and driver-task authority verification gate.

Title/ID: m27b-state-models
Goal: Model and check selected Cohesix authority/state machines.
Inputs: Secure9P session semantics, ticket policy, worker lifecycle, policy/audit/replay, persistence spool, driver-task admission.
Changes:
  - specs/secure9p_session.tla — session/fid lifecycle model.
  - specs/ticket_authority.tla — issue/use/revoke/deny model.
  - specs/driver_task_admission.tla — generated resource grant and fail-closed service model.
  - specs/persistence_spool.tla — crash-safe append/read/ack model.
Commands:
  - scripts/ci/verification_gate.sh --models-only
Checks:
  - Models run at documented bounds and counterexamples are either fixed or recorded as blockers.
Deliverables:
  - State-machine model evidence tied to generated Cohesix fields.

Title/ID: m27b-verification-ci
Goal: Add the deterministic formal-verification gate to CI and the staged Test Plan.
Inputs: scripts/ci/, docs/TEST_PLAN.md, proof harnesses, witness verifier, HAL checker, model runner.
Changes:
  - scripts/ci/verification_gate.sh — deterministic verification runner.
  - docs/TEST_PLAN.md — formal-verification stage and evidence paths.
Commands:
  - scripts/ci/verification_gate.sh
Checks:
  - The gate emits stable evidence under `out/verification/<run-id>/` and fails closed on drift, proof failures, or unsupported proof claims.
Deliverables:
  - CI-ready verification baseline that later milestones can cite.
```


## Milestone 27c — Core-Local Service-Turn Scheduling (SMP Hot-Path Optimization) <a id="27c"></a>
[Milestones](#Milestones)

**Why now (core-local performance with proof):** Milestone 25 established the architectural rule for multicore Cohesix: use isolated seL4 tasks and manifest affinity, not bulky SMP libraries, shared thread pools, or hidden work stealing. Milestone 26b closes the immediate isolated runtime same-harness benchmark parity gate. Milestones 26c, 26d, 27, and 27b add the missing enforcement substrate around generated worker/driver scheduling evidence, seL4 baseline alignment, persistence drains, profile-qualified MCS/non-MCS budget fields, proof witnesses, HAL authority checks, and verification gates. Milestone 27c is the right point to turn affinity placement and the 26b hot-path evidence into compiler-owned core-local service scheduling without weakening authority, replay, or hardware-proof boundaries.

**As-built alignment note:** Cohesix already has manifest affinity, `smp activity`, manifest-declared isolated driver runtime active-slot rules, bounded service-turn language, and host-safe pressure evidence. Milestone 26b owns the first isolated runtime benchmark comparator, same-harness Pi/QEMU parity result, and immediate bounded driver hot-path fixes. Cohesix does **not** yet have compiler-owned core-local service buckets, generated per-core service-turn budgets, per-core telemetry/spool drain policy, IRQ-locality witnesses, or Pi/QEMU evidence proving that hot paths stay local to their assigned core under mixed load. Older prose must not claim core-local hot-path scheduling or multicore throughput closure until this milestone has passing evidence.

**Non-negotiable constraints**
- No POSIX threads, general SMP runtime, async executor, shared work-stealing queue, or bulky SMP library inside the VM.
- No new in-VM protocols, console grammar, Secure9P verbs, namespace authority, or root-owned physical-driver hot paths.
- Authoritative state remains serialized through the authority path; parallelism is only at bounded, manifest-declared service edges.
- Physical hardware driver service remains restricted to manifest-declared isolated driver runtimes. Root-task may admit HAL resources, publish descriptors, and observe counters; it must not regain steady-state device ownership.
- Backpressure is explicit and deterministic: saturated service buckets return bounded busy/overrun evidence, not unbounded queue growth.
- Physical Pi 4 multicore throughput claims require fresh target evidence and must stay separate from shell transport, USB keyboard, Wi-Fi, HDMI, and flash proof lanes.

### Prerequisite
- Milestone **26c** completed for the selected profile, including worker/driver scheduling evidence, notification lifecycle evidence, and the MCS/non-MCS budget distinction.
- Milestone **27b** completed for the selected profile, including proof witnesses, HAL/driver-task authority checks, and verification-gate evidence.
- Reopened Milestones **26a** and **26b**, plus Milestones **26c** and **26d**, completed or explicitly scoped where their artifacts are inputs to Pi 4 driver-runtime, isolated runtime benchmark parity, affinity, seL4 baseline, and target-qualified proof.

### Goal
Convert existing manifest affinity into **core-local, bounded service-turn execution** for workers, manifest-declared isolated driver runtimes, NineDoor/provider paths, telemetry drains, and persistent-spool drains while preserving Cohesix's file-shaped control plane, deterministic ordering, and tiny TCB.

This milestone optimizes the runtime shape; it does not add user-visible capabilities. Its success criteria are lower contention, bounded hot-path latency, deterministic busy/yield evidence, and proof that multicore work stays inside generated authority and scheduling contracts.

### Deliverables

#### A) Compiler-owned core-local service IR
- Extend `coh-rtc` with profile-qualified scheduling fields for:
  - per-core service buckets,
  - per-role and per-driver service-turn budgets,
  - bounded burst size,
  - queue depth,
  - IRQ/locality hints,
  - backpressure policy,
  - telemetry/spool drain assignment.
- Generated manifests identify which core owns each bucket, which roles/drivers feed it, and which counters prove bounded execution.
- Validation rejects:
  - roles assigned to unavailable cores,
  - physical-driver hot paths without isolated runtime ownership,
  - unbounded queues or bursts,
  - overlapping authority that would let a worker or driver bypass its cap bundle or HAL-declared resources.

#### B) Core-local event pumps
- Root-task, NineDoor/provider adapters, workers, and manifest-declared isolated driver runtimes drain only their assigned bucket unless an explicit manifest rule declares a bounded handoff.
- Each service turn has fixed max work, max bytes, and max completions.
- Authority decisions remain serialized; local buckets may prepare, parse, drain, publish counters, and return deterministic busy/yield status.
- Non-MCS profiles expose priority/domain plus service-turn fallback evidence; MCS profiles expose scheduling-context binding, consumed-budget, and timeout evidence.

#### C) Core-local linked-driver hot-path integration
- Build on the Milestone 26b bounded batching and counter evidence by binding GENET, CYW43, SDIO, USB, HDMI, serial, and PCIe service loops to generated service buckets.
- Payload-bearing submits continue to use the staged active-slot path: range validation, staged-byte fingerprint, busy-on-conflict, and no overwrite of an in-flight turn.
- Routine successful dataplane turns must not spam UART or corrupt foreground console output; hot counters are exposed through bounded observability instead.
- IRQ notification, DMA/cache maintenance, service turn, and completion publication stay local to the assigned runtime core wherever the platform profile supports it.

#### D) Sharded telemetry and spool drains
- Per-core telemetry buffers keep producer hot paths local and publish deterministic summaries into the existing namespace.
- Persistent-spool drain policy from Milestone 27 remains authoritative: no general filesystem, no `/proc` mutation, and append/ack semantics remain role-scoped.
- Merge order is deterministic by generated bucket id, sequence, and timestamp fields; lost, dropped, or overwritten records carry explicit bounded evidence.

#### E) Observability, evidence, and verification
- `smp activity` extends from assignment-bucket diagnostics to service-bucket evidence:
  - per-core service turns,
  - budget exhaustions/yields,
  - busy returns,
  - max observed turn latency,
  - queue depth high-water marks,
  - driver/worker bucket membership,
  - IRQ/locality proof where available.
- `/proc/schedule/*`, evidence packs, and generated proof witnesses include the same bounded service-bucket records.
- Verification checks prove the generated bucket layout matches resolved manifests, generated Rust tables, HAL grants, driver-task resources, and cap-bundle authority where enabled.

#### F) Pressure and target-qualified proof lanes
- Host-safe pressure tests validate mixed Secure9P, worker telemetry, driver-task, and spool-drain load without claiming Pi hardware throughput.
- QEMU SMP tests prove semantic stability and contention reduction against the generated schedule evidence.
- Pi 4 tests prove hardware throughput only with fresh logs that also separate flash proof, shell transport, USB/local-seat, Wi-Fi/GENET, HDMI, and SMP service-bucket evidence.
- Full same-harness benchmark closure is required in this milestone because the runtime scheduling model changes: QEMU and Pi 4 REST harness evidence must compare 27c results against the accepted 26d rolling baseline, and every claimed improvement or regression must cite service-bucket counters plus target-qualified proof.

### Commands
- `cargo test -p coh-rtc`
- `cargo test -p root-task --tests schedule`
- `cargo test -p pi4-driver-abi`
- `cargo test -p pi4-driver-runtime`
- `scripts/check-generated.sh`
- `scripts/ci/verification_gate.sh`
- `python3 scripts/rest_perf_harness.py --mode perf --suite all --runs 5 --log-dir out/bench --log-prefix m27c-qemu-service-buckets`
- `python3 scripts/rest_perf_harness.py --mode perf --suite all --runs 5 --no-qemu --no-gateway --rest-url http://<pi4-gateway-host>:<port> --log-dir out/bench --log-prefix m27c-pi4-service-buckets`
- `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m27c-qemu-smp`
- `scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m27c-pi4-smp`

### Checks (DoD)
- Generated manifests and proof witnesses identify every service bucket, owner core, role/driver membership, budget, burst limit, queue bound, and backpressure rule.
- MCS profiles prove scheduling-context binding and consumed-budget evidence; non-MCS profiles prove priority/domain plus bounded service-turn fallback evidence without claiming MCS enforcement.
- Manifest-declared isolated driver runtimes keep payload-bearing work on staged active-slot APIs, return busy on conflicting payloads, and never overwrite in-flight turns.
- Service-bucket integration preserves the Milestone 26b batching and counter bounds while reducing multicore contention without changing ACK/ERR/END, Secure9P, console, worker namespace, or persistent-spool semantics.
- `smp activity` and `/proc/schedule/*` report bounded service-bucket counters with `cpu_pct=unavailable` unless a real kernel-backed utilization source exists.
- Host-safe pressure tests pass and remain classified as semantic/regression evidence, not Pi hardware throughput proof.
- QEMU and Pi 4 target-qualified Test Plan runs pass with no undocumented output drift; Pi 4 throughput claims cite fresh target evidence and keep USB/Wi-Fi/HDMI/shell proof lanes separate.
- Same-harness REST benchmark artifacts show whether service buckets improved, preserved, or regressed the 26d rolling baseline; any tuning remains bounded by generated service-bucket policy and cannot relax backpressure or proof-lane separation.

### Compiler touchpoints
- `coh-rtc` emits service-bucket tables beside existing affinity, worker/driver scheduling, driver-image, persistence, and proof-witness outputs.
- Manifest validation fails closed when service-bucket topology conflicts with authority, HAL resource grants, driver-runtime ownership, cap-bundle records, or persistence bounds.
- Generated docs snippets summarize the service-bucket layout; hand-maintained docs may describe those snippets but must not become the scheduling source of truth.

### Task Breakdown
```
Title/ID: m27c-smp-service-ir
Goal: Add compiler-owned core-local service bucket IR and validation.
Inputs: tools/coh-rtc, configs/root_task*.toml, docs/ROLES_AND_SCHEDULING.md, docs/INTERFACES.md.
Changes:
  - tools/coh-rtc/src/ir.rs — service-bucket schema for core, role/driver membership, budget, burst, queue, IRQ-locality, and backpressure fields.
  - tools/coh-rtc/src/validate.rs — reject unavailable cores, unbounded queues/bursts, physical-driver ownership drift, and authority conflicts.
  - tools/coh-rtc/src/codegen/* — emit generated Rust/docs/proof-witness service-bucket tables.
Commands: cargo test -p coh-rtc && scripts/check-generated.sh
Checks: Service-bucket manifests are generated from IR, invalid topology fails closed, and generated docs match resolved manifests.
Deliverables: Compiler-owned service-bucket topology for QEMU and Pi 4 profiles.

Title/ID: m27c-core-local-event-pumps
Goal: Drain worker, provider, NineDoor, and root-task work through bounded core-local service turns.
Inputs: apps/root-task/src/event, apps/root-task/src/ninedoor.rs, apps/worker-*, apps/root-task/src/generated.
Changes:
  - apps/root-task/src/event/** — service-turn dispatcher keyed by generated bucket id.
  - apps/root-task/src/ninedoor.rs — bounded provider/session drains using generated bucket membership.
  - apps/worker-heart + apps/worker-gpu + apps/worker-lora — keep worker loops within generated service-turn budgets where enabled.
Commands: cargo test -p root-task --tests schedule && cargo test -p worker-heart && cargo test -p worker-gpu && cargo test -p worker-lora
Checks: Each loop respects max work, bytes, completions, and deterministic busy/yield behavior; authority decisions remain serialized.
Deliverables: Core-local event-pump execution without a VM thread pool or work-stealing runtime.

Title/ID: m27c-linked-driver-hotpath-batching
Goal: Bind Milestone 26b manifest-declared isolated driver runtime batching and counters to generated core-local service buckets while preserving staged active-slot semantics.
Inputs: apps/pi4-driver-runtime, crates/pi4-driver-abi, apps/root-task/src/hal/driver_task.rs, docs/DRIVERS.md.
Changes:
  - apps/pi4-driver-runtime/src/** — assign GENET, CYW43, SDIO, USB, HDMI, serial, and PCIe bursts/counters to generated service buckets.
  - crates/pi4-driver-abi/src/** — expose fixed-layout service-bucket membership, counter, and max-turn evidence.
  - apps/root-task/src/hal/driver_task.rs — preserve staged active-slot submit, busy-on-conflict, and completion publication invariants under batching.
Commands: cargo test -p pi4-driver-abi && cargo test -p pi4-driver-runtime && cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib
Checks: Service-bucket integration keeps batching bounded, does not add root-owned physical-driver paths, and cannot overwrite active payload-bearing turns.
Deliverables: Core-local isolated runtime hot paths with contract-local backpressure.

Title/ID: m27c-telemetry-spool-sharded-drain
Goal: Keep telemetry and persistent-spool drains core-local while preserving existing namespace and persistence semantics.
Inputs: apps/root-task/src/storage, apps/root-task/src/ninedoor.rs, docs/ARCHITECTURE.md, docs/INTERFACES.md.
Changes:
  - apps/root-task/src/storage/spool.rs — generated bucket assignment for drain/flush work without changing append/ack semantics.
  - apps/root-task/src/ninedoor.rs — deterministic merge order for per-core telemetry summaries and spool status reads.
  - docs/ARCHITECTURE.md + docs/INTERFACES.md — document per-core drain evidence as observability, not new authority.
Commands: cargo test -p root-task --test spool && cargo test -p nine-door --test spool
Checks: Spool append/read/ack fixtures stay byte-stable; per-core telemetry merge order is deterministic and bounded.
Deliverables: Core-local telemetry/spool drain path that does not become a general filesystem or new protocol.

Title/ID: m27c-smp-observability-and-proof
Goal: Expose service-bucket proof through `smp activity`, `/proc/schedule/*`, evidence packs, and verification witnesses.
Inputs: apps/root-task/src/event/mod.rs, apps/root-task/src/ninedoor.rs, apps/coh/src/evidence.rs, tools/coh-rtc, docs/USERLAND_AND_CLI.md, docs/TEST_PLAN.md.
Changes:
  - apps/root-task/src/event/mod.rs — extend `smp activity` with service-bucket counters, busy/yield counts, max-turn latency, and IRQ-locality proof rows.
  - apps/root-task/src/ninedoor.rs — bounded `/proc/schedule/*` service-bucket summaries.
  - apps/coh/src/evidence.rs — include service-bucket snapshots in evidence packs.
  - tools/coh-rtc/src/verify.rs — verify service-bucket witnesses against resolved manifests and generated Rust tables.
Commands: cargo test -p root-task --tests schedule && cargo test -p coh --test evidence && scripts/ci/verification_gate.sh
Checks: Observability is bounded, read-only, generated-manifest aligned, and keeps `cpu_pct=unavailable` unless real utilization evidence exists.
Deliverables: Auditable service-bucket proof surface for operators and verification gates.

Title/ID: m27c-pressure-and-target-proof
Goal: Add host-safe pressure coverage and target-qualified QEMU/Pi proof lanes for core-local service scheduling.
Inputs: scripts/ci/test_plan_run.sh, docs/TEST_PLAN.md, docs/BENCHMARKS.md, scripts/pi4_trace_normalize.py.
Changes:
  - scripts/ci/test_plan_run.sh — add m27c QEMU and Pi target stages for service-bucket evidence.
  - docs/TEST_PLAN.md — classify host pressure, QEMU semantic proof, and Pi hardware throughput proof separately.
  - docs/BENCHMARKS.md — document required fresh-evidence fields before making throughput claims.
  - scripts/pi4_trace_normalize.py — parse service-bucket counters only as SMP evidence, not USB/Wi-Fi/HDMI acceptance by itself.
Commands:
  - scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m27c-qemu-smp
  - scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m27c-pi4-smp
  - python3 scripts/rest_perf_harness.py --mode perf --suite all --runs 5 --log-dir out/bench --log-prefix m27c-qemu-service-buckets
  - python3 scripts/rest_perf_harness.py --mode perf --suite all --runs 5 --no-qemu --no-gateway --rest-url http://<pi4-gateway-host>:<port> --log-dir out/bench --log-prefix m27c-pi4-service-buckets
Checks: Host pressure stays semantic-only; QEMU proves regression stability; Pi throughput claims require fresh target logs and separated acceptance lanes; same-harness REST artifacts compare service-bucket results to the 26d rolling baseline with service-bucket counters attached.
Deliverables: Repeatable validation and benchmark lanes for core-local SMP optimization.
```


## Milestone 27d — Operator-Lane Scheduler + Multi-Surface Responsiveness <a id="27d"></a>
[Milestones](#Milestones)

**Why now (operator concurrency without a larger TCB):** Milestone 27c turns
manifest affinity into generated service buckets. That is necessary but not
sufficient for field responsiveness: serial, USB local-seat, authenticated TCP
console, HDMI feedback, diagnostics, network progress, telemetry drains, and
persistence work still need an explicit operator-facing fairness contract. 27d
adds that contract as generated lane policy over the 27c service buckets before
Milestone 28 host tools begin presenting pressure and state to operators.

**As-built alignment note:** Cohesix already has a cooperative event pump,
serial/TCP/local-seat console paths, isolated driver runtimes, bounded
service-turn language, `smp activity`, and pressure counters. It does **not**
yet have compiler-owned operator lanes, lane starvation deadlines, lane-aware
large-output resumability, or target-qualified proof that serial, USB keyboard,
TCP responses, HDMI redraws, diagnostics, network control/data, telemetry, and
persistent-spool drains remain responsive under mixed load. Older prose must not
claim Linux-like parallel activity handling until this milestone has evidence.

**Non-negotiable constraints**
- No POSIX threads, in-VM async executor, shared work-stealing queue, or bulky
  runtime library.
- No new in-VM protocol, unaudited RPC path, console grammar drift, Secure9P
  verb change, namespace authority change, or unbounded queue.
- Authoritative command execution remains serialized through the existing
  authority path; 27d improves concurrent I/O progress, buffering, response
  flush, and resumable work around that serialized command boundary.
- Physical hardware service remains restricted to manifest-declared isolated
  driver runtimes. Root-task may schedule bounded service turns and observe
  evidence; it must not regain steady-state device ownership.
- Operator priority is conditional on authenticated TCP-console state, not one
  static ranking. With no authenticated TCP session, service serial input,
  then USB local-seat input, then HDMI feedback. With an authenticated TCP
  session, make TCP command/response progress primary while preserving bounded
  serial/local-seat, emergency-diagnostic, and fatal-status service.
- Under load, active operator input and emergency/fatal status must not be
  hidden behind HDMI redraws, verbose diagnostics, large tails, telemetry spam,
  storage drains, or network proof traffic.
- A "100x better" claim must be grounded in accepted 26d/27c baseline evidence:
  either worst-observed operator input stall improves by two orders of magnitude
  under the same pressure harness, or the milestone records a manifest-declared
  hard latency bound with hardware reasons why a ratio claim is invalid.

### Prerequisite
- Milestone **27c** completed for the selected profile so generated service
  buckets, core assignment, and service-turn counters exist.
- Milestones **26c/26d**, **27**, and **27b** completed or explicitly scoped
  where their artifacts define the selected profile, persistence drains,
  formal witnesses, and rolling performance baseline.

### Goal
Convert the 27c service-bucket substrate into an **operator-lane scheduler**
that preserves Cohesix's single-authority command model while making serial,
USB local-seat, authenticated TCP, HDMI, diagnostics, network, telemetry, and
persistence work progress fairly and observably under mixed load.

### Deliverables

#### A) Compiler-owned operator lane IR
- Extend `coh-rtc` with generated lane records for:
  - `serial-input`
  - `usb-local-seat`
  - `tcp-console-rx`
  - `tcp-console-tx`
  - `network-control`
  - `network-data`
  - `hdmi-display`
  - `diagnostics`
  - `telemetry-spool`
- Each lane declares priority class, starvation deadline, max work per turn,
  max bytes per turn, bounded queue depth, backpressure policy, degradation
  policy, and its generated behavior for TCP-authenticated versus no-TCP
  operator modes. Runtime session state selects between those generated modes;
  clients cannot supply or raise lane priority.
- Validation rejects unbounded lanes, authority overlap, physical-driver lane
  ownership drift, and any lane that can bypass generated service-bucket or HAL
  resource limits.

#### B) Deterministic lane scheduler
- Root-task drains generated lanes using bounded deterministic policy over the
  27c service buckets.
- With no authenticated TCP session, serial input precedes USB local-seat
  input, which precedes HDMI feedback; all three stay above routine network
  data, verbose diagnostics, telemetry, and storage drains.
- With an authenticated TCP session, TCP receive and response flush become the
  primary control-plane shell lanes for bounded `ACK`/`ERR`/`END` liveness, but
  cannot starve serial/local-seat input, emergency diagnostics, or fatal status.
- Saturated lanes return explicit busy/yield/drop evidence rather than growing
  queues.

#### C) Serialized authority with parallel I/O progress
- The console command parser and authority decisions remain single-writer and
  deterministic.
- Serial, local-seat, and TCP input arbitration records which surface supplied
  each command and how conflicting partial lines were handled.
- Long commands, diagnostics, transcript flushes, large `tail` output, and HDMI
  redraws become resumable bounded work items so input polling and response
  flushing can interleave.

#### D) Backpressure and degradation policy
- HDMI redraws coalesce or drop superseded frames before physical input or TCP
  `ACK`/`END` liveness is affected.
- Verbose telemetry, routine progress breadcrumbs, network mirroring, and large
  tails degrade before command liveness.
- Storage/spool drains inherit Milestone 27 persistence semantics and must not
  preempt USB local-seat or serial input.
- Serial and local-seat output includes rate-limited `idle`, `busy`,
  `high-load`, or `overload` summaries plus the strongest known blocker; these
  summaries are bounded lane work and cannot create a new output backlog.
- Network proof traffic remains classified separately from production TCP or
  REST throughput; proof-mode overrides must be explicit and lane-visible.

#### E) Observability and evidence
- `smp activity` adds operator-lane rows: per-lane turns, ready/backlog state,
  max observed latency, starvation yields, busy returns, drops, coalesces, and
  suppression counts.
- `/proc/pressure/*` exposes the same bounded read-only lane-pressure summary.
- Evidence packs include lane snapshots so Milestone 28 tools can explain
  pressure without reconstructing scheduler internals.
- `scripts/pi4_trace_normalize.py` treats lane proof as responsiveness evidence
  only; it must not convert lane counters into USB, Wi-Fi, HDMI, TCP, or flash
  acceptance by itself.

#### F) Target-qualified pressure proof
- Host-safe tests simulate mixed serial, USB, TCP, HDMI, diagnostics, network,
  telemetry, and persistence pressure without making Pi throughput claims.
- QEMU proves transcript stability, bounded lane latency, and no ACK/ERR/END
  drift.
- Pi 4 proof, when claimed, requires fresh logs that keep serial responsiveness,
  USB local-seat, TCP/`cohsh`, Wi-Fi/GENET, HDMI, persistence, and flash proof
  lanes separate.

### Commands
- `cargo test -p coh-rtc`
- `cargo test -p root-task --tests schedule`
- `cargo test -p root-task --test operator_lanes`
- `cargo test -p pi4-driver-abi`
- `cargo test -p pi4-driver-runtime`
- `scripts/check-generated.sh`
- `scripts/ci/verification_gate.sh`
- `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m27d-qemu-lanes`
- `scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m27d-pi4-lanes`
- `python3 scripts/rest_perf_harness.py --mode perf --suite console-pressure --runs 5 --log-dir out/bench --log-prefix m27d-qemu-lanes`

### Checks (DoD)
- Generated manifests and docs identify every operator lane, priority, deadline,
  max work, queue bound, and degradation policy.
- Serial and USB local-seat input have bounded key/line-to-echo and
  line-to-dispatch latency under mixed TCP/HDMI/network/diagnostic/storage
  pressure.
- Authenticated TCP console responses preserve bounded `ACK`/`ERR`/`END`
  liveness without starving physical input or fatal/emergency status.
- No-TCP and authenticated-TCP pressure fixtures prove the generated priority
  transition: serial -> USB local-seat -> HDMI feedback without TCP, and TCP as
  the primary shell with bounded physical/emergency progress when authenticated.
- Serial and local-seat load summaries are rate-limited, bounded, and report
  only `idle`, `busy`, `high-load`, or `overload` plus the strongest blocker.
- Long diagnostics, large tails, HDMI redraws, telemetry drains, and persistence
  drains are resumable and cannot monopolize an event-pump turn.
- Console grammar, Secure9P semantics, worker namespace paths, persistence
  append/ack behavior, and driver-runtime authority remain byte-stable unless a
  separately versioned breaking-change process is followed.
- QEMU and Pi 4 target-qualified Test Plan runs pass with no undocumented output
  drift; Pi 4 claims cite fresh target logs and separated proof lanes.
- Any 100x responsiveness claim cites the accepted 26d/27c baseline, the exact
  pressure harness, and the lane counters that prove the improvement.

### Compiler touchpoints
- `coh-rtc` emits operator-lane tables beside service-bucket, affinity,
  persistence, proof-witness, and cap-bundle outputs.
- Manifest validation fails closed when lane policy conflicts with generated
  authority, HAL grants, driver-runtime ownership, Secure9P bounds, or
  persistence drain semantics.
- Generated docs snippets summarize lane policy; hand-maintained docs may
  explain those snippets but must not become the scheduling source of truth.

### Task Breakdown
```
Title/ID: m27d-operator-lane-ir
Goal: Add generated operator-lane policy and validation.
Inputs: tools/coh-rtc, configs/root_task*.toml, docs/ROLES_AND_SCHEDULING.md, docs/USERLAND_AND_CLI.md.
Changes:
  - tools/coh-rtc/src/ir.rs — lane schema for TCP-authenticated/no-TCP priority modes, deadline, work/byte bounds, queue depth, and degradation policy.
  - tools/coh-rtc/src/validate.rs — reject unbounded lanes, authority overlap, physical-driver ownership drift, and incompatible service-bucket references.
  - tools/coh-rtc/src/codegen/* — emit Rust/docs/proof-witness lane tables.
Commands: cargo test -p coh-rtc && scripts/check-generated.sh
Checks: Lane policy and authenticated-session mode transitions are compiler-owned, generated docs match resolved manifests, clients cannot raise priority, and invalid topology fails closed.
Deliverables: Generated operator-lane contract for QEMU and Pi 4 profiles.

Title/ID: m27d-event-pump-qos
Goal: Schedule serial, USB local-seat, TCP, HDMI, diagnostics, network, telemetry, and spool work through bounded lanes.
Inputs: apps/root-task/src/event/**, apps/root-task/src/local_seat.rs, apps/root-task/src/net/**, apps/root-task/src/storage/**.
Changes:
  - apps/root-task/src/event/** — deterministic lane scheduler over generated service buckets.
  - apps/root-task/src/local_seat.rs — lane-aware keyboard drain, echo, and HDMI coalescing policy.
  - apps/root-task/src/net/** — lane-aware TCP response flush and network-control/data polling.
  - apps/root-task/src/storage/** — persistence drain scheduling that cannot preempt physical input.
Commands: cargo test -p root-task --tests schedule && cargo test -p root-task --test operator_lanes
Checks: No-TCP and authenticated-TCP priority modes match the charter, physical input and TCP response liveness stay bounded, and saturated lanes report busy/yield/drop evidence.
Deliverables: Bounded operator-lane scheduler without a thread pool or new protocol.

Title/ID: m27d-resumable-output-and-diagnostics
Goal: Prevent diagnostics, large tails, HDMI redraws, and transcript flushes from monopolizing turns.
Inputs: apps/root-task/src/event/mod.rs, apps/root-task/src/ninedoor.rs, docs/USERLAND_AND_CLI.md, tests/fixtures/transcripts/.
Changes:
  - apps/root-task/src/event/mod.rs — resumable diagnostic and output jobs with stable ACK/ERR/END behavior.
  - apps/root-task/src/ninedoor.rs — bounded tail/log/read output chunks with lane-visible continuation.
  - tests/fixtures/transcripts/ — pressure transcripts proving byte-stable command results.
Commands: cargo test -p root-task --test operator_lanes && cargo test -p nine-door
Checks: Existing command output remains byte-stable where semantics do not change; large outputs yield between bounded chunks.
Deliverables: Long-running console-visible work that remains responsive under input pressure.

Title/ID: m27d-pressure-observability
Goal: Expose lane pressure through `smp activity`, `/proc/pressure/*`, evidence packs, and trace normalization.
Inputs: apps/root-task/src/event/mod.rs, apps/root-task/src/ninedoor.rs, apps/coh/src/evidence.rs, scripts/pi4_trace_normalize.py.
Changes:
  - apps/root-task/src/event/mod.rs — operator-lane rows in `smp activity`.
  - apps/root-task/src/ninedoor.rs — bounded read-only `/proc/pressure/*` summaries.
  - apps/coh/src/evidence.rs — lane snapshots in evidence packs.
  - scripts/pi4_trace_normalize.py — parse lane proof without treating it as device acceptance.
Commands: cargo test -p root-task --tests schedule && cargo test -p coh --test evidence && pytest tests/test_pi4_trace_normalize.py
Checks: Pressure evidence is bounded, read-only, manifest-aligned, and separated from USB/Wi-Fi/HDMI/TCP acceptance gates.
Deliverables: Operator-visible pressure proof ready for Milestone 28 tooling.

Title/ID: m27d-pressure-and-target-proof
Goal: Add host-safe, QEMU, and Pi 4 pressure gates for multi-surface responsiveness.
Inputs: scripts/ci/test_plan_run.sh, scripts/rest_perf_harness.py, docs/TEST_PLAN.md, docs/BENCHMARKS.md.
Changes:
  - scripts/ci/test_plan_run.sh — m27d QEMU and Pi target stages.
  - scripts/rest_perf_harness.py — console-pressure suite or equivalent bounded pressure harness.
  - docs/TEST_PLAN.md + docs/BENCHMARKS.md — classify lane responsiveness, throughput, and target proof separately.
Commands:
  - scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m27d-qemu-lanes
  - scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m27d-pi4-lanes
  - python3 scripts/rest_perf_harness.py --mode perf --suite console-pressure --runs 5 --log-dir out/bench --log-prefix m27d-qemu-lanes
Checks: QEMU proves semantic stability and latency bounds; Pi 4 claims require fresh target logs and separated serial, USB, TCP, Wi-Fi/GENET, HDMI, persistence, and flash proof lanes.
Deliverables: Repeatable validation for Cohesix multi-surface responsiveness.
```


## Milestone 28 — Operator Utilities: Inspect, Trace, Bundle, Diff, Attest <a id="28"></a>
[Milestones](#Milestones)

**Why now (operator & adoption):**  
After Milestones 26b, 26c, 26d, 27, 27b, 27c, and 27d close, Cohesix should have the Pi 4 isolated runtime benchmark baseline, refreshed seL4 baseline, persistence evidence, formal-verification claim register, core-local SMP service-bucket evidence, and operator-lane pressure evidence needed for read-only operator tooling. Milestone 28 is deliberately read-only: it gives operators and integrators deterministic tools to understand, reproduce, compare, and prove system behavior without expanding the VM TCB or introducing new protocols. Mutating authority hardening remains Milestone 28b, not a hidden prerequisite inside 28.

**As-built alignment note:** `coh evidence pack` and `coh evidence timeline` already exist and are reused here. `coh inspect`, a first-class trace diagnostics command, `coh diff`, `coh attest`, and any `coh bundle` alias are not implemented as of the 26c planning audit and must be added as thin, read-only projections over existing file-shaped state and evidence packs.

This milestone delivers a small, opinionated set of host-side utilities that read existing file-shaped state and artifacts. They do not mutate system state, do not self-heal, and do not bypass policy.

Milestone 28 is a **convergence milestone**. It does not create a second trace format or a second reproducibility artifact. Instead:
- the existing `cohsh-core` / `cohsh` trace format remains the canonical trace substrate;
- the existing `coh evidence pack` / `coh evidence timeline` surface remains the canonical reproducibility pack;
- this milestone fills the missing operator-facing commands and makes those existing foundations compose cleanly.

---

## Goal
Provide a coherent operator toolkit that:
1. Explains current system state (`inspect`)
2. Reuses canonical trace artifacts for deterministic diagnostics (`trace`)
3. Produces self-contained reproducibility artifacts without inventing a second pack format (`bundle`)
4. Compares system state and policy deterministically (`diff`)
5. Verifies device identity and attestation evidence (`attest`)
6. Refreshes audit blocker/risk ledgers from current generated artifacts and evidence packs so later milestones do not inherit stale proof claims.

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
- No second trace recorder/replayer distinct from the existing `cohsh-core` trace stack
- No second bundle/evidence artifact format distinct from `coh evidence pack`

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

### 2) `trace` — Canonical Trace Consumption

**Purpose:**  
Consume the existing deterministic trace format for debugging, testing, and operator diagnostics without defining a parallel recorder.

Capabilities:
- Reuse `.trace` artifacts emitted by the existing `cohsh` / `cohsh-core` stack
- Correlate traces with relevant `/proc/*` and evidence-pack state when available
- Replay traces against:
  - `cohsh`
  - SwarmUI Live Hive
  - read-only field diagnostics surfaces that consume the same shared core

Constraints:
- No live mutation during replay
- Byte-identical ACK/ERR ordering required
- No second trace grammar, schema, or recorder is introduced by this milestone

---

### 3) `bundle` — Canonical Reproducibility Pack

**Purpose:**  
Produce a single, self-contained artifact for bug reports, audits, and incident review by extending the existing `coh evidence pack` / `coh evidence timeline` workflow rather than creating a new pack format.

Bundle contents (bounded):
- Manifest + resolved manifest hash
- Serial log excerpt (if available from the host capture)
- Trace files (if present)
- `/proc` snapshots (inspect-equivalent)
- Spool status summary
- Attestation summary
- When Milestone 28c host-side AI control is enabled, the same canonical evidence pack may additionally include bounded AI run envelopes, checkpoint manifests, retrieval manifests, provider receipts, and prefix-reuse summaries; no AI-specific bundle format is introduced.

Output:
- Deterministic directory or archive layout
- No secrets unless explicitly authorized
- Hash recorded and printed

Canonical surface:
- `coh evidence pack` remains the primary CLI
- If a `coh bundle` alias is later added, it must be a thin alias over the exact same artifact layout and tests

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
- AI run/checkpoint/prefix evidence when present through the canonical evidence-pack layout

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
- Host tools centered on `apps/coh/` with shared read-only internals reusable by `coh-status` and SwarmUI where appropriate
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
- `docs/audit/*`
  - Current blocker ledger, risk baseline, and accepted-risk cross references
- `docs/ARCHITECTURE.md`
  - Operator interaction layer (read-only tools)

---

## Testing & Validation
- Golden output fixtures for each command
- Evidence-pack/bundle → diff → inspect roundtrip tests
- Trace replay regression using the existing canonical trace fixtures
- Attestation positive and negative cases
- Audit ledger consistency checks across blockers, exceptions, findings, and risk baseline files
- Bounded command-latency checks for `inspect`, `diff`, `attest`, and evidence-pack reads over representative live and offline artifacts. This is a host-tool microbenchmark gate, not a QEMU/Pi throughput rerun.
- Tools must operate correctly against:
  - QEMU single-core
  - QEMU multicore
  - Pi 4 boot profile (where applicable)

---

## Checks (Definition of Done)
- All tools produce deterministic output
- No tool mutates system state
- No new protocols introduced
- Trace replay yields byte-identical ACK/ERR using the existing canonical trace format
- Canonical evidence packs/bundles are sufficient for offline diagnosis
- Audit blocker/risk ledgers agree with current generated artifacts and evidence-pack schema before Milestone 28b/28c tasks cite them.
- Read-only operator-tool latency evidence stays bounded on representative artifacts and any regression is classified as parser, evidence-pack, transport, or artifact-size overhead before downstream tools depend on the shared core.
- Documentation reflects as-built behavior

---

## Outcome
After Milestone 28:
- Cohesix is operable, not just correct
- Incidents are explainable and reproducible
- Operators can reason about state without guesswork
- Support and integration costs drop sharply
- The control plane remains small, auditable, and boring
- Milestone 28c reuses the same evidence/timeline/diff substrate rather than introducing an AI-only forensic path

## Sharpened Implementation Sequence
1. Add `coh inspect` as a read-only synthesis over existing `/proc/*`, fleet, and evidence-pack state.
2. Keep `coh evidence pack` / `coh evidence timeline` as the canonical reproducibility artifact; if `bundle` is exposed, make it a thin alias only.
3. Add `coh diff` against live targets and canonical evidence packs.
4. Add `coh attest` over `/proc/boot` + `/proc/attest/*` with trust-anchor validation.
5. Reuse the existing trace substrate for pack correlation and diagnostics; do not add a second trace recorder.
6. Refresh audit ledgers from the current repo state and generated outputs before 28b/28c hardening work cites blocker or exception state.

## Task Breakdown
```
Title/ID: m28-audit-ledger-refresh
Goal: Refresh audit blockers, exceptions, findings, and risk baselines so later hardening milestones cite current state.
Inputs: docs/audit/, scripts/check-generated.sh, docs/BUILD_PLAN.md, current evidence-pack schema.
Changes:
  - docs/audit/BLOCKERS.md — update blocker status and classify remaining issues by milestone owner.
  - docs/audit/EXCEPTIONS.md — remove contradictory `None` state when active exceptions exist and cross-link accepted-risk ids.
  - docs/audit/findings.csv + docs/audit/rust_risk_baseline.toml — align risk ids, counts, and accepted exceptions with current code.
  - docs/TEST_PLAN.md — add audit-ledger consistency check before 28b/28c closure.
Commands: scripts/check-generated.sh && cargo test -p tests --test audit_ledgers
Checks: Blockers, exceptions, findings, and risk baseline agree; stale audit snapshots cannot be cited as current closure evidence.
Deliverables: Current, internally consistent audit ledgers for authority hardening and host-side AI milestones.

Title/ID: m28-readonly-command-latency
Goal: Add bounded latency coverage for read-only operator utilities without treating Milestone 28 as a full runtime benchmark gate.
Inputs: apps/coh, tests/fixtures/traces/, representative evidence packs, docs/TEST_PLAN.md, docs/BENCHMARKS.md.
Changes:
  - apps/coh/tests/operator_latency.rs — deterministic fixture-backed checks for inspect, diff, attest, and evidence-pack read latency.
  - docs/TEST_PLAN.md + docs/BENCHMARKS.md — classify the evidence as host-tool latency, separate from REST/QEMU/Pi throughput proof.
Commands: cargo test -p coh --test operator_latency
Checks: Representative artifact sizes stay within declared parse/read bounds; failures classify parser, evidence-pack, transport, or artifact-size overhead.
Deliverables: Read-only operator utility latency evidence that later field and UI tools can cite without rerunning full hardware benchmarks.
```

## Milestone 28b — Authority Hardening: Delegated REST Identity, Fenced Failover, Idempotent Queen Intents <a id="28b"></a>
[Milestones](#Milestones)

**Why now (risk closure):**  
Milestones 25g and 25h delivered host tickets, federation relay, and bounded WAL behavior. As-built deployments still have two critical exposure points:
1) REST callers collapse into one gateway-attached Queen principal, and
2) failover correctness depends on external fencing discipline rather than deterministic writer fencing in the control plane path.

This milestone closes those gaps using existing as-built mechanisms (`hive-gateway`, `cohsh-core` ticket claims, `/host/tickets/*`, relay WAL, manifest compiler) without introducing new VM protocols or relaxing single-writer semantics.

**As-built alignment note:** The current REST gateway requires a gateway request-auth token for mutating routes, but REST writes still execute through the gateway's configured role/ticket rather than a delegated per-request capability ticket. Host-ticket idempotency by `id + idempotency_key` and relay dedupe exist, but writer-epoch fencing and strict Queen intent dedupe are not yet implemented. Milestone 28b hardens those specific gaps; it must not present current request-auth, relay dedupe, or host-ticket idempotency as delegated REST identity or failover fencing. Because the current upstream console session still authenticates as the gateway role/ticket, 28b must also distinguish **gateway-enforced caller delegation** from any future **VM-verified caller identity** claim.

**Sequencing note:** Milestone 28b closes the host/gateway authority floor required by the Milestone 28b1 coexistence gate and by Milestones 28c, 28d, and 29b. Full VM cap-bundle authority and structured worker/driver fault lifecycle are split into Milestone 28e so the host actuation floor can ship and be audited without bundling every seL4 cap-bundle conversion into the same atomic gate.

---

## Goal
Strengthen authority and failover guarantees while preserving current transport and grammar boundaries:
1. Require caller-scoped delegated authority for mutating REST operations.
2. Make Queen control intents replay-safe with deterministic idempotency keys.
3. Add explicit writer-epoch fencing for failover and relay paths.
4. Eliminate fixture/bootstrap secret usage from release profiles.
5. Ship production profiles with audit/replay enabled and bounded by manifest limits.
6. Establish the mandatory authority floor for Milestone 28b1 provider conformance, Milestone 28c host-side AI actuation, and Milestone 29b AI namespace projections.
7. Harden host-ticket execution so target/arg validation and replay durability are strong enough for host side effects.
8. Harden host bridge and debug surfaces that can otherwise bypass the authority story: GPU bridge authentication/frame bounds and root-console memory diagnostics.
9. Leave full worker/driver cap-bundle authority and structured fault lifecycle as the explicit Milestone 28e follow-on, not a hidden prerequisite inside the host/gateway write-safety gate.

---

## Non-Goals (Explicit)
- No active/active multi-queen writers for one logical hive.
- No in-VM HTTP services or new RPC channels.
- No changes to ACK/ERR/END grammar or Secure9P transport framing.
- No silent replacement of the existing `/queen/ctl` raw-command schema. Any strict intent envelope is introduced through a versioned compatibility path or the full breaking-change process: manifest schema bump, regenerated snippets, updated CLI fixtures, updated docs, and migration notes.
- No best-effort reconciliation loops or autonomous remediation behavior.

---

## Deliverables

### 1) Caller-Scoped REST Delegation (No Undifferentiated Write Authority)
**Purpose:** Preserve gateway multiplexing while restoring caller-level capability boundaries.

Implementation requirements:
- Mutating REST routes (`/v1/fs/echo` and equivalent write paths) require:
  - gateway request-auth token, and
  - delegated capability ticket header (`x-cohesix-ticket`), validated using existing ticket claims rules.
- Gateway maintains caller identity and quota state keyed by delegated ticket identity (hash). The gateway's configured upstream role/ticket is a transport-authority ceiling, not the caller identity: a request is admitted only when the delegated ticket is valid and the concrete path/action is within both the delegated claims and the gateway ceiling.
- If the VM console transport remains single-client or single-writer for the selected profile, the gateway serializes writes over the existing authenticated console path rather than opening parallel sessions or reattaching a shared session between callers. Audit/evidence records bind the delegated ticket hash, gateway credential class, concrete path/action, and upstream `ACK`/`ERR` result.
- Delegated ticket claims (`role`, `subject`, `mount scopes`, `budgets`) constrain what each REST caller can mutate. Gateway-enforced delegation must not be described as VM-verified caller identity unless a separately versioned VM envelope/path actually carries and verifies the caller binding.
- Read-only REST routes may remain gateway-role scoped in compatibility mode. The delegated-ticket requirement for mutating REST paths is an authority-contract change and must update `docs/API_GUIDELINES.md`, `docs/HOST_API.md`, OpenAPI fixtures, CLI/REST tests, and release notes in the same milestone work.

As-built leverage:
- Reuse `cohsh-core` ticket parsing/validation and existing ticket quota semantics.
- Reuse deterministic permission errors (`EPERM`, `ELIMIT`) and existing audit logging surfaces.

---

### 2) Idempotent Queen Control Grammar
**Purpose:** Ensure retries/replays never duplicate side effects for Queen control intents while preserving legacy `/queen/ctl` compatibility unless a breaking-change path is taken.

Implementation requirements:
- Introduce strict envelope schema for idempotent Queen intents:
  - required: `schema`, `id`, `idempotency_key`, `issued_unix_ms`, `cmd`.
- Preserve existing `/queen/ctl` raw-command behavior in compatibility profiles, or introduce a new versioned path such as `/queen/intents/ctl` for strict envelopes. Production profiles may require strict envelopes only after the generated manifest schema and fixtures record that breaking authority posture.
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

### 6) Host Ticket Validation and Durable Execution
**Purpose:** Keep `/host/tickets/spec` as a safe host-side actuation lane rather than an under-validated side-effect queue.

Implementation requirements:
- Introduce provider-specific validated newtypes for host ticket targets, args, ids, and action fields.
- Reject empty components, `..`, slash-bearing identifiers where a single token is expected, control characters, overlong values, leading option-like values where they become argv operands, and unsupported provider-specific fields.
- Enforce manifest action allowlists before executor dispatch, not only after schema parsing.
- Add durable execution WAL states such as `prepared`, `executed`, and `terminal`, keyed by `id + idempotency_key`.
- A crash or writeback failure after host side effects must not cause silent duplicate execution on restart; recovery must either publish the terminal receipt or deadletter with explicit replay state.

As-built leverage:
- Reuse the existing host-ticket schema, relay WAL, receipts, and idempotency key model.

---

### 7) Host Bridge Auth/Frame Caps and Debug Memory Gates
**Purpose:** Remove authority bypasses from host bridge and debug surfaces before they become production dependencies.

Implementation requirements:
- `gpu-bridge-host` must reject placeholder auth such as `changeme` outside explicit mock/test modes and require a configured token source for live publish paths.
- `gpu-bridge-host` console/REST clients must cap frame length before allocating buffers from peer-controlled sizes; the cap must be documented and tested.
- Root-console memory diagnostics such as arbitrary-address hexdump must be disabled in release/production profiles or restricted to HAL-classified diagnostic ranges.
- Documentation must distinguish emergency bring-up diagnostics from production operator surfaces.

As-built leverage:
- Reuse existing cohsh placeholder-auth rejection patterns, Secure9P/console frame limits, HAL device coverage checks, and test-plan profile gates.

---

### 8) VM Cap-Bundle/Fault Follow-on Boundary
**Purpose:** Keep 28b atomic around host/gateway authority while preserving the required VM authority hardening path.

Implementation requirements:
- Milestone 28b must emit generated profile gates and documentation that distinguish:
  - host tickets, REST delegated tickets, and provider/PEFT tickets as host authority records;
  - 26c endpoint-cap-backed VM worker tickets as the compatibility floor;
  - full worker/driver seL4 cap bundles and structured fault lifecycle as Milestone 28e requirements.
- Target profiles must fail validation if they claim full cap-bundle ticket authority or structured worker/driver fault containment before Milestone 28e evidence exists.
- Milestones 28c, 28d, and 29b may depend on 28b host/gateway delegated identity, idempotency, fencing, audit/replay, and host-ticket durability, but must not cite full cap-bundle or fault-lifecycle closure until Milestone 28e is complete.

As-built leverage:
- Reuse 26c endpoint-cap terminology, current worker/driver scheduling evidence, and 28b audit/replay profile gates without overclaiming full seL4 cap-bundle authority.

---

### 9) Gateway authority performance guard
**Purpose:** Prove delegated REST identity, idempotency, and writer-epoch fencing do not create hidden gateway backpressure or push operators toward unsafe retry masking.

Implementation requirements:
- Add a targeted gateway benchmark for read-only REST status, delegated mutating REST writes, duplicate-idempotency refusals, and stale-writer refusals.
- Report p50/p95 latency, error/refusal counts, broker queue depth, backpressure responses, delegated-ticket cache behavior, and audit-line emission cost.
- Compare against the accepted 26d rolling baseline for equivalent REST status reads and against a pre-28b local gateway authority baseline for write-path overhead.
- This is a gateway/auth microbenchmark only. It must not be counted as fresh Pi hardware throughput proof unless the benchmark exposes a runtime-path regression that requires a full same-harness rerun.

---

## Commands
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`
- `cargo test -p hive-gateway`
- `cargo test -p coh`
- `cargo test -p cohsh`
- `cargo test -p gpu-bridge-host`
- `cargo test -p host-ticket-agent`
- `cargo test -p root-task`
- `cargo test -p tests --test host_ticket_agent`
- `cargo test -p tests --test failover`
- `cargo test -p coh-rtc`
- `scripts/cohsh/run_regression_batch.sh`
- `scripts/ci/gateway_perf_probe.sh --scenario delegated-rest-authority --state-dir out/bench/m28b-gateway-authority`

---

## Checks (Definition of Done)
- Mutating REST request without delegated ticket is denied deterministically and audited.
- Delegated caller cannot exceed ticket scopes/quotas even when gateway is multiplexing many clients.
- Generated profile state and evidence distinguish gateway-enforced delegation from VM-verified caller identity; the default single-console projection cannot claim the latter.
- Duplicate strict Queen intent (`id + idempotency_key`) never repeats side effects.
- Legacy `/queen/ctl` fixtures either continue to pass through compatibility mode or are updated under the full breaking-change process with schema-version evidence.
- Stale writer epoch is rejected deterministically across local and relayed host tickets.
- Production manifest/profile fails validation if fixture/default secrets are present.
- Production profile surfaces `/audit/*` and `/replay/*`; evidence packs include epoch and dedupe state.
- Host ticket targets and args are provider-validated before side effects; unsupported or ambiguous values fail deterministically with no executor call.
- Host ticket crash/restart tests prove no silent duplicate host side effects after an executor succeeds but status writeback fails.
- `gpu-bridge-host` rejects placeholder auth in live mode and refuses oversized peer frames before allocation.
- Root-console arbitrary memory diagnostics are unavailable in release/production profiles or constrained to HAL-classified diagnostic ranges.
- Generated profiles and docs do not claim full worker/driver cap-bundle authority or structured fault containment until Milestone 28e evidence exists.
- Milestone 28c host-side AI control cannot be enabled in target profiles unless delegated REST identity, writer-epoch fencing, and audit/replay requirements are all active.
- Regression pack passes unchanged in compatibility mode; any production-mode fixture update caused by delegated REST identity or strict Queen intent envelopes follows the documented breaking-change process.
- Gateway authority benchmark evidence shows delegated REST identity, idempotency, writer-epoch refusal, and audit/replay emission stay bounded; any material status/read or write-path regression is classified before downstream MCP/A2A or AI host-control milestones depend on the gateway.

---

## Compiler touchpoints
- `coh-rtc` schema/version update for:
  - delegated-ticket enforcement policy on REST mutating paths,
  - delegated-identity claim class (`gateway_enforced` versus a separately evidenced `vm_verified` path) and gateway transport-authority ceiling,
  - idempotent Queen intent envelope schema, compatibility mode, and dedupe bounds,
  - writer-epoch fencing policy and relay requirements,
  - production secret references,
  - audit/replay required defaults for release profiles,
  - host-ticket provider target/arg grammars and execution WAL policy,
  - live host-bridge auth and frame-length limits,
  - profile-gated debug diagnostic policy,
  - deferred full-cap-bundle and structured-fault profile gates consumed by Milestone 28e,
  - provider-registry and host-AI enablement dependency gates consumed by Milestone 28b1, Milestone 28c, and Milestone 29b.
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
  - apps/hive-gateway/src/main.rs — require x-cohesix-ticket on mutating routes, intersect delegated scope with the gateway transport-authority ceiling, serialize the single upstream console session, and correlate each write with its upstream ACK/ERR result.
  - apps/hive-gateway/src/auth.rs — delegated ticket validation + bounded cache keyed by ticket hash, with explicit gateway-enforced versus VM-verified claim classification.
  - apps/coh/src/rest.rs — pass delegated ticket header for mutating REST calls.
  - apps/cohsh/src/transport/rest.rs — propagate delegated ticket on write paths.
  - docs/HOST_API.md + docs/API_GUIDELINES.md + resources/openapi/hive-gateway.yaml — version and document the delegated REST authority contract.
Commands: cargo test -p hive-gateway && cargo test -p coh && cargo test -p cohsh
Checks: Writes without delegated ticket fail deterministically; writes with scoped tickets succeed only within the intersection of caller claims and gateway ceiling; single-console profiles do not claim VM-verified caller identity.
Deliverables: Gateway writes are caller-scoped and attributable without overstating the identity visible to the VM.

Title/ID: m28b-queen-ctl-idempotency
Goal: Add deterministic idempotency for Queen intents without silently breaking legacy /queen/ctl fixtures.
Inputs: apps/root-task, apps/nine-door, docs/INTERFACES.md
Changes:
  - apps/root-task/src/control/queen_ctl.rs — strict envelope parser with required id/idempotency_key and dedupe guard for the versioned intent path or compatibility-gated /queen/ctl mode.
  - apps/root-task/src/control/dedupe.rs — bounded dedupe table with deterministic eviction and audit lines.
  - apps/nine-door/src/host/proc.rs — read-only dedupe status surface for operators.
Commands: cargo test -p root-task && cargo test -p nine-door
Checks: Duplicate intent never repeats side effects; deterministic audit and /proc visibility prove dedupe behavior; legacy raw /queen/ctl behavior is either preserved or changed only with schema-bump fixtures.
Deliverables: Replay-safe Queen intent grammar with bounded dedupe state and explicit compatibility posture.

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
Commands: cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json && scripts/ci/due_diligence_gate.sh
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

Title/ID: m28b-host-ticket-validation-replay
Goal: Harden host-ticket target/arg validation and make host side-effect replay durable across crashes.
Inputs: apps/host-ticket-agent, docs/INTERFACES.md, docs/SECURITY.md, docs/FAILOVER.md
Changes:
  - apps/host-ticket-agent/src/claim.rs — parse provider-specific target and arg newtypes with strict component, length, and token validation.
  - apps/host-ticket-agent/src/executors/mod.rs — dispatch only after manifest allowlist and provider grammar validation pass.
  - apps/host-ticket-agent/src/wal.rs — record prepared/executed/terminal states keyed by id + idempotency_key.
  - apps/host-ticket-agent/tests/replay.rs — simulate crash after executor success but before status writeback.
  - docs/INTERFACES.md + docs/SECURITY.md — document host-ticket validation, replay states, and deterministic refusal semantics.
Commands: cargo test -p host-ticket-agent && cargo test -p tests --test host_ticket_agent
Checks: Invalid target/arg values never reach executors; crash/restart replay publishes or deadletters existing execution state without duplicating side effects.
Deliverables: `/host/tickets/spec` is a durable, grammar-checked actuation lane rather than a best-effort command queue.

Title/ID: m28b-gpu-bridge-auth-frame-caps
Goal: Enforce live GPU bridge auth discipline and bounded peer frame allocation.
Inputs: apps/gpu-bridge-host, docs/HOST_TOOLS.md, docs/GPU_NODES.md, docs/SECURITY.md
Changes:
  - apps/gpu-bridge-host/src/main.rs — reject placeholder auth outside mock/test mode and require explicit token/env resolution for live publish.
  - apps/gpu-bridge-host/src/console.rs — cap length-prefixed frames before allocation and return deterministic errors on oversize input.
  - apps/gpu-bridge-host/tests/auth_frame_bounds.rs — placeholder-auth and oversized-frame negative tests.
  - docs/HOST_TOOLS.md + docs/GPU_NODES.md — document live auth requirements and frame-size limits.
Commands: cargo test -p gpu-bridge-host && cargo test -p coh --features mock
Checks: Live bridge cannot start with placeholder auth; malicious or corrupted frame lengths are rejected without unbounded allocation.
Deliverables: GPU bridge remains a bounded host-side projection of Cohesix authority.

Title/ID: m28b-console-debug-memory-gate
Goal: Gate arbitrary memory diagnostics behind explicit bring-up policy and HAL-classified ranges.
Inputs: apps/root-task/src/kernel.rs, apps/root-task/src/hal, docs/SECURITY.md, docs/HARDWARE_BRINGUP.md
Changes:
  - apps/root-task/src/kernel.rs — compile or profile gate raw hexdump commands out of production/release console builds.
  - apps/root-task/src/hal/mod.rs — expose diagnostic-range classification for allowed bring-up memory reads where needed.
  - apps/root-task/tests/console_debug_gate.rs — prove release profile rejects hexdump and bring-up profile bounds addresses/lengths.
  - docs/SECURITY.md + docs/HARDWARE_BRINGUP.md — document emergency diagnostic scope and production refusal behavior.
Commands: cargo test -p root-task --test console_debug_gate && cargo test -p root-task
Checks: Production console cannot read arbitrary memory; bring-up diagnostics are range-bounded and audited.
Deliverables: Debug memory access is no longer an unbounded operator surface.

Title/ID: m28b-deferred-vm-authority-gates
Goal: Add explicit validation/docs gates that reserve full worker/driver cap-bundle and structured-fault claims for Milestone 28e.
Inputs: tools/coh-rtc/src/**, docs/WORKER_TICKETS.md, docs/SECURITY.md, docs/TEST_PLAN.md, docs/BUILD_PLAN.md.
Changes:
  - tools/coh-rtc/src/** — profile flags for 26c endpoint-cap compatibility, 28b host-authority readiness, and 28e full-cap-bundle/fault-lifecycle closure.
  - docs/WORKER_TICKETS.md + docs/SECURITY.md + docs/TEST_PLAN.md — document that full cap bundles and structured fault containment remain pending until 28e evidence exists.
Commands:
  - cargo test -p coh-rtc
  - scripts/check-generated.sh
Checks:
  - Profiles cannot claim full worker/driver cap-bundle or structured fault lifecycle authority from 28b alone.
Deliverables:
  - Honest dependency gate for 28c/28d/29b host actuation work and 28e VM authority closure.

Title/ID: m28b-gateway-authority-performance
Goal: Prove delegated REST identity, idempotency, writer-epoch fencing, and audit/replay emission stay bounded in the gateway path.
Inputs: apps/hive-gateway, apps/host-ticket-agent, scripts/rest_perf_harness.py, docs/BENCHMARKS.md, docs/TEST_PLAN.md.
Changes:
  - scripts/ci/gateway_perf_probe.sh — targeted gateway benchmark scenarios for read-only status, delegated writes, duplicate-idempotency refusal, stale-writer refusal, and audit/replay emission cost.
  - docs/BENCHMARKS.md + docs/TEST_PLAN.md — classify gateway authority benchmark evidence separately from Pi/QEMU throughput proof.
Commands:
  - scripts/ci/gateway_perf_probe.sh --scenario delegated-rest-authority --state-dir out/bench/m28b-gateway-authority
Checks:
  - Read/status latency, delegated write latency, refusal counts, broker queue depth, ticket-cache behavior, and audit emission cost remain bounded or are classified before 28c/28d depend on the gateway.
  - Probe evidence preserves no-retry accounting and does not count as fresh Pi hardware throughput proof unless it exposes a runtime-path regression that triggers the full REST harness.
Deliverables:
  - Gateway authority performance ledger for downstream MCP/A2A and AI-host-control milestones.
```

---

## Outcome
After Milestone 28b:
- REST multiplexing retains convenience without collapsing caller identity.
- Failover safety relies on deterministic writer fencing, not operator luck.
- Queen control retries are safe by construction.
- Release profiles enforce key hygiene and enable audit-grade reconstruction by default.
- Production coexistence is still not complete until Milestone 28b1 turns the hardened authority floor into provider schemas, identity mapping, deployment artifacts, and conformance evidence.
- Host-side AI supervisors gain no special bypass: they inherit delegated identity, idempotency, fencing, and audit/replay before any live actuation is allowed.
- Worker/driver full cap-bundle authority and structured fault lifecycle remain explicitly pending for Milestone 28e rather than being buried inside the host/gateway authority floor.

## Milestone 28b1 — Provider Action Registry + Ecosystem Coexistence Conformance <a id="28b1"></a>
[Milestones](#Milestones)

**Why now (coexistence floor):**
Milestone 25g delivered the host-ticket mechanism and high-value adapters. Milestone 28b makes writes attributable, replay-safe, fenced, durable, and audit-first. The remaining adoption gap is broader: Cohesix needs one compiler-owned provider/action contract, real conformance evidence for each ecosystem it claims to coexist with, and installable host-side deployment shapes that do not smuggle new authority or heavy stacks into the VM. This milestone turns the hardened authority floor into a production coexistence gate before AI run control, MCP/A2A, or AI namespace work can depend on those providers.

**As-built alignment note:** Current host tools already project GPU/CUDA inventory, PEFT/model lifecycle, host sidecar state, host tickets, REST gateway access, evidence packs, and SIEM exports. Kubernetes, systemd, Docker, NVIDIA/CUDA/NVML, PEFT, and federation flows have useful fixtures and mock/dry-run coverage. Cohesix does **not** yet have one generated provider action registry shared by host-ticket-agent, REST, Python, MCP/A2A, docs, and tests; it does not have an ecosystem-wide conformance matrix; and it does not have production install bundles, identity-federation mappings, Prometheus/OpenTelemetry projections, or a use-case evidence matrix that separates supported, mock-only, and explicitly unsupported integrations.

**Prerequisites**
- Milestone **28** completed for read-only inspect, attest, evidence, and audit-ledger refresh.
- Milestone **28b** completed for delegated REST identity, idempotent intents, writer-epoch fencing, production audit/replay defaults, host-ticket durable execution, live bridge auth/frame caps, and production debug gates.
- Milestone **25g** and **25h** remain feature-complete inputs, but production coexistence claims must cite this milestone's conformance evidence, not only the earlier ticket/relay implementation.

**Goal**
Define and prove the production coexistence contract:
1. Generate one provider/action registry from compiler IR and use it everywhere host-side actuation is described, validated, displayed, or exposed.
2. Map enterprise, Kubernetes, and host identities into delegated Cohesix tickets without making an identity provider part of the VM TCB.
3. Prove provider conformance for systemd, Docker, Kubernetes, NVIDIA/CUDA/NVML, PEFT/model registry, SIEM, Prometheus/OpenTelemetry, and staged OT/industry sidecars.
4. Package gateway, host-ticket-agent, sidecar bridges, GPU bridge, evidence exporters, and doctor checks as installable host-side bundles for realistic coexistence deployments.
5. Add a use-case evidence matrix so docs cannot claim an ecosystem integration without code, generated policy, tests, deployment instructions, and evidence-pack reconstruction.
6. Classify every ecosystem-facing read projection as public, delegated-ticket scoped, or admin-only before REST, MCP, A2A, FUSE, Python, or UI clients can expose it.

**Non-Goals (Explicit)**
- No in-VM Kubernetes, Docker, systemd, CUDA/NVML, PEFT, NeMo, Prometheus, OpenTelemetry, DICOM, CCSDS, MODBUS, CAN, DNP3, IEC-104, or other ecosystem runtime.
- No POSIX compatibility layer, libc facade, process supervisor, shell command executor, package manager, or mutable general-purpose filesystem inside Cohesix.
- No direct host executor calls from REST, Python, MCP, A2A, FUSE, or UI code. Side effects still resolve to validated Cohesix control files or `/host/tickets/spec`.
- No provider-specific schema maintained only in docs, Python, OpenAPI, MCP, or an executor crate. Provider action truth comes from generated manifests and checked registry output.
- No write-capable OT/industry sidecar action until the corresponding read-only evidence, safety policy, failure mapping, and operator approval flow are proven.
- No identity claim, group, service account, or model prompt text is authorization by itself. All authority resolves to delegated Cohesix tickets and manifest-scoped actions.

## Deliverables

### 1) Generated provider action registry
**Purpose:** Make provider actions one compiler-owned contract instead of a set of hand-aligned adapters.

Implementation requirements:
- Extend `coh-rtc` with `providers.*` IR for:
  - provider id, version, enablement profile, action names, target selectors, dry-run/live mode, idempotency requirements, writer-epoch requirements, policy approval requirements, receipt schema, redaction rules, and evidence refs.
  - initial families: `systemd`, `docker`, `k8s`, `nvidia`, `gpu.lease`, `peft`, `model_registry`, `siem`, `prometheus`, `otel`, `modbus`, `dnp3`, `can`, `iec104`, `dicom`, and `ccsds`.
  - optional future families may be reserved but must generate unavailable status until an implementation and tests exist.
- Generate or check the registry for:
  - `host-ticket-agent` validators/executors,
  - `coh` and `cohsh` host-tool help,
  - Python SDK integration defaults,
  - OpenAPI/REST docs,
  - later MCP tool schemas and A2A skill schemas,
  - docs snippets and test fixtures.
- Reject provider actions with free-form shell commands, arbitrary host paths, raw provider credentials, unbounded payloads, or target grammars that cannot be validated before executor dispatch.

### 2) Ecosystem conformance matrix
**Purpose:** Distinguish real coexistence from mock-only or aspirational support.

Implementation requirements:
- Add a checked matrix covering each enabled provider:
  - supported host OS/profile,
  - provider version bounds or unavailable state,
  - auth mode,
  - mock, dry-run, and live-safe coverage,
  - negative cases,
  - expected receipt fields,
  - evidence-pack reconstruction path,
  - failure and rollback behavior.
- Minimum acceptance families:
  - systemd status/start/stop/restart,
  - Docker status/stop/restart,
  - Kubernetes cordon/drain/lease-sync/RBAC-to-ticket translation,
  - NVIDIA/CUDA/NVML inventory and GPU lease projection,
  - PEFT/model registry import/activate/rollback receipts,
  - SIEM export,
  - Prometheus/OpenTelemetry read-only export,
  - MODBUS/DNP3 read-only sidecar parity from Milestone 18,
  - CAN/IEC-104/DICOM/CCSDS explicitly staged as read-only or not-enabled until their bridge contracts land.
- Unsupported providers must produce deterministic `not_enabled` or `not_implemented` diagnostics; silent omission is not acceptable for documented use cases.

### 3) Read visibility classification
**Purpose:** Ensure read-only projections do not leak cross-caller state.

Implementation requirements:
- Generate or check a visibility class for every provider, evidence, ticket, audit, replay, telemetry, and `/proc` read exposed to REST, Python, FUSE, UI, later MCP, or later A2A surfaces:
  - `public` for non-sensitive version/capability summaries,
  - `ticket_scoped` for caller-owned tickets, receipts, provider status, run/checkpoint state, and evidence refs,
  - `admin_only` for global provider state, deadletters, audit/replay internals, identity-mapping diagnostics, and raw conformance snapshots.
- Read-only REST compatibility mode may remain gateway-role scoped only for paths explicitly classified as public or admin-only under the configured gateway role. Ticket-scoped reads require delegated identity before they can be exposed through multi-caller surfaces.
- Negative tests must prove a caller cannot read another caller's host tickets, provider receipts, evidence artifacts, task state, or identity-mapping diagnostics through REST, Python, FUSE, MCP, A2A, or UI projections.

### 4) Enterprise identity and subject mapping
**Purpose:** Let existing identity systems coexist with Cohesix without entering the VM TCB.

Implementation requirements:
- Add host-side subject mappers for OIDC/JWT claims, SPIFFE IDs, Kubernetes service accounts/RBAC, and local host identities where enabled.
- Mappers produce delegated Cohesix ticket requests or ticket references; they do not mint authority outside the existing Cohesix ticket rules.
- Generated policy defines accepted issuer/audience/subject/group mappings, TTL bounds, role scopes, provider action scopes, and audit subject normalization.
- Forged, expired, overbroad, wrong-audience, or unmapped claims fail before any mutation reaches REST, `/queen/ctl`, or `/host/tickets/spec`.

### 5) Packaging and deployment profiles
**Purpose:** Make coexistence deployable without bespoke host assembly.

Implementation requirements:
- Add installable host-side profiles for:
  - local macOS operator,
  - Linux/systemd edge node,
  - GPU host/Jetson-compatible host sidecar mode,
  - Kubernetes-adjacent gateway/agent deployment using Helm, Kustomize, or plain YAML.
- Packages may install `hive-gateway`, `host-ticket-agent`, `host-sidecar-bridge`, `gpu-bridge-host`, evidence exporters, and doctor checks; they must not add VM services or in-VM ecosystem stacks.
- Systemd units, launchd examples, container manifests, and Kubernetes manifests must run with least privilege, explicit secret refs, loopback-by-default network posture, and documented non-loopback risk overrides.
- `coh doctor` reports installed provider status, missing optional providers, auth configuration, ticket scope, registry version, evidence export availability, and packaging drift.

### 6) Observability and compliance projections
**Purpose:** Fit existing monitoring/compliance systems without replacing them.

Implementation requirements:
- Prometheus and OpenTelemetry exporters are host-side read-only projections from `/proc`, audit, replay, telemetry, provider receipts, and evidence-pack summaries.
- SIEM exports remain bounded, redacted, schema-versioned JSONL/NDJSON with stable field names.
- Exporters must not expose raw tickets, secret refs, provider credentials, large payloads, PHI, model weights, gradients, or prompt transcripts unless a documented profile explicitly authorizes the field and redaction policy.
- Evidence packs include provider registry version, conformance matrix snapshot, identity-mapping decision refs, packaging profile, exporter schema version, and provider receipt correlation.

### 7) OT and industry sidecar staging
**Purpose:** Keep industrial/health/science integrations as host-side bridges with explicit safety posture.

Implementation requirements:
- MODBUS, DNP3, CAN, IEC-104, DICOM, and CCSDS adapters are host-side sidecars only.
- Read-only inventory/status/event capture must land before write/control actions.
- Any write-capable action requires a separate provider action, explicit policy approval, target grammar, safety interlock description, dry-run fixture, live-safe refusal case, and evidence-pack reconstruction.
- DICOM and other sensitive-domain adapters must default to metadata and evidence refs only; raw PHI or large payload publication is disabled unless a later profile explicitly approves the redaction and retention model.

### 8) Use-case evidence matrix and release gate
**Purpose:** Prevent documentation from outrunning implementation.

Implementation requirements:
- Add a generated or checked `docs/USE_CASES` mapping table that classifies each use case as:
  - implemented and production-proven,
  - mock/dry-run only,
  - read-only only,
  - explicitly not enabled,
  - future milestone.
- Each production-proven row must link to:
  - provider registry action(s),
  - delegated authority requirement,
  - package/deployment profile,
  - identity mapping where relevant,
  - observability/export surface,
  - failure-mode fixture,
  - evidence-pack reconstruction path.
- Release gates fail if public docs claim production coexistence for an ecosystem without registry, tests, packaging, identity/security posture, and evidence matrix entries.

### 9) Provider/exporter performance guard
**Purpose:** Prove provider conformance, read-visibility checks, identity mapping, and exporter projections remain bounded without turning coexistence validation into a full runtime throughput gate.

Implementation requirements:
- Add a targeted provider/exporter timing mode to the conformance runner for registry lookup, target validation, read-visibility refusal, identity-mapping refusal, receipt rendering, and Prometheus/OpenTelemetry/SIEM export over representative evidence packs.
- Report per-provider p50/p95, refusal counts, exporter row/byte counts, and artifact-size bounds.
- Compare against the accepted 26d rolling baseline only for shared REST/read status context; provider/exporter results are host-side coexistence evidence, not Pi hardware proof.

**Commands**
- `cargo test -p coh-rtc`
- `cargo test -p host-ticket-agent`
- `cargo test -p coh`
- `cargo test -p cohsh`
- `cargo test -p hive-gateway`
- `python -m pytest tools/cohesix-py/tests/test_integrations.py`
- `scripts/check-generated.sh`
- `scripts/ci/provider_conformance_run.sh --matrix configs/provider_conformance.toml --state-dir out/provider-conformance/m28b1`
- `scripts/ci/provider_conformance_run.sh --perf-only --matrix configs/provider_conformance.toml --state-dir out/bench/m28b1-provider-overhead`
- `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m28b1-coexistence`

**Checks (Definition of Done)**
- Provider actions are generated or checked from one registry; host-ticket-agent, REST/OpenAPI docs, Python defaults, host tools, and later MCP/A2A schema inputs cannot drift independently.
- Every REST/Python/FUSE/UI read projection is classified as public, ticket-scoped, or admin-only; cross-caller ticket, provider, evidence, audit, replay, and identity reads fail deterministic negative tests.
- Invalid provider target, action, argument, auth, writer epoch, idempotency key, dry-run/live-mode, or identity claim fails before executor dispatch.
- Every documented production provider has mock, dry-run, negative, and at least one live-safe conformance path or an explicit not-enabled status.
- Kubernetes RBAC, OIDC/JWT, SPIFFE, and local-host subject mappings produce bounded delegated ticket scopes and deterministic audit lines.
- Packaging artifacts deploy only host-side tools and preserve loopback defaults, least privilege, secret references, and delegated-ticket write requirements.
- Prometheus/OpenTelemetry/SIEM outputs are read-only, bounded, redacted, schema-versioned, and reconstructable from Cohesix evidence without becoming authority.
- OT/industry sidecars remain host-only, read-only-first, and profile-gated; write-capable actions are refused unless explicitly admitted by provider policy and approval flow.
- Evidence packs include provider registry, conformance matrix, identity-mapping refs, packaging profile, exporter schemas, and provider receipts.
- Provider/exporter performance evidence shows registry lookup, validation/refusal, identity mapping, receipt rendering, and exporter projection remain bounded; material regressions are classified as host provider overhead, gateway/auth overhead, or evidence-pack size overhead before 28c/28d/29b depend on the coexistence registry.
- `docs/USE_CASES.md`, `docs/HOST_TOOLS.md`, `docs/API_GUIDELINES.md`, `docs/INTERFACES.md`, `docs/ARCHITECTURE.md`, `docs/SECURITY.md`, and `docs/TEST_PLAN.md` describe support levels exactly as proven.

**Compiler touchpoints**
- `coh-rtc` emits provider registry artifacts, provider availability snippets, provider action schemas, read visibility classes, identity-mapping bounds, conformance matrix defaults, exporter schema bounds, and package profile metadata.
- Manifest validation rejects enabled provider actions without target grammar, authority scope, idempotency policy, receipt schema, redaction policy, and evidence mapping.
- Generated docs distinguish production-proven, mock-only, read-only-only, not-enabled, and future integrations.

## Task Breakdown
```
Title/ID: m28b1-provider-action-registry
Goal: Create the compiler-owned provider/action registry used by host tickets, REST/OpenAPI docs, Python, host tools, and later MCP/A2A schemas.
Inputs: tools/coh-rtc, apps/host-ticket-agent, apps/coh, apps/cohsh, docs/INTERFACES.md, docs/HOST_API.md.
Changes:
  - tools/coh-rtc/src/ir.rs — `providers.*` schema for action names, targets, modes, authority, receipts, redaction, and evidence refs.
  - tools/coh-rtc/src/codegen/{docs,rust}.rs — generated provider registry artifacts and snippets.
  - apps/host-ticket-agent/src/registry.rs — consume generated provider/action metadata before executor dispatch.
  - apps/coh/src/provider.rs + apps/cohsh/src/provider.rs — display generated provider action availability without hand-maintained lists.
Commands: cargo test -p coh-rtc && cargo test -p host-ticket-agent && cargo test -p coh && cargo test -p cohsh
Checks: Provider action schemas cannot drift between executor validation, host tools, docs, and future gateway protocol schema inputs.
Deliverables: Single source of truth for host-side ecosystem action semantics.

Title/ID: m28b1-ecosystem-conformance-matrix
Goal: Add provider conformance fixtures and evidence lanes for production coexistence claims.
Inputs: configs/provider_conformance.toml, scripts/ci/, docs/TEST_PLAN.md, docs/USE_CASES.md.
Changes:
  - configs/provider_conformance.toml — provider version, profile, auth, mock/dry-run/live-safe, negative, receipt, and evidence requirements.
  - scripts/ci/provider_conformance_run.sh — deterministic matrix runner.
  - docs/TEST_PLAN.md — provider conformance stage and evidence paths.
  - docs/USE_CASES.md — support-level mapping for each documented ecosystem integration.
Commands: scripts/ci/provider_conformance_run.sh --matrix configs/provider_conformance.toml --state-dir out/provider-conformance/m28b1
Checks: Every claimed production provider has passing conformance evidence or an explicit not-enabled status.
Deliverables: Production coexistence matrix tied to release evidence.

Title/ID: m28b1-read-visibility-classes
Goal: Classify every ecosystem-facing read projection before multi-caller REST, Python, FUSE, MCP, A2A, or UI clients expose it.
Inputs: tools/coh-rtc, apps/hive-gateway, apps/coh, apps/cohsh, apps/swarmui, docs/SECURITY.md, docs/API_GUIDELINES.md.
Changes:
  - tools/coh-rtc/src/ir.rs — read visibility class for provider, evidence, ticket, audit, replay, telemetry, and `/proc` paths.
  - apps/hive-gateway/src/auth.rs — enforce delegated identity for ticket-scoped reads and admin-only policy for global reads.
  - apps/coh/src/read_scope.rs + apps/cohsh/src/read_scope.rs — preserve read-scope refusals in host tools.
  - docs/SECURITY.md + docs/API_GUIDELINES.md — document public, ticket-scoped, and admin-only read behavior.
Commands: cargo test -p coh-rtc && cargo test -p hive-gateway && cargo test -p coh && cargo test -p cohsh
Checks: Cross-caller ticket, provider receipt, evidence, audit/replay, task, and identity diagnostics reads fail deterministically before data is returned.
Deliverables: Read-only ecosystem projections are scoped with the same rigor as writes.

Title/ID: m28b1-identity-mapping
Goal: Map external identities into delegated Cohesix tickets without adding an identity provider to the VM.
Inputs: apps/hive-gateway, apps/coh, tools/cohesix-py, tools/coh-rtc, docs/SECURITY.md.
Changes:
  - apps/hive-gateway/src/identity.rs — validate configured OIDC/JWT, SPIFFE, Kubernetes service account, and local-host subject claims.
  - tools/coh-rtc/src/validate.rs — issuer/audience/subject/group/TTL/action-scope mapping validation.
  - tools/cohesix-py/cohesix/identity.py — reference host-side subject mapping helpers.
  - docs/SECURITY.md + docs/API_GUIDELINES.md — identity-to-ticket mapping and refusal semantics.
Commands: cargo test -p hive-gateway && cargo test -p coh-rtc && python -m pytest tools/cohesix-py/tests/test_identity.py
Checks: Forged, expired, wrong-audience, overbroad, or unmapped claims fail before mutation and emit deterministic audit evidence.
Deliverables: Enterprise identity coexistence without expanding the VM TCB.

Title/ID: m28b1-packaging-deployment-profiles
Goal: Ship least-privilege host-side deployment shapes for real coexistence environments.
Inputs: scripts/install/, packaging/, apps/hive-gateway, apps/host-ticket-agent, apps/host-sidecar-bridge, apps/gpu-bridge-host, apps/coh.
Changes:
  - packaging/systemd/ — hardened unit templates for gateway, ticket agent, sidecar bridge, GPU bridge, and exporters.
  - packaging/launchd/ — macOS operator launchd examples.
  - packaging/k8s/ — Helm/Kustomize or plain YAML for gateway/agent sidecar deployments.
  - apps/coh/src/doctor.rs — provider registry, package profile, auth, secret-ref, and exporter health checks.
  - docs/HOST_TOOLS.md — install and risk guidance for each deployment profile.
Commands: cargo test -p coh --test doctor && scripts/ci/provider_conformance_run.sh --packaging-only
Checks: Packages install only host-side tools, preserve least privilege and loopback defaults, and report optional missing providers deterministically.
Deliverables: Repeatable host deployment profiles that coexist with existing systems instead of replacing them.

Title/ID: m28b1-observability-exporters
Goal: Add host-side Prometheus/OpenTelemetry/SIEM projection adapters over existing evidence and telemetry.
Inputs: apps/coh, apps/hive-gateway, docs/HOST_TOOLS.md, docs/SECURITY.md, docs/TEST_PLAN.md.
Changes:
  - apps/coh/src/export/prometheus.rs — bounded read-only metrics projection.
  - apps/coh/src/export/otel.rs — bounded OpenTelemetry projection.
  - apps/coh/src/export/siem.rs — schema-versioned SIEM export reuse/extension.
  - docs/HOST_TOOLS.md + docs/SECURITY.md — redaction and authority posture.
Commands: cargo test -p coh --test export && scripts/ci/provider_conformance_run.sh --observability-only
Checks: Exporters are read-only, bounded, redacted, schema-versioned, and reconstructable from evidence packs.
Deliverables: Observability coexistence without in-VM monitoring stacks.

Title/ID: m28b1-industry-sidecar-contracts
Goal: Stage OT, healthcare, and science sidecars as host-only, read-only-first provider families.
Inputs: apps/host-sidecar-bridge, tools/coh-rtc, docs/USE_CASES.md, docs/SECURITY.md.
Changes:
  - apps/host-sidecar-bridge/src/providers/ — read-only contracts for MODBUS, DNP3, CAN, IEC-104, DICOM metadata, and CCSDS telemetry.
  - tools/coh-rtc/src/ir.rs — provider gating and write-action refusal defaults for safety-critical sidecars.
  - docs/SECURITY.md + docs/INTERFACES.md — read-only-first and sensitive-data redaction posture.
Commands: cargo test -p host-sidecar-bridge && cargo test -p coh-rtc
Checks: Protocol code stays host-side, reads are bounded and redacted, and write-capable actions are unavailable until a later profile explicitly admits them.
Deliverables: Clear bridge contracts for broader industry coexistence without TCB expansion.

Title/ID: m28b1-use-case-release-gate
Goal: Prevent public use-case claims from exceeding implementation and evidence.
Inputs: docs/USE_CASES.md, docs/BUILD_PLAN.md, docs/TEST_PLAN.md, provider registry artifacts, evidence packs.
Changes:
  - docs/USE_CASES.md — support-level matrix for each use case and ecosystem.
  - scripts/ci/use_case_gate.sh — validate docs links to provider registry, conformance evidence, packaging, identity, observability, and failure fixtures.
  - apps/coh/src/evidence.rs — include provider registry and conformance snapshots in evidence packs.
Commands: scripts/ci/use_case_gate.sh && cargo test -p coh --test evidence_pack
Checks: Release docs cannot claim production coexistence without matching generated policy, tests, deployment profile, and evidence reconstruction.
Deliverables: Cohesix adoption claims stay honest and auditable.

Title/ID: m28b1-provider-exporter-performance
Goal: Add targeted provider/exporter timing evidence without requiring a full Pi/QEMU throughput benchmark.
Inputs: configs/provider_conformance.toml, scripts/ci/provider_conformance_run.sh, docs/BENCHMARKS.md, docs/TEST_PLAN.md.
Changes:
  - scripts/ci/provider_conformance_run.sh — perf-only mode for registry lookup, provider validation/refusal, identity mapping, receipt rendering, and exporter projection over representative evidence packs.
  - docs/BENCHMARKS.md + docs/TEST_PLAN.md — classify coexistence overhead separately from REST hardware performance.
Commands: scripts/ci/provider_conformance_run.sh --perf-only --matrix configs/provider_conformance.toml --state-dir out/bench/m28b1-provider-overhead
Checks: Provider/exporter timing stays bounded by generated limits and reports artifact-size context; failures classify host-provider, gateway/auth, or evidence-pack overhead.
Deliverables: Provider/exporter overhead evidence for downstream AI and gateway protocol work.
```

## Outcome
After Milestone 28b1:
- Cohesix has a production coexistence contract, not only individual host adapters.
- Provider action semantics are compiler-owned and shared by host tickets, REST, Python, docs, tests, and later MCP/A2A schemas.
- Existing ecosystems stay outside the VM TCB while still gaining delegated authority, receipts, evidence, deployment profiles, identity mapping, and observability exports.
- AI run control and MCP/A2A interop can build on proven provider schemas instead of inventing their own action catalogs.

## Milestone 28c — Host-Side AI Coexistence: Delegated Runs, Durable Context, Provider Receipts <a id="28c"></a>
[Milestones](#Milestones)

**Why now (bridge):**
Milestone 28b makes writes attributable, replay-safe, fenced, and audit-first. Milestone 28b1 then freezes provider action schemas, identity mapping, deployment posture, read visibility, observability exports, and conformance evidence for the ecosystems AI supervisors will touch. Before promoting AI lifecycle state into first-class VM-visible namespace roots in 29b, Cohesix needs a host-only proving ground for the operating model refined here:
1. Cohesix is the trusted execution, evidence, and governance layer beneath agent frameworks, not a replacement for them.
2. Long-context cost is dominated by repeated prefill, duplicated prompt state, and lossy summarization.
3. The highest-leverage fix is to keep durable run state, retrieval manifests, approvals, and evidence outside the live prompt, then route each run to the right host-side inference strategy under ticketed authority.

This milestone uses existing host-side surfaces (`/host/tickets/*`, delegated REST, provider action registry, evidence packs, GPU leases, telemetry ingest, Python playbooks) to prove that model without adding VM AI roots yet.

**As-built alignment note:** Python orchestration currently provides typed schedule, lease, export, host-ticket, federation, and Kubernetes coexistence helpers over existing control files. That substrate is useful leverage, but it is not yet the AI run/task graph envelope, checkpoint model, context-budget contract, prefix/hotset lifecycle, or NeMo provider family described here. Milestone 28c extends the host-ticket/evidence model; it must not re-label existing generic orchestration helpers as completed AI run control.

**Prerequisites**
- Milestone **28b** completed for delegated REST identity, idempotency, writer-epoch fencing, host-ticket durable execution, audit/replay defaults, and production secret/debug gates.
- Milestone **28b1** completed for provider action registry, read visibility classification, provider conformance, identity mapping, packaging profiles, observability exports, and use-case evidence matrix.

**Goal**
Add a host-side AI run substrate that lets external supervisors and agent frameworks coordinate long-context workflows through delegated tickets and existing host-ticket flows while preserving Cohesix's single-writer, append-only, audit-first discipline:
1. Make run/task/step identity and context budget explicit.
2. Keep checkpoints, exact constraints, retrieved spans, and tool receipts durable outside the prompt.
3. Route workloads to host-side inference backends by recall/cost policy rather than one fixed attention strategy.
4. Reuse warmed prefixes and hotsets across related runs within bounded quotas and TTLs.
5. Expose TTFT, decode, cache-hit, and resume metrics as first-class evidence.
6. Represent multi-agent work as explicit task graphs and handoffs, not as an opaque shared transcript or hidden message bus.
7. Verify worker implementation boundaries before AI supervisors depend on worker claims: VM worker roles must already have the real ticket/lease/telemetry loops, cap-backed endpoint authority, notification-backed lifecycle signaling, and generated scheduling evidence required by 26c, and AI supervisors may only reference those proven boundaries.
8. Make PEFT/model registry import, activation, rollback, and provider receipts transactionally auditable before AI run control treats them as dependable actuation.

**Non-Goals (Explicit)**
- No in-VM transformer kernels, sparse-attention implementations, KV-compression implementations, or CUDA/NVML changes.
- No direct many-agent writes to raw `/queen/ctl`; multi-agent host automation writes through delegated REST and/or `/host/tickets/spec`.
- No active/active multi-queen control for one logical hive.
- No live AI, PEFT, NeMo, Kubernetes, Docker, systemd, CUDA/NVML, or model-registry mutation from Python, adapters, playbooks, or framework integrations unless it appends a validated host ticket and is executed by `host-ticket-agent` under the generated provider action registry.
- No generic mutable `/store`, vector database, or prompt blob sink divorced from existing evidence/CAS discipline.
- No new 9P verbs, no console grammar changes, and no hidden RPC behind file names.
- No claim that Cohesix replaces agent planners/orchestrators; it remains the authority, evidence, and actuation layer beneath them.
- No opaque prompt transcript as the source of truth for agent state, approvals, retrieval, or tool output.
- No hidden inter-agent mailbox or side-channel coordination surface outside delegated tickets, durable artifacts, and existing evidence flows.
- No NeMo-specific control plane, namespace grammar, or provider lock-in semantics; NeMo support must remain an optional host-side provider family under the same Cohesix authority/evidence contract as other backends.
- No claim that VM worker-gpu, worker-lora, or worker-heart kernel binaries are full task implementations unless they attach, consume scoped tickets, service the documented lease/telemetry loop, and have tests/evidence matching the documentation.

**Deliverables**

### 1) AI run/task graph envelope and context-budget contract (host-only)
**Purpose:** Make long-context cost, dependency ordering, and handoff policy explicit instead of burying them inside prompts.

Implementation requirements:
- Add typed host-run envelope fields for AI ticket/playbook flows:
  - `run_id`, `parent_run_id`, `task_id`, `step_id`, `attempt`
  - `context_budget_tokens`, `latency_slo_ms`, `recall_mode`, `loss_tolerance`
  - `prefix_group`, `dataset_refs`, `artifact_refs`, `deadline_unix_ms`, `human_owner`
- Add explicit task-graph and handoff fields:
  - `depends_on`, `handoff_ref`, `instruction_ref`, `retrieval_manifest_ref`
  - `provider_profile_hash`, `prefix_cache_key`
  - `max_parallel_agents`, `human_attention_budget`
- Live mutating AI flows inherit Milestone 28b safety requirements: delegated ticket, `id`, `idempotency_key`, and `writer_epoch` where applicable.
- Dry-run and mock playbooks validate budget/policy mismatches before any host side effect.

As-built leverage:
- Reuse `cohesix` Python orchestration APIs and `/host/tickets/spec` idempotent lifecycle.

---

### 2) Durable checkpoints instead of prompt-only memory
**Purpose:** Reduce prompt bloat and resume cost without relying on lossy summarization.

Implementation requirements:
- Record structured checkpoints containing:
  - exact constraints/instructions,
  - retrieval manifests and span references,
  - approvals/policy receipts,
  - tool receipts/artifact refs,
  - resume cursor and prior-step linkage.
- Retrieval manifests are first-class artifacts: they record what was admitted into context, the originating refs, and any bounded filtering/truncation decisions.
- Large tool outputs and retrieved corpora are offloaded into bounded host-side artifacts with hashes, redacted previews, and exact refs instead of being re-inlined into every prompt.
- Checkpoints are host-side artifacts and evidence inputs, not a new VM filesystem or opaque prompt cache.
- Evidence/timeline tooling can reconstruct why a run acted without replaying the entire prompt transcript.

As-built leverage:
- Reuse existing evidence pack/timeline flows, audit redaction, and telemetry ingest.

---

### 3) Attention routing and prefix/hotset reuse
**Purpose:** Let operators choose the right long-context strategy per workload instead of paying full attention cost by default, while making cache eligibility and invalidation explainable.

Implementation requirements:
- Host-only provider policy classifies requests as bounded strategy hints, for example:
  - `full`
  - `retrieval-first`
  - `sparse-preferred`
  - `compression-preserve`
  - `needle-sensitive`
- Prefix groups and hotsets can be warmed, leased, resumed, and evicted through bounded host tickets with TTL/quota enforcement.
- Prefix reuse eligibility and miss reasons are recorded using bounded evidence fields such as model/provider profile hash, instruction hash/ref, retrieval-manifest hash/ref, tool-schema hash, TTL expiry, and quota eviction cause.
- GPU lease and provider selection remain host-side and reuse existing lease/publish flows.

As-built leverage:
- Reuse GPU lease control, host-ticket executor model, and telemetry/evidence surfaces.

---

### 4) AI run metrics and guardrails
**Purpose:** Make long-context efficiency measurable and auditable.

Implementation requirements:
- Collect bounded per-run metrics:
  - TTFT,
  - decode tokens/sec,
  - prompt bytes/tokens submitted,
  - prompt bytes/tokens avoided via checkpoint/prefix reuse,
  - prefix hit/miss counts,
  - prefix invalidation/miss reasons,
  - retrieval miss rate,
  - resume count,
  - GPU lease pressure/provider queue wait,
  - handoff count and checkpoint restart count.
- Metrics remain read-only exports and evidence inputs; they do not become a second source of truth for control.
- Policy gating remains mandatory for high-risk live mutations initiated by AI supervisors.
- Add a targeted AI run-cost benchmark for dry-run/mock playbooks that records checkpoint/resume overhead, prefix/hotset hit/miss behavior, prompt bytes avoided, receipt/evidence export cost, and provider queue-wait simulation. This is host-side AI evidence, not a full Pi/QEMU runtime benchmark.

As-built leverage:
- Reuse `/queen/telemetry/*`, existing evidence packs, and host snapshots.

---

### 5) Framework coexistence, not framework replacement
**Purpose:** Make Cohesix usable beneath agent supervisors without inventing another agent framework.

Implementation requirements:
- Provide Python-side adapters/examples for long-context supervisors to submit delegated host tickets and consume receipts/checkpoints.
- Ship at least one reference playbook that coordinates repo-scale analysis or closed-loop AI factory work in dry-run/mock mode before any live actuation.
- Reference adapters and playbooks must model explicit delegation/handoff chains over the run envelope and checkpoint model; they must not rely on implicit shared transcript state.
- Export receipts/checkpoints in derived, host-side forms suitable for downstream observability tooling; exports remain non-authoritative.

As-built leverage:
- Reuse `cohesix-playbook`, mock backend, and host-only REST/filesystem backends.

---

### 6) Worker Boundary and Documentation Closure
**Purpose:** Prevent host-side AI orchestration from depending on stronger worker claims than the VM implementation and generated evidence actually support.

Implementation requirements:
- Audit worker-heart, worker-gpu, and worker-lora kernel entrypoints against README, GPU, worker-ticket, role/scheduling, and interface docs.
- Verify that each worker role's ticket attach, lease/telemetry, notification-backed shutdown/revoke, cap-backed endpoint authority, and generated scheduling evidence match the 26c implementation contract.
- Add tests that prevent future docs from claiming worker spawn/lease semantics not backed by code and generated manifest truth.
- Host-side AI run envelopes must reference host-ticket/provider receipts, not undocumented VM worker behavior.

As-built leverage:
- Reuse role-scoped ticket docs, worker crates, host-ticket receipts, and evidence-pack schemas.

---

### 7) PEFT/Model Registry Transaction and Provenance Closure
**Purpose:** Make model/adapter activation safe enough for AI run control and PEFT reviewers without moving ML runtimes into the VM.

Implementation requirements:
- Add registry locking, canonical-path confinement, symlink rejection or explicit canonicalization policy, unique temp files, fsync/rename ordering, and rollback-safe activation records.
- Validate adapter metadata, model id, LoRA id, artifact hashes, source job refs, and optional signature/provenance refs before import or activation.
- Activation must record both host-registry state and VM `/gpu/models/*` publish/ack state so partial activation is visible and recoverable.
- Evidence packs must include bounded PEFT registry transaction receipts and activation/rollback provenance, redacted where needed.
- Direct PEFT/NeMo/provider activation remains host-side and ticket-scoped; Cohesix records authority/evidence, not training internals.

As-built leverage:
- Reuse `coh peft`, `/gpu/models`, host-ticket-agent, evidence timeline, and Milestone 28b audit/replay defaults.

---

### 8) Optional NeMo runtime family (host-only, governed, cross-provider)
**Purpose:** Support NVIDIA NeMo where it creates clear operational leverage over simpler direct-serving alternatives, while keeping Cohesix as the authority, evidence, and policy layer.

Implementation requirements:
- Treat NeMo as an optional host-side provider family under the Milestone 28c run-envelope contract, not as a new runtime plane:
  - `nemo.infer` for NIM / NeMo Export-Deploy / Triton / TensorRT-LLM-backed inference
  - `nemo.guardrails` for safety policy evaluation and refusal receipts
  - `nemo.evaluate` for model / RAG / agent evaluation jobs and score receipts
  - optional `nemo.retrieve` and `nemo.customize` only when they remain host-side and ticket-scoped
- Add host capability probes that discover NeMo endpoints, deployed model profiles, guardrail policy ids, evaluator availability, and deployment state without making NeMo the source of truth.
- Live NeMo-backed actuation remains ticketed and fenced:
  - all mutating NeMo actions flow through delegated host tickets,
  - all actions carry `id`, `idempotency_key`, and `writer_epoch` where applicable,
  - unsupported or unauthorised NeMo actions fail deterministically with no side effects.
- Guardrail and evaluator outputs become first-class receipts and evidence inputs:
  - `guardrail_policy_hash`,
  - `guardrail_decision`,
  - `evaluation_profile_hash`,
  - `evaluation_job_ref`,
  - `evaluation_summary_ref`,
  - `deployment_config_hash`.
- Cohesix must remain more valuable than direct NeMo or direct vLLM use:
  - the same run envelope, delegated ticket model, evidence pack, and policy gates must work against NeMo and at least one alternate provider family,
  - NeMo support must not introduce NeMo-only authoritative state or bypasses that other providers cannot satisfy.
- NeMo Agent Toolkit, MCP, or A2A features may be consumed only as downstream host integrations behind existing Cohesix auth and evidence boundaries; they must not become public control surfaces or authoritative coordination channels.

As-built leverage:
- Reuse `host-ticket-agent`, `cohesix-py` orchestration/playbooks, evidence packs, telemetry exports, GPU lease flows, and production audit/replay defaults from Milestone 28b.

**Commands**
- `cargo test -p host-ticket-agent`
- `cargo test -p coh`
- `python -m pytest tools/cohesix-py/tests/test_orchestration.py`
- `python -m pytest tools/cohesix-py/tests/test_playbooks.py`
- `python -m pytest tools/cohesix-py/tests/test_evidence_receipts.py`
- `python -m pytest tools/cohesix-py/tests/test_integrations.py -k nemo`
- `python -m pytest tools/cohesix-py/tests/test_playbooks.py -k nemo`
- `cargo test -p tests --test host_ticket_agent -- nemo`
- `cohesix-playbook --playbook long-context-agent-factory --dry-run --mock`
- `cohesix-playbook --playbook long-context-agent-factory --dry-run --mock --metrics-out out/bench/m28c-ai-run-cost.json`
- `cargo test -p worker-gpu && cargo test -p worker-lora && cargo test -p worker-heart`
- `cargo test -p coh --test peft && cargo test -p coh --test peft_registry_transactions`

**Checks (Definition of Done)**
- Multi-agent host workflows never require an undifferentiated shared Queen writer.
- A run can restart from checkpoint/evidence with the same authoritative constraints and receipts, without reconstructing a full prompt transcript.
- Evidence-only reconstruction preserves task graph ordering, handoff lineage, retrieval-manifest identity, and offloaded tool-artifact references.
- Prefix/hotset reuse is bounded by TTL/quota, attributable to a caller, and visible in evidence.
- Prefix cache hits and misses are explainable from bounded eligibility/invalidation fields rather than hidden provider behavior.
- Strategy selection and long-context cost metrics are observable per run/step.
- High-risk live AI mutations remain policy-gated and ticket-scoped.
- Worker-role documentation and generated snippets match actual kernel/host implementation boundaries; no AI task depends on undocumented VM worker behavior.
- PEFT/model registry import, activation, and rollback are transactional, provenance-recorded, and recoverable after partial host/VM publish failure.
- Optional NeMo support remains host-side, ticket-scoped, writer-fenced, and evidence-backed; disabling NeMo leaves the baseline 28c substrate intact.
- The same Cohesix run envelope and evidence model works against NeMo and at least one alternate provider family, proving NeMo support adds governed lifecycle value rather than vendor-specific lock-in.
- Guardrail and evaluator receipts can gate live promotion or actuation decisions deterministically in dry-run/mock tests before any real provider mutation is allowed.
- AI run-cost evidence records checkpoint/resume, prefix/hotset reuse, prompt bytes avoided, provider queue-wait simulation, and evidence export overhead for the dry-run/mock playbook; material regressions are classified as host orchestration, provider adapter, evidence-pack, or gateway overhead before 29b depends on the run model.
- No new in-VM listeners, runtime, or control protocols are introduced.

**Compiler touchpoints**
- `coh-rtc` emits generated host-tool defaults for AI run envelope limits, task-graph/handoff bounds, context budget ceilings, retrieval-manifest and artifact-ref bounds, prefix/hotset TTLs, and metrics bounds under the existing host policy/codegen path.
- Manifest validation rejects AI host-control enablement when Milestone 28b delegated identity or audit/replay requirements are disabled in the target profile.
- Manifest validation rejects AI host-control enablement when Milestone 28b1 provider registry, read visibility, identity mapping, provider conformance, or use-case evidence requirements are missing for a side-effecting provider action.
- Live provider mutation tests prove Python adapters, framework adapters, NeMo helpers, PEFT flows, Kubernetes helpers, Docker helpers, and systemd helpers cannot bypass `/host/tickets/spec`, `host-ticket-agent`, generated provider action validation, delegated ticket scope, idempotency, writer epoch, and evidence receipts.
- Manifest/docs validation rejects worker-role claims that exceed the current code/generated worker implementation boundary.
- `coh-rtc` emits PEFT/model registry transaction/provenance bounds consumed by host tools and evidence exports.
- `coh-rtc` additionally emits optional provider-family policy for NeMo capability probes, action allowlists, endpoint/auth refs, deployment/evaluation/guardrail bounds, and alternate-provider parity requirements.
- Canonical interface/architecture docs refreshed in:
  - `docs/INTERFACES.md`
  - `docs/ARCHITECTURE.md`
- Generated host-tool/docs snippets refreshed in:
  - `docs/PYTHON_SUPPORT.md`
  - `docs/HOST_TOOLS.md`
  - `docs/SECURITY.md`
  - `docs/TEST_PLAN.md`

**Task Breakdown**
```
Title/ID: m28c-ai-run-envelopes
Goal: Add typed host-side AI run/task/step envelopes with explicit handoff, dependency, and context-budget contracts.
Inputs: tools/cohesix-py/cohesix/orchestration.py, tools/cohesix-py/cohesix/playbooks.py, docs/PYTHON_SUPPORT.md, docs/SECURITY.md
Changes:
  - tools/cohesix-py/cohesix/orchestration.py — RunRequest, RunTask, RunStep, HandoffRef, RetrievalManifestRef, and ContextBudget validators for delegated AI ticket flows.
  - tools/cohesix-py/cohesix/playbooks.py — `long-context-agent-factory` dry-run/mock playbook with explicit task graph, delegation, and handoff receipts.
  - tools/cohesix-py/cohesix/receipts.py — typed derived receipts for run/task/step/handoff identity.
  - tools/cohesix-py/tests/test_orchestration.py + tools/cohesix-py/tests/test_playbooks.py — budget, idempotency, and dry-run coverage.
Commands: python -m pytest tools/cohesix-py/tests/test_orchestration.py && python -m pytest tools/cohesix-py/tests/test_playbooks.py
Checks: Invalid budget/policy combinations fail before writes; dry-run outputs show run/task/step identity, dependencies, handoffs, and budgets deterministically.
Deliverables: Host-side AI runs become explicit, typed, replay-addressable, and suitable for delegated multi-agent coordination without hidden state.

Title/ID: m28c-host-ticket-ai-actions
Goal: Extend the host ticket plane with bounded AI control actions for inference runs, checkpoints, and prefix lifecycle.
Inputs: apps/host-ticket-agent, docs/ARCHITECTURE.md, docs/INTERFACES.md, docs/HOST_TOOLS.md
Changes:
  - apps/host-ticket-agent/src/lib.rs — allowlist and schema validation for `infer.run|resume|abort`, `context.checkpoint|resume|evict`, and `prefix.warm|evict`.
  - apps/host-ticket-agent/src/status.rs — bounded lifecycle/state counters for AI run, checkpoint, and prefix operations.
  - apps/host-ticket-agent/src/executors/infer.rs — host-only provider adapter contract; no VM-side model runtime.
  - docs/INTERFACES.md — canonical host-ticket AI action envelopes, receipt fields, retrieval-manifest refs, and refusal semantics.
  - docs/ARCHITECTURE.md — authority flow for delegated AI runs, checkpoints, prefix lifecycle, task handoffs, and evidence correlation.
Commands: cargo test -p host-ticket-agent && cargo test -p tests --test host_ticket_agent
Checks: AI host tickets stay idempotent, allowlist-gated, and stale-writer safe; unsupported actions fail deterministically with no side effects; handoff/checkpoint references remain bounded and attributable.
Deliverables: `/host/tickets/spec` becomes the canonical AI actuation path before 29b VM roots exist.

Title/ID: m28c-ai-evidence-checkpoints
Goal: Extend evidence pack/timeline flows to reconstruct AI runs from checkpoints, receipts, and cost telemetry.
Inputs: apps/coh/src/evidence.rs, apps/coh/src/evidence_timeline.rs, apps/coh/tests/evidence_pack.rs, apps/coh/tests/evidence_timeline.rs, docs/TEST_PLAN.md
Changes:
  - apps/coh/src/evidence.rs — include run envelopes, checkpoint manifests, retrieval manifests, offloaded tool-artifact refs, prefix reuse stats, and provider receipts with redaction.
  - apps/coh/src/evidence_timeline.rs — correlate run/task/step/handoff/checkpoint/prefix events into a deterministic operator timeline.
  - apps/coh/tests/evidence_pack.rs + apps/coh/tests/evidence_timeline.rs — restart/resume reconstruction tests from evidence-only inputs.
Commands: cargo test -p coh --test evidence_pack && cargo test -p coh --test evidence_timeline
Checks: Evidence-only reconstruction preserves authoritative constraints, handoff lineage, retrieval identity, and receipts; sensitive keys stay redacted.
Deliverables: Long-context AI runs become auditable and resumable without prompt archaeology or transcript dependence.

Title/ID: m28c-ai-policy-and-metrics
Goal: Generate host defaults and bounded metrics for AI context budgets, prefix reuse, and run efficiency.
Inputs: tools/coh-rtc, configs/root_task.toml, apps/coh/src/telemetry.rs, tools/cohesix-py/cohesix/generated.py, docs/PYTHON_SUPPORT.md, docs/TEST_PLAN.md
Changes:
  - tools/coh-rtc/src/ir.rs + tools/coh-rtc/src/validate.rs — host AI budget/TTL/metrics bounds, retrieval/offload bounds, task-graph/handoff bounds, and dependency on 28b safety gates.
  - apps/coh/src/telemetry.rs — bounded run-efficiency metric export helpers.
  - tools/cohesix-py/cohesix/generated.py — generated defaults consumed by Python orchestration/playbooks.
Commands:
  - cargo test -p coh-rtc
  - cargo test -p coh
  - python -m pytest tools/cohesix-py/tests/test_evidence_receipts.py
  - cohesix-playbook --playbook long-context-agent-factory --dry-run --mock --metrics-out out/bench/m28c-ai-run-cost.json
Checks: Generated defaults bound AI runs consistently across CLI/Python flows; cache eligibility/invalidation fields and metrics stay bounded and byte-stable; dry-run/mock run-cost evidence reports checkpoint/resume, prefix reuse, prompt bytes avoided, and evidence export overhead without invoking live providers.
Deliverables: Context budgets, retrieval/offload bounds, and efficiency metrics are compiler-aligned rather than ad hoc.

Title/ID: m28c-framework-adapters
Goal: Provide host-side reference adapters that let external supervisors coexist with Cohesix through delegated tickets, checkpoints, and evidence exports.
Inputs: tools/cohesix-py/cohesix/integrations.py, tools/cohesix-py/examples/, docs/PYTHON_SUPPORT.md, docs/HOST_TOOLS.md
Changes:
  - tools/cohesix-py/cohesix/integrations.py — reference supervisor adapter helpers for delegated submission, receipt polling, checkpoint lookup, and evidence export.
  - tools/cohesix-py/examples/ — bounded examples for repo-scale analysis and dry-run delegated handoff flows.
  - docs/PYTHON_SUPPORT.md + docs/HOST_TOOLS.md — integration contract for external supervisors over delegated tickets and evidence receipts.
Commands: python -m pytest tools/cohesix-py/tests/test_integrations.py && python -m pytest tools/cohesix-py/tests/test_examples_ci_siem.py
Checks: Reference adapters use delegated tickets, explicit handoff/checkpoint refs, generated provider actions, and evidence exports only; no hidden side-channel state, direct `/queen/ctl` mutation, direct provider API mutation, or direct host executor call is required or accepted.
Deliverables: Cohesix remains the authority/evidence layer beneath supervisor frameworks instead of becoming one.

Title/ID: m28c-worker-boundary-closure
Goal: Verify worker role documentation, generated snippets, and AI run references against the implemented 26c worker boundary.
Inputs: apps/worker-heart, apps/worker-gpu, apps/worker-lora, docs/GPU_NODES.md, docs/WORKER_TICKETS.md, docs/ROLES_AND_SCHEDULING.md, docs/INTERFACES.md
Changes:
  - apps/worker-heart/src/kernel.rs + apps/worker-gpu/src/kernel.rs + apps/worker-lora/src/lib.rs — verify scoped ticket/lease/telemetry loops, cap-backed endpoint attach, notification-backed lifecycle handling, and generated scheduling evidence remain aligned with 26c contracts.
  - docs/GPU_NODES.md + docs/WORKER_TICKETS.md + docs/ROLES_AND_SCHEDULING.md — describe worker roles exactly as implemented, separating VM control-plane workers from host GPU/PEFT execution and avoiding any restored stub/scaffolding language.
  - tools/coh-rtc/src/validate.rs — reject generated worker-spawn claims that do not match enabled worker implementation status.
  - apps/root-task/tests/worker_docs_alignment.rs — guard documented worker paths and generated role state against implementation drift.
Commands: cargo test -p worker-heart && cargo test -p worker-gpu && cargo test -p worker-lora && cargo test -p root-task --test worker_docs_alignment
Checks: Docs no longer overclaim or undercut VM worker behavior; every worker-role claim used by AI run control is backed by code, generated manifest state, cap-backed authority evidence, notification lifecycle evidence, scheduling evidence, and tests.
Deliverables: Host-side AI orchestration has an honest worker boundary and cannot cite undocumented or stale worker semantics.

Title/ID: m28c-peft-registry-transactions
Goal: Make PEFT/model registry import, activation, rollback, and evidence receipts transactional and provenance-complete.
Inputs: apps/coh/src/peft, apps/coh/src/evidence.rs, apps/host-ticket-agent, docs/GPU_NODES.md, docs/SECURITY.md, docs/TEST_PLAN.md
Changes:
  - apps/coh/src/peft/mod.rs — registry lock, canonical-path confinement, unique temp files, fsync/rename ordering, adapter metadata validation, and rollback-safe transaction records.
  - apps/coh/src/peft/activate.rs — record host-registry state and VM `/gpu/models/*` publish/ack state as one recoverable activation transaction.
  - apps/host-ticket-agent/src/executors/peft.rs — require transaction/provenance receipts for `peft.import|activate|rollback` actions.
  - apps/coh/src/evidence.rs + apps/coh/src/evidence_timeline.rs — include bounded PEFT transaction, provenance, activation, and rollback receipts.
  - docs/GPU_NODES.md + docs/SECURITY.md + docs/TEST_PLAN.md — document PEFT transaction/provenance requirements and partial-failure recovery tests.
Commands: cargo test -p coh --test peft && cargo test -p coh --test peft_registry_transactions && cargo test -p host-ticket-agent
Checks: Partial import/activation failure is recoverable and visible; adapter provenance and hashes are verified before activation; evidence reconstructs host and VM publish state without relying on prompt transcripts.
Deliverables: PEFT/model lifecycle becomes a governed host-side actuation path suitable for 28c AI run control.

Title/ID: m28c-nemo-capability-probes
Goal: Detect and classify optional NeMo runtime capabilities without making NeMo the source of truth.
Inputs: tools/cohesix-py/cohesix/integrations.py, tools/cohesix-py/cohesix/generated.py, docs/PYTHON_SUPPORT.md, docs/HOST_TOOLS.md
Changes:
  - tools/cohesix-py/cohesix/integrations.py — `probe_nemo_runtime`, `probe_nemo_guardrails`, and `probe_nemo_evaluator` helpers that resolve configured endpoints/auth refs, deployed model profiles, and capability summaries.
  - tools/cohesix-py/cohesix/generated.py — generated NeMo capability defaults and bounded endpoint/profile limits from `coh-rtc`.
  - docs/PYTHON_SUPPORT.md + docs/HOST_TOOLS.md — operator-visible NeMo capability probe contract and failure semantics.
Commands: python -m pytest tools/cohesix-py/tests/test_integrations.py -k nemo_probe
Checks: Capability probes are read-only, bounded, deterministic, and return the same shape whether NeMo is unavailable, partially configured, or fully available.
Deliverables: Cohesix can reason about NeMo availability and profile shape before choosing provider strategy or issuing any host-ticket mutation.

Title/ID: m28c-nemo-provider-family
Goal: Add NeMo-backed provider adapters under the same delegated ticket, checkpoint, and evidence contract as other AI backends.
Inputs: apps/host-ticket-agent/src/executors/infer.rs, tools/cohesix-py/cohesix/orchestration.py, docs/ARCHITECTURE.md, docs/INTERFACES.md
Changes:
  - apps/host-ticket-agent/src/executors/infer.rs — optional NeMo provider adapters for `nemo.infer`, `nemo.guardrails`, and `nemo.evaluate`, including deterministic refusal mapping and bounded receipt fields.
  - tools/cohesix-py/cohesix/orchestration.py — provider-family selection hints and provider receipt normalization for NeMo vs alternate backends.
  - docs/INTERFACES.md + docs/ARCHITECTURE.md — host-ticket NeMo action envelopes, receipt fields, and authority mapping back to delegated tickets and checkpoints.
Commands: cargo test -p host-ticket-agent && cargo test -p tests --test host_ticket_agent -- nemo_provider
Checks: NeMo-backed actions are idempotent, allowlist-gated, writer-fenced, and produce the same Cohesix-level receipt contract as alternate providers; unsupported NeMo features fail deterministically with no side effects.
Deliverables: NeMo becomes an optional provider family beneath Cohesix rather than a special-case control plane.

Title/ID: m28c-nemo-guardrails-and-eval
Goal: Make NeMo guardrail and evaluator results first-class policy receipts that can gate live AI actions.
Inputs: apps/coh/src/evidence.rs, apps/coh/src/evidence_timeline.rs, tools/cohesix-py/cohesix/playbooks.py, docs/SECURITY.md, docs/TEST_PLAN.md
Changes:
  - apps/coh/src/evidence.rs + apps/coh/src/evidence_timeline.rs — include guardrail policy hashes, decisions, evaluation refs/summaries, and deployment config hashes in evidence and timeline correlation.
  - tools/cohesix-py/cohesix/playbooks.py — dry-run/mock playbooks that require successful NeMo guardrail/evaluator receipts before promotion or live mutation steps become admissible.
  - docs/SECURITY.md + docs/TEST_PLAN.md — policy-gating contract for NeMo-backed guardrails/evaluations and additive regression expectations.
Commands: cargo test -p coh --test evidence_pack && cargo test -p coh --test evidence_timeline && python -m pytest tools/cohesix-py/tests/test_playbooks.py -k nemo_gate
Checks: Guardrail/evaluator receipts are durable, redacted where needed, correlated to run/task/step identity, and can deterministically block promotion or actuation in mock and evidence-only reconstruction paths.
Deliverables: NeMo safety and evaluation add operational value to Cohesix instead of existing as unaudited provider-side metadata.

Title/ID: m28c-nemo-policy-and-parity
Goal: Enforce that optional NeMo support remains governed, bounded, and more valuable than direct backend-specific alternatives.
Inputs: tools/coh-rtc/src/ir.rs, tools/coh-rtc/src/validate.rs, tools/cohesix-py/cohesix/generated.py, docs/BUILD_PLAN.md, docs/HOST_TOOLS.md
Changes:
  - tools/coh-rtc/src/ir.rs + tools/coh-rtc/src/validate.rs — optional NeMo endpoint/auth refs, action allowlists, receipt-field bounds, and alternate-provider parity requirements under the host AI policy path.
  - tools/cohesix-py/cohesix/generated.py — compiler-aligned NeMo policy defaults consumed by orchestration and probes.
  - docs/HOST_TOOLS.md — operational guidance stating that NeMo support is optional, host-side, and governed by the same Cohesix authority/evidence model as other providers.
Commands: cargo test -p coh-rtc && python -m pytest tools/cohesix-py/tests/test_integrations.py -k nemo_policy
Checks: Invalid NeMo policy, missing delegated-authority prerequisites, or NeMo-only authoritative semantics are rejected at validation time; the same run envelope/evidence contract remains valid with NeMo disabled or replaced by another provider family.
Deliverables: NeMo support is compiler-governed, optional, and demonstrably cross-provider rather than a lock-in path.
```

## Outcome
After Milestone 28c:
- Cohesix is positioned as the trusted actuation, evidence, and governance layer beneath agent frameworks.
- Long-context AI runs stop treating the prompt as the sole system of record.
- Attention strategy becomes a schedulable, measurable host-side concern instead of a hidden model-side accident.
- Delegation, retrieval admission, cache eligibility, and handoff lineage become explicit artifacts rather than implicit prompt behavior.
- Optional NeMo capabilities can be used where they materially improve guardrails, evaluation, deployment, or retrieval workflows, without becoming a second control plane or displacing Cohesix authority.
- Milestone 29b can expose stable AI namespace roots based on proven host-side semantics rather than speculation.

## Milestone 28d — MCP/A2A Gateway Projection: Read-Only First, Ticketed Writes Later <a id="28d"></a>
[Milestones](#Milestones)

**Why now (ecosystem boundary):**
Milestone 28b gives `hive-gateway` caller-attributed, fenced, audit-first write authority. Milestone 28b1 provides the generated provider action registry, read visibility classes, identity mappings, conformance matrix, and deployment posture that gateway protocol projections must consume. Milestone 28c defines the host-side AI/provider model for delegated runs, PEFT/model lifecycle, optional NeMo providers, GPU leases, and evidence receipts. That is the right point to add a Model Context Protocol (MCP) server: external agent hosts need standard MCP tools, resources, and prompts, but Cohesix must not create a second authority plane or a new VM grammar to satisfy them.

MCP support belongs inside or immediately beside `hive-gateway` because the gateway is already the host-only multiplexer over existing Cohesix file semantics. This milestone makes MCP a client-facing projection over the same `LS`, `CAT`, `TAIL`, and `ECHO` paths, plus the existing `/host/tickets/spec` actuation lane. It is not a new runtime, not an in-VM endpoint, and not an excuse to expose `systemctl`, `docker`, `kubectl`, CUDA, PEFT, or NeMo APIs directly to an agent.

A2A belongs in the same gateway milestone only as a companion agent-delegation facade. MCP answers "what tools/resources can this agent host use?"; A2A answers "what task can one external agent delegate to Cohesix and how is progress/artifact state observed?" Cohesix should support that distinction because 28c already defines durable run/task/checkpoint/evidence records, but A2A must project those records rather than introduce an opaque agent bus.

**As-built alignment note:** There is no MCP server or A2A facade in `hive-gateway` today. Current gateway behavior is REST/OpenAPI over `LS`/`CAT`/`ECHO`, and the host ecosystem already has bounded providers for CUDA/NVIDIA discovery, GPU leases, PEFT, systemd, Docker, and K8s through Cohesix host tools and `/host/tickets/*`. `coh mount --rest-url` already mounts through `hive-gateway` and is the primary FUSE path for the live Cohesix namespace; Milestone 28d must not rebuild that through MCP or A2A. Milestone 28d adds MCP-compatible and A2A-compatible surfaces only after those existing flows are the implementation substrate. Older prose must not claim MCP or A2A support until the gateway exposes lifecycle/discovery/execution/authorization/conformance evidence for the relevant protocol.

**Sequencing note:** Milestone 28d is staged inside one milestone. Phase 1 is read-only MCP transport/resource/prompt discovery and conformance over existing bounded reads, with every resource classified by the Milestone 28b1 visibility model. Phase 2 may add mutating MCP tools and A2A task facades only after the 28b1 provider action registry, 28b delegated authority floor, and 28c run/checkpoint/evidence model are proven. No mutating MCP/A2A path can be accepted solely because read-only MCP conformance passes. Any mutating MCP/A2A path that claims production VM worker or driver authority must also cite the Milestone 28e cap-bundle and structured-fault evidence; host-ticket-only and read-only projections must not claim that authority.

**Goal**
Expose Cohesix to MCP clients through standard MCP server primitives and to A2A peers through task/artifact protocol primitives while preserving Cohesix's existing grammar and authority model:
1. MCP resources provide bounded read-only context from existing Cohesix paths and evidence artifacts.
2. MCP tools either read existing files or submit existing host tickets; mutating tools never call host executors directly.
3. MCP prompts encode safe operational playbooks for CUDA/GPU, PEFT, NeMo, K8s, systemd, and Docker workflows without becoming authority.
4. All writes inherit Milestone 28b delegated ticket, idempotency, writer-epoch, audit/replay, and request-auth rules.
5. The shared `cohsh-core` console grammar, NineDoor semantics, and generated manifest bounds remain byte-stable.
6. `coh mount --rest-url` remains the canonical gateway-backed namespace mount; any MCP-backed mount mode is a read-only MCP resource/catalog view for MCP-admitted context, not a replacement write path.
7. A2A Agent Cards, messages, tasks, artifacts, and streaming updates are projections of 28c run/checkpoint/evidence records and existing host-ticket receipts, not a separate scheduler or agent memory.
8. Read-only MCP/A2A conformance is an ecosystem compatibility claim, not a write-authority claim. Mutating protocol evidence must name the delegated-ticket, provider-action, idempotency, writer-epoch, audit/replay, and, where applicable, 28e VM cap-bundle prerequisites it consumes.

**Non-Goals (Explicit)**
- No in-VM MCP endpoint, MCP listener, MCP filesystem root, or MCP-specific root-task parser.
- No new console verbs, no new 9P verbs, no ACK/ERR/END grammar changes, and no hidden RPC behind MCP tool names.
- No direct execution of `systemctl`, `docker`, `kubectl`, CUDA/NVML, PEFT, or NeMo provider APIs from the MCP server. Side effects go through delegated REST and/or `/host/tickets/spec`.
- No MCP tool that bypasses role-scoped tickets, policy approval, writer-epoch fencing, host-ticket allowlists, or evidence exports.
- No model-controlled prompt or MCP client metadata is trusted as authorization. Tool descriptions, prompts, and annotations are documentation only.
- No CUDA/NVML, PEFT, NeMo, Kubernetes, systemd, or Docker code enters the VM TCB.
- No implicit translation from arbitrary FUSE writes into MCP `tools/call`. Write-capable Cohesix mounts continue to use existing console/REST `ECHO` semantics and the existing append-only control files.
- No in-VM A2A endpoint, no A2A-specific root-task queue, no A2A peer mesh, no opaque inter-agent mailbox, and no direct A2A-to-provider execution path.
- No A2A push notification callback is accepted without SSRF-safe URL validation, explicit allowlist policy, per-task auth material, bounded retry policy, and audit evidence.

**Deliverables**

### 1) MCP protocol endpoint and lifecycle in `hive-gateway`
**Purpose:** Let standard MCP hosts connect to Cohesix without client-specific shims while keeping the gateway's loopback/auth defaults.

Implementation requirements:
- Add an MCP server mode to `apps/hive-gateway` with:
  - stdio transport for local MCP hosts that launch the gateway as a subprocess,
  - Streamable HTTP endpoint for remote-capable MCP clients, sharing the gateway's loopback-only default and non-loopback risk override,
  - protocol revision, JSON Schema dialect, authorization mode, and capability negotiation pinned in `docs/HOST_API.md` and generated gateway metadata,
  - HTTP `MCP-Protocol-Version` handling, optional `Mcp-Session-Id` lifecycle, explicit session termination behavior, and deterministic unsupported-version errors,
  - `tools`, `resources`, and `prompts` capabilities with paginated discovery where needed,
  - optional list-change notifications only when the implementation has deterministic change detection.
- Remote MCP transport must validate `Origin`, require gateway request auth, and require delegated tickets for mutating tools. A production non-loopback profile must implement the authorization contract of the pinned MCP revision or explicitly document and test a narrower compatibility mode; a preconfigured loopback bearer token alone is not generic remote MCP authorization conformance.
- Stdio mode must read credentials only from environment/config, never from prompts or tool arguments.
- MCP stdout/stdin must carry only valid MCP JSON-RPC messages; logs go to stderr or the existing gateway log path.
- Streamable HTTP mode must support the accepted request/response content types, bounded SSE streams when enabled, explicit cancellation handling, and no broadcast of one client's server messages to another client.
- The gateway must expose enough server metadata for common MCP clients and inspectors to identify the server, protocol revision, tool names, resource URI scheme, and auth requirements.

As-built leverage:
- Reuse `hive-gateway` broker queues, request-auth checks, loopback binding policy, OpenAPI bounds, and existing `cohsh` REST transport code.

---

### 2) Resource catalog over existing Cohesix paths
**Purpose:** Give MCP clients context without giving them a new read model.

Implementation requirements:
- Define `cohesix://` resource URIs that map one-to-one to existing bounded reads:
  - `/proc/boot`, `/proc/root/*`, `/proc/9p/*`, `/proc/lease/*`, `/proc/schedule/*`, `/proc/spool/*`, `/proc/attest/*`
  - `/gpu/*`, `/gpu/models/*`, `/gpu/telemetry/schema.json`
  - `/host/tickets/status`, `/host/tickets/deadletter`, and provider status under `/host/systemd/*`, `/host/docker/*`, and `/host/k8s/*`
  - evidence-pack and timeline summaries when Milestone 28/28b/28c evidence is available
  - NeMo capability, guardrail, evaluator, and provider receipt summaries only when the 28c optional provider family is enabled.
- Resource reads must use only `LS`, `CAT`, or `TAIL` through the existing gateway/session machinery and must enforce manifest-derived path, line, byte, and walk-depth bounds.
- Resource templates may expose common path families, but template expansion must reject `..`, absolute host filesystem paths, overlong components, and undeclared provider roots.
- Resource contents must be redacted with the same rules as evidence packs: no raw tickets, auth tokens, provider credentials, or secret refs.

As-built leverage:
- Reuse `coh evidence pack`, `coh evidence timeline`, generated Cohesix path defaults, and the existing `/host` provider status surfaces.

---

### 3) Tool catalog for real Cohesix operations
**Purpose:** Support useful MCP automation while preserving the Cohesix write path.

Implementation requirements:
- Read-only tools:
  - `cohesix.fs.ls`, `cohesix.fs.cat`, `cohesix.fs.tail`
  - `cohesix.cuda.inventory` for bounded host CUDA/NVIDIA capability and GPU inventory summaries
  - `cohesix.evidence.timeline` for bounded evidence/timeline summaries.
- Mutating or side-effect-capable tools must produce existing Cohesix writes only:
  - `cohesix.host_ticket.submit` appends a validated `host-ticket/v1` line to `/host/tickets/spec`.
  - `cohesix.gpu.lease_grant`, `cohesix.gpu.lease_renew`, and `cohesix.gpu.lease_release` map to existing GPU lease actions.
  - `cohesix.peft.import`, `cohesix.peft.activate`, and `cohesix.peft.rollback` map to existing PEFT ticket/action flows and 28c transaction receipts.
  - `cohesix.nemo.probe`, `cohesix.nemo.infer`, `cohesix.nemo.guardrails`, and `cohesix.nemo.evaluate` map to 28c optional provider actions or deterministically return unavailable when NeMo is not enabled.
  - `cohesix.k8s.cordon`, `cohesix.k8s.drain`, and `cohesix.k8s.lease_sync` map to existing K8s host-ticket actions.
  - `cohesix.systemd.status_check`, `cohesix.systemd.start`, `cohesix.systemd.stop`, and `cohesix.systemd.restart` map to existing systemd host-ticket actions.
  - `cohesix.docker.status_check`, `cohesix.docker.stop`, and `cohesix.docker.restart` map to existing Docker host-ticket actions.
- MCP tool schemas and A2A skill schemas must derive from a shared manifest/provider action registry. Provider action names, target selectors, dry-run flags, idempotency keys, and receipt fields must not be hand-maintained separately for the two protocols.
- Every tool schema must be generated or checked against manifest/provider policy:
  - bounded string lengths,
  - explicit enum values for actions and providers,
  - no free-form shell command field,
  - id/idempotency-key/writer-epoch requirements for mutating calls,
  - target path validation using existing Cohesix path rules.
- Tool results must return structured MCP output plus a text fallback containing the Cohesix receipt id, ticket id, action, target, state path, and evidence refs. They must not expose raw tickets or provider credentials.

As-built leverage:
- Reuse `host-ticket-agent` executors, `coh peft`, `host-cuda`, generated policy defaults, delegated REST identity, writer-epoch fencing, and evidence/timeline redaction.

---

### 4) Prompt templates for safe operator workflows
**Purpose:** Provide MCP-native workflows without making prompts authoritative.

Implementation requirements:
- Add prompt templates that assemble existing tools/resources for common Cohesix tasks:
  - CUDA capacity triage before a GPU lease,
  - PEFT import/promotion/rollback review,
  - NeMo provider readiness and guardrail/evaluator receipt review,
  - K8s cordon/drain with lease and evidence checks,
  - systemd service recovery with Docker workload status,
  - Docker remediation with post-action evidence collection.
- Prompts must require explicit user approval for side-effecting tools and must name the exact host-ticket action that would be submitted.
- Prompt text must not embed secrets, tickets, endpoint auth, or unbounded host paths.
- Prompt outputs are guidance only; only existing Cohesix tickets, receipts, and evidence determine state.

As-built leverage:
- Reuse Milestone 28 operator utilities, 28b audit/replay/fencing, 28c run envelopes/checkpoints, and existing host-ticket provider receipts.

---

### 5) A2A Agent Card and task facade over existing runs
**Purpose:** Let external A2A peers delegate bounded Cohesix operational tasks and observe status/artifacts without making A2A a coordination plane.

Implementation requirements:
- Add an A2A-compatible HTTP facade in `hive-gateway` behind existing gateway request auth, loopback default, non-loopback exposure override, rate limits, and broker backpressure.
- Record the accepted A2A protocol revision, Agent Card `protocolVersion`, endpoint paths, supported binding, media type, extension policy, unsupported-version errors, and streaming/push support in `docs/HOST_API.md` and generated gateway metadata.
- Pin one accepted binding/revision mapping in generated policy. JSON-RPC, HTTP+JSON, and gRPC method or endpoint names must not be mixed across protocol revisions, and fixtures must be regenerated when that mapping changes.
- Publish an Agent Card from `/.well-known/agent-card.json` when A2A is enabled; alternate generated paths may exist only as additional configured aliases. The card must advertise only enabled Cohesix skills, authentication requirements, endpoint interfaces, and safe capability summaries; it must not expose raw tickets, secrets, host paths, or executor internals.
- Provide the authenticated extended Agent Card endpoint only when policy enables it, and ensure the extended card obeys stricter access checks than the public discovery card.
- A2A skills map to the same real-world operational families as MCP tools: CUDA/GPU inventory and leases, PEFT import/activate/rollback, optional NeMo probe/infer/guardrail/evaluator actions, K8s cordon/drain/lease sync, systemd status/start/stop/restart, Docker status/stop/restart, and evidence/timeline inspection.
- A2A `SendMessage` and `SendStreamingMessage` operations (or the exact generated equivalents for the pinned binding/revision) create or resume 28c run/task envelopes only after fixed skill/action/input schema validation. Free-form natural language is never translated directly into host execution.
- A2A `GetTask`, `ListTasks`, `CancelTask`, `SubscribeToTask`, push-notification configuration, and streaming update operations (or their generated binding equivalents) are projections of existing run/checkpoint/evidence records, host-ticket receipt state, and gateway audit state. Cancellation may append a validated Cohesix cancel/control request when one exists; it must not kill provider executors directly.
- A2A artifacts are bounded, redacted references to evidence packs, timelines, checkpoint summaries, provider receipts, and MCP/Cohesix resource refs. Large files, secrets, raw ticket material, and provider credentials are never embedded in artifacts.
- A2A push notification configs are disabled by default. If enabled, they require SSRF-safe URL validation, generated allowlists, per-task auth material, bounded retry/backoff, signed or authenticated delivery where configured, and audit evidence for every callback attempt.

As-built leverage:
- Reuse Milestone 28c run envelopes, checkpoints, provider receipts, evidence exports, `host-ticket-agent` state, gateway request auth, and delegated REST identity.

---

### 6) Security, audit, and confused-deputy controls
**Purpose:** Keep model-controlled MCP and A2A calls inside Cohesix's existing capability discipline.

Implementation requirements:
- Mutating MCP tools require:
  - gateway request auth,
  - delegated capability ticket with matching path/action scope,
  - id/idempotency_key,
  - writer_epoch when the target profile enables fencing,
  - policy approval where the underlying Cohesix path already requires it.
- A2A task-creating or task-mutating calls require the same gateway request auth, delegated scope, id/idempotency key, writer epoch, and policy approval as the underlying Cohesix ticket/control action.
- Gateway audit lines must record protocol (`mcp` or `a2a`), method, tool/resource/prompt/skill/task name, delegated ticket hash, Cohesix path/action, idempotency key, writer epoch, upstream ACK/ERR, task state, and evidence refs.
- MCP clients cannot supply arbitrary upstream paths for provider-specific mutating tools; provider tools must expand from checked target fields into manifest-allowlisted Cohesix paths/actions.
- A2A clients cannot supply arbitrary provider targets, host paths, or executor commands through message text or metadata; A2A skill inputs must expand only into manifest-allowlisted Cohesix paths/actions.
- Tool listing, prompt listing, Agent Cards, and A2A skills are not authorization. Calls fail closed if the current request lacks the required delegated scope.
- Remote MCP and A2A transports inherit loopback default, non-loopback exposure warning, origin validation, request-auth, rate limits, broker backpressure, and bounded response sizes.
- MCP and A2A conformance tests must include prompt-injection and confused-deputy negative cases: a resource, prompt, Agent Card, or peer message that asks the model to bypass tickets must not change server-side authorization.

As-built leverage:
- Reuse REST delegated identity from 28b, host-ticket WAL/replay, evidence redaction, policy rules, and gateway queue/backpressure controls.

---

### 7) Ecosystem conformance and client configuration
**Purpose:** Make Cohesix usable from standard MCP hosts and A2A peers without custom client forks.

Implementation requirements:
- Add checked examples for:
  - local stdio MCP server config,
  - remote Streamable HTTP MCP endpoint config,
  - read-only resource browsing,
  - delegated mutating tool calls with explicit ticket/auth configuration,
  - A2A Agent Card discovery,
  - A2A task submission, streaming status, artifact retrieval, and cancellation against mock/dry-run providers.
- Add checked protocol fixtures/schemas for MCP JSON-RPC messages, A2A HTTP+JSON requests, gateway REST/OpenAPI compatibility, and generated provider action schemas so future client regressions are reviewable as data.
- Validate with at least one MCP inspector/client conformance path and archive the transcript/output under the milestone evidence directory.
- Validate with at least one A2A-compatible client/conformance path and archive the transcript/output under the milestone evidence directory.
- Add a gateway protocol performance probe covering MCP resource reads, MCP tool calls that submit host tickets, A2A task creation/status streaming, and backpressure/refusal paths. Compare against REST read/write behavior from the accepted 28b gateway authority baseline; do not treat this as Pi hardware throughput proof unless the gateway probe exposes an upstream runtime regression.
- Document how MCP clients should treat Cohesix resources, tools, prompts, approval prompts, and errors.
- Document how A2A peers should treat Cohesix Agent Cards, skills, task status, artifacts, push notification limits, and errors.
- Expose deterministic error mapping from Cohesix `ERR` lines and REST gateway errors into MCP errors without losing the original Cohesix reason.
- Expose deterministic error mapping from Cohesix `ERR` lines, host-ticket refusals, and REST gateway errors into A2A task/error states without losing the original Cohesix reason.

As-built leverage:
- Reuse `docs/HOST_API.md`, `docs/API_GUIDELINES.md`, `docs/HOST_TOOLS.md`, `resources/openapi/hive-gateway.yaml`, and existing gateway status counters.

---

### 8) `coh mount` interoperability: REST primary, MCP context view optional
**Purpose:** Keep `coh mount --rest-url` as the direct gateway-backed namespace mount, while adding a useful MCP-facing filesystem view only where MCP resource discovery brings additional value.

Implementation requirements:
- Preserve the existing `coh mount --rest-url` behavior as the canonical FUSE view over Cohesix namespaces through `hive-gateway`; it remains the path for normal file-shaped reads and append-only writes.
- Add an optional MCP resource mount mode only if the MCP server exposes a resource/tool/prompt catalog that a local filesystem consumer cannot get from the existing mount without speaking MCP:
  - `coh mount --mcp-url <endpoint> --read-only --at <path>` mounts MCP-admitted context, not the full Cohesix namespace.
  - The mounted tree exposes bounded MCP resources, resource templates, tool schemas, prompt templates, and evidence/resource links as files.
  - Resource file reads call MCP `resources/list`, `resources/templates/list`, and `resources/read`; tool and prompt catalog files are generated from `tools/list`, `prompts/list`, and `prompts/get`.
  - The tree must make the Cohesix backing path or action explicit for every resource/tool entry so reviewers can trace MCP context back to existing `LS`/`CAT`/`TAIL`/`ECHO` or `/host/tickets/spec` semantics.
- The MCP mount is read-only by default and in the milestone acceptance path. Writes, renames, chmod, symlink creation, and host filesystem path escapes fail deterministically with no MCP `tools/call`.
- If a later task proposes write-capable MCP mount nodes, it must be a separate breaking-risk review and may only append a fully validated `host-ticket/v1` line with delegated ticket, idempotency key, writer epoch, policy approval, and local operator confirmation. It must never map arbitrary file writes to arbitrary MCP tools.
- A2A does not get a FUSE mode in this milestone. A2A task status and artifact links may appear as read-only MCP/evidence files, but task creation remains an A2A HTTP operation or an existing Cohesix ticket/control write.
- `coh doctor` and mount validation should report whether the REST mount, MCP resource mount, both, or neither are available, and should distinguish FUSE availability from MCP protocol availability.
- MCP resource mount caches must be bounded, TTL-governed, and invalidated on MCP list-change notifications when enabled; stale cache reads must be marked as stale rather than silently presented as live state.

As-built leverage:
- Reuse `coh mount` FUSE validators, REST mount exclusivity, `CohAccess` read helpers, gateway MCP resource catalog, and evidence redaction rules.

---

### 9) Operator walkthrough and docs-as-built alignment
**Purpose:** Keep operator-facing guidance accurate as MCP, A2A, gateway REST, and mount modes become adjacent surfaces.

Implementation requirements:
- Audit and update `docs/OPERATOR_WALKTHROUGH.md` so the happy path, prerequisites, command ordering, failure handling, and expected evidence match the as-built gateway/MCP/A2A/mount behavior.
- Audit and update related canonical docs in the same milestone work:
  - `docs/HOST_API.md` for REST, MCP, and A2A endpoint/auth behavior,
  - `docs/HOST_TOOLS.md` for `coh mount --rest-url`, optional `coh mount --mcp-url`, `hive-gateway`, A2A Agent Card/task facade, host-ticket-agent, GPU bridge, and sidecar workflows,
  - `docs/API_GUIDELINES.md` for transport choice and MCP-vs-A2A-vs-REST-vs-filesystem guidance,
  - `docs/USERLAND_AND_CLI.md` for operator-visible commands and grammar-stability wording,
  - `docs/INTERFACES.md` for path/action mappings and refusal semantics,
  - `docs/ARCHITECTURE.md` for host-only MCP/A2A projections and the VM/host boundary,
  - `docs/SECURITY.md` for delegated ticket, prompt-injection, confused-deputy, redaction, and non-loopback exposure guidance,
  - `docs/TEST_PLAN.md` for the MCP, A2A, and mount evidence matrix.
- The audit must start from as-built code and generated truth:
  - `apps/hive-gateway/src/**`,
  - `apps/coh/src/mount.rs`,
  - `apps/coh/src/doctor.rs`,
  - `apps/host-ticket-agent/src/executors/**`,
  - `docs/snippets/*`,
  - generated manifests and `coh-rtc` outputs.
- Documentation must distinguish:
  - direct TCP `cohsh` proof,
  - REST/gateway proof,
  - gateway-backed `coh mount --rest-url`,
  - optional read-only MCP resource mount,
  - MCP tools/prompts that submit Cohesix tickets rather than executing host commands directly,
  - A2A Agent Card discovery, task submission, streaming status, artifact retrieval, and refusal behavior.
- Generated snippets and derived docs must be refreshed through `coh-rtc` or their owning generator; hand-editing generated blocks is invalid.

As-built leverage:
- Reuse 26c docs-as-built audit discipline, existing host-tool docs, generated snippets, and the 28d MCP/A2A conformance evidence.

**Commands**
- `cargo test -p hive-gateway`
- `cargo test -p hive-gateway --test mcp_protocol`
- `cargo test -p hive-gateway --test mcp_resources`
- `cargo test -p hive-gateway --test mcp_tools`
- `cargo test -p hive-gateway --test mcp_prompts`
- `cargo test -p hive-gateway --test mcp_security`
- `cargo test -p hive-gateway --test gateway_action_registry`
- `cargo test -p hive-gateway --test a2a_protocol`
- `cargo test -p hive-gateway --test a2a_tasks`
- `cargo test -p hive-gateway --test a2a_security`
- `cargo test -p coh --test mount_mcp`
- `cargo test -p host-ticket-agent`
- `cargo test -p coh --test evidence_pack`
- `cargo test -p coh --test evidence_timeline`
- `cargo test -p coh-rtc`
- `scripts/ci/gateway_perf_probe.sh --scenario mcp-a2a-protocols --state-dir out/bench/m28d-gateway-protocols`
- `git diff --check -- docs/BUILD_PLAN.md docs/OPERATOR_WALKTHROUGH.md docs/HOST_API.md docs/HOST_TOOLS.md docs/API_GUIDELINES.md docs/USERLAND_AND_CLI.md docs/INTERFACES.md docs/ARCHITECTURE.md docs/SECURITY.md docs/TEST_PLAN.md`
- `scripts/check-generated.sh`
- `scripts/cohsh/run_regression_batch.sh`
- `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m28d-qemu-gateway-agents`

**Checks (Definition of Done)**
- MCP lifecycle, `MCP-Protocol-Version`, optional `Mcp-Session-Id`, cancellation, `tools/list`, `tools/call`, `resources/list`, `resources/templates/list`, `resources/read`, `prompts/list`, and `prompts/get` pass against the accepted protocol revision recorded in the docs.
- A2A Agent Card discovery, Agent Card `protocolVersion`, optional authenticated extended Agent Card, and the generated `SendMessage`, streaming, task query/list/cancel/subscribe, push-notification, artifact, and update mappings pass against the accepted protocol revision and binding recorded in the docs.
- `crates/cohsh-core/fixtures/grammar.sha256` and generated `docs/snippets/cohsh_grammar.md` remain unchanged unless a separately approved breaking grammar milestone changes them.
- Every MCP read maps to existing `LS`, `CAT`, or `TAIL`; every MCP write maps to existing `ECHO` into a documented Cohesix control file or `/host/tickets/spec`.
- Every A2A task maps to an existing 28c run/checkpoint/evidence record and, when mutating, an existing Cohesix host-ticket/control action. No A2A message text or metadata becomes authorization.
- Read-only MCP acceptance passes before mutating MCP tools or A2A task creation are enabled in the milestone evidence path.
- Read-only MCP acceptance is not sufficient evidence for mutating tools, A2A task creation, provider action execution, or production VM worker/driver authority. Each mutating acceptance artifact must name the 28b, 28b1, 28c, and, when VM worker/driver authority is claimed, 28e evidence it depends on.
- Read-only MCP and A2A artifact/resource acceptance includes negative tests for public, ticket-scoped, and admin-only read visibility; ticket/provider/evidence/audit reads for the wrong delegated identity fail before payload construction.
- MCP tool schemas and A2A skill schemas are generated from the same provider action registry; parity tests fail if CUDA/GPU, PEFT, NeMo, K8s, systemd, Docker, or evidence actions drift between protocols.
- No MCP tool directly invokes host executors, shell commands, CUDA/NVML calls, PEFT filesystem mutation, NeMo endpoints, `systemctl`, `docker`, or `kubectl` outside the existing Cohesix adapters.
- No A2A skill directly invokes host executors, shell commands, CUDA/NVML calls, PEFT filesystem mutation, NeMo endpoints, `systemctl`, `docker`, or `kubectl` outside the existing Cohesix adapters.
- CUDA/GPU, PEFT, NeMo, K8s, systemd, and Docker scenarios have deterministic mock tests and at least one live-safe dry-run/conformance transcript.
- Mutating tools and A2A task actions fail without delegated scope and leave no side effects; duplicate mutating calls with the same id/idempotency key do not duplicate side effects.
- Remote MCP and A2A endpoints validate `Origin`, enforce auth, respect loopback defaults, and return bounded protocol errors under gateway backpressure.
- Evidence packs and timelines can reconstruct the MCP call or A2A task, delegated ticket hash, underlying Cohesix path/action, provider receipt, and final state without raw secret leakage.
- Standard MCP clients can discover resources/tools/prompts and call read-only tools without Cohesix-specific patches.
- Standard A2A clients can discover the Agent Card, submit a dry-run task, observe status/artifacts, and handle refusals without Cohesix-specific patches.
- Existing `coh mount --rest-url` semantics remain unchanged, including REST mount exclusivity and append-only write behavior.
- `coh mount --mcp-url` exposes only MCP-admitted resources/tool schemas/prompt templates/evidence links, is read-only in the acceptance path, and never invokes MCP tools during filesystem metadata or write operations.
- There is no A2A FUSE mode; A2A task/artifact state appears through gateway protocol responses and read-only evidence/resource projections only.
- MCP-mounted resource contents match the corresponding MCP `resources/read` output and, for Cohesix namespace-backed resources, the corresponding REST/console read within documented bounds.
- A2A artifacts and push notification attempts are bounded, redacted, policy-gated, and reconstructable from audit/evidence without raw secret leakage.
- Gateway protocol performance evidence shows MCP resource/tool and A2A task/artifact paths stay bounded relative to the 28b gateway authority baseline; any full Pi/QEMU benchmark is triggered only by evidence of upstream runtime-path regression.
- `docs/OPERATOR_WALKTHROUGH.md` and related canonical docs describe the as-built transport, mount, MCP, A2A, host-ticket, provider, and evidence behavior without claiming implemented support before code/tests/generated outputs exist.

**Compiler touchpoints**
- `coh-rtc` emits `gateway.mcp.*` policy:
  - enabled transports (`stdio`, `streamable_http`),
  - accepted MCP protocol revision,
  - JSON Schema dialect and authorization mode for the accepted revision,
  - HTTP protocol-version header policy, optional session-id policy, cancellation policy, and SSE enablement/bounds,
  - endpoint path,
  - resource URI roots and path allowlists,
  - tool allowlists and provider action mappings,
  - prompt template ids,
  - MCP resource-mount enablement, read-only requirement, cache TTL, and synthetic tree bounds,
  - per-tool max input/output bytes,
  - delegated-ticket and writer-epoch requirements,
  - redaction and evidence-export flags.
- Manifest validation rejects MCP enablement when Milestone 28b delegated write identity or required audit/replay/fencing prerequisites are disabled for mutating tools.
- Manifest validation rejects NeMo MCP tools unless the 28c optional NeMo provider family and parity checks are enabled.
- `coh-rtc` emits a protocol-neutral `gateway.provider_actions.*` projection derived solely from the Milestone 28b1 `providers.*` registry for every provider operation exposed through MCP tools or A2A skills, including action ids, target schema refs, dry-run support, idempotency requirements, writer-epoch requirements, receipt schema refs, and evidence-export behavior. It is generated compatibility metadata, not a second action registry or authority source.
- Manifest validation rejects any MCP tool or A2A skill whose provider action mapping is absent from the shared registry or whose schema diverges between the two protocol projections.
- `coh-rtc` emits `gateway.a2a.*` policy:
  - enabled endpoint/binding,
  - accepted A2A protocol revision,
  - binding-specific operation and endpoint mappings for that revision,
  - Agent Card `protocolVersion` and extension policy,
  - Agent Card path, provider metadata, skill ids, and interface declarations,
  - task, artifact, stream, and push-notification bounds,
  - skill allowlists and provider action mappings,
  - per-skill max input/output bytes,
  - delegated-ticket, idempotency, and writer-epoch requirements,
  - redaction, evidence-export, and callback allowlist flags.
- Manifest validation rejects A2A enablement when Milestone 28b/28c delegated authority, durable run/task state, audit/replay, evidence export, or fencing prerequisites are disabled for task-creating or task-mutating skills.
- Manifest validation rejects NeMo A2A skills unless the 28c optional NeMo provider family and parity checks are enabled.
- Generated docs refresh:
  - `docs/HOST_API.md`
  - `docs/API_GUIDELINES.md`
  - `docs/HOST_TOOLS.md`
  - `docs/INTERFACES.md`
  - `docs/ARCHITECTURE.md`
  - `docs/SECURITY.md`
  - `docs/TEST_PLAN.md`
  - `docs/USERLAND_AND_CLI.md`
- Human-authored as-built docs refreshed in:
  - `docs/OPERATOR_WALKTHROUGH.md`

**Task Breakdown**
```
Title/ID: m28d-mcp-policy-ir
Goal: Admit MCP gateway policy in compiler IR without changing Cohesix console or NineDoor grammar.
Inputs: tools/coh-rtc, configs/root_task.toml, docs/HOST_API.md, docs/SECURITY.md.
Changes:
  - tools/coh-rtc/src/ir.rs — `gateway.mcp.*` schema for revision, JSON Schema dialect, authorization mode, transports, endpoint, resource roots, tool allowlists, prompt ids, bounds, and prerequisite gates.
  - tools/coh-rtc/src/validate.rs — reject mutating MCP tools without delegated identity, audit/replay, and provider action prerequisites, and reject non-loopback production profiles whose authorization mode does not satisfy the pinned revision.
  - tools/coh-rtc/src/codegen/* — generated gateway MCP defaults and docs snippets.
Commands: cargo test -p coh-rtc && scripts/check-generated.sh
Checks: MCP revision/schema/auth policy is compiler-owned, profile-gated, conformance claims match the selected mode, and `cohsh-core` grammar specs remain untouched.
Deliverables: Generated MCP gateway policy and validation gates.

Title/ID: m28d-a2a-policy-ir
Goal: Admit A2A gateway policy in compiler IR without changing Cohesix console or NineDoor grammar.
Inputs: tools/coh-rtc, configs/root_task.toml, docs/HOST_API.md, docs/SECURITY.md, docs/TEST_PLAN.md.
Changes:
  - tools/coh-rtc/src/ir.rs — `gateway.a2a.*` schema for endpoint/binding, accepted revision, binding-specific operation mappings, Agent Card path, skill ids, task/artifact/stream/push bounds, callback allowlists, and prerequisite gates.
  - tools/coh-rtc/src/validate.rs — reject mixed-revision or mixed-binding operation names and reject A2A task-creating or task-mutating skills without 28b delegated authority, 28c durable run/task state, audit/replay, evidence export, and provider action prerequisites.
  - tools/coh-rtc/src/codegen/* — generated A2A gateway defaults, Agent Card metadata, and docs snippets.
Commands: cargo test -p coh-rtc && scripts/check-generated.sh
Checks: A2A revision/binding mappings are compiler-owned and internally consistent, profile gates hold, and `cohsh-core` grammar specs and NineDoor semantics remain untouched.
Deliverables: Generated A2A gateway policy, Agent Card metadata, and validation gates.

Title/ID: m28d-provider-action-registry-projection
Goal: Consume the Milestone 28b1 provider action registry so MCP tools and A2A skills cannot define independent action schemas.
Inputs: tools/coh-rtc, apps/hive-gateway, apps/host-ticket-agent, crates/host-cuda, docs/INTERFACES.md, docs/HOST_API.md.
Changes:
  - tools/coh-rtc/src/validate.rs — reject MCP/A2A projections that reference undeclared 28b1 provider actions or drift from generated target/receipt/read-visibility schemas.
  - apps/hive-gateway/src/actions/registry.rs — generated 28b1 provider action registry view consumed by MCP tools, A2A skills, evidence mapping, and security checks.
  - apps/hive-gateway/tests/gateway_action_registry.rs — parity fixtures proving MCP and A2A expose the same allowed actions, bounds, receipt refs, and refusal semantics where the same provider operation exists.
Commands: cargo test -p coh-rtc && cargo test -p hive-gateway --test gateway_action_registry && scripts/check-generated.sh
Checks: Provider action metadata is generated once by 28b1, remains protocol-neutral, and rejects action drift between MCP tools, A2A skills, host-ticket lines, read visibility classes, and evidence receipts.
Deliverables: Gateway protocol projection and parity tests for CUDA/GPU, PEFT, NeMo, K8s, systemd, Docker, and evidence operations without a second registry.

Title/ID: m28d-gateway-mcp-transport
Goal: Implement MCP lifecycle and stdio/Streamable HTTP transport in `hive-gateway`.
Inputs: apps/hive-gateway/src/main.rs, gateway auth/broker code, docs/HOST_API.md.
Changes:
  - apps/hive-gateway/src/mcp/protocol.rs — MCP JSON-RPC lifecycle, capability negotiation, pagination, and error mapping.
  - apps/hive-gateway/src/mcp/transport.rs — stdio and Streamable HTTP endpoint handling with stdout/stderr separation and Origin validation.
  - apps/hive-gateway/src/main.rs — CLI/env flags for MCP enablement and endpoint selection.
Commands: cargo test -p hive-gateway --test mcp_protocol
Checks: Standard lifecycle and discovery requests work over both transports; invalid JSON-RPC, bad Origin, missing auth, and oversize messages fail deterministically.
Deliverables: MCP-capable gateway process with safe defaults.

Title/ID: m28d-mcp-resource-catalog
Goal: Expose Cohesix state as MCP resources backed only by bounded existing reads.
Inputs: apps/hive-gateway, apps/coh/src/evidence.rs, generated path defaults.
Changes:
  - apps/hive-gateway/src/mcp/resources.rs — `cohesix://` URI catalog, templates, path validation, and read dispatch through existing `LS`/`CAT`/`TAIL`.
  - apps/hive-gateway/tests/mcp_resources.rs — resource list/read fixtures for `/proc`, `/gpu`, `/host`, and evidence summaries.
Commands: cargo test -p hive-gateway --test mcp_resources
Checks: Resource reads enforce manifest bounds, reject undeclared paths, redact secrets, and match existing REST/console output for canonical fixtures.
Deliverables: MCP resource catalog that is a faithful read-only projection of Cohesix state.

Title/ID: m28d-mcp-prompts
Goal: Add MCP prompt templates for safe operational workflows over existing Cohesix tools/resources.
Inputs: apps/hive-gateway/src/mcp, docs/HOST_TOOLS.md, docs/SECURITY.md.
Changes:
  - apps/hive-gateway/src/mcp/prompts.rs — prompt templates for CUDA triage, PEFT promotion, NeMo readiness, K8s drain, systemd recovery, and Docker remediation.
  - docs/HOST_TOOLS.md — operator guidance for MCP prompt use, approval expectations, and non-authority status.
Commands: cargo test -p hive-gateway --test mcp_prompts
Checks: Prompts contain no secrets, name exact Cohesix tools/actions, and require user approval before side-effecting tool calls.
Deliverables: MCP prompt catalog that improves operator ergonomics without becoming control state.

Title/ID: m28d-readonly-mcp-acceptance-gate
Goal: Prove read-only MCP transport, resources, prompts, and conformance before enabling mutating tools or A2A task creation.
Inputs: apps/hive-gateway, generated MCP policy, docs/HOST_API.md, docs/TEST_PLAN.md.
Changes:
  - apps/hive-gateway/tests/mcp_readonly_acceptance.rs — lifecycle, resources, templates, prompts, redaction, auth, Origin, and oversize negative fixtures with mutating tools disabled.
  - docs/TEST_PLAN.md — record read-only MCP acceptance as the first 28d evidence gate.
Commands: cargo test -p hive-gateway --test mcp_readonly_acceptance && scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m28d-qemu-mcp-readonly
Checks: Standard MCP clients can discover and read admitted context, but mutating tools are unavailable or deterministically refused until provider action registry and delegated-authority gates pass.
Deliverables: Archived read-only MCP conformance evidence that later mutating MCP/A2A work must cite.

Title/ID: m28d-mcp-tool-catalog
Goal: Expose real Cohesix operations as MCP tools without direct host execution.
Inputs: apps/hive-gateway, apps/host-ticket-agent, apps/coh, crates/host-cuda, docs/INTERFACES.md.
Changes:
  - apps/hive-gateway/src/mcp/tools.rs — schema-defined tools for file reads, CUDA/GPU inventory, host-ticket submission, GPU leases, PEFT, NeMo, K8s, systemd, Docker, and evidence summaries.
  - apps/hive-gateway/src/mcp/tickets.rs — host-ticket line builder with id/idempotency/writer-epoch validation and provider action mapping.
  - apps/hive-gateway/tests/mcp_tools.rs — success and refusal fixtures for read-only, delegated mutating, duplicate, and unauthorized calls.
Commands: cargo test -p hive-gateway --test mcp_tools && cargo test -p host-ticket-agent
Checks: Mutating tools append only validated Cohesix ticket/control lines; provider executors are never called directly by the MCP server; read-only MCP acceptance and provider action registry parity evidence already exists.
Deliverables: Tool catalog covering CUDA/GPU, PEFT, NeMo, K8s, systemd, and Docker under existing Cohesix authority.

Title/ID: m28d-a2a-agent-facade
Goal: Implement A2A Agent Card discovery and task/message/artifact projection over existing Cohesix run and ticket state.
Inputs: apps/hive-gateway, apps/host-ticket-agent, apps/coh/src/evidence.rs, generated provider action registry, docs/HOST_API.md, docs/API_GUIDELINES.md, accepted A2A protocol revision.
Changes:
  - apps/hive-gateway/src/a2a/agent_card.rs — generated Agent Card publication with enabled skills, auth requirements, supported interfaces, and no secret/internal executor data.
  - apps/hive-gateway/src/a2a/tasks.rs — generated `SendMessage`, streaming, task query/list/cancel/subscribe, push-notification, status, idempotency, and refusal mappings for the pinned A2A binding/revision, backed by 28c run/checkpoint/evidence records.
  - apps/hive-gateway/src/a2a/artifacts.rs — bounded redacted artifact references for evidence packs, timelines, checkpoints, provider receipts, and Cohesix resource refs.
  - apps/hive-gateway/src/a2a/push.rs — disabled-by-default push notification config with allowlist, SSRF validation, per-task auth material, bounded retry, and audit evidence.
  - apps/hive-gateway/tests/a2a_protocol.rs + apps/hive-gateway/tests/a2a_tasks.rs — Agent Card, message, stream, task, artifact, cancel, push-refusal, duplicate, and unauthorized fixtures.
Commands: cargo test -p hive-gateway --test a2a_protocol && cargo test -p hive-gateway --test a2a_tasks
Checks: A2A clients can discover Cohesix skills, submit dry-run tasks, observe status/artifacts, and receive deterministic refusals; no A2A path bypasses Cohesix tickets, run records, gateway auth, or provider allowlists; provider action registry parity and read-only MCP acceptance evidence already exists.
Deliverables: A2A-compatible gateway facade for bounded Cohesix delegation and observation.

Title/ID: m28d-coh-mount-mcp-resource-view
Goal: Add an optional read-only `coh mount --mcp-url` mode for MCP-admitted resources, schemas, prompts, and evidence links without replacing the existing REST-backed Cohesix namespace mount.
Inputs: apps/coh/src/mount.rs, apps/coh/src/doctor.rs, apps/hive-gateway/src/mcp/resources.rs, apps/hive-gateway/src/mcp/tools.rs, docs/HOST_TOOLS.md, docs/API_GUIDELINES.md.
Changes:
  - apps/coh/src/mount.rs — read-only MCP resource/catalog FUSE adapter with bounded readdir/read, no write-to-tool translation, no symlink/rename/chmod support, and explicit stale-cache markers.
  - apps/coh/src/main.rs — `coh mount --mcp-url <endpoint> --read-only --at <path>` flags that are mutually exclusive with write-capable REST/console mount modes.
  - apps/coh/src/doctor.rs — report REST mount availability separately from MCP resource-mount protocol/FUSE availability.
  - apps/coh/tests/mount_mcp.rs — fixtures for resource reads, tool schema files, prompt template files, cache invalidation, write denial, and parity with MCP `resources/read`.
  - docs/HOST_TOOLS.md + docs/API_GUIDELINES.md — document when to use REST mount versus MCP resource mount and why MCP mount is read-only by default.
Commands: cargo test -p coh --test mount_mcp && cargo test -p hive-gateway --test mcp_resources
Checks: Existing `coh mount --rest-url` behavior is unchanged; MCP mount reads only MCP-admitted context, refuses all filesystem mutations by default, never calls MCP tools from FUSE operations, and traces every mounted file back to a Cohesix path/action or MCP catalog entry.
Deliverables: Agent- and filesystem-friendly MCP context mount that adds discovery/schema/prompt value without adding a new Cohesix write path.

Title/ID: m28d-operator-docs-as-built-audit
Goal: Audit and update the Operator Walkthrough plus related canonical docs so gateway, MCP, A2A, mount, provider, and evidence guidance matches the as-built implementation.
Inputs: docs/OPERATOR_WALKTHROUGH.md, docs/HOST_API.md, docs/HOST_TOOLS.md, docs/API_GUIDELINES.md, docs/USERLAND_AND_CLI.md, docs/INTERFACES.md, docs/ARCHITECTURE.md, docs/SECURITY.md, docs/TEST_PLAN.md, resources/openapi/hive-gateway.yaml, apps/hive-gateway/src/**, apps/coh/src/mount.rs, apps/coh/src/doctor.rs, apps/host-ticket-agent/src/executors/**, docs/snippets/**.
Changes:
  - docs/OPERATOR_WALKTHROUGH.md — operator sequence for direct TCP, REST/gateway, `coh mount --rest-url`, MCP resources/tools/prompts, A2A Agent Card/task flow, optional read-only MCP resource mount, evidence capture, and deterministic failure handling.
  - docs/HOST_API.md + docs/API_GUIDELINES.md — MCP and A2A endpoint/auth/error semantics and transport-choice guidance aligned with the REST gateway contract.
  - docs/HOST_TOOLS.md + docs/USERLAND_AND_CLI.md — command references and prerequisites for `hive-gateway`, `coh mount`, MCP resource mount, A2A task facade, host-ticket-agent, GPU bridge, sidecar bridge, and grammar-stability constraints.
  - docs/INTERFACES.md + docs/ARCHITECTURE.md + docs/SECURITY.md + docs/TEST_PLAN.md — as-built path/action/task/artifact mappings, VM/host boundary, delegated ticket/security posture, and evidence matrix.
  - resources/openapi/hive-gateway.yaml — keep REST/OpenAPI routes aligned with any gateway endpoint additions and document that MCP/A2A are adjacent protocol surfaces, not REST authority replacements.
Commands: git diff --check -- docs/OPERATOR_WALKTHROUGH.md docs/HOST_API.md docs/HOST_TOOLS.md docs/API_GUIDELINES.md docs/USERLAND_AND_CLI.md docs/INTERFACES.md docs/ARCHITECTURE.md docs/SECURITY.md docs/TEST_PLAN.md && scripts/check-generated.sh
Checks: Documentation distinguishes direct TCP proof, REST gateway proof, gateway-backed FUSE mount, optional read-only MCP resource mount, MCP ticket-submission tools, and A2A task/artifact/status behavior; no doc claims MCP, A2A, mount, provider, or host-ticket behavior that lacks code/tests/generated evidence.
Deliverables: Operator-facing documentation that is coherent with as-built 28d behavior and usable for live, mock, and dry-run workflows.

Title/ID: m28d-gateway-protocol-performance
Goal: Prove MCP/A2A gateway protocol projections remain bounded relative to the 28b gateway authority baseline.
Inputs: apps/hive-gateway, scripts/ci/gateway_perf_probe.sh, docs/BENCHMARKS.md, docs/TEST_PLAN.md, accepted 28b gateway authority artifacts.
Changes:
  - scripts/ci/gateway_perf_probe.sh — add MCP resource read, MCP ticket-submitting tool call, A2A task create/status stream, artifact read, and backpressure/refusal scenarios.
  - docs/BENCHMARKS.md + docs/TEST_PLAN.md — record protocol-projection performance as gateway evidence, separate from Pi/QEMU runtime throughput proof.
Commands:
  - scripts/ci/gateway_perf_probe.sh --scenario mcp-a2a-protocols --state-dir out/bench/m28d-gateway-protocols
Checks:
  - MCP/A2A protocol paths stay bounded relative to the 28b gateway authority baseline, with queue depth, refusal counts, auth checks, and evidence/audit emission cost recorded.
  - A full REST/Pi/QEMU benchmark is triggered only if gateway protocol evidence shows an upstream runtime-path regression.
Deliverables:
  - Gateway protocol performance ledger for MCP/A2A clients and downstream AI namespace work.

Title/ID: m28d-gateway-agent-security-conformance
Goal: Prove MCP/A2A ecosystem compatibility and preserve Cohesix security boundaries.
Inputs: apps/hive-gateway, docs/SECURITY.md, docs/TEST_PLAN.md, accepted MCP protocol revision, accepted A2A protocol revision.
Changes:
  - apps/hive-gateway/tests/mcp_security.rs — confused-deputy, prompt-injection, auth, Origin, backpressure, duplicate, and redaction negative tests.
  - apps/hive-gateway/tests/a2a_security.rs — Agent Card disclosure, message injection, task idempotency, artifact redaction, push callback, cancellation, auth, Origin, and backpressure negative tests.
  - docs/TEST_PLAN.md — MCP/A2A conformance and security evidence stage.
  - docs/HOST_API.md + docs/API_GUIDELINES.md — MCP/A2A endpoint, transport, auth, error, and client configuration guidance.
Commands: cargo test -p hive-gateway --test mcp_security && cargo test -p hive-gateway --test a2a_security && scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m28d-qemu-gateway-agents
Checks: Standard MCP clients and A2A peers can discover and call allowed read-only/dry-run flows; unauthorized, prompt-injected, forged callback, duplicate, or overbroad calls cannot bypass delegated tickets or Cohesix policy.
Deliverables: Archived MCP/A2A conformance and security evidence.
```

**Outcome**
After Milestone 28d:
- Cohesix can be used from standard MCP-capable agent hosts and A2A-capable peer agents through `hive-gateway`.
- MCP clients see useful resources, tools, and prompts for CUDA/GPU, PEFT, NeMo, K8s, systemd, and Docker operations.
- A2A peers can discover Cohesix skills, submit bounded dry-run or delegated tasks, observe task status, and retrieve redacted artifacts for CUDA/GPU, PEFT, NeMo, K8s, systemd, Docker, and evidence workflows.
- All side effects still flow through Cohesix tickets, files, policy, audit, and evidence.
- The VM grammar, Secure9P semantics, and generated manifest authority remain unchanged.

## Milestone 28e — VM Cap-Bundle Authority + Structured Fault Lifecycle <a id="28e"></a>
[Milestones](#Milestones)

**Why now (VM authority closure):** Milestone 28b makes host/gateway writes attributable, idempotent, fenced, durable, and audit-first. The remaining VM-side authority concern is separate: production worker and linked-driver tickets must correspond to generated seL4 cap bundles, and faults must revoke stale authority with bounded evidence. This milestone closes that seL4-facing gap without delaying the host actuation floor that 28c and 28d need.

**Prerequisites**
- Milestone **26c** `m26c-cap-backed-worker-endpoints` completed, including generated role state for badged endpoint caps and negative metadata-only ticket tests.
- Milestone **26d** seL4 baseline refresh completed for the selected profiles, so CSpace/VSpace/syscall assumptions match the accepted seL4 generated artifacts.
- Milestone **28b** completed, including audit/replay defaults and generated gates that distinguish host authority records from VM cap-backed tickets.

**Production authority gate:** Milestone 28e is mandatory for any production profile that claims VM worker or linked-driver authority is represented by live seL4 cap bundles, including AI namespace projections, MCP/A2A task projections, provider projections, and driver-runtime control paths. Read-only profiles, host-ticket-only profiles, and gateway projections may proceed without 28e only when their generated profile state and docs explicitly avoid VM cap-bundle or structured-fault claims.

**Goal**
Complete production VM authority by making worker and driver tickets correspond to generated seL4 cap bundles and by converting worker/driver faults into structured lifecycle events.

**Non-Goals (Explicit)**
- No change to REST delegated identity, host-ticket action semantics, MCP/A2A protocol surfaces, or AI run control.
- No new VM protocol, console grammar, Secure9P verb, or root-owned physical-driver path.
- No claim that host tickets, REST delegated tickets, provider tickets, or PEFT receipts are seL4 cap-backed unless a generated VM projection explicitly maps them to live caps.

**Deliverables**
- Generated per-role cap-bundle records for endpoint caps, notification caps, fault endpoint caps, shared-ring frames, allowed data frames, declared MMIO frames, and DMA/shared-buffer frames where applicable.
- Root-task constructs per-role child CSpaces/VSpaces from generated bundle records and never hands out catch-all root authority, broad namespace authority, or undeclared frame caps.
- Revocation deletes/revokes derived caps and makes late invocations, stale shared-ring turns, and stale telemetry writes fail deterministically.
- Every production worker and driver TCB receives a generated badged fault endpoint whose badge identifies role, instance, lease epoch, and cap-bundle generation.
- Root-task records bounded fault evidence, revokes the affected cap bundle, marks the lease terminal or quarantined, and refuses late telemetry, shared-ring turns, or receipts from the old epoch.
- Restart is allowed only through a new ticket/lease/cap-bundle path; fault recovery must not silently reuse stale caps, stale DMA mappings, or stale shared rings.

**Commands**
- `cargo test -p root-task --tests cap_bundle`
- `cargo test -p root-task --tests fault`
- `cargo test -p root-task --test fault_recovery_timing`
- `cargo test -p pi4-driver-abi`
- `cargo test -p pi4-driver-runtime`
- `cargo test -p worker-heart`
- `cargo test -p worker-gpu`
- `cargo test -p worker-lora`
- `cargo test -p coh --test evidence`
- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`
- `scripts/check-generated.sh`
- `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m28e-qemu-cap-bundles`
- `scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m28e-pi4-cap-bundles`

**Checks (DoD)**
- Production cap-backed ticket profiles fail generation if any worker or driver role has metadata scope without the corresponding generated cap-bundle inventory.
- Production profiles that expose AI, MCP/A2A, provider, or driver-runtime projections fail validation if they claim VM worker/driver cap-bundle authority or structured-fault containment without the generated 28e cap-bundle and fault-lifecycle evidence.
- Read-only and host-ticket-only profiles remain valid without 28e only when generated profile state, docs, and evidence packs explicitly state that VM worker/driver cap-bundle authority is not claimed.
- Revoked tickets lose endpoint, notification, frame, shared-ring, MMIO, DMA/shared-buffer, and fault authority; stale invocations and stale ring turns fail deterministically.
- Recovery reconciles ticket ledger and CSpace/VSpace state so no active ticket exists without live caps and no live caps remain for terminal tickets.
- Fault badges deterministically identify the faulting worker or driver role, instance, lease epoch, and cap-bundle generation.
- Fault handling revokes old caps, rejects stale shared-ring turns and telemetry, and requires fresh lease/cap-bundle construction before restart.
- Fault evidence is bounded, redaction-safe where needed, included in evidence packs, and does not alter console grammar or Secure9P framing.
- Compatibility profiles may retain 26c endpoint-cap-only tickets only when docs and generated profile state explicitly say full cap bundles are disabled.
- Fault/revoke timing evidence stays bounded for representative worker and driver fault cases; material regressions are classified as cap-revocation, evidence-export, restart-policy, or driver-runtime recovery overhead before production profiles depend on structured fault recovery.

**Compiler touchpoints**
- `coh-rtc` emits full cap-bundle ticket authority profiles, generated per-role cap inventories, revoke/recovery evidence, production enablement gates, generated badged fault endpoint records, terminal/quarantine policy, and bounded fault evidence paths.
- `coh-rtc` emits explicit profile-state distinctions for read-only projection, host-ticket-only actuation, endpoint-cap compatibility, full VM cap-bundle authority, and structured-fault containment so downstream milestones cannot inherit stronger VM authority claims by implication.
- Generated snippets refresh `docs/WORKER_TICKETS.md`, `docs/SECURITY.md`, `docs/HARDWARE_BRINGUP.md`, `docs/INTERFACES.md`, and `docs/TEST_PLAN.md`.

**Task Breakdown**
```
Title/ID: m28e-full-cap-bundle-ticket-authority
Goal: Complete cap-backed tickets by making production worker and driver tickets correspond to generated seL4 cap bundles for endpoints, notifications, frames, shared rings, MMIO, DMA, and fault handling.
Inputs: apps/root-task/src/lifecycle.rs, apps/root-task/src/hal/**, apps/root-task/src/generated/**, apps/root-task/src/ninedoor.rs, apps/pi4-driver-runtime/src/**, crates/pi4-driver-abi/src/**, apps/worker-heart/src/**, apps/worker-gpu/src/**, apps/worker-lora/src/**, tools/coh-rtc/src/**, docs/WORKER_TICKETS.md, docs/SECURITY.md, docs/HARDWARE_BRINGUP.md, docs/TEST_PLAN.md
Changes:
  - tools/coh-rtc/src/** — add generated per-role cap-bundle records for endpoint, notification, fault, shared-ring, frame, MMIO, DMA/shared-buffer, and revoke policy fields.
  - apps/root-task/src/lifecycle.rs + apps/root-task/src/hal/** — construct child CSpaces/VSpaces from generated cap-bundle records and retain parent/origin caps for deterministic revoke.
  - apps/root-task/src/ninedoor.rs + apps/root-task/src/event/** — reconcile ticket ledger state with live cap-bundle state and refuse metadata-only authority in production profiles.
  - apps/pi4-driver-runtime/src/** + crates/pi4-driver-abi/src/** — bind driver-runtime command/completion, MMIO, DMA, shared-buffer, IRQ notification, and fault handling to generated cap-bundle descriptors.
  - apps/worker-heart/src/** + apps/worker-gpu/src/** + apps/worker-lora/src/** — consume full role cap bundles where enabled while preserving the 26c endpoint-cap compatibility floor.
  - docs/WORKER_TICKETS.md + docs/SECURITY.md + docs/HARDWARE_BRINGUP.md + docs/TEST_PLAN.md — document full cap-bundle semantics, production profile gates, revoke/recovery behavior, and negative tests.
Commands:
  - cargo test -p root-task --tests cap_bundle
  - cargo test -p pi4-driver-abi
  - cargo test -p pi4-driver-runtime
  - cargo test -p worker-heart
  - cargo test -p worker-gpu
  - cargo test -p worker-lora
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
  - scripts/check-generated.sh
Checks:
  - Production cap-backed ticket profiles fail generation if any worker or driver role has metadata scope without the corresponding generated cap-bundle inventory.
  - Revoked tickets lose endpoint, notification, frame, shared-ring, MMIO, DMA/shared-buffer, and fault authority; stale invocations and stale ring turns fail deterministically.
  - Recovery reconciles ticket ledger and CSpace/VSpace state so no active ticket exists without live caps and no live caps remain for terminal tickets.
Deliverables:
  - Production-grade cap-backed ticket authority with generated seL4 cap bundles and evidence strong enough to answer the seL4 criticism that tickets are only application metadata.

Title/ID: m28e-structured-fault-lifecycle
Goal: Convert worker and driver seL4 faults into structured lifecycle events that revoke stale authority and produce bounded evidence.
Inputs: apps/root-task/src/lifecycle.rs, apps/root-task/src/event/**, apps/root-task/src/hal/**, apps/root-task/src/generated/**, apps/pi4-driver-runtime/src/**, apps/worker-heart/src/**, apps/worker-gpu/src/**, apps/worker-lora/src/**, apps/coh/src/evidence.rs, tools/coh-rtc/src/**, docs/SECURITY.md, docs/INTERFACES.md, docs/TEST_PLAN.md
Changes:
  - tools/coh-rtc/src/** — emit generated badged fault endpoint records with role, instance, lease epoch, cap-bundle generation, terminal/quarantine policy, and bounded evidence paths.
  - apps/root-task/src/lifecycle.rs + apps/root-task/src/event/** — receive worker/driver fault IPC, record bounded evidence, revoke the affected cap bundle, and transition the lease to terminal or quarantined state.
  - apps/root-task/src/hal/** + apps/pi4-driver-runtime/src/** — ensure driver faults cut off IRQ, shared-ring, MMIO, DMA/shared-buffer, and fault authority for the old epoch.
  - apps/worker-heart/src/** + apps/worker-gpu/src/** + apps/worker-lora/src/** — restart only through a fresh ticket/lease/cap-bundle path and refuse stale telemetry or receipts after a fault epoch.
  - apps/root-task/tests/fault_recovery_timing.rs — bounded timing checks for revoke, evidence export, stale-turn refusal, and fresh restart admission under representative worker/driver faults.
  - apps/coh/src/evidence.rs + docs/SECURITY.md + docs/INTERFACES.md + docs/TEST_PLAN.md — include bounded fault lifecycle records in evidence exports and document restart/quarantine semantics.
Commands:
  - cargo test -p root-task --tests fault
  - cargo test -p root-task --test fault_recovery_timing
  - cargo test -p pi4-driver-runtime
  - cargo test -p worker-heart
  - cargo test -p worker-gpu
  - cargo test -p worker-lora
  - cargo test -p coh --test evidence
  - cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
  - scripts/check-generated.sh
Checks:
  - Fault badges deterministically identify the faulting worker or driver role, instance, lease epoch, and cap-bundle generation.
  - Fault handling revokes old caps, rejects stale shared-ring turns and telemetry, and requires fresh lease/cap-bundle construction before restart.
  - Fault evidence is bounded, redaction-safe where needed, included in evidence packs, and does not alter console grammar or Secure9P framing.
  - Fault/revoke timing remains bounded and is reported as VM authority recovery evidence, not Pi/QEMU network throughput proof.
Deliverables:
  - Production fault lifecycle evidence that lets seL4 reviewers audit how Cohesix contains and recovers from faulty workers and drivers.
```


## Milestone 28f — SwarmUI Desktop Workbench: Spectrum Shell + Live Hive Continuity <a id="28f"></a>
[Milestones](#Milestones)

**Why now (operator workflow):** Milestone 20c/20d proved SwarmUI as a host-only, ticket-scoped UI and PixiJS Live Hive renderer. Milestone 24e added REST/gateway mode so SwarmUI can share the sole console client through `hive-gateway`. Milestone 28 gives Cohesix the read-only inspect, trace, bundle/evidence, diff, and attest substrate operators need. The remaining UI gap is workflow shape: SwarmUI is still organized like a dense dashboard instead of a desktop operator workbench. This milestone redesigns the presentation layer around familiar desktop navigation, Spectrum Web Components, deterministic evidence workflows, and the existing PixiJS Live Hive canvas without changing Cohesix authority or protocol semantics.

**As-built alignment note:** Current SwarmUI is a Tauri host UI with Rust-owned transport/session/cache/replay semantics, vendored Spectrum Web Components, and a PixiJS Live Hive renderer. Existing generated SwarmUI defaults still use `/worker` roots while the canonical worker namespace is `/shard/<label>/worker/<id>/telemetry` with `/worker/<id>/telemetry` available only when `sharding.legacy_worker_alias = true`. This milestone must present both correctly and must not hard-code legacy aliases as the future namespace shape.

**Prerequisites**
- Milestone **20c** complete for SwarmUI Tauri, ticket-scoped sessions, transcript parity, and bounded offline cache.
- Milestone **20d** complete for PixiJS Live Hive rendering, replay fixtures, design tokens, and no UI-owned control logic.
- Milestone **24e** complete for REST/gateway mode so desktop multi-tool workflows use `hive-gateway` rather than direct concurrent TCP clients.
- Milestone **28** complete for shared read-only inspect, trace, evidence-pack/timeline, diff, and attest internals reused by the workbench.
- Milestone **28b** is required before enabling any new delegated mutating REST workflow beyond existing console-projected `LS`/`CAT`/`ECHO` semantics. Before 28b, mutating desktop affordances may only render command previews, transcript proof, or existing console-compatible actions already admitted by the active profile.

**Goal**
Redesign SwarmUI into a desktop-style operator workbench that uses Spectrum for shell chrome, controls, forms, menus, dialogs, status, and workflow affordances while retaining PixiJS/Live Hive as the primary high-performance visualization surface. The UI must make Secure9P namespaces, evidence packs, replay, tickets, policy gates, and gateway state navigable through familiar desktop patterns without introducing a new authority plane.

Significant frontend redesign and refactoring is explicitly in scope. The implementation may replace the current single-dashboard layout, split the large frontend controller into workbench/view/state modules, rebuild CSS around Spectrum density and layout primitives, and restructure Playwright coverage around the new desktop model. The stability boundary is the Rust-owned protocol/session/replay/evidence semantics, generated defaults, transcript grammar, and PixiJS Live Hive renderer contract, not the current HTML panel arrangement.

**Non-Goals (Explicit)**
- No new Cohesix console verbs, Secure9P verbs, in-VM TCP listeners, ad-hoc RPC, or UI-owned control semantics.
- No replacement of PixiJS Live Hive with DOM, SVG, D3, Spectrum charts, or a generic graph library.
- No second evidence-pack, replay, trace, or bundle format distinct from the canonical `cohsh-core` trace stack and `coh evidence pack` layout.
- No hidden background watchers when a desktop view is closed, inactive, or offline.
- No generalized window manager, plugin host, web browser shell, or arbitrary host filesystem explorer.
- No CDN dependency, network font dependency, or unbounded Spectrum dependency expansion; all UI assets remain vendored/offline-safe.
- No change to ACK/ERR/END grammar, generated path defaults, namespace roots, or ticket policy unless routed through manifest IR, `coh-rtc`, docs, fixtures, and regression tests in the same scoped change.

**Desktop Model**
- **Application frame:** compact desktop top bar with transport mode, role/ticket state, lifecycle, gateway health, pressure, replay/offline status, and active evidence target.
- **Navigation dock:** Spectrum action buttons or tab rail for `Hive`, `Namespaces`, `Tickets`, `Policy`, `Evidence`, `Replay`, `Console`, and `Settings`.
- **Workspace:** tabbed or split document area where each selected tool opens as a stable workbench view, not a stacked marketing/dashboard section.
- **Inspector:** right-side details for the selected worker, namespace path, ticket receipt, evidence pack, replay frame, or policy denial.
- **Transcript drawer:** bottom proof pane showing exact `OK`/`ERR`/`END` lines for every backend action and preserving console parity.
- **Command palette:** bounded local launcher for existing workbench actions and known paths; it must not execute arbitrary new commands or bypass role/ticket checks.

**Frontend Architecture Expectations**
- Split the current frontend controller into small modules for workbench routing, session/transport state, transcript state, namespace explorer state, evidence/replay state, Live Hive coordination, and shared Spectrum component wrappers.
- Keep DOM updates incremental and keyed for high-churn areas such as telemetry overlays, namespace listings, schedule/lease tables, and transcripts.
- Keep Live Hive canvas rendering independent from Spectrum layout reflows; workbench panels may select or inspect Live Hive entities, but must not make per-frame DOM updates.
- Preserve accessibility and keyboard efficiency: visible focus rings, keyboard navigation for dock/tabs/menus, command palette search, copy-path shortcuts, and screen-reader labels for status, transcript, and dialogs.
- Treat performance budgets as product requirements: bounded polling, no hidden watchers, no layout thrash in high-frequency paths, nonblank canvas proof, release-bundle asset proof, and deterministic replay screenshots.
- Existing element IDs may change when the desktop model requires it, but Playwright tests must migrate to stable user-facing roles, labels, test IDs, and replay fixtures instead of fragile dashboard layout assumptions.


**Spectrum Design-System Use**
- Use Spectrum components for desktop chrome and operator workflows:
  - `sp-action-button`, `sp-action-group`, or equivalent local wrappers for dock and toolbar commands.
  - `sp-tabs` or a Spectrum-styled tab rail for workspace switching.
  - `sp-menu`, `sp-popover`, and context menus for namespace entries, workers, evidence files, and replay artifacts.
  - `sp-dialog` for ticket minting, evidence export setup, policy approval preview, replay open, and risky write confirmation.
  - `sp-field-label`, `sp-textfield`, `sp-picker`, `sp-checkbox`, and `sp-switch` for structured forms.
  - Spectrum status/alert patterns for lifecycle, gateway health, policy denials, backpressure, offline/replay, and tamper rejection.
- Inventory the current vendored Spectrum bundle before adding components. Any new Spectrum component must be self-hosted, tested in the release bundle, and covered by the dependency policy.
- Spectrum tokens become the primary source for HTML UI density, spacing, focus rings, control states, and accessible contrast. PixiJS may mirror semantic colors through generated or documented tokens, but must remain a renderer over bounded telemetry state.

**Live Hive Continuity**
- PixiJS remains the owner of the Live Hive canvas, world model, LOD/degrade behavior, selection hooks, and replay rendering.
- Spectrum frames the Live Hive as a desktop document view:
  - toolbar controls for connect/start/stop, fit/reset, detail toggle, replay speed, and snapshot source;
  - status strip for frame cap, polling cadence, replay/live source, and degraded mode;
  - inspector for selected worker, canonical namespace path, role, lease, schedule state, and bounded telemetry detail;
  - transcript drawer for the exact `tail`, `cat`, `ls`, replay, or attach proof that produced visible state.
- Live Hive state remains reconstructable from streams, traces, and CBOR snapshots. Restarting SwarmUI must not require hidden UI state to rebuild the view.

**Deliverables**
- Desktop shell refactor for `apps/swarmui/frontend/` with Spectrum-backed app frame, dock/tab rail, workspace, inspector, and transcript drawer.
- Namespace Explorer workbench:
  - path breadcrumb and tree/list split for `/proc`, `/queen`, `/shard`, `/worker` when enabled, `/log`, `/gpu`, `/host`, `/policy`, `/actions`, `/audit`, and `/replay` as allowed by generated roots and role policy;
  - explicit read-only, append-only, control-file, generated, legacy-alias, and unavailable-provider labels;
  - bounded `cat`, `tail`, copy-path, reveal-in-transcript, and open-in-replay affordances.
- Evidence Desk:
  - front-end workflow over existing `coh evidence pack`, `coh evidence timeline`, CI summary, and SIEM NDJSON export;
  - visible pack contents, manifest/policy hashes, trace references, redaction state, and deterministic output path;
  - no second pack schema or UI-owned evidence serializer.
- Replay Desk:
  - open trace, Hive CBOR snapshot, and evidence pack artifacts offline;
  - show tamper/replay validation, trace limits, frame/ACK counts, selected timeline event, and Live Hive replay source;
  - network access remains disabled in offline/replay mode.
- Tickets and Policy Desk:
  - read-only ticket status/deadletter, idempotency key, relay fields, policy rules, pressure, audit, and approval queue views;
  - optional command-preview forms for existing host-ticket or approval lines, with submit disabled unless the active milestone/profile admits the write path.
- Live Hive desktop integration:
  - preserve PixiJS renderer and replay fixtures;
  - update canvas framing, toolbar, inspector, worker selection, canonical shard path display, and `No telemetry yet` remediation flow.
- UI performance gate:
  - add deterministic source and release-bundle checks for Live Hive frame cadence, pending-event backlog, inactive-view polling, replay rendering, and layout thrash under fixture load;
  - record UI render/backlog metrics separately from REST or Pi hardware performance; UI regressions do not reopen driver/runtime performance unless the transcript or gateway evidence shows backend slowdown.
- Documentation updates:
  - `docs/USERLAND_AND_CLI.md` describes the SwarmUI desktop workbench, Spectrum/PixiJS split, namespace navigation, evidence desk, replay desk, and non-goals.
  - `docs/INTERFACES.md` records that the UI remains a projection of existing Secure9P/console/evidence/replay semantics.
  - `docs/TEST_PLAN.md` records Rust, generated-drift, replay, and Playwright validation.

**Commands**
- `cargo fmt --all -- --check`
- `cargo test -p swarmui`
- `cargo test -p swarmui --test transcript`
- `cargo test -p swarmui --test console_parity`
- `cargo test -p swarmui --test security`
- `cargo test -p swarmui --test cache`
- `cargo test -p swarmui --test trace`
- `cargo test -p swarmui --test replay`
- `cargo test -p coh --test evidence`
- `cargo test -p cohsh --test trace`
- `scripts/check-generated.sh`
- `cd tools/swarmui-ui-tests && npm ci`
- `cd tools/swarmui-ui-tests && npx playwright install webkit`
- `cd tools/swarmui-ui-tests && npm test`
- `cd tools/swarmui-ui-tests && npm test -- --grep live-hive-performance`
- `SWARMUI_RELEASE_DIR=../../releases/<latest> npm test` from `tools/swarmui-ui-tests` during release-bundle validation.

**Checks (DoD)**
- SwarmUI uses Spectrum for desktop shell, controls, forms, menus, dialogs, status, focus, and workflow chrome while PixiJS remains the only Live Hive rendering engine.
- The frontend is refactored into coherent workbench modules; `app.js` does not remain a monolithic controller for every view, transcript, and Live Hive interaction.
- UI actions preserve byte-stable `OK`/`ERR`/`END` transcripts for equivalent `cohsh` operations; no ACK grammar, NineDoor error, `/proc` format, trace, or evidence-pack schema drift occurs.
- Namespace Explorer rejects relative paths, `.`, `..`, NUL, over-depth walks, unsupported roots, and paths outside role/ticket scope before reaching provider logic.
- Canonical `/shard/<label>/worker/<id>/telemetry` is preferred in labels and inspectors; legacy `/worker/<id>/telemetry` is shown only as a generated compatibility alias when enabled.
- Evidence Desk invokes/reuses canonical evidence-pack and timeline internals and never serializes a UI-specific pack format or raw auth tokens/tickets.
- Replay Desk rejects tampered traces and oversized artifacts, disables network access in offline/replay mode, and reconstructs Live Hive state from trace/CBOR/evidence artifacts only.
- Ticket/Policy Desk shows status and denials without granting new authority; any write-capable affordance is explicitly gated by active profile/milestone state and emits transcript proof.
- Direct TCP mode clearly warns about single-client console ownership; REST/gateway mode presents gateway health, request-auth state, and backpressure counters before operators tune publish rates.
- No hidden polling or background watchers run when a workbench view is inactive, stopped, or offline; Live Hive polling remains bounded by generated defaults.
- Playwright desktop and narrow screenshots are updated intentionally, with checks for nonblank PixiJS canvas, visible legends/status, text fit, no overlapping controls, keyboard/focus behavior, and accessible labels.
- Live Hive UI performance evidence records frame cadence, pending/backlog bounds, inactive-view polling state, and replay render stability for source and release-bundle runs; failures are fixed in the UI/render loop unless backend transcript evidence proves a runtime regression.
- Release-bundle UI tests pass against the latest bundle assets, not only source files.

**Compiler touchpoints**
- `coh-rtc` remains the source for SwarmUI path roots, cache limits, trace limits, Live Hive frame/LOD limits, line caps, ticket scope, and any new desktop-workbench defaults.
- If additional UI roots, Spectrum component allowlists, evidence-desk defaults, replay-desk defaults, or workbench view limits are needed, add manifest IR and regenerate `apps/swarmui/src/generated.rs`, `docs/snippets/swarmui_defaults.md`, docs snippets, and tests in the same change.
- Generated docs must be refreshed before implementation patches land; stale embedded snippets in `docs/USERLAND_AND_CLI.md` or other canonical docs block merge.

**Task Breakdown**
```
Title/ID: m28f-swarmui-scope-and-drift
Goal: Establish the desktop-workbench scope and clear generated-doc/grammar drift before changing UI layout.
Inputs: AGENTS.md, docs/BUILD_PLAN.md, docs/USERLAND_AND_CLI.md, docs/snippets/*.md, tools/coh-rtc/tests/swarmui_docs.rs, apps/swarmui/src/generated.rs, crates/cohsh-core/src/verb.rs, apps/cohsh/src/lib.rs, apps/swarmui/src/lib.rs
Changes:
  - docs/USERLAND_AND_CLI.md — refresh generated snippets and document the desktop-workbench non-goals.
  - tools/coh-rtc/tests/swarmui_docs.rs — keep generated SwarmUI snippet checks authoritative.
  - docs/INTERFACES.md — record Spectrum/PixiJS split and no-new-semantics guardrails.
Commands:
  - scripts/check-generated.sh
  - cargo test -p swarmui --test transcript
  - cargo test -p swarmui --test console_parity
Checks:
  - Embedded generated snippets match docs/snippets and generated Rust.
  - Echo grammar documentation agrees with actual parser/help or the mismatch is resolved through the manifest/compiler path.
  - No protocol, ACK, path, ticket, or evidence semantics change in this task.
Deliverables:
  - Clean scope baseline for the SwarmUI desktop redesign.

Title/ID: m28f-spectrum-desktop-shell
Goal: Rebuild SwarmUI chrome around Spectrum-backed desktop primitives and split the frontend into maintainable workbench modules while preserving backend protocol semantics.
Inputs: apps/swarmui/frontend/index.html, apps/swarmui/frontend/app.js, apps/swarmui/frontend/styles/**, apps/swarmui/frontend/components/**, apps/swarmui/frontend/vendor/spectrum.bundle.js, tools/swarmui-ui-tests/**
Changes:
  - apps/swarmui/frontend/index.html — introduce app frame, navigation dock/tab rail, workspace, inspector, and transcript drawer.
  - apps/swarmui/frontend/app.js — split monolithic controller behavior into workbench routing, state, transcript, namespace, evidence/replay, and Live Hive coordination modules.
  - apps/swarmui/frontend/styles/** — align layout, density, focus, status, and form styling with Spectrum tokens and remove dashboard-specific layout assumptions.
  - apps/swarmui/frontend/components/** — add reusable workbench, toolbar, status, dialog, menu, command-palette, and inspector wrappers around vendored Spectrum components.
  - tools/swarmui-ui-tests/** — update UI-only desktop/narrow tests and screenshots to target stable user-facing roles, labels, test IDs, and replay fixtures.
Commands:
  - cargo test -p swarmui
  - cd tools/swarmui-ui-tests && npm test
Checks:
  - Existing Tauri command names and backend semantics remain stable even if frontend module boundaries and DOM structure change substantially.
  - `app.js` is no longer the owner of all workbench, transcript, namespace, evidence/replay, and Live Hive UI behavior.
  - Spectrum components render offline from vendored assets with no CDN or network dependency.
  - Desktop and narrow layouts have no overlapping controls, clipped text, unreachable focus targets, or keyboard traps.
Deliverables:
  - Spectrum-backed SwarmUI workbench shell.

Title/ID: m28f-namespace-explorer
Goal: Replace the root picker with a desktop namespace explorer that makes Secure9P paths familiar and safe.
Inputs: apps/swarmui/src/lib.rs, apps/swarmui/src-tauri/main.rs, apps/swarmui/frontend/**, configs/root_task.toml, docs/ROLES_AND_SCHEDULING.md, docs/SECURE9P.md, tests/fixtures/traces/trace_v0.trace
Changes:
  - apps/swarmui/src/lib.rs — add read-only list/cat/tail helpers if needed without changing command grammar or provider semantics.
  - apps/swarmui/frontend/** — add breadcrumb, tree/list split, preview/tail pane, path copy, transcript reveal, and path metadata labels.
  - docs/USERLAND_AND_CLI.md — document canonical shard navigation and legacy `/worker` alias presentation.
Commands:
  - cargo test -p swarmui --test security
  - cargo test -p swarmui --test trace
  - cd tools/swarmui-ui-tests && npm test
Checks:
  - Absolute-path, walk-depth, role/ticket, legacy-alias, and unsupported-root behavior is enforced and visible.
  - `ls`, `cat`, and `tail` transcript order remains byte-stable with `cohsh`.
Deliverables:
  - Secure9P namespace explorer suitable for day-to-day operator browsing.

Title/ID: m28f-evidence-and-replay-desks
Goal: Make evidence packs, timelines, traces, snapshots, and replay first-class desktop workbench flows using canonical artifacts only.
Inputs: apps/coh evidence internals, apps/swarmui/src/cache.rs, apps/swarmui/src/hive.rs, apps/swarmui/src-tauri/main.rs, crates/cohsh-core/src/trace.rs, tools/cohesix-py/examples/ci_evidence_pack.py, tools/cohesix-py/examples/siem_export_ndjson.py, docs/OPERATOR_WALKTHROUGH.md
Changes:
  - apps/swarmui/src/** — expose host-side wrappers or shared read-only internals for evidence/timeline/replay metadata where milestone 28 provides them.
  - apps/swarmui/frontend/** — add Evidence Desk and Replay Desk views with pack contents, hashes, validation, timeline, SIEM/CI export, and Live Hive replay source selection.
  - docs/USERLAND_AND_CLI.md + docs/TEST_PLAN.md — document evidence/replay workflows and validation.
Commands:
  - cargo test -p swarmui --test cache
  - cargo test -p swarmui --test replay
  - cargo test -p swarmui --test trace
  - cargo test -p coh --test evidence
  - cd tools/swarmui-ui-tests && npm test
Checks:
  - Evidence Desk reuses canonical pack/timeline output and does not leak raw tickets or auth tokens.
  - Replay Desk rejects tampered/oversized traces and keeps network disabled in offline/replay mode.
  - Live Hive replay remains deterministic from trace-adjacent `.hive.cbor` and snapshot CBOR artifacts.
Deliverables:
  - Desktop evidence and replay workflows ready for audit, support, CI, and demos.

Title/ID: m28f-live-hive-continuity
Goal: Preserve PixiJS Live Hive while integrating it into the desktop workbench with Spectrum toolbar and inspector controls.
Inputs: apps/swarmui/frontend/hive/**, apps/swarmui/frontend/app.js, apps/swarmui/frontend/styles/hive.css, apps/swarmui/tests/replay.rs, tools/swarmui-ui-tests/tests/swarmui.spec.js
Changes:
  - apps/swarmui/frontend/hive/** — preserve PixiJS renderer, world model, LOD, and debug hooks while accepting workbench selection/inspector events.
  - apps/swarmui/frontend/** — add Spectrum toolbar, source status, replay speed, fit/reset, detail toggle, and selected-worker inspector.
  - tools/swarmui-ui-tests/** — assert canvas is nonblank, responsive, and framed correctly across desktop/narrow modes.
Commands:
  - cargo test -p swarmui --test replay
  - cd tools/swarmui-ui-tests && npm test
  - cd tools/swarmui-ui-tests && npm test -- --grep live-hive-performance
Checks:
  - PixiJS remains the rendering engine; Spectrum does not replace canvas rendering.
  - Selection, overlays, details, replay, and degraded-mode indicators remain bounded and reconstructable.
  - Canvas pixel checks, screenshots, and Live Hive performance checks prove nonblank rendering, bounded frame cadence/backlog, inactive-view polling state, and no UI overlap.
Deliverables:
  - Live Hive preserved as the high-performance visualization inside the desktop workbench.

Title/ID: m28f-release-bundle-ui-regression
Goal: Prove the redesigned workbench works from source and the latest release bundle without hidden runtime or asset assumptions.
Inputs: tools/swarmui-ui-tests/**, releases/<latest>/ui/swarmui/**, apps/swarmui/frontend/**, docs/TEST_PLAN.md
Changes:
  - tools/swarmui-ui-tests/** — add desktop workbench regression coverage for dock, tabs, namespace explorer, evidence desk, replay desk, Live Hive, console, and dialogs.
  - docs/TEST_PLAN.md — update SwarmUI UI regression commands and screenshot policy.
Commands:
  - cd tools/swarmui-ui-tests && npm ci
  - cd tools/swarmui-ui-tests && npx playwright install webkit
  - cd tools/swarmui-ui-tests && npm test
  - cd tools/swarmui-ui-tests && npm test -- --grep live-hive-performance
  - cd tools/swarmui-ui-tests && SWARMUI_RELEASE_DIR=../../releases/<latest> npm test
  - cd tools/swarmui-ui-tests && SWARMUI_RELEASE_DIR=../../releases/<latest> npm test -- --grep live-hive-performance
Checks:
  - Source and release-bundle UI tests pass with deterministic fixtures.
  - Screenshots are intentionally updated and stable.
  - Release bundle includes all Spectrum, icon, font, and PixiJS assets needed for offline operation.
  - Source and release-bundle Live Hive performance checks record frame cadence, pending/backlog bounds, inactive-view polling state, and replay stability separately from backend REST/Pi performance.
Deliverables:
  - Replay-first UI regression gate for the SwarmUI desktop workbench.
```

**Outcome**
After Milestone 28f:
- SwarmUI is the primary desktop operator workbench for Cohesix, not a dense single-page dashboard.
- Operators can browse Secure9P namespaces with file-manager familiarity while still seeing exact role/ticket/path bounds.
- Evidence packs, timelines, traces, snapshots, and Live Hive replay are first-class, deterministic desktop workflows.
- Spectrum owns the desktop interaction language; PixiJS Live Hive remains the rendering engine for hive state.
- Cohesix gains usability without increasing the VM TCB, adding protocols, weakening transcript parity, or creating a second evidence/replay format.

## Milestone 29 — Edge Local Status (Pi 4 Host Tool)  <a id="29"></a> 
[Milestones](#Milestones)

**Why now (compiler):** Field techs need offline status on edge devices using the same 9P grammar. Tool must respect Pi 4 boot profile semantics and attestation outputs.

**As-built alignment note:** `apps/coh-status` currently exists as a library crate with trace replay support and a convergence transcript fixture. It is not yet a standalone read-only field CLI, and its current convergence fixture still exercises `/queen/ctl` writes. Milestone 29 promotes that crate into the read-only tool described below and replaces generic convergence coverage with status-specific read-only fixtures.

**Prerequisite**
- Milestone **28** completed for the shared read-only inspect/attest/evidence-pack internals that `coh-status` reuses. Milestone 29 must not fork a second status parser, attestation verifier, trace reader, or snapshot schema when the Milestone 28 host-tool core already owns that behavior.

**Goal**
Promote `coh-status` into a **small read-only CLI** for local field inspection of boot/attest data using the existing TCP console transport and offline artifacts, without adding any in-VM 9P/TCP listener and without introducing a second UI stack.

**Non-Goals**
- Repo-wide SPDX/NOTICE header sweeps (track separately; not required for the status tool).
- A separate Tauri/UI variant; SwarmUI already occupies the UI role.
- Generic shell verbs (`echo`, `spawn`, arbitrary writes) or any write-capable façade over `/queen/ctl`.

**Deliverables**
- `coh-status` binary with:
  - default read-only status summary
  - `attest` subcommand for manifest/evidence verification
  - offline replay/snapshot mode for field diagnostics
- Shared read-only status/snapshot core reused by `coh inspect`, `coh attest`, SwarmUI, and `coh-status`.
- Shared offline snapshot/CBOR parsing code extracted from existing SwarmUI logic so grammar stays aligned.
- Status-specific fixtures proving read-only behavior; no `/queen/ctl` writes in the canonical `coh-status` test flows.
- Bounded command-latency fixture for offline and live-read status paths. This is field-tool latency evidence, not a full runtime or hardware throughput benchmark.

**Commands**
- `cargo build -p coh-status`
- `cargo run -p coh-status -- --help`
- `cargo run -p coh-status -- offline --trace tests/fixtures/traces/trace_v0.trace`
- `cargo run -p coh-status -- attest --help`
- `cargo test -p coh-status --test latency`

**Checks (DoD)**
- Works offline; wrong/expired ticket → deterministic `ERR reason=Permission` surfaced to user.
- Shared snapshot/CBOR parsing identical to SwarmUI for overlapping flows.
- Abuse case: attempt to write via `coh-status` returns deterministic denial and does not mutate state.
- UI/CLI/console equivalence MUST be preserved: ACK/ERR/END sequences must remain byte-stable relative to the 7c baseline.
- `coh-status` latency evidence stays bounded on representative offline traces and live-read fixtures; regressions are classified as snapshot parsing, attestation verification, transport, or artifact-size overhead before 29a/29b field surfaces reuse the status core.

**Compiler touchpoints**
- `coh-rtc` emits localhost binding guidance and attestation paths for Pi 4 boot profile into `docs/HARDWARE_BRINGUP.md` and `docs/USERLAND_AND_CLI.md`.

**Task Breakdown**
```
Title/ID: m29-status-tool
Goal: Build a real read-only `coh-status` CLI on top of shared inspect/attest internals.
Inputs: apps/coh-status/, shared host-tool status core, Pi 4 boot profile manifest outputs, attestation nodes.
Changes:
  - apps/coh-status/src/main.rs — status CLI entrypoint with read-only subcommands only.
  - apps/coh-status/src/lib.rs — preserve trace replay helpers while exposing only read-only status/attestation operations to the CLI.
  - apps/coh-status/tests/offline.rs — simulate offline read and expired ticket.
  - apps/coh-status/tests/transcript.rs — remove the current generic `/queen/ctl` convergence flow from the canonical status transcript.
Commands:
  - cargo build -p coh-status
  - cargo run -p coh-status -- --help
Checks:
  - Expired ticket returns ERR; offline cache used when transport unavailable.
Deliverables:
  - Tool usage documented in docs/HARDWARE_BRINGUP.md and docs/USERLAND_AND_CLI.md.

Title/ID: m29-shared-snapshot-core
Goal: Extract shared read-only snapshot/CBOR parsing for SwarmUI and `coh-status`.
Inputs: apps/swarmui snapshot/cache code, apps/coh-status, shared host-tool internals.
Changes:
  - shared status/snapshot module — read-only offline artifact decoding.
  - apps/coh-status/src/lib.rs — consume shared snapshot/trace helpers.
Commands:
  - cargo test -p coh-status --test offline
  - cargo test -p swarmui --test trace
Checks:
  - Shared offline parsing stays byte-stable across SwarmUI and `coh-status`.
Deliverables:
  - Shared read-only snapshot core with no duplicate parsers.

Title/ID: m29-attest-verify
Goal: Verify TPM attestation parsing parity with the shared host-tool stack.
Inputs: /proc/attest outputs, shared attestation helpers.
Changes:
  - apps/coh-status/src/attest.rs — verify manifest fingerprint against cached reference.
  - shared attestation verifier reused by `coh inspect` / `coh attest` where applicable.
Commands:
  - cargo test -p coh-status --test attest
Checks:
  - Malformed attestation rejected with ERR; valid attestation matches manifest hash identically to the shared verifier.
Deliverables:
  - Verified attestation workflow documented; regression outputs stored.

Title/ID: m29-readonly-regressions
Goal: Replace generic convergence fixtures with `coh-status`-specific read-only flows.
Inputs: apps/coh-status/tests/, tests/fixtures/transcripts/, docs/TEST_PLAN.md.
Changes:
  - apps/coh-status/tests/transcript.rs — read-only status transcript only.
  - apps/coh-status/tests/latency.rs — bounded offline/live-read status latency fixtures over representative artifacts.
  - tests/fixtures/transcripts/ — `coh-status` fixtures with no `/queen/ctl` writes.
Commands:
  - cargo test -p coh-status --test transcript
  - cargo test -p coh-status --test latency
Checks:
  - Canonical `coh-status` flows read `/proc/boot`, `/proc/attest/*`, and selected telemetry without mutating state.
  - Offline/live-read status latency stays bounded and is reported as field-tool evidence, not REST/Pi throughput proof.
Deliverables:
  - Field-tech read-only transcript fixtures and explicit docs.

```

## Milestone 29a — Pi 4 Root-Shell Hardware Status (`hw-status`)  <a id="29a"></a>
[Milestones](#Milestones)

**Why now (field diagnostics):** Milestone 29 gives field techs a host-side read-only status tool, but Pi 4 bring-up still needs a serial-local command when TCP, host tooling, or storage artifacts are unavailable. `hw-status` is a Pi 4 U-Boot profile diagnostic for quick board/firmware inspection from `cohesix>` without changing device state or promoting root-task back into steady-state hardware ownership.

**As-built alignment note:** There is no `hw-status` command today. Current Pi 4 hardware facts are split across boot logs, framebuffer hints, driver-task progress lines, timer summaries, and isolated runtime diagnostics. Milestone 29a adds one bounded, read-only root-shell view; older prose must not claim a Pi 4 hardware-status command or firmware property snapshot until this milestone has implementation and transcript evidence.

**Prerequisite**
- Milestone **26a/26b** owner-state and isolated runtime proof restored for the selected Pi 4 profile.
- Milestone **28** completed for shared read-only snapshot/evidence conventions.
- Milestone **29** completed or explicitly scoped so `hw-status` field names can be reused by `coh-status` rather than creating a second status vocabulary.

**Goal**
Add a Pi 4-build-only `hw-status` command on the root shell that prints bounded, stable, read-only board status:
- firmware-reported power-state flags,
- selected clock rates,
- selected voltage domains,
- SoC temperature and throttle/undervoltage flags when exposed,
- framebuffer geometry plus ARM/GPU memory split,
- bounded firmware-managed device notification counters or last-notification summaries.

**Non-Goals**
- No firmware writes, power-state changes, clock changes, voltage changes, turbo/overclock controls, reboot policy changes, or recovery actions.
- No root-owned HDMI, USB, Wi-Fi, GENET, SDIO, PCIe, or GPU driver path; isolated runtimes remain the hardware owners for steady service.
- No new in-VM TCP listener, 9P provider requirement, host RPC, or `cohsh-core` grammar expansion beyond the documented root-console command.
- No direct physical-address probing or mailbox/MMIO access outside HAL-admitted Pi 4 resources.

**Deliverables**
- `hw-status` root-shell command available only for the Pi 4 U-Boot profile. Other profiles must omit it or return deterministic `ERR reason=unsupported` per generated grammar/docs.
- HAL-mediated Pi 4 firmware-property/status helper with an explicit allowlist of read-only property tags, bounded buffers, timeout evidence, and failure rows.
- Stable serial transcript format using sectioned rows such as `hw power`, `hw clock`, `hw voltage`, `hw thermal`, `hw memory`, `hw framebuffer`, and `hw firmware-notify`.
- Optional read-only status mirror for `coh-status` only if it reuses the same field names and does not add authority or a second parser.
- Documentation and regression fixtures proving the command is passive, Pi 4-profile gated, and does not count as isolated runtime hardware acceptance.
- Bounded serial-local command-latency evidence for `hw-status` so passive diagnostics stay quick without reopening runtime throughput gates.

**Commands**
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hw_status -- --test-threads=1`
- `cargo test -p cohsh-core`
- `SEL4_BUILD_DIR=$REPO/seL4/build_UBOOT cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-pi4`
- `scripts/check-generated.sh`
- `scripts/ci/test_plan_run.sh --state-dir out/test-plan/m29a-hw-status`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hw_status_latency -- --test-threads=1`

**Checks (DoD)**
- `help` and `docs/USERLAND_AND_CLI.md` list `hw-status` only for the Pi 4 root-console surface; TCP/`cohsh` shared grammar does not accidentally accept undocumented raw hardware commands.
- Mocked firmware-property tests cover success, unsupported tag, timeout, malformed response, and partial data without panics or unbounded waits.
- Live Pi 4 acceptance, when claimed, includes a fresh serial transcript showing `hw-status` returning bounded rows while USB/local-seat, Wi-Fi/GENET, HDMI, and shell proof lanes remain separate.
- No `hw-status` path writes firmware state, changes clocks/voltage/power domains, clears notifications, or touches isolated runtime device state.
- Generated docs, fixtures, and trace-normalizer expectations remain aligned; command output drift is intentional only with matching docs/tests.
- `hw-status` latency evidence stays bounded in mocked tests and, when live Pi evidence is claimed, is reported as serial-local passive diagnostic timing rather than hardware runtime throughput proof.

**Compiler touchpoints**
- `coh-rtc` owns the profile gate and generated command/help snippets for `hw-status`.
- Manifest validation rejects enabling `hw-status` outside Pi 4 hardware profiles or without the HAL firmware-property resource declared.

**Task Breakdown**
```
Title/ID: m29a-hw-status-ir
Goal: Add the Pi 4-only generated command gate and status field vocabulary.
Inputs: tools/coh-rtc, configs/root_task_pi4_uboot_aarch64.toml, docs/USERLAND_AND_CLI.md.
Changes:
  - tools/coh-rtc/src/** — profile-gated `hw-status` command/help generation and field-name constants.
  - configs/root_task_pi4_uboot_aarch64.toml — declare read-only Pi 4 firmware-status capability for the root diagnostics surface.
  - docs/snippets/* + docs/USERLAND_AND_CLI.md — generated and hand-maintained command reference updates.
Commands: cargo test -p coh-rtc && scripts/check-generated.sh
Checks: Non-Pi profiles reject the command gate; generated help/docs match resolved Pi 4 manifest truth.
Deliverables: Compiler-owned `hw-status` profile gate and stable field vocabulary.

Title/ID: m29a-hw-status-hal
Goal: Implement bounded read-only Pi 4 firmware-property status queries behind HAL.
Inputs: apps/root-task/src/hal/**, apps/root-task/src/arch/aarch64/**, seL4/build_UBOOT generated headers.
Changes:
  - apps/root-task/src/hal/** — Pi 4 firmware-property read helper with tag allowlist, bounded mailbox buffers, timeout/fault evidence, and no write/control tags.
  - apps/root-task/src/arch/aarch64/** — reuse existing cache/MMIO primitives only through HAL-approved mappings.
Commands: cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hw_status -- --test-threads=1
Checks: Success, timeout, malformed response, unsupported tag, and partial-data fixtures return typed status rows without panics or unbounded waits.
Deliverables: HAL-owned passive Pi 4 hardware-status snapshot provider.

Title/ID: m29a-hw-status-shell
Goal: Add the `hw-status` root-shell command and stable serial transcript.
Inputs: apps/root-task/src/console, apps/root-task/src/event/mod.rs, docs/USERLAND_AND_CLI.md.
Changes:
  - apps/root-task/src/console/** + apps/root-task/src/event/mod.rs — parse and dispatch `hw-status` only on the Pi 4 root-console diagnostics surface.
  - apps/root-task/src/event/mod.rs — emit bounded `hw ...` rows for power, clocks, voltage, thermal, framebuffer/GPU memory, and firmware notifications.
  - docs/USERLAND_AND_CLI.md — document serial/local-only behavior and unsupported-profile result.
Commands:
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hw_status -- --test-threads=1
  - SEL4_BUILD_DIR=$REPO/seL4/build_UBOOT cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-pi4
Checks: Output is stable, bounded, read-only, and does not change ACK/ERR/END semantics for existing commands.
Deliverables: Pi 4 serial `cohesix> hw-status` command.

Title/ID: m29a-hw-status-regressions
Goal: Add fixtures and Pi 4 evidence parsing for passive hardware-status diagnostics.
Inputs: docs/TEST_PLAN.md, scripts/pi4_trace_normalize.py, tests/fixtures/transcripts/.
Changes:
  - docs/TEST_PLAN.md — classify `hw-status` as passive Pi 4 diagnostics, not hardware acceptance for USB/Wi-Fi/GENET/HDMI.
  - scripts/pi4_trace_normalize.py — parse optional `HW_STATUS_*` fields from captured transcripts without making them gate blockers.
  - apps/root-task/tests/hw_status_latency.rs — mocked bounded timing checks for HAL firmware-property status rows and unsupported-profile refusal.
  - tests/fixtures/transcripts/ — add serial-local `hw-status` success and unsupported-profile fixtures.
Commands:
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hw_status -- --test-threads=1
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hw_status_latency -- --test-threads=1
  - pytest tests/test_pi4_trace_normalize.py
Checks: Transcript, latency, and normalizer coverage prove passive behavior, bounded command timing, stable field extraction, and no regression in existing Pi 4 gates.
Deliverables: Repeatable host and Pi 4 validation for `hw-status`.
```

## Milestone 29b — AI-Native Namespace Surfaces (Control-Plane Only)  <a id="29b"></a> 
[Milestones](#Milestones)

**Why now (positioning):**  
Cohesix already exposes bounded, file-shaped control surfaces for workers, GPU state, updates, models, and observability. Milestone 28b1 proves the provider coexistence registry and evidence contract, Milestone 28c proves the host-side AI operating model first: delegated agent authority, durable checkpoints/evidence, explicit context budgets, and host-ticket-based actuation. Milestone 28d then proves that standard MCP clients and A2A peers can consume those semantics through `hive-gateway` without a new Cohesix grammar. The next strategic step is to make that AI fleet state legible through the same namespace discipline without turning Cohesix into a general-purpose runtime OS or creating a second executor.

**Goal**  
Add a manifest-defined, role-scoped AI control namespace that lets operators and automation inspect and drive AI lifecycle state through existing Secure9P semantics. This milestone is limited to **control-plane surfaces only**: no in-VM application runtime, no general UI stack, no mutable POSIX-like filesystem, and no new transport or RPC model.

**As-built alignment note:** There is no `ecosystem.ai.*` manifest IR and no `/jobs`, `/datasets`, `/experiments`, `/infer`, or `/metrics` AI namespace provider in the host or VM NineDoor implementations as of the 26c planning audit. Milestone 29b adds those roots only after 28b, 28b1, 28c, and 28d prove delegated authority, provider action/read-visibility conformance, host-ticket AI actions, checkpoints/evidence semantics, and MCP/A2A gateway projection without grammar drift.

### Prerequisites
- Milestone **28b** completed (delegated REST identity, idempotent queen intents, writer-epoch fencing, audit/replay baseline).
- Milestone **28b1** completed (provider action registry, read visibility classification, provider conformance, identity mapping, packaging, observability, and use-case evidence matrix).
- Milestone **28c** completed (host-side AI run envelopes, checkpoint/evidence model, and `/host/tickets/spec` AI actuation semantics).
- Milestone **28d** completed (MCP/A2A gateway projection over existing Cohesix grammar, with no new VM protocol, agent bus, or host-executor bypass).
- Production profiles that claim VM worker/driver authority for AI namespace projections require Milestone **28e** cap-bundle and structured fault lifecycle evidence. Read-model-only or host-ticket-only profiles may remain gated by 28b/28b1/28c/28d without claiming full VM cap-bundle authority.

**Non-Goals**
- No app runtime, package manager, bundle loader, or process model beyond the existing Queen/worker control model.
- No in-VM GUI, mutable `/ui` runtime surface, or second operator protocol.
- No generic `/net` service API, sockets-by-file, or hidden request/response RPC behind file names.
- No mutable general-purpose `/store`; existing CAS, models, telemetry, and spool semantics remain authoritative and distinct.
- No new 9P verbs, no console grammar changes, and no deviation from `ERR = no side effects`.
- No second host-execution plane parallel to `/host/tickets/spec`; VM-visible AI paths do not bypass delegated host-ticket authority or Milestone 28b fencing/idempotency rules.
- No create, unlink, rename, chmod, symlink, dynamic prompt/blob tree, arbitrary path component, or general directory mutation semantics. AI namespace writes target fixed generated control files only.
- No opaque inter-agent mailbox or prompt-transcript tree exposed as a first-class namespace primitive.

**Deliverables**
- AI namespace roots are the VM-visible control-plane projection of the host-side semantics proven in 28c.
- Any AI path whose write can cause host-side side effects MUST resolve to documented host-ticket actions and inherit delegated identity, idempotency, writer epoch, and audit/replay guarantees.
- Read-only AI paths (`/metrics/*` and read views of `/jobs/*`, `/datasets/*`, `/experiments/*`) do not become a second source of truth for execution authority.
- Manifest-gated AI control roots under the Secure9P namespace, with paths aligned to current authority rules:
  - `/jobs/*` for bounded job submission, queue state, completion records, checkpoint refs, and handoff lineage views
  - `/datasets/*` for dataset metadata, lineage pointers, and policy-visible readiness state
  - `/experiments/*` for append-only run metadata, retrieval-manifest refs, and result summaries
  - `/infer/*` for bounded inference request/receipt control surfaces where explicitly enabled, including prefix-group read views
  - `/metrics/*` for read-only fleet and model metrics summaries
- Queen-only control files remain append-only; worker and observer views remain role-filtered and read-only where appropriate.
- Host tools (`cohsh`, `coh`, REST projection, SwarmUI read models where relevant) discover and render the new paths without introducing new verbs.
- Canonical schemas for all new paths documented in `docs/INTERFACES.md` and emitted from `coh-rtc`.
- Namespace state remains a projection of 28c run envelopes, checkpoints, retrieval manifests, and receipts; it does not introduce an independent scheduler or executor.
- Namespace-scale microbenchmark evidence for representative job/run/checkpoint counts proves `ls`, `cat`, `tail`, and fixed-control-file writes remain bounded. This is a namespace/provider microbenchmark, not a full Pi hardware throughput gate, unless high-churn AI namespace paths change the root-task hot runtime path.

**Commands**
- `cargo test -p coh-rtc`
- `cargo test -p nine-door`
- `cargo test -p root-task`
- `cargo test -p cohsh`
- `cohsh --script scripts/cohsh/ai_namespace_roundtrip.coh`
- `cohsh --script scripts/cohsh/ai_namespace_scale.coh`

**Checks (DoD)**
- All new AI namespace paths are manifest-gated, bounded, and role-scoped.
- Write-capable AI paths are fixed generated control files with declared target grammar; no dynamic file creation, unlink, rename, chmod, symlink, arbitrary path expansion, prompt/blob tree, or hidden request/response RPC is accepted.
- Writes remain append-only and auditable; reads stay within declared `msize` and path bounds.
- Existing ACK/ERR/END grammar, Secure9P transport behavior, and host-tool semantics remain byte-stable unless intentionally versioned in the same change.
- Missing AI paths are treated as gate state, not client bugs.
- Read views expose 28c task/handoff/checkpoint/prefix evidence without becoming a second source of authority.
- No new in-VM listener, runtime, or hidden RPC behavior is introduced.
- AI namespace scale evidence shows bounded path walk/list/read/write behavior at representative job/run counts; material regressions are classified as namespace-provider overhead, host-tool projection overhead, or runtime hot-path regression before downstream profiles depend on the new roots.

**Compiler touchpoints**
- `coh-rtc` admits `ecosystem.ai.*` IR fields for path gating, quotas, and per-surface limits.
- `coh-rtc` rejects AI namespace definitions that require dynamic path creation, unconstrained components, directory mutation, prompt/blob-tree storage, or side effects outside generated host-ticket-backed receipt mappings.
- Generated snippets refresh `docs/INTERFACES.md`, `docs/ARCHITECTURE.md`, and `docs/USERLAND_AND_CLI.md` so host tools consume authoritative bounds and namespace roots.
- Validation and generated docs make the authority mapping explicit: AI namespace writes that trigger external execution are projections over the 28c host-ticket/evidence model, not an independent executor.
- Validation rejects configurations that overload existing `/updates`, `/models`, telemetry, or spool semantics.

**Task Breakdown**
```
Title/ID: m29b-ai-ir
Goal: Admit AI namespace surfaces in compiler IR without changing Cohesix transport or runtime boundaries.
Inputs: tools/coh-rtc, docs/ARCHITECTURE.md, docs/INTERFACES.md, docs/USERLAND_AND_CLI.md.
Changes:
  - tools/coh-rtc/src/ir.rs — `ecosystem.ai.*` schema, gating, receipt-projection mapping, and bounds validation.
  - tools/coh-rtc/src/codegen/{docs,rust,cohsh}.rs — generated AI namespace snippets and client defaults.
Commands:
  - cargo test -p coh-rtc
Checks:
  - AI namespace admission is compiler-defined, rejects overlap with existing CAS/model/spool surfaces, and preserves the 28c host-ticket authority mapping for side-effecting flows and read-model projections.
Deliverables:
  - Authoritative manifest + docs snippets for AI namespace roots.

Title/ID: m29b-ninedoor-ai-providers
Goal: Add bounded NineDoor providers for AI control-plane paths.
Inputs: apps/nine-door, apps/root-task/src/ninedoor.rs, generated manifest outputs.
Changes:
  - apps/nine-door/src/host/namespace.rs — host-mode AI namespace providers for tests.
  - apps/root-task/src/ninedoor.rs — in-VM AI namespace provider wiring and policy enforcement.
Commands:
  - cargo test -p nine-door
  - cargo test -p root-task
Checks:
  - Paths enforce append-only/read-only semantics, role filters, deterministic refusals, fixed generated control files, and documented handoff to host-ticket-backed actuation where side effects are involved; checkpoint/handoff/prefix read views remain receipt projections only.
  - Dynamic create/unlink/rename/chmod/symlink/path-component expansion and prompt/blob-tree storage are refused before any state mutation.
Deliverables:
  - AI control-plane namespace available in host and VM implementations with matching semantics.

Title/ID: m29b-host-tool-discovery
Goal: Extend host-tool discovery and read models for AI namespace paths without adding verbs.
Inputs: apps/cohsh, apps/coh, apps/swarmui, generated client defaults.
Changes:
  - apps/cohsh — list/cat/tail/echo flows for `/jobs`, `/datasets`, `/experiments`, `/infer`, `/metrics`, including checkpoint/handoff/prefix-status read paths.
  - apps/coh — host-side helpers remain projections of existing file semantics only.
  - apps/swarmui — optional read-only views backed by existing `/proc` and AI namespace tails.
Commands:
  - cargo test -p cohsh
Checks:
  - Host tools discover AI paths using existing grammar and deterministic error handling, with read models aligned to 28c evidence receipts rather than inferred hidden state.
Deliverables:
  - Operator-facing tooling parity for AI namespace surfaces.

Title/ID: m29b-ai-regressions
Goal: Add deterministic regression coverage for AI namespace semantics.
Inputs: scripts/cohsh/, tests/fixtures/, docs/TEST_PLAN.md.
Changes:
  - scripts/cohsh/ai_namespace_roundtrip.coh — canonical AI namespace script using existing verbs only.
  - scripts/cohsh/ai_namespace_scale.coh — representative namespace-scale listing, read, tail, and fixed-control-file write probe for high-churn job/run roots.
  - tests/fixtures/transcripts/ — stable transcript fixtures for gated/missing/enabled AI paths.
Commands:
  - cohsh --script scripts/cohsh/ai_namespace_roundtrip.coh
  - cohsh --script scripts/cohsh/ai_namespace_scale.coh
Checks:
  - Missing paths, denied writes, successful reads/writes, and representative high-churn namespace listings all preserve existing grammar, refusal semantics, and bounded runtime behavior.
Deliverables:
  - Canonical AI namespace regression pack and test-plan coverage.
```

## Milestone 30 — AWS AMI (UEFI → Cohesix, ENA, Diskless 9door)  <a id="30"></a> 
[Milestones](#Milestones)

**Why now (platform):**  
Cohesix is ready to operate as the operating system. To make EC2 a first-class, production target without Linux, agents, or filesystems, Cohesix must boot directly from UEFI and bring up Nitro networking natively. ENA is mandatory on AWS. This milestone establishes a guest-stateless AMI sourced from an immutable snapshot containing the ESP image (UEFI loader + kernel + rootserver + manifest).

On EC2, an EBS-backed AMI launches with a persistent root EBS volume created from the AMI snapshot. "Diskless" therefore means that Cohesix admits no runtime block-storage service, never mounts or writes the boot volume after firmware/loader handoff, and reconstructs runtime state from signed boot/fabric inputs; it does **not** mean EC2 presents physically read-only or absent boot media.

**Goal**  
Boot Cohesix on AWS EC2 (Arm64) via **UEFI -> elfloader.efi -> seL4 -> root-task**, then bring up ENA networking through a manifest-declared isolated AWS network runtime admitted by root-task and mount the Cohesix 9door namespace over the network with **no local filesystem**, **no Linux**, and **no virtio**.

Milestone 30 first reconciles the **generic UEFI ESP/QEMU baseline** currently described by the repo with the newer Pi 4 U-Boot path, then adds the **AWS-specific delta**: AWS profile admission, ENA, outbound bootstrap, optional IMDSv2, and AMI registration.

**As-built alignment note:** The repo currently has UEFI profile/configuration material, a UEFI shim crate, and `scripts/uefi/*` helpers, but it does not have AWS profile admission, `scripts/aws/*`, isolated ENA runtime descriptors/images, outbound 9door mount code, or approved root-task TLS/HTTP/IMDS support. Milestone 30 must start with boot-chain and TCB reconciliation before runtime code depends on UEFI, TLS/HTTP, IMDS, or AWS-specific assumptions.

**Prerequisites**
- Milestone **26d** completed for the accepted seL4 15 provenance and timer/syscall baseline. Milestone 30 must still create and validate a separate AWS-selected seL4 build; Pi 4 or QEMU artifacts are not AWS boot proof.
- Milestone **27b** completed before production AWS assurance claims so generated witnesses, HAL authority checks, and claim-class separation cover the AWS profile.
- Milestone **27c** completed before D2 peak-performance work so ENA queue affinity and service-bucket evidence consume the generated core-local scheduling substrate rather than inventing a parallel scheduler.
- Milestone **28e** completed before any production profile claims linked ENA runtime cap-bundle authority or structured fault containment. A pre-28e EC2 boot/first-link probe is permitted only as explicitly non-production feasibility evidence.

**Non-negotiable constraints**
- The first executable gate is EC2 Arm64 platform feasibility, not ENA implementation. Re-check the selected instance family's current custom-OS support posture, then prove AMI registration, UEFI entry, serial/console evidence, selected seL4 platform support, and the hardware-description handoff used for memory, GIC/timer, PCIe configuration, and interrupts (ACPI, DT, or a documented conversion). If that gate fails, stop before ENA, TLS/HTTP, IMDS, or fabric-mount code.
- Milestone 30 may not assume the UEFI/ESP baseline is authoritative until the first AWS task reconciles it against the current Pi 4 U-Boot pivot, `scripts/uefi/esp-build.sh`, `docs/BOOT_REFERENCE.md`, `docs/HARDWARE_BRINGUP.md`, and the charter rule for UEFI tooling. If the baseline is stale, AWS work starts by refreshing or reintroducing it under this milestone with docs and tests.
- AWS boot work must produce a boot-resource map before ENA, TLS/HTTP, IMDS, or outbound fabric code depends on the profile. The map records AMI snapshot/root-volume geometry, ESP contents, kernel/root-task/rootserver artifacts, manifest hash, signed bootstrap manifest, trust anchors, seL4 handoff and ACPI/DT assumptions, attestation evidence, firmware-managed persistent state, and explicit non-claims.
- The AWS diskless profile disables Milestone 27 VM-local persistence and rejects the EC2 root EBS device, ESP, or any other local block device as a spool/settings backend. The immutable AMI snapshot is build provenance; the launched root EBS volume is persistent platform boot media that Cohesix treats as read-only and outside runtime authority.
- In-VM TLS, HTTP, and IMDSv2 are a deliberate TCB expansion, not a routine AWS delta. They are disabled by default until `docs/ARCHITECTURE.md`, `docs/NETWORK_CONFIG.md`, `docs/SECURITY.md`, `docs/SECURITY_NIST_800_53.md`, and `docs/AWS_AMI.md` explicitly approve the bounded client-only threat model and generated manifest gates.
- AWS VM profiles require dependency-closure evidence before runtime code lands: no `std`, libc, POSIX filesystem/process API, DNS resolver, web framework, unapproved TLS/HTTP stack, or host-only ecosystem dependency may enter the root-task or isolated runtime closure.
- No listener is introduced in the VM. AWS networking is outbound-only after seL4, and any Secure9P fabric mount must preserve existing frame bounds, role-scoped authority, and deterministic error behavior.
- AWS ecosystem coexistence remains host/fabric-side unless explicitly admitted by the TCB gate. Cloud-side adapters, observability, deployment automation, and policy workflows must reuse `/host/tickets/*`, provider registry evidence, and host/fabric termination where the security review rejects in-VM TLS/HTTP.
- If the security review rejects in-VM TLS/HTTP, the milestone must use a signed bootstrap manifest and a host/fabric-side termination design instead of importing a web/TLS stack into the root-task closure.
- ENA is PCIe/MMIO/DMA-backed physical hardware. Steady ENA admin queue, IO queue, interrupt/poll, RX/TX descriptor, and DMA/cache service must live in a manifest-declared isolated AWS network runtime over the fixed driver-task ABI after HAL admission. Root-task remains the HAL/resource admitter, descriptor publisher, bounded service-turn client, network stack owner, and diagnostics publisher; it must not contain a root-owned steady ENA driver.
- AWS ENA records must extend the existing fixed driver-task ABI without forking a second incompatible contract. A platform-neutral ABI extraction is allowed only with byte-layout/version compatibility tests for existing Pi 4 descriptors, generated migration, and no change to accepted Pi evidence by implication.
- The initial single TX/RX queue polling path is bootstrap evidence only. Peak-performance AWS claims require a later generated multi-queue, MSI-X or equivalent notification, queue-affinity, and core-local service-bucket evidence phase.

**Deliverables**
#### A0) Boot-chain / TCB reconciliation and boot-resource map
- Reconcile the current UEFI/ESP builder, Pi 4 U-Boot pivot, selected EC2 instance family, seL4 platform/handoff assumptions, and AWS profile requirements before runtime AWS code lands.
- Prove a bounded EC2 Arm64 feasibility chain before ENA work: register an EBS-backed UEFI AMI, reach a deterministic UEFI/serial marker, identify the selected seL4 platform gap or reach root-task, and capture the ACPI/DT, memory, GIC/timer, PCIe, interrupt, and console inputs the port must consume.
- Produce a boot-resource map covering AMI snapshot/root-volume geometry, ESP layout, elfloader, kernel, root task/rootserver, optional initrd, resolved manifest hash, signed fabric bootstrap manifest, root trust anchors, selected attestation inputs, hardware-description handoff, firmware-managed persistent state, and explicit non-claims.
- Record whether root-task TLS/HTTP/IMDS is accepted, rejected, or deferred. If deferred or rejected, AWS bootstrap must use signed manifest inputs and host/fabric-side termination rather than importing those stacks into the VM closure.

#### A) AWS compiler + profile admission
- Dedicated AWS Arm64 profile and manifest vocabulary for:
  - `ena` as a network backend
  - ENA queue bounds
  - bootstrap retry limits
  - signed fabric bootstrap manifest requirements
  - optional IMDSv2 allowlist and bounded response sizes
  - explicit diskless state that disables `persistence.*` and rejects local block backends

#### B) EFI System Partition integration for AWS
- Reuse the existing deterministic ESP builder as the canonical packager and produce an AWS-consumable EBS root image whose firmware-visible ESP contains:
  - `EFI/BOOT/BOOTAA64.EFI` (elfloader EFI)
  - `kernel.elf`
  - `rootserver` (root task ELF)
  - optional `initrd.cpio`
  - `manifest.json` and `manifest.sha256`
  - embedded, signed fabric bootstrap manifest (≥2 endpoints, root trust anchors)

#### C) ENA isolated network runtime (adminq + single TX/RX queue bootstrap)
- ENA PCIe discovery, BAR admission, adminq, completion path, and single TX/RX queue pair run in a manifest-declared isolated AWS network runtime using HAL-declared PCIe/MMIO/DMA/shared-buffer resources.
- ENA command/completion records consume the single fixed driver-task ABI contract; any platform-neutral extraction preserves existing Pi 4 ABI layout/version behavior through explicit compatibility fixtures.
- Root-task owns only generated descriptor publication, bounded ENA service turns, net-stack integration, and diagnostics.
- Minimal polling dataplane with single TX/RX queue pair is accepted only as first-link/bootstrap evidence, not as peak AWS throughput closure.

#### D) Outbound bootstrap core after seL4
- Reuse the Milestone 26b `no_std` DHCP core over the root-task net stack fed by isolated ENA runtime RX/TX service turns
- Add bounded **outbound** TCP connection management
- Add bounded TLS client support only after the AWS security/TCB expansion gate accepts it for the selected profile
- Add a minimal outbound Secure9P/9door mount client
- Diskless bootstrap path **after seL4**: HAL-admitted isolated ENA runtime -> DHCP (26b core) -> outbound TCP -> approved security/session layer -> 9door mount

#### D2) AWS ENA performance closure (post-bootstrap)
- Add generated queue-count, queue-affinity, notification/MSI-X or poll-budget policy, burst, and backpressure bounds after first-link smoke passes.
- Map ENA queues into core-local service buckets from Milestone 27c without changing Secure9P, console, or namespace semantics.
- Keep root-task as the serialized authority/net-stack client; isolated runtime owns queue service and DMA/cache maintenance.
- Produce archived EC2 performance evidence for first-link single-queue bootstrap and later multi-queue/notification-backed ENA closure. Benchmark evidence must include instance type, ENA feature set, generated queue policy, service-bucket mapping, request suite, error-budget policy, and hostile-fabric/refusal context. Single-queue polling results are bootstrap/link evidence only and cannot be promoted to peak-performance claims.

#### E) Optional IMDSv2 bootstrap
- Optional IMDSv2 bootstrap (instance identity + config) is deferred until the bounded HTTP client threat model is approved; the default AWS path uses manifest-authored or signed-bootstrap inputs without IMDS.
- No listeners; no background refresh loop

#### F) AMI registration tooling
- AWS scripts for ESP image registration and smoke tests for Arm64 (`uefi` / `uefi-preferred`)
- Documentation in `docs/AWS_AMI.md` covering boot path, failure modes, and recovery
- Dependency-closure and hostile-fabric evidence for every AWS-enabled VM profile before production claims.

**Commands**
- `cmake --build "$SEL4_BUILD_DIR" --target rootserver_image`
- `scripts/uefi/esp-build.sh --manifest configs/generated/root_task_resolved.json --sel4-build-dir "$SEL4_BUILD_DIR"`
- `scripts/aws/platform-feasibility.sh --state-dir out/aws/m30-platform-feasibility`
- `scripts/aws/build-esp.sh`
- `scripts/aws/register-ami.sh`
- `scripts/aws/launch-smoke.sh`
- `scripts/aws/ena-bench.sh --mode bootstrap --state-dir out/bench/m30-ena-bootstrap`
- `scripts/aws/ena-bench.sh --mode peak --state-dir out/bench/m30-ena-peak`
- `cargo tree -p root-task --target aarch64-unknown-none --no-default-features`
- `cargo deny check bans`

**Checks (DoD)**
- EC2 instance boots directly into Cohesix with no intermediate OS.
- EC2 Arm64 feasibility evidence identifies the supported instance family, AMI registration path, UEFI/serial result, selected seL4 platform status, and authoritative ACPI/DT hardware-description path before ENA runtime work is accepted.
- Boot-chain reconciliation and the boot-resource map are complete before AWS runtime code cites UEFI, TLS/HTTP, IMDS, ENA, or outbound fabric assumptions.
- AWS dependency-closure gate proves the VM profile remains `no_std` and excludes libc, POSIX filesystem/process APIs, DNS resolver, web framework, and unapproved TLS/HTTP dependencies.
- ENA link comes up deterministically; DHCP lease acquired within bounded time.
- ENA hardware service must use a manifest-declared isolated ENA runtime; root-task direct ENA MMIO/DMA paths fail profile validation outside explicitly named QEMU/host compatibility tests.
- ENA runtime records use the fixed shared driver-task ABI, and any platform-neutral extraction proves existing Pi 4 descriptor layout/version compatibility.
- 9door namespace mounts successfully and control plane is reachable.
- Single-queue polling evidence is classified as bootstrap/link proof. Peak AWS performance claims require the D2 multi-queue/core-local evidence lane.
- ENA benchmark artifacts are archived for bootstrap and, where peak-performance claims are made, multi-queue or notification-backed service-bucket runs. EC2 results are compared against the latest accepted rolling benchmark baseline only for capacity/context; AWS target evidence is its own lane and must not overwrite Pi/QEMU proof.
- IMDSv2 metadata fetch is absent by default or optional and bounded after approval; if unavailable or denied, boot continues safely with explicit diagnostics and no unbounded retries.
- If TLS/HTTP is approved, proof shows it remains client-only and non-persistent: no listener, no background refresh loop, bounded trust-anchor/cert storage from signed manifest inputs, bounded handshake/input parsing, and deterministic boot behavior when fabric, TLS, or IMDS is unavailable.
- Cohesix never mounts or writes the root EBS volume after boot handoff, the AWS profile exposes no Milestone 27 persistence backend, and reboot/stop-start restores runtime state only from signed inputs. Any firmware-managed UEFI-variable persistence is inventoried separately and cannot be described as Cohesix control-plane persistence.
- Failure cases (no fabric, hostile fabric, forged remote manifest, auth failure, ticket widening attempt, Secure9P-bound relaxation attempt, replay, malformed frame, link down) fail terminally or refuse boundedly with explicit console diagnostics, no local policy mutation, no widened authority, and no partial trust state.
- AWS security docs record the accepted posture for any root-task TLS/HTTP code, or explicitly state that TLS/HTTP termination remains outside the VM.

**Compiler touchpoints**
- `coh-rtc` emits:
  - AWS profile / backend admission for `ena`
  - boot-resource map schema, AMI/root-volume geometry, artifact hashes, hardware-description handoff, signed-bootstrap manifest references, trust-anchor references, selected attestation inputs, firmware-state inventory, and generated non-claim summaries for AWS profiles.
  - isolated ENA runtime descriptors, ENA queue bounds, bootstrap retry limits, and later multi-queue/core-local performance gates.
  - explicit diskless-profile validation that disables `persistence.*` and rejects the root EBS/ESP or other local block devices as runtime persistence backends.
  - the single fixed driver-task ABI version and any generated AWS extension records, with compatibility evidence for existing Pi 4 descriptors.
  - Fabric bootstrap manifest schema and signature requirements.
  - dependency-closure allow/deny lists for AWS VM profile crates.
  - IMDSv2 allowlist, max response bytes, and retry bounds (optional gate).
- Regeneration guard verifies EFI binary hash against recorded compiler output.

**Task Breakdown**
```
Title/ID: m30-uefi-and-tcb-reconciliation
Goal: Reconcile AWS platform, boot, and security assumptions with the current Cohesix UEFI baseline and tiny-TCB networking posture before runtime work starts.
Inputs: scripts/uefi/esp-build.sh, docs/BOOT_REFERENCE.md, docs/HARDWARE_BRINGUP.md, docs/NETWORK_CONFIG.md, docs/SECURITY.md, docs/SECURITY_NIST_800_53.md, docs/AWS_AMI.md, AGENTS.md.
Changes:
- docs/AWS_AMI.md — state the selected EC2 Arm64/custom-OS support posture, accepted AWS boot chain, whether the UEFI/ESP builder is current, the boot-resource map fields, EBS root-volume truth, hardware-description handoff, and what evidence proves them.
- docs/NETWORK_CONFIG.md + docs/SECURITY.md + docs/SECURITY_NIST_800_53.md — record whether root-task TLS/HTTP/IMDS is accepted, rejected, or deferred for AWS.
- docs/BUILD_PLAN.md — keep Milestone 30 subtasks synchronized with the accepted TCB posture.
Commands:
- scripts/uefi/esp-build.sh --help
- scripts/ci/check_test_plan.sh
Checks:
- AWS work has an explicit platform-support decision, boot-chain baseline, boot-resource map, hardware-description path, and security-approved plan for TLS/HTTP/IMDS before ENA bootstrap code depends on those assumptions.
Deliverables:
- EC2 platform/UEFI and AWS TCB reconciliation note, including a boot-resource map, that blocks unsupported platform assumptions and accidental import of web/TLS stacks into the VM.

Title/ID: m30-uefi-esp
Goal: Validate or refresh the deterministic ESP builder and selected seL4 UEFI build before EC2 feasibility work.
Inputs: upstream elfloader EFI build, selected `SEL4_BUILD_DIR`, `scripts/uefi/esp-build.sh`, manifest outputs, accepted m30 UEFI/TCB reconciliation.
Changes:
- scripts/uefi/esp-build.sh — remain the canonical ESP builder for Cohesix only if `m30-uefi-and-tcb-reconciliation` accepts it as current; otherwise refresh or replace it under the same documented contract.
- scripts/aws/build-esp.sh — thin AWS wrapper producing an AMI-ready EBS root/ESP image from the canonical builder output.
Commands:
- cmake --build "$SEL4_BUILD_DIR" --target rootserver_image
- scripts/uefi/esp-build.sh --manifest configs/generated/root_task_resolved.json --sel4-build-dir "$SEL4_BUILD_DIR"
Checks:
- The selected build directory supplies the UEFI elfloader, kernel, and embedded rootserver truth; the ESP reaches the deterministic UEFI/root-task marker required by the platform-feasibility probe.
Deliverables:
- Documented ESP layout and build recipe for the selected Arm64 UEFI profile.

Title/ID: m30-ec2-arm64-platform-feasibility
Goal: Prove the selected EC2 Arm64 family can launch a custom Cohesix UEFI/seL4 payload and expose the platform inputs required before ENA implementation.
Inputs: accepted m30 UEFI/TCB reconciliation, selected `SEL4_BUILD_DIR`, scripts/uefi/esp-build.sh, AWS account/region/instance-family test profile, docs/AWS_AMI.md.
Changes:
- scripts/aws/platform-feasibility.sh — register a disposable EBS-backed Arm64 UEFI probe image, launch the selected instance family, capture console/serial and lifecycle evidence, and clean up bounded test resources.
- docs/audit/M30_EC2_PLATFORM_FEASIBILITY.md — record AMI registration, UEFI marker, seL4/root-task reachability or exact platform blocker, ACPI/DT handoff, GIC/timer, memory, PCIe/interrupt, console, root-volume, and unsupported-feature evidence.
- docs/AWS_AMI.md — classify the selected family as supported for further implementation, experimental with a named blocker, or unsupported.
Commands:
- scripts/aws/platform-feasibility.sh --state-dir out/aws/m30-platform-feasibility
Checks:
- AMI registration and UEFI entry are proven on the named instance family, and the selected seL4 platform either reaches root-task or has a bounded, explicit porting task before ENA work.
- The evidence identifies the authoritative ACPI/DT and PCIe/interrupt discovery path; QEMU `virt` addresses or Pi DT assumptions are not reused implicitly.
Deliverables:
- Go/no-go EC2 Arm64 platform evidence that gates every later AWS runtime task.

Title/ID: m30-aws-profile
Goal: Admit AWS/ENA/IMDS bootstrap in compiler IR and profile selection before runtime implementation.
Inputs: tools/coh-rtc, configs/, docs/AWS_AMI.md.
Changes:
- tools/coh-rtc/src/ir.rs — AWS profile, selected hardware-description path, `ena` backend, isolated ENA runtime descriptor schema, bounded bootstrap schema, explicit diskless/no-persistence state, and later performance-gate fields.
- tools/coh-rtc/src/codegen/{docs,rust}.rs — generated AWS/profile snippets and dependency allow/deny policy summaries.
Commands:
- cargo test -p coh-rtc
Checks:
- Runtime code can consume authoritative AWS/ENA/IMDS limits from generated outputs, and physical AWS profiles cannot enable root-owned ENA MMIO/DMA paths.
- AWS profiles cannot enable TLS/HTTP/IMDS/fabric support without matching TCB posture and dependency-closure gates.
- AWS diskless profiles reject `persistence.*`, the root EBS/ESP, and all local block backends as runtime spool/settings storage.
Deliverables:
- Compiler-defined AWS admission with docs snippets updated.

Title/ID: m30-vm-dependency-closure
Goal: Prove AWS VM profiles do not import unapproved host, POSIX, web, or TLS/HTTP dependencies into root-task or isolated runtimes.
Inputs: Cargo.toml workspace metadata, apps/root-task, apps/aws-driver-runtime, tools/coh-rtc, deny configuration, docs/AWS_AMI.md.
Changes:
- tools/coh-rtc/src/validate.rs — generated AWS dependency allow/deny policy for selected VM profiles.
- scripts/ci/aws_dependency_closure.sh — target-qualified `cargo tree`/deny check for root-task and isolated AWS runtime closure.
- docs/AWS_AMI.md + docs/SECURITY.md — accepted dependency posture and approved exceptions, if any.
Commands:
- scripts/ci/aws_dependency_closure.sh
- cargo tree -p root-task --target aarch64-unknown-none --no-default-features
- cargo deny check bans
Checks:
- `std`, libc, POSIX filesystem/process APIs, DNS resolver crates, web frameworks, host-only ecosystem crates, and unapproved TLS/HTTP dependencies are absent from AWS VM artifacts.
Deliverables:
- Reproducible AWS VM dependency-closure evidence before runtime code can cite TLS/HTTP/IMDS or outbound fabric support.

Title/ID: m30-shared-driver-task-abi
Goal: Extend one fixed driver-task ABI for AWS without cloning or silently changing the accepted Pi 4 contract.
Inputs: crates/pi4-driver-abi, apps/pi4-driver-runtime, apps/root-task/src/hal/driver_task.rs, tools/coh-rtc, accepted Pi 4 ABI fixtures and proof evidence.
Changes:
- crates/driver-task-abi/src/** — extract only platform-neutral versioned headers, resource descriptors, command/completion framing, and proof fields needed by both Pi 4 and AWS runtimes.
- crates/pi4-driver-abi/src/** — consume or re-export the shared records while preserving existing Pi 4 wire layout, version behavior, and profile-generated descriptors.
- tools/coh-rtc/src/** — emit one ABI version/family plus platform-specific extension records; reject conflicting Pi/AWS layouts.
- tests/fixtures/driver_task_abi/** — byte-layout, version, bounds, and old-Pi-fixture compatibility tests.
Commands:
- cargo test -p driver-task-abi
- cargo test -p pi4-driver-abi
- cargo test -p pi4-driver-runtime
- scripts/check-generated.sh
Checks:
- Existing Pi 4 descriptors and accepted fixtures remain byte-compatible, AWS extensions are versioned and bounded, and no second incompatible ring/service-turn contract is introduced.
Deliverables:
- One compiler-declared fixed driver-task ABI family ready for isolated ENA records.

Title/ID: m30-ena-runtime-adminq
Goal: Implement ENA PCIe discovery and admin queue in a manifest-declared isolated AWS network runtime.
Inputs: accepted m30 platform-feasibility evidence, apps/root-task HAL admission, m30 shared driver-task ABI, apps/aws-driver-runtime, docs/AWS_AMI.md.
Changes:
- apps/root-task/src/hal/aws_ena.rs — HAL admission for ENA PCIe BARs, IRQ/notification policy, DMA/shared-buffer descriptors, and runtime image selection.
- crates/driver-task-abi/src/** — fixed-layout ENA init/adminq command records and completion evidence, preserving compatibility with the Pi 4 driver-task ABI shape.
- apps/aws-driver-runtime/src/** — isolated ENA runtime adminq and completion queue service.
- apps/root-task/src/net/ena.rs — root-side ENA ring-client/net-stack wiring only.
Commands:
- cargo test -p root-task --test ena_adminq
- cargo test -p driver-task-abi
- cargo test -p aws-driver-runtime
Checks:
- Feature negotiation succeeds with minimal feature set.
- ENA BAR/MMIO/DMA access occurs only in the isolated runtime after HAL admission; root-task remains a bounded client.
- ENA records use the generated shared ABI version and cannot alter existing Pi descriptor layout or evidence classification.
Deliverables:
- AdminQ protocol notes in docs/AWS_AMI.md.

Title/ID: m30-ena-runtime-io-bootstrap
Goal: Bring up minimal isolated runtime ENA dataplane for first-link bootstrap.
Inputs: manifest-declared isolated AWS network runtime, root-task net stack abstractions, generated ENA descriptors.
Changes:
- apps/aws-driver-runtime/src/** — single TX/RX SQ + CQ and polling dataplane service turns.
- crates/driver-task-abi/src/** — fixed-layout RX/TX descriptor service records, queue bounds, and completion counters.
- apps/root-task/src/net/mod.rs — integrate isolated ENA runtime RX/TX service into the root-task net stack.
Commands:
- cargo test -p root-task --test ena_ioq
- cargo test -p driver-task-abi
- cargo test -p aws-driver-runtime
Checks:
- TX reclaim and RX refill invariants hold under sustained traffic, root-owned ENA dataplane paths are absent from physical AWS profiles, and single-queue polling evidence is labeled bootstrap-only.
Deliverables:
- Deterministic dataplane invariants documented.

Title/ID: m30-ena-performance-closure
Goal: Add peak-performance AWS ENA evidence after first-link bootstrap without changing the authority model.
Inputs: generated ENA queue policy, Milestone 27c service-bucket outputs, manifest-declared isolated AWS network runtime, docs/BENCHMARKS.md, docs/AWS_AMI.md.
Changes:
- tools/coh-rtc/src/ir.rs — generated ENA queue-count, queue-affinity, notification/MSI-X or poll-budget, burst, and backpressure limits.
- apps/aws-driver-runtime/src/** — bounded multi-queue service turns and queue-local counters where the platform profile admits them.
- apps/root-task/src/net/ena.rs — deterministic merge/backpressure handling for isolated runtime queue completions.
- scripts/aws/ena-bench.sh — archived EC2 benchmark runner for bootstrap and peak ENA lanes, including target metadata, queue policy, service-bucket evidence, and error-budget summary.
- docs/BENCHMARKS.md + docs/AWS_AMI.md — classify bootstrap link proof separately from peak-throughput proof.
Commands:
- cargo test -p coh-rtc
- cargo test -p root-task --test ena_ioq
- cargo test -p aws-driver-runtime
- scripts/aws/ena-bench.sh --mode bootstrap --state-dir out/bench/m30-ena-bootstrap
- scripts/aws/ena-bench.sh --mode peak --state-dir out/bench/m30-ena-peak
Checks:
- Multi-queue or notification-backed claims cite generated bounds and archived EC2 evidence; single-queue polling cannot be described as peak performance.
- Benchmark artifacts preserve target metadata, generated queue policy, service-bucket counters, error-budget results, and hostile-fabric/refusal context; AWS performance is recorded as a target-specific lane, not substituted for Pi/QEMU evidence.
Deliverables:
- AWS ENA performance lane aligned to Cohesix service-bucket proof rather than root-owned driver shortcuts.

Title/ID: m30-outbound-bootstrap-core
Goal: Add bounded outbound TCP/session primitives and only the approved security primitive required before fabric mount.
Inputs: apps/root-task net stack, approved TLS or fabric-termination helpers, docs/AWS_AMI.md.
Changes:
- apps/root-task/src/net/dhcp.rs — adapt/reuse Milestone 26b DHCP core for ENA link and AWS-specific bounds.
- apps/root-task/src/net/tcp.rs — outbound TCP session support for long-lived sessions.
- apps/root-task/src/net/tls.rs — fabric-auth TLS handshake only if `m30-uefi-and-tcb-reconciliation` approves root-task TLS; otherwise this task wires the approved host/fabric-side termination alternative.
- apps/root-task/src/net/bootstrap.rs — deterministic sequencing and retries.
Commands:
- cargo test -p root-task --test net_bootstrap
Checks:
- Network reaches "fabric-ready" state within defined bounds.
- If TLS/HTTP is approved, it is client-only, non-persistent, bounded, and deterministic when fabric/TLS/IMDS is unavailable; no listener or background refresh loop is introduced.
Deliverables:
- Bootstrap timing guarantees recorded.

Title/ID: m30-imdsv2-bootstrap
Goal: If approved by the AWS TCB gate, read bounded instance metadata (IMDSv2) and feed boot policy inputs.
Inputs: apps/root-task net stack, docs/AWS_AMI.md.
Changes:
- apps/root-task/src/net/http.rs — minimal HTTP request/response parsing (bounded, no chunked), only if the AWS TCB gate approves root-task HTTP.
- apps/root-task/src/net/imdsv2.rs — token fetch + allowlisted metadata queries, disabled by default and profile-gated.
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
- Namespace mount preserves Secure9P frame/path/fid/ticket bounds and read/write semantics.
- Hostile fabric, forged manifest, auth failure, ticket widening, Secure9P-bound relaxation, replay, and malformed-frame cases are terminal or bounded refusals that leave no local policy mutation, widened authority, or partial trust state.
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
   - In the Queen view (SwarmUI or cohsh), confirm workers appear under `/shard/<label>/worker` before proceeding. Legacy `/worker` appears only when `sharding.legacy_worker_alias = true`.
   - If `/shard` has no worker entries, request a queen-side heartbeat spawn to seed a visible worker entry, then re-check:
     - `echo {"id":"spawn-1","target":"/queen/ctl","decision":"approve"} > /actions/queue`
     - `spawn heartbeat ticks=100`
     - `ls /shard`
6) Keep Live Hive active (optional):
   - `echo {"id":"spawn-2","target":"/queen/ctl","decision":"approve"} > /actions/queue`
   - `spawn heartbeat ticks=100`.
7) Host tools prove control-plane surface (Linux queen host or G5g, host tools only):
   - Live GPU bridge publish (required for `/gpu/models` and PEFT):
     - `./bin/gpu-bridge-host --publish --tcp-host <queen-host> --tcp-port 31337 --auth-token "$COH_AUTH_TOKEN" --interval-ms 1000 --registry demo/peft_registry`
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
   - `ls /shard` (ensure no active workers; legacy `/worker` may exist only when enabled) and confirm no active leases.
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
     - `ls /shard` and then inspect the relevant `/shard/<label>/worker` entry (legacy `/worker` only if aliasing is enabled)
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
   - Enforced in the compiler and restated here: 9P2000.L, `msize ≤ 8192`, walk depth ≤ 8, no `..`, no fid reuse after clunk, no in-VM TCP listeners except the authenticated root-task console, CPIO < 4 MiB, no POSIX façade, maintain `no_std` for VM artefacts.

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
