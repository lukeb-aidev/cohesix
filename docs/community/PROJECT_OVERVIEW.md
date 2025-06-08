// CLASSIFICATION: COMMUNITY
// Filename: PROJECT_OVERVIEW.md v1.6
// Date Modified: 2025-05-24
// Author: Lukas Bower


# Project Overview
*See [Mission](MISSION.md) for our vision, objectives, and value proposition.*

Welcome to **Cohesix**, a self-contained, formally verified operating system and compiler suite optimized for edge and wearable devices. Cohesix combines the seL4 microkernel’s proven security model with a modular Plan 9–style userland and GPU/physics offload services—enabling next-gen applications in AR, robotics, and autonomous systems without cloud dependencies.

## Why It Matters

- **Security by Design:** seL4 proofs ensure complete isolation between processes, drivers, and services, eliminating critical vulnerability classes.
- **Blazing-Fast Boot:** Achieves sub-200 ms cold starts on reference SBCs (Jetson Orin Nano, Raspberry Pi 5).
- **High-Performance Offload:** < 5 ms latency for GPU tasks via `/srv/cuda`, and real-time physics through `/sim/`.
- **Extensible Architecture:** 9P-driven namespaces let you mount and compose services dynamically, from logging to simulators.
- **Familiar Toolchain:** BusyBox-powered CLI with POSIX shims accelerates developer onboarding using standard *nix workflows.

## Phases & Milestones

| Phase               | Deliverables                                              | Target Date  |
|---------------------|-----------------------------------------------------------|--------------|
| **Compiler**        | Coh_CC passes (IR → WASM/C) + unit tests                  | 2025-06-30   |
| **Boot & Runtime**  | seL4 + Plan 9 userland bootable image                     | 2025-08-15   |
| **GPU & Physics**   | `/srv/cuda` service + Rapier physics integration          | 2025-09-30   |
| **Coreutils & CLI** | BusyBox coreutils, SSH, `man`, logging utilities          | 2025-10-15   |
| **Codex Enablement**| README_Codex, agent task specs, CI smoke tests            | 2025-11-01   |

## Success Criteria

- **Performance:** Cold start < 200 ms; GPU offload < 5 ms.
- **Footprint:** < 256 MB RAM on target hardware.
- **Security:** Zero unresolved CVEs; seL4 proofs validated in CI.
- **Coverage:** ≥ 80% automated test coverage across core components.

---

For detailed tasks and schedules, see [`RELEASE_AND_BATCH_PLAN.md`](RELEASE_AND_BATCH_PLAN.md). To onboard AI automation, consult [`README_Codex.md`](README_Codex.md) and [`AGENTS_AND_CLI.md`](AGENTS_AND_CLI.md).
