// CLASSIFICATION: PRIVATE
// Filename: COMMERCIAL_PLAN.md v1.5
// Date Modified: 2025-06-08
// Author: Lukas Bower

# Cohesix Commercial Plan

## 1. Executive Summary
Cohesix delivers a formally verified, micro‑kernel–based edge compute platform combining seL4 security guarantees with Plan 9 simplicity. Targeting mission‑critical robotics, IoT, and next‑gen AR deployments, the platform enables deterministic, secure workloads at scale. Revenue stems from device licensing, enterprise support, and cloud‑managed services.

## 2. Problem Statement
- **Security & Compliance Gaps** – Linux edge stacks expose large attack surfaces and inconsistent patching.
- **Complex Dev Experience** – Fragmented tooling across languages slows time‑to‑market for OEMs.
- **Performance Trade‑offs** – RTOS options lack extensibility; general OSes remain too heavy for constrained devices.

## 3. Solution Overview
- **Core Platform** – seL4 micro‑kernel with formal proofs and a Plan 9 userland served via 9P namespaces.
- **Modular Roles** – QueenPrimary, DroneWorker, KioskInteractive, GlassesAgent, and SensorRelay profiles ship pre‑configured.
- **Integrated Toolchain** – Coh‑CLI and Coh‑SDK in Go, Node.js, and Python for virtualization, SBOM generation, and CI automation.
- **Ecosystem Services** – Managed updates over TLS, remote attestation, and telemetry dashboards.

## 4. Market Analysis
- **Total Addressable Market (2026)**
  - Industrial robotics controllers: USD $4.2 B
  - Commercial IoT gateways: USD $6.5 B
  - AR/VR edge devices: USD $2.8 B
- **Key Trends**
  - Regulatory demand for verifiable security (IEC 61508, ISO 27001)
  - Shift toward cloud‑edge continuum computing
  - Growth in multi‑agent robotics and real‑time world models

## 5. Business Model & GTM Milestones
1. **Per‑Device Licensing** – Tiered by role profile (DroneWorker AUD $50/device/year; QueenPrimary AUD $200/device/year).
2. **Enterprise Support & Consulting** – SLA tiers (Standard, Premium, Platinum) with custom driver and integration services.
3. **Managed Services** – Cloud‑hosted updates, telemetry analytics, and rule governance starting at AUD $1,000/month per 1,000 devices.

**Quarterly Milestones**
| Quarter | Focus                                         |
|---------|-----------------------------------------------|
| Q3 2025 | Closed beta pilots with three industrial partners |
| Q4 2025 | Public beta and case‑study marketing             |
| Q1 2026 | General availability and channel onboarding      |
| Q2 2026 | Marketplace listings on AWS and Azure            |
| Q3 2026 | Expansion into consumer AR/VR OEM engagements    |

## 6. Financial Projections (AUD)
| Year                | 2025 (H2) | 2026       | 2027       | 2028       |
|---------------------|-----------|------------|------------|------------|
| License Revenue     | 0.2 M     | 2.5 M      | 7.8 M      | 15.4 M     |
| Support & Services  | 0.1 M     | 1.2 M      | 3.6 M      | 7.2 M      |
| Managed Services    | 0.0 M     | 0.5 M      | 2.0 M      | 5.0 M      |
| **Total Revenue**   | **0.3 M** | **4.2 M**  | **13.4 M** | **27.6 M** |
| **Gross Margin**    | 65%       | 68%        | 70%        | 72%        |

## 7. Key Partnerships & Channels
- **Hardware OEMs** – NVIDIA (Jetson), Raspberry Pi Foundation
- **Cloud Providers** – AWS and Microsoft Azure channel listings
- **Systems Integrators** – Robotics and industrial automation VARs
- **Standards Bodies** – seL4 Foundation and IEC working groups

### Investors & Partners
- Seed investors from Australian deep‑tech funds
- Ongoing discussions with Telstra Ventures
- NVIDIA Inception partner status
- University collaboration via UNSW and Monash

### Leveraging the EY Network
As an EY Technology Partner, we can tap the global EY ecosystem for warm
introductions and credibility with enterprise clients. Engagements remain
non‑exclusive and focus on high‑level advisory so that Cohesix retains full
commercial independence and IP control.

## 8. Open‑Source Benchmarking
Cohesix regularly publishes upstream performance comparisons against Linux and FreeRTOS on reference hardware. Benchmarks include boot time, IPC latency, and deterministic scheduling metrics.

## 9. Expert Panel
| Name            | Expertise                 | Contribution                |
|-----------------|---------------------------|-----------------------------|
| Dr. Jane Rowe   | Formal methods            | Kernel proof verification   |
| Alex Chen       | Robotics integration      | Industrial partner liaison  |
| Priya Natarajan | Cloud infrastructure      | Managed service scaling     |
| Victor Ng       | OSS community management  | Open‑source outreach        |

## 10. Risk Assessment & Mitigation
| Risk                                    | Mitigation                                |
|-----------------------------------------|-------------------------------------------|
| Slow enterprise procurement cycles      | Leverage pilot partners; target green‑field deployments |
| Adoption inertia vs. entrenched Linux   | Highlight formal security proofs and early wins |
| OSS license compliance                  | Enforce Apache 2.0/MIT/BSD via automated SBOM |
| Toolchain fragmentation                 | Prioritize Coh‑CLI SDK with plugin model  |

## 11. Metrics & Traction Plan
- **Time‑to‑integration** – target < 4 weeks for new OEMs
- **Uptime SLA** – maintain 99.9% device availability
- **Developer NPS** – aim for a score above 40
- **Monthly active deployments** – report growth rate quarterly

## 12. Appendix
- **IP Strategy** – kernel extensions dual‑licensed; userland and SDK permissive OSS
- **Milestones** – M1 closed beta (Q3 2025); M2 GA release (Q1 2026); M3 marketplace certification (Q3 2026)

