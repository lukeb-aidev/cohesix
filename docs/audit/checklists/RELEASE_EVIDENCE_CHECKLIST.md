<!-- Author: Lukas Bower -->
<!-- Purpose: Checklist for release evidence completeness, independence verification, and final due-diligence decision. -->
<!-- Copyright 2026 Lukas Bower -->

# Release Evidence Checklist

## Run Metadata
- Audit window: `2026-02-13T22:24:03Z` -> `2026-02-13T22:33:24Z`
- Baseline commit SHA: `b89a7cf333aa3bac70dde338817a718fdacdc0fc`
- Auditor: `automation-agent`
- Independent reviewer: `TBD`
- Gate evidence root: `out/audit/gate/20260213T222403Z`

## Required Evidence Pack
- [x] `scripts/ci/due_diligence_gate.sh` output captured.
- [x] `scripts/check-generated.sh` output captured and passing.
- [x] `scripts/ci/check_test_plan.sh` output captured and reviewed.
- [x] `scripts/cohsh/run_regression_batch.sh` output captured and reviewed.
- [x] `docs/audit/findings.csv` updated with current dispositions, owners, and closure evidence fields.
- [x] `docs/audit/BLOCKERS.md` updated with current open `P0/P1` blockers.
- [x] `docs/audit/CONTROL_TRACEABILITY.md` updated with control-to-evidence links for this run.
- [x] `docs/audit/EXCEPTIONS.md` reviewed; no expired active exceptions.
- [x] `docs/audit/checklists/ARCHITECTURE_CHECKLIST.md` completed and signed.
- [x] `docs/audit/checklists/SECURITY_CHECKLIST.md` completed and signed.
- [x] `docs/audit/AUDIT_REPORT_2026-02-13.md` produced for this run.

## Decision Guardrails
- [ ] No open `P0/P1` findings in `docs/audit/findings.csv`.
- [x] Any `P2` accepted risks have owner and expiration in `docs/audit/EXCEPTIONS.md`.
- [x] All closures include reproducible command/log evidence and commit SHA.
- [ ] Independent reviewer verified all `P0/P1` remediations.

## Release Decision
- Decision: `FAIL`
- Decision date: `2026-02-13`
- Decision authority: `Due-diligence automation run (pending human review authority)`
- Residual risk summary: `8 open P1 findings remain (auth defaults, auth log leakage, gateway request authz, and sel4-sys cfg-gating risk).`
- Follow-up deadlines: `2026-02-20` for open P1 remediation; independent verification required before release.

## Prior Snapshot (2026-02-12 Baseline)
- Decision: `FAIL`
- Decision date: `2026-02-12`
- Decision owner: `Audit run pending human sign-off`
- Residual risks: `Not accepted (blocking P1 findings open)`
