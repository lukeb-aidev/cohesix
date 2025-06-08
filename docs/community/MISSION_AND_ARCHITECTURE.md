// CLASSIFICATION: COMMUNITY
// Filename: MISSION_AND_ARCHITECTURE.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-06-20

# Mission and Architecture

Cohesix aims to deliver a secure, fast edge operating system with an integrated compiler toolchain. The project uses the seL4 microkernel and a Plan 9 userland to ensure minimal attack surface and deterministic behaviour. GPU and physics services are first‑class citizens, enabling real‑time AR and robotics workloads.

## Architecture Snapshot
- **Kernel:** vanilla seL4 with Cohesix patches
- **Userland:** Plan 9 services in a 9P namespace
- **Boot Target:** cold start under 200 ms on reference SBCs
- **Security:** seL4 proofs plus sandbox caps for services
- **CohRole:** declared before kernel init and exposed via `/srv/cohrole`
- **Roles:** QueenPrimary, KioskInteractive, DroneWorker, GlassesAgent, SensorRelay, SimulatorTest
- **Physics Core:** Rapier‑based `/sim/` for workers
- **GPU Support:** `/srv/cuda` with graceful fallback if absent
- **OSS Policy:** only Apache 2.0, MIT, or BSD components

## Implementation Overview
- **Bootloader:** minimal seL4 loader with early telemetry
- **Startup Path:** fixed sequence reaching userland in <200 ms
- **Service Layout:** modular services exported under `/srv/`
- **Validation:** embedded rule engine monitors syscalls and traces
- **Upgrades:** monthly upstream sync of seL4 and 9front

## Project Objectives
1. Preserve seL4 proofs end‑to‑end.
2. Provide high performance physics and AI offload.
3. Offer a cohesive compiler (`cohcc`) with robust tests.
4. Encourage open development under permissive licenses.

Cohesix delivers a formally verified foundation for tomorrow’s edge and wearable devices while keeping tooling familiar and efficient.
