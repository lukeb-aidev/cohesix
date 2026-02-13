<!-- Author: Lukas Bower -->
<!-- Purpose: Checklist for release evidence completeness, independence verification, and final due-diligence decision. -->
<!-- Copyright 2026 Lukas Bower -->

# Release Evidence Checklist

## Run Metadata
- Audit window:
- Baseline commit SHA:
- Auditor:
- Independent reviewer:

## Required Evidence Pack
- [ ] `scripts/ci/due_diligence_gate.sh` output captured.
- [ ] `scripts/check-generated.sh` output captured and passing.
- [ ] `scripts/ci/check_test_plan.sh` output captured and reviewed.
- [ ] `scripts/cohsh/run_regression_batch.sh` output captured and reviewed.
- [ ] `docs/audit/findings.csv` updated with current dispositions, owners, and closure evidence fields.
- [ ] `docs/audit/BLOCKERS.md` updated with current open `P0/P1` blockers.
- [ ] `docs/audit/CONTROL_TRACEABILITY.md` updated with control-to-evidence links for this run.
- [ ] `docs/audit/EXCEPTIONS.md` reviewed; no expired active exceptions.
- [ ] `docs/audit/checklists/ARCHITECTURE_CHECKLIST.md` completed and signed.
- [ ] `docs/audit/checklists/SECURITY_CHECKLIST.md` completed and signed.
- [ ] `docs/audit/AUDIT_REPORT_<YYYY-MM-DD>.md` produced for this run.

## Decision Guardrails
- [ ] No open `P0/P1` findings in `docs/audit/findings.csv`.
- [ ] Any `P2` accepted risks have owner and expiration in `docs/audit/EXCEPTIONS.md`.
- [ ] All closures include reproducible command/log evidence and commit SHA.
- [ ] Independent reviewer verified all `P0/P1` remediations.

## Release Decision
- Decision: `PASS` | `PASS_WITH_RESIDUAL_RISK` | `FAIL`
- Decision date:
- Decision authority:
- Residual risk summary:
- Follow-up deadlines:

## Current Snapshot (2026-02-12 Baseline)
- Decision: `FAIL`
- Decision date: `2026-02-12`
- Decision owner: `Audit run pending human sign-off`
- Residual risks: `Not accepted (blocking P1 findings open)`
