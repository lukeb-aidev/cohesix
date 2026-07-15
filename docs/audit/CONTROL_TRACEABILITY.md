<!-- Author: Lukas Bower -->
<!-- Purpose: Maintain control-to-evidence traceability for each due-diligence run and release decision. -->
<!-- Copyright 2026 Lukas Bower -->

# Control-to-Evidence Traceability Register

## Run Metadata
- Audit run date: `2026-02-14`
- Registry maintenance date: `2026-07-16`
- Baseline commit: `22cd5017d060c3439b6f7fc4f70717f329134803`
- Gate evidence root: `out/audit/gate/20260214T044955Z`
- Auditor: `automation-agent`
- Independent reviewer: `TBD`

## Update Rules
- Every required claim in `docs/audit/DUE_DILIGENCE_PLAN.md` sections 5 and 9 maps to at least one evidence source.
- Evidence must be reproducible and include command/log path plus commit SHA.
- Missing evidence is recorded as `GAP` and must produce or reference a finding in `docs/audit/findings.csv`.
- A `CLOSED_VERIFIED` finding requires an independent reviewer reference in this register or in linked evidence.

## Register
| Domain | Control ID | Requirement | Evidence Source | Evidence Path | Commit SHA | Status | Notes |
|---|---|---|---|---|---|---|---|
| Security | AC-3 / IA-2 | AuthN/AuthZ enforced on control-plane entry points | Due-diligence gate + findings | `out/audit/gate/20260214T044955Z/hardcoded-secret-scan.log`, `out/audit/gate/20260214T044955Z/workspace-tests.log`, `docs/audit/findings.csv` | `22cd5017d060c3439b6f7fc4f70717f329134803` | `PASS` | Findings `DD-2026-0001`, `0002`, `0010`, `0013`, `0014`, `0015` closed in this run. |
| Security | AU-2 / AU-3 | Security events are logged without sensitive leakage | Findings register + workspace tests | `docs/audit/findings.csv`, `out/audit/gate/20260214T044955Z/workspace-tests.log` | `22cd5017d060c3439b6f7fc4f70717f329134803` | `PASS` | `DD-2026-0003` closed; token-adjacent reject logging removed. |
| Architecture | CM-7 | No unauthorized in-VM services beyond approved console exception | Regression run evidence + checklist | `out/audit/gate/20260214T044955Z/regression-batch.log`, `docs/audit/checklists/ARCHITECTURE_CHECKLIST.md` | `22cd5017d060c3439b6f7fc4f70717f329134803` | `PASS` | Regression and console-auth readiness checks pass. |
| Architecture | Project Charter Rule 6 | Capability discipline preserved | Regression fixtures + checklist | `out/audit/gate/20260214T044955Z/regression-batch.log`, `docs/audit/checklists/ARCHITECTURE_CHECKLIST.md` | `22cd5017d060c3439b6f7fc4f70717f329134803` | `PASS` | No protocol-contract drift detected in baseline gate. |
| Architecture | Project Charter HAL | HAL-only device access boundary preserved | Architecture checklist | `docs/audit/checklists/ARCHITECTURE_CHECKLIST.md` | `22cd5017d060c3439b6f7fc4f70717f329134803` | `PASS` | No HAL bypass introduced in this remediation set. |
| Determinism | CM-2 | Generated artifacts and docs snippets are reproducible | Generated artifact drift check | `out/audit/gate/20260214T044955Z/generated-artifacts.log` | `22cd5017d060c3439b6f7fc4f70717f329134803` | `PASS` | `scripts/check-generated.sh` passes for current tree. |
| Correctness | SI-2 | Defect remediation is tracked and independently verified | Findings + blockers + release checklist | `docs/audit/findings.csv`, `docs/audit/BLOCKERS.md`, `docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md` | `22cd5017d060c3439b6f7fc4f70717f329134803` | `PARTIAL` | Automated closure complete; independent reviewer verification still pending. |
| Test Assurance | Project Charter Regression Pack | Regression pack re-run unchanged for behavior stability | Regression batch | `out/audit/gate/20260214T044955Z/regression-batch.log` | `22cd5017d060c3439b6f7fc4f70717f329134803` | `PASS` | Base, telemetry, shard, and gated suites passed (`17 scripts passed`). |
| Supply Chain | SSDF / SCRM | Dependency and artifact risk checks executed | Security checklist + due-diligence plan | `docs/audit/checklists/SECURITY_CHECKLIST.md`, `docs/audit/DUE_DILIGENCE_PLAN.md` | `22cd5017d060c3439b6f7fc4f70717f329134803` | `GAP` | `cargo-audit`, `cargo-deny`, SBOM, and vulnerability scan evidence remain outside baseline gate scope. |
| Governance | RMF Authorize/Monitor | Release decision includes residual risk ownership and expirations | Release checklist + blockers + exceptions register | `docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md`, `docs/audit/BLOCKERS.md`, `docs/audit/EXCEPTIONS.md` | `22cd5017d060c3439b6f7fc4f70717f329134803` | `PASS` | Baseline decision is `PASS`; no accepted-risk records are active. |

## Milestone 26d P2 Closure Addendum (2026-07-16)

- Scope: `m26d-linked-runtime-unsafe-exception-remediation` under Milestone 26d repository gate closure and the reopened Milestone 26b WiFi SDIO DPC defect.
- Verified implementation commit: `68dd774d6ceb0706e162877f74766dd324572425`.
- Independent reviews: `p2_final_review` and `commit_scope_audit` both reported `PASS` before immutable-commit validation.

| Domain | Control ID | Requirement | Evidence Source | Evidence Path | Commit SHA | Status | Notes |
|---|---|---|---|---|---|---|---|
| Correctness | LLM-assisted Rust audit gate | Production risk indicators do not exceed exact global and component budgets | Scanner v4 current scan + exact historical replay | `out/test-plan/m26d-unsafe-remediation-qemu/logs/stage-05-due-diligence.log`, `docs/audit/rust_risk_baseline.toml` | `68dd774d6ceb0706e162877f74766dd324572425` | `PASS` | Current global `691/38/240/96`, linked-runtime/HAL `144/0/2/0`, outside-component `547/38/238/96`; exact `cf8f9ee30` replay `693/38/240/96`, `146/0/2/0`, `547/38/238/96`. |
| Correctness | SI-2 | P2 findings and exceptions have a one-to-one verified terminal lifecycle | Lifecycle tests + exception-register gate | `docs/audit/findings.csv`, `docs/audit/EXCEPTIONS.md`, `out/test-plan/m26d-unsafe-remediation-qemu/logs/stage-05-due-diligence.log` | `68dd774d6ceb0706e162877f74766dd324572425` | `PASS` | Lifecycle suite passed 19 tests; `DD-2026-0016` through `0018` are `CLOSED_VERIFIED` and matching exceptions are `CLOSED`. |
| Architecture | Project Charter HAL | Linked-runtime shared state, MMIO, DPC, and ring access remain bounded and HAL-admitted | Focused runtime/root/ABI tests + package build | `out/test-plan/m26d-unsafe-remediation-qemu/logs/stage-02-host-fast.log`, `out/cohesix/cohesix-system.cpio` | `68dd774d6ceb0706e162877f74766dd324572425` | `PASS` | Offline evidence: 466 runtime tests, 1425 Pi-feature root tests, 25 ABI tests, aarch64 check, and 2,542,080-byte CPIO passed. |
| Test Assurance | Project Charter Regression Pack | Canonical staged Test Plan passes on all hardware-independent surfaces | QEMU Stages 01-05 + Pi offline Stages 01-02 | `out/test-plan/m26d-unsafe-remediation-qemu`, `out/test-plan/m26d-unsafe-remediation-pi4` | `68dd774d6ceb0706e162877f74766dd324572425` | `PASS` | Console, REST, due-diligence, and Pi host-side gates passed in clean detached worktrees. |
| Governance | RMF Authorize/Monitor | No active P2 exception remains for the linked-runtime unsafe boundary | Findings + exceptions + release checklist | `docs/audit/findings.csv`, `docs/audit/EXCEPTIONS.md`, `docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md` | `68dd774d6ceb0706e162877f74766dd324572425` | `PASS` | EX16, EX17, and EX18 are closed after verified remediation. |
| Hardware Acceptance | Milestone 26b/26d Pi proof | Exact image connects reliably on every eligible Pi 4 boot | Fresh serial, boot-paired pcap, and repeated-boot ledger | `REBOOT.md` | `68dd774d6ceb0706e162877f74766dd324572425` | `NOT_CLAIMED` | Hardware proof is unavailable while the operator is travelling; offline closure does not establish live Pi or repeated-boot WiFi success. |
