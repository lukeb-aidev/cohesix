// CLASSIFICATION: COMMUNITY
// Filename: BATCH_PLAN.md v0.4
// Date Modified: 2025-06-10
// Author: Lukas Bower

## Cohesix Batch Plan

This document tracks all high-level work phases and discrete batches for the Cohesix project. Each batch is a self-contained unit with clear deliverables, status, and dependencies.

Batches are executed through ChatGPT agents defined in `AGENTS.md`. A 15‑minute watchdog described in `WATCHDOG_POLICY.md` ensures tasks do not stall and provides automated oversight.

### 1 · Coh_CC Compiler Batches
All compiler-related work for building and scaffolding the Coh_CC toolchain.

| Batch Range      | Deliverables                                                                                   | Status    |
|------------------|-----------------------------------------------------------------------------------------------|-----------|
| 1–4              | IR design, pass framework, example passes, unit tests                                          | 🟢 Hydrated |
| 5–11             | Optimization passes (NOP removal, dead code, constant folding), SSA renaming                  | 🟢 Hydrated |
| 12–17            | SSA phi insertion, IR validation, codegen interface, WASM/C backends, CLI integration          | 🟢 Hydrated |
| 18               | WASM & C backends scaffold                                                                      | 🟢 Hydrated |
| 19–24            | Codegen dispatcher, CLI args & integration, example IR input                                   | 🟢 Hydrated |
| 25               | Example IR module stub                                                                         | 🟢 Hydrated |
| 26               | Regression & performance harness (benchmarks, code-size, timing)                               | ⏳ Queued  |
| 27               | API documentation generator (Rustdoc, markdown)                                    | ⏳ Queued  |

*Agents*: `scaffold_service`, `add_pass`, `run_pass`


### 2 · Cohesix OS Batches
Operating system and runtime environment scaffolding.

| Batch  | Deliverables                                                                                   | Status    |
|--------|-----------------------------------------------------------------------------------------------|-----------|
| O1     | seL4 boot hydration & CohRole init; Plan 9 mount logic                                         | ⏳ Queued  |
| O2     | Plan 9 namespace server, `rc` shell adjustments, POSIX shims                                   | ⏳ Queued  |
| O3     | Sandbox caps, security proof integration, validation scripts                                   | ⏳ Queued  |
| O4     | Physics core service (`/sim/`) integration with Rapier                                         | ⏳ Queued  |
| O5     | GPU offload service (`/srv/cuda`) with Torch/TensorRT integration                              | ⏳ Queued  |
| O6     | Driver model & hardware abstraction layer                                                      | ⏳ Queued  |
| O7     | Full OS image assembly, reproducible build, CI smoke tests                                     | ⏳ Queued  |
| O8     | Service health & recovery: watchdog scripts, container healthchecks, auto-restart logic        | ⏳ Queued  |

*Agents*: `scaffold_service`

### 3 · Tooling Batches
Common CLI and system utilities adaptation for a Linux-like UX.

| Batch | Deliverables                                                                                   | Status    |
|-------|-----------------------------------------------------------------------------------------------|-----------|
| T1    | BusyBox coreutils integration                                                                  | ⏳ Queued  |
| T2    | SSH & networking tools                                                                         | ⏳ Queued  |
| T3    | Manual pages & help system (`man`, `help`, `mandoc`)                                           | ⏳ Queued  |
| T4    | Logging utilities (`last`, `finger`, rotation)                                                 | ⏳ Queued  |
| T5    | Package manager stub & installation scripts                                                    | ⏳ Queued  |
| T6    | Monitoring & healthcheck services                                                               | ⏳ Queued  |
| T7    | Distributed build tooling (SSH-driven CI, remote artifact staging)                             | ⏳ Queued  |
| T8    | CI helper scripts (`build-busybox.sh`, `deploy-ci.sh`, smoke-test runners)                      | ⏳ Queued  |

*Agents*: `add_cli_option`

### 4 · Codex Enablement Batches
Preparation and validation for AI-driven code generation and automation.

| Batch | Deliverables                                                                                   | Status    |
|-------|-----------------------------------------------------------------------------------------------|-----------|
| C1    | Stub specifications & version pinning in `DEPENDENCIES.md`                                     | ⏳ Queued  |
| C2    | CI smoke tests & `README_Codex.md`                                                              | ⏳ Queued  |
| C3    | API adapter & driver integration for Codex agents                                               | ⏳ Queued  |
| C4    | Agent instructions, example tasks, initial tests (`tests/codex/`)                               | ⏳ Queued  |
| C5    | Logging & audit trails for Codex outputs (`codex_logs/`)                                        | ⏳ Queued  |
| C6    | Agent self-test harness: validate `AGENTS.md` schema and sample runs                            | ⏳ Queued  |

*Agents*: `hydrate_docs`, `validate_metadata`

### 5 · Testing & QA Batches
End-to-end testing infrastructure to ensure platform quality.

| Batch | Deliverables                                                                                   | Status    |
|-------|-----------------------------------------------------------------------------------------------|-----------|
| X1    | Root `tests/` directory with subfolders and dummy tests for compiler, passes, OS, Codex         | 🟢 Hydrated |
| X2    | Security & OWASP compliance tests: threat model, dependency scanning, container scanning        | ⏳ Queued  |

*Agents*: `run_pass`

### 6 · Documentation & Auxiliary Batches
Ensure project documentation and support files are complete and maintained.

| Batch | Deliverables                                                                                   | Status    |
|-------|-----------------------------------------------------------------------------------------------|-----------|
| D1    | Security docs (`docs/security/SECURITY_POLICY.md`, `THREAT_MODEL.md`)                          | 🟢 Hydrated |
| D2    | Build plan doc (`BUILD_PLAN.md`)                                                               | 🟢 Hydrated |
| D3    | CHANGELOG.md entry maintenance and consistency check                                          | 🟢 Hydrated |
| D4    | README.md, CONTRIBUTING.md, CODE_OF_CONDUCT.md                         | 🟢 Hydrated |

*Agents*: `hydrate_docs`, `validate_metadata`

### 7 · Demo Integration Batches
End-user demo pipelines and helper services for showcasing Cohesix features.

| Batch | Deliverables                                             | Status    |
|-------|----------------------------------------------------------|-----------|
| D1    | Webcam capture + gesture service for Workers (demos 1–3) | ⏳ Queued |
| D2    | QR-based SLM loader and app swap (demo 6)                | ⏳ Queued |
| D3    | NAT rendezvous service for auto-attach (demo 10)         | ⏳ Queued |
| D4    | Trace replay and fairness harness (demos 4 & 8)          | ⏳ Queued |
| D5    | KioskInteractive UI provisioning pipeline (demo 5)       | ⏳ Queued |
| D6    | CLI scenario orchestration & Codex trigger helpers (demo 9) | ⏳ Queued |

---

*Statuses*: ✅ = Complete, ⏳ = Queued, ⏸️ = Deferred, 🚧 = Blocked, 🗑️ = Deprecated, 🟢 = Hydrated
