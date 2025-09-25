// CLASSIFICATION: COMMUNITY
// Filename: AGENTS.md v4.0
// Author: Lukas Bower
// Date Modified: 2029-09-21

# Cohesix Codex Agent Charter

This charter aligns autonomous development agents with the Cohesix Solution Architecture and SAFe backlog, ensuring every automation path preserves the secure seL4 + Plan 9 platform mission, architectural principles, and delivery cadence.

---

## 1. Mission Hooks

- **Architecture Alignment**: Agents must uphold the Solution Architecture outcomes—sub-200 ms boot, Secure9P governance, deterministic physics/GPU orchestration, and 100% traceability—from preliminary capability to ADM Phase H change management.
- **Backlog Traceability**: Every automated action maps to SAFe epics, features, or enabler stories. Agents log the epic/feature ID they serve (e.g., `E1-F7 Trace Path Normalizer`).
- **Metadata Discipline**: Enforce repository-wide metadata headers (`Author`, `Classification`, dates) and reject artefacts without compliant headers or with stale values.
- **No Binary Artefacts**: Pull requests introducing binaries, large media, or generated blobs outside approved manifests must fail.

---

## 2. Architectural Pillars & Guardrails

| Pillar | Architecture Reference | Agent Expectations |
|--------|------------------------|--------------------|
| **Proof-Preserving Modularity** | ADM Phases A–D; Solution Architecture §4 | Block merges that break seL4 proofs, namespace bindings, or Plan-9 semantics. Require `cargo build --target cohesix_aarch64.json -C linker=lld` success and ELF validation (`readelf`, `nm`) for rootserver outputs. |
| **Traceability First** | Solution Architecture §§3, 6, 10; Backlog E3 | Validate trace hooks, consensus regression artefacts, and ensure privileged actions emit entries under `/log/trace/`. CI jobs must fail if traces fall outside governed paths. |
| **Governed Extension** | Solution Architecture §§2, 6.2, 7; Backlog E1/E2 | Ensure Secure9P manifests, capability policies, and GPU annex telemetry follow mutual TLS, capability tokens, and health heartbeat requirements. Reject unauthenticated namespace operations. |
| **Zero-Trust Operations** | Solution Architecture §§2, 7; Role Policy | Verify authentication (mTLS, auth middleware) remains enabled, rate limits configured, and validator hooks intact for all roles (QueenPrimary, DroneWorker, KioskInteractive, etc.). |
| **Documentation as Code** | Solution Architecture §§1, 10, 13; Backlog F19 | Require doc/test updates with code changes, enforce metadata headers, and sync diagnostics with implementation updates. |

---

## 3. SAFe Delivery Integration

- **Epic Enforcement**: Agents monitor portfolio epics E1–E7. Backlog slippage (missing acceptance criteria, absent telemetry, or unimplemented guardrails) must surface as CI failures with WSJF context in logs.
- **Program Increment Checks**: For each PI, agents confirm committed objectives (e.g., PI-2029.3 boot telemetry, GUI gRPC parity, metadata validation) are gated by automated tests before release.
- **Definition of Ready/Done**: Stories without documented trace IDs, dependency mitigation, or security review are not actionable; agents should block merges lacking these attributes or references.

---

## 4. Agent Task Blueprint

Each agent defines:

- **Task Title & ID** (e.g., `AGENT:TRACE_DIFF_PIPELINE`).
- **Aligned Epic/Feature** (`E3-F9`).
- **Goal** tied to architecture principle and PI objective.
- **Inputs** (directories, configuration manifests, telemetry logs).
- **Outputs** (markdown or JSON logs stored under `$COHESIX_TRACE_TMP` or `$TMPDIR`).
- **Checks** with explicit pass/fail rules.
- **Evidence Hooks** linking to validator traces, consensus snapshots, or diagnostics for auditability.

Agents must never write to `/tmp` or `/dev/shm`; they respect `TMPDIR`, `COHESIX_TRACE_TMP`, or `COHESIX_ENS_TMP`. All logs include timestamp, epic/feature tag, and trace ID when available.

---

## 5. Canonical Tasks (Illustrative)

1. **Kernel & Namespace Trace Hooks** (`AGENT:KERNEL_TRACE`, E3-F8)
   - Goal: Assert validator hook presence across boot stages, fail if any privileged path lacks trace emissions.
   - Checks: `readelf`/`nm` verification, trace hook diff coverage, PI objective alignment.

2. **Secure9P Policy Validation** (`AGENT:SECURE9P_POLICY`, E1-F1/F6)
   - Goal: Validate capability manifests, mutual TLS configuration, and heartbeat metrics.
   - Checks: Schema linting, certificate expiry windows, heartbeat telemetry under `/log/trace/net_secure9p/`.

3. **GUI Control Plane Integrity** (`AGENT:GUI_GPRC_PARITY`, E4-F10/F11)
   - Goal: Ensure GUI commands mirror gRPC APIs with auth enabled and metrics exported.
   - Checks: Integration tests, rate-limit telemetry, trace IDs on `/api/control` calls.

4. **Boot Performance Instrumentation** (`AGENT:BOOT_TELEMETRY`, E5-F13/F15)
   - Goal: Confirm sub-200 ms boot, instrumentation thresholds, and ELF validation artefacts.
   - Checks: QEMU boot timers, `/log/boot/elf_checks/` signatures, watchdog trace coverage.

5. **Metadata Governance** (`AGENT:METADATA_LINT`, E7-F19)
   - Goal: Guarantee metadata headers across code, docs, and assets; block missing or stale entries.
   - Checks: Header presence (`Author: Lukas Bower` or role-specific values), classification consistency, last-modified dates.

---

## 6. Execution & Environment Guidance

- CI runs on x86_64 (GitHub Actions) with optional aarch64 emulation. CUDA workloads execute only on dedicated Linux annex runners—never within Plan 9 roles.
- Enforce `cargo`, `pytest`, `go test`, `mypy`, and fuzzing gates relevant to modified components; no disabling of existing tests.
- QEMU executions use `-M virt -cpu cortex-a57 -m 1024` with elfloader CPIO images and console on `-serial mon:stdio`.
- Long-lived processes, absolute host paths, or stateful dependencies are forbidden.
- Agents must surface remediation guidance referencing Solution Architecture and Backlog sections when failing.

---

## 7. Audit & Reporting

- Logs are versioned artefacts stored under trace directories and attached to release notes when applicable.
- Every failure includes: architecture principle impacted, epic/feature, PI objective, remediation steps, and related documentation path.
- Success criteria aggregate into architecture maturity dashboards and SAFe Inspect & Adapt metrics.

---

## 8. Outcome Statement

When these directives are enforced, Cohesix maintains a secure, traceable, and governable platform: cold boots remain sub-200 ms, Secure9P policies are verifiable, GPU annex orchestration is deterministic, and every backlog commitment is evidenced through automated checks. This keeps the architecture, delivery pipeline, and compliance posture in lockstep.
