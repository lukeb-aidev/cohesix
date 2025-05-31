// CLASSIFICATION: COMMUNITY
// Filename: RELEASE_CRITERIA.md v1.1
// Date Modified: 2025-05-25
// Author: Lukas Bower

# Release Criteria (v1.0-alpha)

To achieve a robust v1.0-alpha release, Cohesix must satisfy the following criteria across all development phases. Criteria are grouped by milestone:

## 1. Compiler Milestone
- **Batches 1–4: IR Core & Pass Framework**
  - All IR operations (Add, Sub, Mul, Div, Load, Store, Jump, Branch, Call, Ret, Nop) implemented.
  - Pass framework operational: passes register and execute without errors.
  - Unit tests covering IR and pass framework with ≥ 80% coverage.
- **Batches 5–11: Optimization Passes**
  - Dead code elimination, NOP removal, constant folding, SSA renaming passes implemented and validated.
  - Test harness verifies pass effects on sample IR modules.
- **Batches 12–17: Codegen & CLI Integration**
  - C and WASM backends generate compilable output from example IR.
  - `cohcc` CLI compiles a sample IR file into a working binary on aarch64 and x86_64.
  - Integration tests pass across both target architectures.
- **Batches 18–25: Dispatch & Example Modules**
  - Codegen dispatcher selects correct backend.
  - Example IR module stub produces expected output in end-to-end tests.

## 2. OS Runtime Milestone
- **Batches O1–O3: Boot & Security**
  - seL4 boots with CohRole initialization; `/srv/cohrole` accessible.
  - Plan 9 namespace mounted; `rc` shell functional.
  - Sandbox proofs preserved; validation scripts (`validate_metadata_sync.py`, proof checks) pass.
- **Batches O4–O7: Services & CI**
  - `/sim/` physics service runs sample simulation with Rapier core.
  - `/srv/cuda` GPU service runs Torch/TensorRT sample workload.
  - OS image builds reproducibly; CI smoke tests on hardware/emulator succeed.

## 3. Tooling Milestone
- **Batches T1–T3: Core Utilities**
  - BusyBox coreutils and shell operational.
  - SSH access via BusyBox `sshd`; manual pages rendered through `mandoc`.
- **Batches T4–T7: Extended Tooling**
  - Logging utilities (`last`, `finger`) capture session history.
  - Package manager stub installs and verifies a sample package.
  - Healthcheck and monitoring services expose metrics.
  - Distributed build tooling triggers and completes a remote build step.

## 4. AI/Codex Milestone
- **Batches C1–C2: Specification & Tests**
  - Agent definitions in `AGENTS.md` validated against schema.
  - CI codex smoke tests (`pytest tests/codex/`) pass.
- **Batches C3–C4: Integration & Workflows**
  - `cohcli codex run <agent_id>` generates valid code or doc stubs according to `output_schema`.
  - Audit logs in `codex_logs/` capture request, response, and metadata.
  - Human review gating enforced via branch protection and PR templates.

---

> **Pass Criteria:** All criteria must be successfully validated on both aarch64 (ARM) and x86_64 architectures.