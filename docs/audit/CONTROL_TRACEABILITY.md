<!-- Author: Lukas Bower -->
<!-- Purpose: Maintain control-to-evidence traceability for each due-diligence run and release decision. -->
<!-- Copyright 2026 Lukas Bower -->

# Control-to-Evidence Traceability Register

## Run Metadata
- Audit run date: `2026-02-12`
- Registry maintenance date: `2026-02-13`
- Baseline commit: `8ca0e264955e015bbc3483a5ba6e43bd2b393834`
- Auditor: `TBD`
- Independent reviewer: `TBD`

## Update Rules
- Every required claim in `docs/audit/DUE_DILIGENCE_PLAN.md` sections 5 and 9 maps to at least one evidence source.
- Evidence must be reproducible and include command/log path plus commit SHA.
- Missing evidence is recorded as `GAP` and must produce or reference a finding in `docs/audit/findings.csv`.
- A `CLOSED_VERIFIED` finding requires an independent reviewer reference in this register or in linked evidence.

## Register
| Domain | Control ID | Requirement | Evidence Source | Evidence Path | Commit SHA | Status | Notes |
|---|---|---|---|---|---|---|---|
| Security | AC-3 / IA-2 | AuthN/AuthZ enforced on control-plane entry points | Code + tests + audit findings | `docs/SECURE9P.md`, `docs/USERLAND_AND_CLI.md`, `docs/audit/findings.csv` | `8ca0e264955e015bbc3483a5ba6e43bd2b393834` | `PARTIAL` | Open auth findings `DD-2026-0001`, `DD-2026-0002`, `DD-2026-0010`. |
| Security | AU-2 / AU-3 | Security events are logged without sensitive leakage | Code + findings | `docs/SECURITY.md`, `docs/audit/findings.csv` | `8ca0e264955e015bbc3483a5ba6e43bd2b393834` | `PARTIAL` | `DD-2026-0003` remains open. |
| Architecture | CM-7 | No unauthorized in-VM services beyond approved console exception | Charter + architecture review | `AGENTS.md`, `docs/audit/checklists/ARCHITECTURE_CHECKLIST.md` | `8ca0e264955e015bbc3483a5ba6e43bd2b393834` | `PARTIAL` | Requires current-run checklist sign-off. |
| Architecture | Project Charter Rule 6 | Capability discipline preserved | Checklist + regression fixtures | `docs/audit/checklists/ARCHITECTURE_CHECKLIST.md`, `tests/fixtures/` | `8ca0e264955e015bbc3483a5ba6e43bd2b393834` | `PARTIAL` | Re-validation required per release candidate. |
| Architecture | Project Charter HAL | HAL-only device access boundary preserved | Code review checklist | `docs/audit/checklists/ARCHITECTURE_CHECKLIST.md` | `8ca0e264955e015bbc3483a5ba6e43bd2b393834` | `PARTIAL` | Pending explicit reviewer sign-off. |
| Determinism | CM-2 | Generated artifacts and docs snippets are reproducible | Scripted generated check | `scripts/check-generated.sh`, `docs/audit/findings.csv` | `8ca0e264955e015bbc3483a5ba6e43bd2b393834` | `GAP` | `DD-2026-0006` open. |
| Correctness | SI-2 | Defect remediation is tracked and independently verified | Findings + blockers + release checklist | `docs/audit/findings.csv`, `docs/audit/BLOCKERS.md`, `docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md` | `8ca0e264955e015bbc3483a5ba6e43bd2b393834` | `PARTIAL` | Disposition schema updated; independent closure still pending. |
| Test Assurance | Project Charter Regression Pack | Regression pack re-run unchanged for behavior stability | Regression batch + findings | `scripts/cohsh/run_regression_batch.sh`, `docs/audit/findings.csv` | `8ca0e264955e015bbc3483a5ba6e43bd2b393834` | `PARTIAL` | `DD-2026-0008` moved to `PENDING_VERIFY`; full rerun evidence still required. |
| Supply Chain | SSDF / SCRM | Dependency and artifact risk checks executed | Planned tooling + evidence checklist | `docs/audit/DUE_DILIGENCE_PLAN.md`, `docs/audit/checklists/SECURITY_CHECKLIST.md` | `8ca0e264955e015bbc3483a5ba6e43bd2b393834` | `GAP` | Add `cargo-audit`, `cargo-deny`, SBOM and vulnerability scan evidence for current run. |
| Governance | RMF Authorize/Monitor | Release decision includes residual risk ownership and expirations | Release checklist + exceptions register | `docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md`, `docs/audit/EXCEPTIONS.md` | `8ca0e264955e015bbc3483a5ba6e43bd2b393834` | `PARTIAL` | Checklist updated; no active accepted risks recorded yet. |
