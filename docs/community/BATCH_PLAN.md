// CLASSIFICATION: COMMUNITY
// Filename: BATCH_PLAN.md v0.6
// Date Modified: 2025-06-08
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

#### Batch Details
- **1–4**
  - IR design and pass framework
  - Example passes with unit tests
  - Dependencies: none
  - Build: `cargo build && cargo test`
  - Demos: N/A
- **5–11**
  - Optimization passes: NOP removal, dead code, constant folding
  - SSA renaming
  - Dependencies: batches 1–4
  - Build: `cargo test --all`
  - Demos: N/A
- **12–17**
  - SSA phi insertion and IR validation
  - Codegen interface, WASM/C backends, CLI integration
  - Dependencies: batches 5–11
  - Build: `cargo build --features wasm`
  - Demos: N/A
- **18**
  - WASM & C backends scaffold
  - Dependencies: batch 12–17
  - Build: `cargo build --examples`
  - Demos: N/A
- **19–24**
  - Codegen dispatcher and CLI integration with example IR input
  - Dependencies: batch 18
  - Build: `cargo test -p cli`
  - Demos: N/A
- **25**
  - Example IR module stub
  - Dependencies: batches 19–24
  - Build: `cargo test`
  - Demos: N/A
- **26**
  - Regression & performance harness (benchmarks, code-size, timing)
  - Dependencies: batch 25
  - Build: `cargo bench`
  - Demos: N/A
- **27**
  - API documentation generator via Rustdoc and markdown
  - Dependencies: batch 26
  - Build: `cargo doc --no-deps`
  - Demos: N/A


### 2 · Cohesix OS Batches
Operating system and runtime environment scaffolding.

| Batch  | Deliverables                                                                                   | Status    |
|--------|-----------------------------------------------------------------------------------------------|-----------|
| O1     | seL4 boot hydration & CohRole init; Plan 9 mount logic                                         | 🟢 Hydrated  |
| O2     | Plan 9 namespace server, `rc` shell adjustments, POSIX shims                                   | 🟢 Hydrated  |
| O3     | Sandbox caps, security proof integration, validation scripts                                   | 🟢 Hydrated  |
| O4     | Physics core service (`/sim/`) integration with Rapier                                         | 🟢 Hydrated  |
| O5     | GPU offload service (`/srv/cuda`) with Torch/TensorRT integration                              | 🟢 Hydrated  |
| O6     | Driver model & hardware abstraction layer                                                      | 🟢 Hydrated  |
| O7     | Full OS image assembly, reproducible build, CI smoke tests                                     | ⏳ Queued  |
| O8     | Service health & recovery: watchdog scripts, container healthchecks, auto-restart logic        | 🟢 Hydrated  |
| O9     | Cloud-native hooks for Queen roles: bootstrapping, auto-scaling triggers, and metrics export  | 🟢 Hydrated  |

*Agents*: `scaffold_service`

#### Batch Details
- **O1**
  - seL4 boot hydration and CohRole init
  - Plan 9 mount logic
  - Dependencies: upstream seL4
  - Build: `make sel4`
  - Demos: boot showcase
- **O2**
  - Plan 9 namespace server with `rc` adjustments and POSIX shims
  - Dependencies: O1
  - Build: `go build ./...`
  - Demos: N/A
- **O3**
  - Sandbox caps and security proof integration
  - Validation scripts
  - Dependencies: O2
  - Build: run verification suite
  - Demos: N/A
- **O4**
  - Physics core service integration using Rapier
  - Dependencies: O3
  - Build: `cargo build -p sim`
  - Demos: demo 8
- **O5**
  - GPU offload service with Torch/TensorRT
  - Dependencies: O4
  - Build: `cargo build -p cuda_service`
  - Demos: demos 1–3
- **O6**
  - Driver model & hardware abstraction layer
  - Dependencies: O5
  - Build: cross-compile drivers
  - Demos: N/A
- **O7**
  - Full OS image assembly and CI smoke tests
  - Dependencies: O6
  - Build: `make image`
  - Demos: N/A
- **O8**
  - Service health & recovery scripts with auto-restart
  - Dependencies: O7
  - Build: watchdog deployment
  - Demos: N/A

### 3 · Tooling Batches
Common CLI and system utilities adaptation for a Linux-like UX.

| Batch | Deliverables                                                                                   | Status    |
|-------|-----------------------------------------------------------------------------------------------|-----------|
| T1    | BusyBox coreutils integration                                                                  | 🟢 Hydrated  |
| T2    | SSH & networking tools                                                                         | 🟢 Hydrated  |
| T3    | Manual pages & help system (`man`, `help`, `mandoc`)                                           | 🟢 Hydrated  |
| T4    | Logging utilities (`last`, `finger`, rotation)                                                 | 🟢 Hydrated  |
| T5    | Package manager stub & installation scripts                                                    | 🟢 Hydrated  |
| T6    | Monitoring & healthcheck services                                                               | 🟢 Hydrated  |
| T7    | Distributed build tooling (SSH-driven CI, remote artifact staging)                             | 🟢 Hydrated  |
| T8    | CI helper scripts (`build-busybox.sh`, `deploy-ci.sh`, smoke-test runners)                      | 🟢 Hydrated  |

*Agents*: `add_cli_option`

#### Batch Details
- **T1**
  - BusyBox coreutils integration
  - Dependencies: O1
  - Build: `scripts/build-busybox.sh`
  - Demos: N/A
- **T2**
  - SSH & networking tools
  - Dependencies: T1
  - Build: package install scripts
  - Demos: N/A
- **T3**
  - Manual pages & help system
  - Dependencies: T2
  - Build: `mandoc` generation
  - Demos: N/A
- **T4**
  - Logging utilities (`last`, `finger`, rotation)
  - Dependencies: T2
  - Build: `cargo build -p logging`
  - Demos: N/A
- **T5**
  - Package manager stub & install scripts
  - Dependencies: T4
  - Build: `package-manager-stub.sh`
  - Demos: N/A
- **T6**
  - Monitoring & healthcheck services
  - Dependencies: T5
  - Build: compile monitoring tools
  - Demos: N/A
- **T7**
  - Distributed build tooling for CI
  - Dependencies: T6
  - Build: remote build scripts
  - Demos: N/A
- **T8**
  - CI helper scripts
  - Dependencies: T7
  - Build: `deploy-ci.sh`
  - Demos: N/A

### 4 · Codex Enablement Batches
Preparation and validation for AI-driven code generation and automation.

| Batch | Deliverables                                                                                   | Status    |
|-------|-----------------------------------------------------------------------------------------------|-----------|
| C1    | Stub specifications & version pinning in `DEPENDENCIES.md`                                     | 🟢 Hydrated  |
| C2    | CI smoke tests & `README_Codex.md`                                                              | 🟢 Hydrated  |
| C3    | API adapter & driver integration for Codex agents                                               | 🟢 Hydrated  |
| C4    | Agent instructions, example tasks, initial tests (`tests/codex/`)                               | 🟢 Hydrated  |
| C5    | Logging & audit trails for Codex outputs (`codex_logs/`)                                        | ⏳ Queued  |
| C6    | Agent self-test harness: validate `AGENTS.md` schema and sample runs                            | ⏳ Queued  |

*Agents*: `hydrate_docs`, `validate_metadata`

#### Batch Details
- **D1**
  - Security docs (`SECURITY_POLICY.md`, `THREAT_MODEL.md`)
  - Dependencies: threat modeling tasks
  - Build: update docs under `docs/security/`
  - Demos: N/A
- **D2**
  - Build plan document (`BUILD_PLAN.md`)
  - Dependencies: toolchain setup
  - Build: finalize Docker and cross-compile notes
  - Demos: N/A
- **D3**
  - Maintain CHANGELOG entries and consistency checks
  - Dependencies: D2
  - Build: update `CHANGELOG.md`
  - Demos: N/A
- **D4**
  - README, CONTRIBUTING, and Code of Conduct updates
  - Dependencies: D3
  - Build: review docs for public release
  - Demos: N/A

#### Batch Details
- **C1**
  - Stub specifications and dependency pinning
  - Dependencies: tooling batches
  - Build: update `DEPENDENCIES.md`
  - Demos: N/A
- **C2**
  - CI smoke tests and `README_Codex.md`
  - Dependencies: C1
  - Build: `scripts/run-smoke-tests.sh`
  - Demos: N/A
- **C3**
  - API adapter & driver integration for Codex agents
  - Dependencies: C2
  - Build: `cargo build -p codex_adapter`
  - Demos: N/A
- **C4**
  - Agent instructions, example tasks, initial tests
  - Dependencies: C3
  - Build: add tests under `tests/codex/`
  - Demos: N/A
- **C5**
  - Logging & audit trails for Codex outputs
  - Dependencies: C4
  - Build: setup `codex_logs/`
  - Demos: N/A
- **C6**
  - Agent self-test harness validating `AGENTS.md`
  - Dependencies: C5
  - Build: run `validate_metadata_sync.py`
  - Demos: N/A

### 5 · Testing & QA Batches
End-to-end testing infrastructure to ensure platform quality.

| Batch | Deliverables                                                                                   | Status    |
|-------|-----------------------------------------------------------------------------------------------|-----------|
| X1    | Root `tests/` directory with subfolders and dummy tests for compiler, passes, OS, Codex         | 🟢 Hydrated |
| X2    | Security & OWASP compliance tests: threat model, dependency scanning, container scanning        | ⏳ Queued  |

*Agents*: `run_pass`

#### Batch Details
- **X1**
  - Root `tests/` directory with example tests
  - Dependencies: compiler & OS batches
  - Build: `cargo test`
  - Demos: N/A
- **X2**
  - Security & OWASP compliance tests
  - Dependencies: X1
  - Build: container scanning scripts
  - Demos: N/A

### 6 · Documentation & Auxiliary Batches (Docs D1–D4)
Ensure project documentation and support files are complete and maintained.

| Batch | Deliverables                                                                                   | Status    |
|-------|-----------------------------------------------------------------------------------------------|-----------|
| D1    | Security docs (`docs/security/SECURITY_POLICY.md`, `THREAT_MODEL.md`)                          | 🟢 Hydrated |
| D2    | Build plan doc (`BUILD_PLAN.md`)                                                               | 🟢 Hydrated |
| D3    | CHANGELOG.md entry maintenance and consistency check                                          | 🟢 Hydrated |
| D4    | README.md, CONTRIBUTING.md, CODE_OF_CONDUCT.md                         | 🟢 Hydrated |

*Agents*: `hydrate_docs`, `validate_metadata`

### 7 · Demo Integration Batches (Demo D1–D6)
End-user demo pipelines and helper services for showcasing Cohesix features.

| Batch | Deliverables                                             | Status    |
|-------|----------------------------------------------------------|-----------|
| D1    | Webcam capture + gesture service for Workers (demos 1–3) | 🟢 Hydrated |
| D2    | QR-based SLM loader and app swap (demo 6)                | ⏳ Queued |
| D3    | NAT rendezvous service for auto-attach (demo 10)         | ⏳ Queued |
| D4    | Trace replay and fairness harness (demos 4 & 8)          | ⏳ Queued |
| D5    | KioskInteractive UI provisioning pipeline (demo 5)       | ⏳ Queued |
| D6    | CLI scenario orchestration & Codex trigger helpers (demo 9) | ⏳ Queued |

---

#### Batch Details
- **D1**
  - Webcam capture and gesture service for Worker demos 1–3
  - Dependencies: camera drivers
  - Build: `cargo build -p webcam_demo`
  - Reference demo: End User Demo #1
- **D2**
  - QR-based SLM loader and app swap for demo 6
  - Dependencies: `D1`
  - Build: `cargo build -p qr_loader`
  - Reference demo: End User Demo #6
- **D3**
  - NAT rendezvous service for auto-attach (demo 10)
  - Dependencies: networking stack
  - Build: `go build cmd/rendezvous`
  - Reference demo: End User Demo #10
- **D4**
  - Trace replay and fairness harness (demos 4 & 8)
  - Dependencies: logging tools
  - Build: `cargo build -p trace_replay`
  - Reference demos: 4 & 8
- **D5**
  - KioskInteractive UI provisioning pipeline (demo 5)
  - Dependencies: `D2`
  - Build: `go build cmd/kiosk_ui`
  - Reference demo: End User Demo #5
- **D6**
  - CLI scenario orchestration & Codex trigger helpers (demo 9)
  - Dependencies: Codex enablement batches
  - Build: `cargo build -p demo_cli`
  - Reference demo: End User Demo #9

*Statuses*: ✅ = Complete, ⏳ = Queued, ⏸️ = Deferred, 🚧 = Blocked, 🗑️ = Deprecated, 🟢 = Hydrated

## Next Steps (BATCH_PLAN)
1. [1] Incorporate COMMERCIAL_PLAN v1.4 updates across marketing materials.
2. [2] Publish TECHNICAL_RISK.md and link from THREAT_MODEL.md.
3. [3] Align METADATA.md versions and rerun validate_metadata_sync.py.
4. [4] Update CHANGELOG.md with new document versions.
5. [5] Schedule matrix CI run to confirm cross‑arch builds after doc updates.
