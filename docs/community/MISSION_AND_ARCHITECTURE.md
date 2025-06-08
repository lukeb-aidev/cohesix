// CLASSIFICATION: COMMUNITY
// Filename: MISSION_AND_ARCHITECTURE.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-07-12

# Cohesix Mission and Architecture

Cohesix is a secure, lightning‑fast operating system and compiler suite for edge devices. It leverages the seL4 microkernel and a Plan 9 style userland to deliver sub‑200 ms boot times and reliable GPU offload.

## Vision
- **Guardian of Trust:** retain seL4 proofs end‑to‑end.
- **Seamless Acceleration:** integrate Rapier physics and TensorRT for real‑time workloads.
- **Rapid Innovation:** provide a cohesive compiler (`Coh_CC`) with a robust pass framework.
- **Community Growth:** open‑source licensing (Apache 2.0/MIT/BSD) encourages contributions.

## Architecture Snapshot
- **Kernel:** seL4 L4-microkernel with Cohesix patches.
- **Userland:** Plan 9 services using 9P and `rc` shell.
- **Boot target:** under 200 ms on reference SBCs.
- **Security:** seL4 proofs preserved; Plan 9 srv sandbox caps.
- **CohRole:** declared before kernel init via `/srv/cohrole`.
- **Roles:** QueenPrimary, KioskInteractive, DroneWorker, GlassesAgent, SensorRelay, SimulatorTest.
- **Physics Core:** Rapier-based `/sim/` for Workers with optional CUDA.
- **GPU Support:** `/srv/cuda` with graceful fallback logging.
- **OSS Policy:** only Apache 2.0, MIT, or BSD licensed components.

## Role Manifest
At boot, the role in `/srv/cohrole` determines which services start:

| Role | Description |
|------|-------------|
| QueenPrimary | Orchestrator and CI coordinator via gRPC control plane. |
| RegionalQueen | Cluster orchestrator handling scaling and failover. |
| BareMetalQueen | Standalone orchestrator for private networks. |
| DroneWorker | Physics and sensor processing via `/sim/`. |
| KioskInteractive | Local HMI and UI rendering. |
| GlassesAgent | AR pipeline using CUDA overlays. |
| SensorRelay | Streams sensor data between roles. |
| SimulatorTest | Scenario replay using SimMount and trace logs. |

## Reference Hardware
- Jetson Orin Nano 8 GB — primary Worker with CUDA tests.
- Raspberry Pi 5 8 GB — fallback Worker for fast boot.
- AWS EC2 Graviton/x86 — Queen role and CI orchestration.
- Intel NUC‑13 Pro — optional development host.

## Project Highlights
- Sub‑200 ms cold boot on Jetson Orin Nano.
- GPU offload latency under 5 ms.
- Unified 9P namespace for dynamic service composition.
- 80%+ compiler pass test coverage.

Cohesix empowers secure, edge‑native applications from robotics to AR. Join the community and build the future one microkernel at a time.
