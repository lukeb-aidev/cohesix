<!-- Author: Lukas Bower -->
<!-- Purpose: Maintain control-to-evidence traceability for each due-diligence run and release decision. -->
<!-- Copyright 2026 Lukas Bower -->

# Control-to-Evidence Traceability Register

## Run Metadata
- Audit run date: `2026-02-13`
- Registry maintenance date: `2026-02-13`
- Baseline commit: `b89a7cf333aa3bac70dde338817a718fdacdc0fc`
- Gate evidence root: `out/audit/gate/20260213T222403Z`
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
| Security | AC-3 / IA-2 | AuthN/AuthZ enforced on control-plane entry points | Due-diligence gate + findings | `out/audit/gate/20260213T222403Z/hardcoded-secret-scan.log`, `docs/audit/findings.csv` | `b89a7cf333aa3bac70dde338817a718fdacdc0fc` | `PARTIAL` | Open auth findings: `DD-2026-0001`, `DD-2026-0002`, `DD-2026-0010`, `DD-2026-0013`, `DD-2026-0014`, `DD-2026-0015`. |
| Security | AU-2 / AU-3 | Security events are logged without sensitive leakage | Findings register | `docs/audit/findings.csv` | `b89a7cf333aa3bac70dde338817a718fdacdc0fc` | `PARTIAL` | `DD-2026-0003` remains open. |
| Architecture | CM-7 | No unauthorized in-VM services beyond approved console exception | Regression run evidence + checklist | `out/audit/gate/20260213T222403Z/regression-batch.log`, `docs/audit/checklists/ARCHITECTURE_CHECKLIST.md` | `b89a7cf333aa3bac70dde338817a718fdacdc0fc` | `PARTIAL` | Console TCP listener verified reachable/auth-responsive; independent checklist sign-off pending. |
| Architecture | Project Charter Rule 6 | Capability discipline preserved | Regression fixtures + checklist | `out/audit/gate/20260213T222403Z/regression-batch.log`, `docs/audit/checklists/ARCHITECTURE_CHECKLIST.md` | `b89a7cf333aa3bac70dde338817a718fdacdc0fc` | `PARTIAL` | Regression scripts passed; formal independent review pending. |
| Architecture | Project Charter HAL | HAL-only device access boundary preserved | Architecture checklist | `docs/audit/checklists/ARCHITECTURE_CHECKLIST.md` | `b89a7cf333aa3bac70dde338817a718fdacdc0fc` | `PARTIAL` | Manual code review evidence remains required. |
| Determinism | CM-2 | Generated artifacts and docs snippets are reproducible | Generated artifact drift check | `out/audit/gate/20260213T222403Z/generated-artifacts.log` | `b89a7cf333aa3bac70dde338817a718fdacdc0fc` | `PASS` | `DD-2026-0006` closed with reproducible evidence. |
| Correctness | SI-2 | Defect remediation is tracked and independently verified | Findings + blockers + release checklist | `docs/audit/findings.csv`, `docs/audit/BLOCKERS.md`, `docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md` | `b89a7cf333aa3bac70dde338817a718fdacdc0fc` | `PARTIAL` | Remediation progressed (`DD-2026-0004`, `0005`, `0006`, `0011`, `0012` closed); independent reviewer still pending. |
| Test Assurance | Project Charter Regression Pack | Regression pack re-run unchanged for behavior stability | Regression batch | `out/audit/gate/20260213T222403Z/regression-batch.log` | `b89a7cf333aa3bac70dde338817a718fdacdc0fc` | `PASS` | Base, telemetry, shard, and gated suites passed (`17 scripts passed`). |
| Supply Chain | SSDF / SCRM | Dependency and artifact risk checks executed | Security checklist + due-diligence plan | `docs/audit/checklists/SECURITY_CHECKLIST.md`, `docs/audit/DUE_DILIGENCE_PLAN.md` | `b89a7cf333aa3bac70dde338817a718fdacdc0fc` | `GAP` | `cargo-audit`, `cargo-deny`, SBOM generation, and vuln scan evidence not captured in this run. |
| Governance | RMF Authorize/Monitor | Release decision includes residual risk ownership and expirations | Release checklist + blockers + exceptions register | `docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md`, `docs/audit/BLOCKERS.md`, `docs/audit/EXCEPTIONS.md` | `b89a7cf333aa3bac70dde338817a718fdacdc0fc` | `PARTIAL` | Release decision remains `FAIL` while open `P1` blockers persist. |
