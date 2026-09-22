<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define Cohesix milestone scope, status, deliverables, and acceptance criteria. -->
<!-- Author: Lukas Bower -->

# Cohesix Build Plan (ARM64, Pure Rust Userspace)

> **Current post-27g plan — 23 September 2026:**
> [28x and later](BUILD_PLAN_28_PLUS.md) is the canonical continuation of this
> build plan. It replaces the former prospective 28–31 bodies and their future
> mapping, catalogue gates and blanket dependencies; those retained sections are
> historical design proposals, not additive implementation requirements.
> [Current ownership mapping](BUILD_PLAN_28_PLUS.md#mapping) resolves old numbers
> and immutable task/schema identifiers. Milestones 0–27g, active qualification,
> narrow reopenings, implementation, evidence and exceptions remain unchanged.
> New 28x work is Planned; this documentation edit activates no runtime work.

This build plan records what Cohesix has implemented, what remains to be built,
and the conditions for completing each milestone. It defines scope, dependencies,
deliverables, acceptance criteria, and known limitations so users and contributors
can distinguish delivered capabilities from planned work.

Completed milestones summarize the as-built result and its qualification limits.
Active and planned milestones define the remaining work. Detailed implementation
history and test-run records belong in linked audit, benchmark, or release
evidence; each milestone retains the decisions and exceptions that affect its
scope or acceptance. Update task descriptions to describe the resulting behavior
and obligations rather than appending a sequence of changes or test attempts.

This plan governs milestone scope and status under the [build charter](../AGENTS.md).
Selected manifests, resolved outputs, and `coh-rtc` artifacts define as-built
configuration. [Architecture](ARCHITECTURE.md) and [interface](INTERFACES.md)
references own runtime contracts; the [test plan](TEST_PLAN.md) and
[benchmark methodology](BENCHMARKS.md) govern qualification evidence.

- **Host:** macOS 26 on Apple Silicon.
- **Targets:** QEMU `aarch64/virt` with GICv3; Raspberry Pi 4 via firmware →
  U-Boot → seL4 binary image.
- **Userspace:** pure Rust; Queen/Worker control uses bounded namespaces and
  console operations, with no ad-hoc host RPC authority.
- **Reference AI host:** NVIDIA Jetson Orin Nano. AI runtimes and GPU tools run
  host-side, using configurable NVMe paths for models, data, caches and evidence.

A **shard** is a manifest-derived Worker namespace bucket. Telemetry uses
`/shard/<label>/worker/<id>/telemetry`; the legacy `/worker/<id>/telemetry`
alias requires `sharding.legacy_worker_alias = true`.

## seL4 Reference Manual Alignment (v16.0.0)

The [seL4 v16.0.0 manual](https://sel4.systems/Info/Docs/seL4-manual-16.0.0.pdf)
governs kernel semantics. The selected external seL4 build, headers and metadata
define enabled options, object layouts and APIs. Check each kernel interaction
against both. Manual availability, configured support, implementation and target
proof are distinct; Cohesix protocols, device behavior and build rules remain
owned by their specific contracts.

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
| [24e](#24e) | REST Multiplexer Transports + SwarmUI Gateway Mode | Reopened — M27g REST mount path-bound restoration |
| [25](#25) | SMP Utilization via Task Isolation (Multicore without Multithreading) | Complete |
| [25a](#25a) | REST Live Hive Performance (Parallel Polling + Batching) | Complete |
| [25b](#25b) | Secure Scale Gateway (1k Worker Readiness + Due Diligence Closure) | Complete |
| [25c](#25c) | Python Orchestration SDK (1k Fleet Playbooks + Host Integrations) | Complete |
| [25d](#25d) | REST Request-Auth Parity Across Host Tools (Gateway Capability Max) | Complete |
| [25e](#25e) | Evidence Packs + Integration Kits (Audit-First Adoption) | Complete |
| [25f](#25f) | Gateway Broker Refactor + Large Telemetry Reference Manifests (No-Retry Reliability Gate) | Complete |
| [25g](#25g) | Host Control Tickets via FUSE (GPU/PEFT + systemd/docker + K8s Coexistence) | Complete |
| [25h](#25h) | Multi-Hive Federation via Ticket Relay (Single-Writer Preserved, 10x1k Fleet Pattern) | Complete |
| [26](#26) | Official Pi 4 Bring-up (U-Boot + Binary Image) | Complete |
| [26a](#26a) | Pi 4 Driver-Task Substrate + GENET/Serial/Display Isolation | Complete |
| [26b](#26b) | Pi 4 USB/Wi-Fi Driver Tasks + DHCP/Benchmark Concurrency | Complete |
| [26c](#26c) | Regression-Gated Refactor + Surface Audit (Zero-Regression) | Complete |
| [26d](#26d) | seL4 16 Baseline Refresh + Reference/Performance Realignment | Complete |
| [26e](#26e) | Root-Service Compartmentalization + Worker Task Isolation + SMP+MCS Temporal Isolation | Reopened (bounded lease-history view) |
| [27](#27) | Operator Utilities: Inspect, Trace, Bundle, Diff, Attest | Complete — owner-approved evidence; 1.1.0-beta (Release A) |
| [27a](#27a) | Authority Hardening: Delegated REST Identity, Fenced Failover, Idempotent Queen Intents | Complete — 1.1.0-beta (Release A) authority floor |
| [27b](#27b) | Executable Host Foundation | Complete — selected foundation |
| [27c](#27c) | Recoverable CUDA Recipes | Complete |
| [27d](#27d) | Verified Private LoRA Release | Complete |
| [27e](#27e) | Installation, Adoption and CI | Complete |
| [27f](#27f) | SwarmUI Community Showcase: Spectrum Workbench + Live AI Hive | Planned — next release |
| [27g](#27g) | Integrated Qualification and Next Release | In Progress — Release A qualification and two-hour Pi burn-in |
| [28](BUILD_PLAN_28_PLUS.md#28) | Shared Governed Jobs and Bounded Unattended Authority | Planned |
| [28a](BUILD_PLAN_28_PLUS.md#28a) | Useful CUDA Workloads and Reliable GPU Operations | Planned |
| [28b](BUILD_PLAN_28_PLUS.md#28b) | Deeper PEFT Lifecycle and Verified Serving | Planned |
| [28c](BUILD_PLAN_28_PLUS.md#28c) | macOS 27: Siri/App Intents, Apple AI and Metal-Backed Workflows | Planned |
| [28d](BUILD_PLAN_28_PLUS.md#28d) | MCP Access to Complete Selected Workflows | Planned |
| [28e](BUILD_PLAN_28_PLUS.md#28e) | A2A Delegation of Durable Selected Jobs | Planned |
| [28f](BUILD_PLAN_28_PLUS.md#28f) | NeMo Agent Toolkit Adoption Kit and Live Integration | Planned |
| [28g](BUILD_PLAN_28_PLUS.md#28g) | Installation, Integrated User Qualification and Release B | Planned |
| [29](BUILD_PLAN_28_PLUS.md#29) | Extended Formal Assurance and NIST Evidence | Deferred — explicit activation |
| [30](BUILD_PLAN_28_PLUS.md#30) | Full Production Bundle Binding and Quarantine Inventory | Deferred — explicit activation |
| [31](BUILD_PLAN_28_PLUS.md#31) | VM-Local Persistence and Reboot Recovery | Deferred — explicit activation |
| [32](BUILD_PLAN_28_PLUS.md#32) | Broader Enterprise, Industry and Protocol Integration | Deferred — explicit activation |
| [33](BUILD_PLAN_28_PLUS.md#33) | Semantic Objects and Context Capsules | Deferred — explicit activation |
| [34](BUILD_PLAN_28_PLUS.md#34) | Advanced Agent Orchestration and Context Optimisation | Deferred — explicit activation |
| [35](BUILD_PLAN_28_PLUS.md#35) | General Inference Gateway and Advanced Routing | Deferred — explicit activation |
| [36](BUILD_PLAN_28_PLUS.md#36) | Broader NeMo Training, Evaluation and Serving Families | Deferred — explicit activation |
| [37](BUILD_PLAN_28_PLUS.md#37) | Measured Target Scheduling and Responsiveness Improvements | Deferred — explicit activation |
| [38](BUILD_PLAN_28_PLUS.md#38) | Local Hardware Status and Additional Namespace Projections | Deferred — explicit activation |
| [39](BUILD_PLAN_28_PLUS.md#39) | AWS Deployment | Deferred — explicit activation |

---

## Post-26e Delivery Sequence <a id="post-26e-investment-constrained-delivery-sequence"></a>

The 19 September 2026 owner instruction makes numerical order implementation
order. Preserve completed 0–27a and their evidence/exception decisions. The next
release is exactly **27b -> 27c -> 27d -> 27e -> 27f -> 27g**. Each milestone
closes against its own focused contracts using only earlier completed owners;
27g qualifies the assembled release. A prompt to implement a milestone means
its complete body/tasks/checks, with no unstated conversation requirement.
Under the charter, a Planned milestone is activated by the owner's implementation
request after its listed prerequisites are complete; record In Progress before
implementation. This planning edit itself activates no runtime work.

