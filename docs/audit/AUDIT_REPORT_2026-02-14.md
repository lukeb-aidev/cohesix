<!-- Author: Lukas Bower -->
<!-- Purpose: Record due-diligence execution evidence, findings closure, and release decision for the 2026-02-14 run. -->
<!-- Copyright 2026 Lukas Bower -->

# Cohesix Audit Report (2026-02-14)

## Report Status
- This report records a full rerun of `docs/audit/DUE_DILIGENCE_PLAN.md` baseline checks.
- All required baseline commands were executed via `scripts/ci/due_diligence_gate.sh`.

## Baseline
- Repo: `/Users/lukasbower/GitHub/cohesix`
- Branch: `main`
- Commit at audit start: `22cd5017d060c3439b6f7fc4f70717f329134803`
- Host: macOS ARM64
- Gate evidence root: `/Users/lukasbower/GitHub/cohesix/out/audit/gate/20260214T044955Z`

## Scope Executed
### Baseline commands from the due-diligence plan
- `scripts/ci/due_diligence_gate.sh` (PASS)
- `scripts/check-generated.sh` (PASS; executed inside gate)
- `scripts/ci/check_test_plan.sh` (PASS; executed inside gate)
- `scripts/cohsh/run_regression_batch.sh` (PASS; executed inside gate)

### Gate step outcomes
- `required-audit-assets`: PASS
- `cargo check --workspace`: PASS
- `cargo test -p secure9p-codec`: PASS
- `cargo test -p tests`: PASS
- `cargo test --workspace`: PASS
- `scripts/check-generated.sh`: PASS
- `scripts/ci/check_test_plan.sh`: PASS
- `scripts/cohsh/run_regression_batch.sh`: PASS
- `release-guardrails-findings`: PASS
- `release-guardrails-exceptions`: PASS
- `hardcoded-secret-scan`: PASS

## Findings Outcome
- Total findings: `15`
- Open findings: `0`
- Closed in this run:
  - `DD-2026-0001`, `DD-2026-0002`, `DD-2026-0003`, `DD-2026-0007`, `DD-2026-0009`, `DD-2026-0010`, `DD-2026-0013`, `DD-2026-0014`, `DD-2026-0015`
- Previously closed and still passing: `DD-2026-0004`, `DD-2026-0005`, `DD-2026-0006`, `DD-2026-0008`, `DD-2026-0011`, `DD-2026-0012`

## Decision and Rationale
- Decision: `PASS`
- Decision date: `2026-02-14`
- Rationale:
  - Due-diligence baseline completed with all required checks passing.
  - No open `P0/P1` findings remain.
  - Hardcoded secret scan and regression packs pass on current run.

## Evidence Index
- Gate orchestrator log root: `out/audit/gate/20260214T044955Z`
- Workspace test evidence: `out/audit/gate/20260214T044955Z/workspace-tests.log`
- Regression evidence: `out/audit/gate/20260214T044955Z/regression-batch.log`
- Generated artifact drift evidence: `out/audit/gate/20260214T044955Z/generated-artifacts.log`
- Test-plan integrity evidence: `out/audit/gate/20260214T044955Z/test-plan-hash-check.log`
- Findings guardrail evidence: `out/audit/gate/20260214T044955Z/release-guardrails-findings.log`
- Exceptions guardrail evidence: `out/audit/gate/20260214T044955Z/release-guardrails-exceptions.log`
- Secret-scan evidence: `out/audit/gate/20260214T044955Z/hardcoded-secret-scan.log`

## Residual Items
- Independent reviewer sign-off for newly closed findings remains tracked as `independent-review-pending` in `docs/audit/findings.csv`.

## Audit Completion Statement
This run completed the required automated due-diligence process and closed all previously open findings in the register with updated evidence paths.
