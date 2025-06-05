// CLASSIFICATION: PRIVATE
// Filename: ARCHITECTURE.md v0.3
// Author: Lukas Bower
// Date Modified: 2025-05-30

# Cohesix System Architecture · v0.3 (Private)

> **Canonical cross‑refs:**  
> • INSTRUCTION_BLOCK.md (community) — build & workflow rules  
> • ROLE_MANIFEST.md (private) — runtime role taxonomy  
> • QUEEN_POLICY.md (private) — policy enforcement & trace validation  
> • CI_SETUP.md (community) — build/CI pipeline  
> • TOOLING_PLAN.md (private) — CLI & dev‑tool roadmap  
> • OSS_REUSE.md (private) — third‑party licence ledger  

---


## 1. Philosophy & Heritage

*Cohesix fuses formally‑verified micro‑kernel security, Plan 9’s unified file‑namespace model, and deterministic embedded physics to create a secure, world‑aware OS that scales seamlessly from cloud queens to edge workers for AI‑centric workloads.*

| Influence | How it shapes Cohesix |
|-----------|-----------------------|
| **Plan 9** (Bell Labs) | • Uniform *everything‑is‑a‑file* model via 9P.  <br>• Per‑process namespaces (bind, mount) for extreme composability. |
| **seL4 micro‑kernel** | • Formal proofs of correctness & isolation.  <br>• Capability‑based security at the lowest level. |
| **Rapier Physics** | • Deterministic, cross‑platform rigid‑body engine embedded in the OS for *world‑aware* agents. |
| **Cloud / Edge swarm** | • “QueenPrimary” orchestrates ephemeral “Workers” (drones, kiosks, sensors) via scalable 9P over QUIC. |

---

## 2. High‑Level Stack Diagram

```text
┌──────────────────────────────────────────────────────────────┐
│  User‑Space Roles                                            │
│  (Rust / Go binaries + 9P namespace)                          │
│  ├─ QueenPrimary  ──┐                                         │
│  │                 │ 9P(QUIC) / CohCap                        │
│  └─ Worker*        ◄┘                                         │
│                                                              │
│  Cohesix Runtime Services (Go)                               │
│  ├─ cohshd  – interactive shell / 9P mux                      │
│  ├─ cohtrace – trace writer + validator hook                  │
│  └─ cohbuild – remote build helper                            │
│                                                              │
│  Fabric OS Layer (Rust)                                      │
│  ├─ Namespace Router (9P)                                     │
│  ├─ Syscall Sandbox                                           │
│  ├─ Rapier Physics Daemon ↔ Worker sensors                    │
│  └─ Runtime Validator (seL4 cap shim)                         │
│                                                              │
│  seL4 Micro‑Kernel + Proofs (C)                               │
│  └─ Capability space, threads, IPC                            │
└──────────────────────────────────────────────────────────────┘
```

---

## 3. Kernel & Capability Model

* **seL4** runs unmodified, with Cohesix patches for:
  * Fast user‑level IPC shortcut (`coh_fastpath`) for physics updates.
  * Additional boot‑time capability slots exposing Rapier DMA buffers.
  * Role‑hint propagation: boot‑loader writes **CohRole** string into `/srv/cohrole` avail­able to userland.

* **Boot target:** <200 ms cold‑start on Raspberry Pi 5 & Jetson Orin.

---

## 4. Namespaces & Protocols

| Layer | Protocol | Notes |
|-------|----------|-------|
| Transport | **QUIC** (via Quinn) | Handles NAT, TLS 1.3, loss recovery. |
| FS / IPC  | **9P2000.L** | Extended Plan 9 protocol (large packets, auth). |
| Control   | **CohCap** | JSON‑capability envelope carried inside 9P attach for fine‑grained rights. |

Every OS object—device, shader, physics body—is surfaced as a 9P file.  
*Examples:* `/sim/objects/net_tilt`, `/cuda/metrics/power0`.

---

## 5. Rapier Physics Service

* Runs in a dedicated user‑space server (`/srv/rapier0`).
* Exposes:
  * `/sim/world` — binary scene snapshot (9P read).
  * `/sim/step`  — write tick request (blocking).
* Deterministic across x86_64 / aarch64 by forcing `f32` maths & Rapier’s `dim3` feature flags.

---

## 6. Roles

### 6.1 QueenPrimary  
* Always cloud‑hosted (Xeon or Ampere Altra).  
* Responsibilities: scheduling, snapshot archive, rule distribution, global telemetry.

### 6.2 Worker (variants)  
| Variant | Hardware | Typical duty |
|---------|----------|--------------|
| **DroneWorker** | Jetson Orin Nano | Edge inference + sensor fusion |
| **KioskInteractive** | Raspberry Pi 5 + display | UI, adverts |
| **GlassesAgent** | Snapdragon XR | AR overlay with 9P mount |
| **SimulatorTest** | GitHub Actions | CI & fuzz harness |

---

## 7. Language Strategy

| Layer | Language | Rationale |
|-------|----------|-----------|
| Kernel patches | **C** | seL4 demands C for proof continuity. |
| Low‑level / drivers | **Rust** | Memory‑safe, zero‑cost abstractions, fearless concurrency. |
| 9P services & CLI | **Go** | CSP model, quick cross‑compile, excellent 9P libs. |
| Trace / tooling | **Python** | Rapid scripting, data science, JIT validation. |
| CUDA models | **C++ / CUDA** | Torch‑&‑TensorRT deployments on Jetson. |

---

## 8. OSS Dependencies (core subset)

| Crate / Project | Licence | Used for |
|-----------------|---------|----------|
| **seL4** | GPL v2 (kernel) + proofs | Micro‑kernel |
| **Rapier** | MIT/Apache‑2 | Physics |
| **quinn** | MIT/Apache‑2 | QUIC transport |
| **go‑plan9** | BSD | 9P client/server |
| **clap** | MIT | CLI parsing |
| **serde** | MIT/Apache‑2 | JSON caps |
| **rust‑crypto** | MIT | Hashing, HMAC |
| **ring** | BSD‑style | TLS helpers |
| **SPDX‑SBOM‑Generator** | Apache‑2 | Licence scan in CI |

(Full ledger lives in **OSS_REUSE.md**.)

---

## 9. Security Model

* seL4 capabilities ↔ 9P attach tokens (CohCap) are 1‑to‑1 mapped.
* Runtime Validator enforces policy rules (see `QUEEN_POLICY.md`) with hot‑reload.
* Supply‑chain: GitHub Actions uploads SBOM to dependency tracker; `cargo deny` blocks unknown licences.

---

## 10. Roadmap Anchors

| Milestone | Kernel | Runtime | Physics | CI |
|-----------|--------|---------|---------|----|
| **v0.4‑fabric‑alpha** | seL4 fast‑path patch + proof regen | Namespace router + sandbox | Rapier service stub | fmt + clippy gate |
| **v0.5‑fabric‑beta** | IRQ shielding | Live validator reload | World replay harness | SBOM, licence check |
| **v0.6‑MVP** | Verified IOMMU | CohCap v1 | Deterministic multi‑agent | Cross‑arch CI |

---

### Appendix A — Glossary

| Term | Meaning |
|------|---------|
| **CohRole** | Immutable string set at boot identifying role (queen vs worker). |
| **CohCap** | JSON envelope holding seL4 cap refs, expiry, and role scopes. |
| **Fabric OS** | Rust user‑space layer providing sandbox, namespace router, validator. |

---

*End of ARCHITECTURE.md v0.3*