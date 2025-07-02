

// CLASSIFICATION: COMMUNITY
// Filename: QUEEN_WORKER_CLOUD_SIMULATION.md
// Author: Lukas Bower
// Date Modified: 2025-07-02

# Cohesix Queen-Worker Cloud Simulation Model

## Purpose
This document defines the canonical architecture for Cohesix orchestration, covering how Queen roles and Worker roles coordinate across local QEMU, edge hardware, and cloud resources.

It ensures consistency with our UEFI + Plan9 + 9P design, removing all Linux dependencies and rejecting earlier Jetson plans.

---

## Orchestration model
- **QueenPrimary:** Main cloud orchestrator, controls multi-node validation, CI/CD flows, scenario execution, telemetry ingestion.
- **RegionalQueen:** Sub-cloud node for multi-region resilience, load balancing orchestration jobs.
- **BareMetalQueen:** On-prem orchestration node, runs the same validator + scenario engine stack.

- **DroneWorker:** Edge compute nodes executing Plan9 workloads directly. Provide local telemetry, run physics via Rapier, enforce 9P validator rules.
- **KioskInteractive / GlassesAgent:** Edge nodes with UI, AR rendering, direct Plan9 interface.
- **SensorRelay:** Collects raw sensor data, aggregates for higher-order nodes.
- **SimulatorTest:** Pure QEMU or container simulation nodes running multi-agent test scenarios, replaying scenario traces.

---

## QEMU + EC2 strategy
- All core roles can be booted in QEMU for validation, using UEFI boot with direct Plan9 microkernel loads.
- We leverage AWS x86 instances (m6i, c6i) for general CI and g4dn.xlarge / g5.xlarge for GPU validation, matching physical worker PCIe topologies.
- Uses 9P mounts to synchronize scenario data and validator logs with the QueenPrimary.

---

## GPU orchestration fallback
- While Cohesix itself is purely UEFI + Plan9, CUDA workloads can be offloaded under tightly controlled conditions to secure PCIe NVIDIA nodes.
- Orchestrators validate workload assignments and ensure telemetry streams into the Plan9 validator before acceptance.

---

## Plan9 + 9P everywhere
- All file systems, logs, scenario traces, CI reports, and agent communications run over 9P.
- This creates a single uniform namespace across cloud, edge, and local development, minimizing toolchain complexity and maximizing validator coverage.

---

## Alignment with HARDWARE_STRATEGY.md
This orchestration model is mapped explicitly to hardware configurations detailed in `HARDWARE_STRATEGY.md`, ensuring each role has a canonical platform (UEFI x86 or ARM64) documented there.

---

# ✅ End of QUEEN_WORKER_CLOUD_SIMULATION.md