// CLASSIFICATION: COMMUNITY
// Filename: SHOPPING_LIST.md v1.3
// Date Modified: 2025-05-25
// Author: Lukas Bower

# Hardware & Cloud Shopping List

Cohesix development and CI leverage both on-prem edge hardware and AWS cloud resources to ensure comprehensive testing, scalability, and cost-efficiency.

## Local Edge Hardware

| Item                    | Qty | Est. Cost | Purpose & Notes                                      |
|-------------------------|-----|-----------|------------------------------------------------------|
| Jetson Orin Nano 8 GB   | 2   | $199      | Primary Worker nodes; test GPU offload via `/srv/cuda` |
| Raspberry Pi 5 8 GB     | 3   | $80       | Fallback Workers & developer kits                     |
| Intel NUC-13 Pro        | 1   | $600      | Optional dev/test host; multi-arch validation         |

## Cloud & CI Resources

| Service                  | Configuration               | Est. Cost    | Purpose & Notes                                      |
|--------------------------|-----------------------------|--------------|------------------------------------------------------|
| AWS Graviton C6g         | c6g.large (2 vCPU, 4 GB RAM) | $0.08/hr     | ARM-native CI runners for aarch64 build validation   |
| AWS EC2 t3.medium        | t3.medium (2 vCPU, 4 GB RAM) | $0.0416/hr   | General x86 CI build & test runner                   |
| AWS S3 Storage           | Standard Tier, 50 GB        | $1.15/mo     | Artifact storage for CI logs, images, `codex_logs/`   |
| Amazon RDS (Optional)    | db.t3.micro (1 vCPU, 1 GB)  | $15/mo       | Central metadata DB for batch orchestration state    |

## Rationale

- **On-Prem Edge Hardware:** Replicates real deployment targets for low-latency I/O and GPU acceleration tests.
- **Intel NUC:** Provides a fast, versatile environment for debugging and cross-architecture verification.
- **ARM CI on Graviton:** Eliminates cross-compilation issues and accelerates CI for aarch64 targets.
- **x86 CI on t3.medium:** Complements ARM builds with x86 coverage, ensuring multi-arch robustness.
- **S3 & RDS:** Persistent storage and state management for CI artifacts, logs, and automated batch control data.