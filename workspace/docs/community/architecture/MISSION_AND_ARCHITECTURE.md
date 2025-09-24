// CLASSIFICATION: COMMUNITY
// Filename: MISSION_AND_ARCHITECTURE.md v1.1
// Author: Lukas Bower
// Date Modified: 2025-07-31

# Mission and Architecture

Cohesix aims to deliver a secure, fast edge operating system with an integrated compiler toolchain. The project uses the seL4 microkernel and a Plan 9 userland to ensure minimal attack surface and deterministic behaviour. Physics services remain embedded, while GPU acceleration is now delivered through managed Linux CUDA microservers that Cohesix governs as a zero-trust control plane.

## Architecture Snapshot
- **Kernel:** vanilla seL4 with Cohesix patches
- **Userland:** Plan 9 services in a 9P namespace
- **Boot Target:** cold start under 200 ms on reference SBCs
- **Security:** seL4 proofs plus sandbox caps for services
- **CohRole:** declared before kernel init and exposed via `/srv/cohrole`
- **Roles:** QueenPrimary, KioskInteractive, DroneWorker, GlassesAgent, SensorRelay, SimulatorTest
- **Physics Core:** Rapier‑based `/sim/` for workers
- **GPU Annex:** `/srv/cuda` proxies to remote Cohesix CUDA Servers with graceful fallback if absent
- **OSS Policy:** only Apache 2.0, MIT, or BSD components
- **Trace Layer:** All syscalls, agent actions, and CLI invocations are recorded in `/log/trace/` and versioned for replay

## Implementation Overview
- **Bootloader:** minimal seL4 loader with early telemetry
- **Startup Path:** fixed sequence reaching userland in <200 ms
- **Service Layout:** modular services exported under `/srv/`
- **Validation:** embedded rule engine monitors syscalls and traces
- **Upgrades:** monthly upstream sync of seL4 and 9front

## Project Objectives
1. Preserve seL4 proofs end‑to‑end.
2. Provide high integrity physics simulation and remote GPU orchestration.
3. Offer a cohesive compiler (`cohcc`) with robust tests.
4. Encourage open development under permissive licenses.

Cohesix delivers a formally verified foundation for tomorrow’s edge and wearable devices while keeping tooling familiar and efficient. It now differentiates on tamper-evident orchestration of external CUDA farms instead of promising native GPU execution within Plan 9 roles.

## Why Cohesix Matters
- **Unmatched Security:** seL4 proofs provide mathematically verified isolation for processes, drivers, and services.
- **Edge‑First Performance:** Boot in under 200 ms while delegating GPU execution to Cohesix CUDA Servers via Secure9P.
- **Modular Architecture:** 9P namespaces let services like `/sim/` and `/srv/cuda` be attached or replaced dynamically, with `/srv/cuda` acting as the policy gateway into the managed microserver annex.
- **Familiar Toolchain:** BusyBox and POSIX shims keep developer ramp‑up short.
- **End-to-End Observability:** Every agent action and system event is traced and can be replayed during CI or simulation

## Phases & Milestones
| Phase               | Deliverables                                   | Target Date |
|---------------------|-------------------------------------------------|-------------|
| **Compiler**        | Coh_CC passes (IR → WASM/C) + unit tests        | 2025-06-30  |
| **Boot & Runtime**  | seL4 + Plan 9 userland bootable image          | 2025-08-15  |
| **GPU & Physics**   | `/srv/cuda` proxy to remote Cohesix CUDA Servers + Rapier integration | 2025-09-30  |
| **Coreutils & CLI** | BusyBox, SSH, `man` utilities                   | 2025-10-15  |
| **Codex Enablement**| README_CODEX, agent specs, CI smoke tests       | 2025-11-01  |

## Key Highlights
- **Sub‑200 ms Cold Boot** on reference seL4 hardware while bootstrapping remote GPU annex links.
- **End-to-End GPU Auditability** through signed Secure9P exchanges with Cohesix CUDA Servers.
- **80%+ Pass Coverage** across compiler pipelines.

## Use Cases & Verticals
- **Financial Services:** Secure transaction enclaves and audit trails.
- **Advertising & Retail:** Privacy‑preserving analytics at the edge.
- **Energy & Mining:** Autonomous control with strict isolation.
- **Defense & Military:** Verifiable platforms for mission‑critical deployments.
- **Autonomy & Robotics:** Embedded control with physics feedback and deterministic execution
