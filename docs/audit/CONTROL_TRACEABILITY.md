<!-- Author: Lukas Bower -->
<!-- Purpose: Maintain control-to-evidence traceability for each due-diligence run and release decision. -->
<!-- Copyright 2026 Lukas Bower -->

# Control-to-Evidence Traceability Register

## Run Metadata
- Audit run date: `2026-02-14`
- Registry maintenance date: `2026-02-14`
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
