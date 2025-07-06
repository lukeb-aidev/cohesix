// CLASSIFICATION: COMMUNITY
// Filename: HARDWARE_STRATEGY.md
// Author: Lukas Bower
// Date Modified: 2025-07-02

# Cohesix Hardware Strategy

## Purpose
This document defines the canonical hardware strategy for Cohesix deployments across cloud, edge, and test environments. It ensures alignment with our pure UEFI + Plan9 + 9P architecture, removing all Linux dependency and rejecting prior Jetson concepts.

All choices comply with the bulletproof rules in INSTRUCTION_BLOCK.md:
- Single-step hydration, atomic writes
- No stubs, validated metadata
- Structured by role, with explicit hardware mapping

## Strategic Principles
- **Pure UEFI Boot:** All Cohesix nodes boot via UEFI firmware, enabling direct load of our microkernel and userland without POSIX layers.
- **Plan9 & 9P Everywhere:** All file operations, telemetry, and validator integrations use the 9P protocol.
- **Secure Enclaves & SeL4 foundation:** Hardware selected must support clean MMU + IOMMU for secure microkernel operations.
- **GPU strategy:** CUDA is only utilized under controlled conditions on PCIe NVIDIA GPUs in x86_64 or ARM64 servers, rejecting Jetson / L4T.

---

## Role-based hardware mapping

| Role                        | Recommended Hardware                    | Notes |
|-----------------------------|----------------------------------------|-------|
| **QueenPrimary / RegionalQueen / BareMetalQueen** | x86_64 servers (Dell, Supermicro, AWS x86 with UEFI) | Orchestration, CI, validator enforcement |
| **DroneWorker (Edge Compute)** | ARM64 UEFI boards (NXP Layerscape LX2160A, Ampere Altra) | Pure Plan9 workloads, no Linux fallback |
| **KioskInteractive / GlassesAgent** | x86_64 micro PCs (Dell OptiPlex Micro, Supermicro E300) | Direct UEFI, runs Plan9 GUI / AR workloads |
| **SensorRelay / SimulatorTest** | ARM64 UEFI (SolidRun LX2) or Intel NUC | Lightweight, sensors + scenario replay validation |
| **AWS Testing / CI burst** | AWS g4dn.xlarge / g5.xlarge (UEFI x86 + NVIDIA GPU), AWS m6i/c6i for standard tests | Mirrors edge hardware, runs validator scenarios |

---

## Hardware examples and shopping notes

### x86_64 UEFI options
- **Dell OptiPlex Micro + NVIDIA T600/T1000:** Small edge boxes, PCIe GPUs, robust UEFI.
- **Supermicro E300-9D:** Compact Xeon D platform, PCIe slots for NVIDIA cards.
- **Intel NUC:** Useful for developer benches and quick scenario test beds.

### ARM64 UEFI options
- **NXP Layerscape LX2160A boards (e.g. SolidRun HoneyComb LX2K):** Telco-grade, passive cooling, Plan9-native edge nodes.
- **Ampere Altra dev boards:** Larger, supports PCIe GPUs, for heavy ARM workloads.

### Cloud mirrors
- **AWS g4dn.xlarge (NVIDIA T4 GPU) or g5.xlarge (A10G):** Fully UEFI, mirrors PCIe CUDA setups.
- **AWS Graviton instances (for non-GPU tests):** Runs pure ARM64, aligned with DroneWorker logic.

---

## Alignment with QUEEN_WORKER_CLOUD_SIMULATION.md
This hardware plan is formally integrated with our canonical orchestration model, as documented in `QUEEN_WORKER_CLOUD_SIMULATION.md`. All roles, scenario replays, and validator tasks described there are hardware mapped here.

---

## Compliance with instruction block
- Hydrated in single atomic write.
- Structured under clear headings.
- All hardware choices traceable to role needs, with explicit UEFI & Plan9 constraints.

---

# ✅ End of HARDWARE_STRATEGY.md
