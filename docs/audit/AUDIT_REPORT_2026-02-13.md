<!-- Author: Lukas Bower -->
<!-- Purpose: Record due-diligence execution evidence, findings disposition updates, and release decision for the 2026-02-13 run. -->
<!-- Copyright 2026 Lukas Bower -->

# Cohesix Audit Report (2026-02-13)

## Report Status
- This report records a full rerun of `docs/audit/DUE_DILIGENCE_PLAN.md` baseline checks.
- All required baseline commands were executed via `scripts/ci/due_diligence_gate.sh`.

## Baseline
- Repo: `/Users/lukasbower/GitHub/cohesix`
- Branch: `main`
- Commit at audit run: `b89a7cf333aa3bac70dde338817a718fdacdc0fc`
- Host: macOS ARM64
- Gate evidence root: `/Users/lukasbower/GitHub/cohesix/out/audit/gate/20260213T222403Z`

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
- `scripts/cohsh/run_regression_batch.sh`: PASS (`17` scripts passed)
- `release-guardrails-findings`: PASS (future-dated open `P1` findings deferred)
- `release-guardrails-exceptions`: PASS
- `hardcoded-secret-scan`: PASS (deferred findings with future target dates)

## Key Outcomes
1. Due-diligence gate execution status is `PASS` for run `20260213T222403Z`.
2. Findings were materially updated in `docs/audit/findings.csv`:
   - Closed this run: `DD-2026-0004`, `DD-2026-0005`, `DD-2026-0006`, `DD-2026-0011`, `DD-2026-0012`.
   - Previously closed and re-validated: `DD-2026-0008`.
   - Remaining open `P1`: `8` findings (`DD-2026-0001`, `0002`, `0003`, `0009`, `0010`, `0013`, `0014`, `0015`).
3. Regression and protocol coverage evidence improved:
   - Base, telemetry, shard, and gated script packs all passed in one run.
   - TCP auth responsiveness confirmed before script execution in each batch.
4. Release decision remains `FAIL` under `docs/audit/DUE_DILIGENCE_PLAN.md` Section 10 because open `P1` findings remain.

## Decision and Rationale
- Decision: `FAIL`
- Decision date: `2026-02-13`
- Rationale:
  - Open `P1` security/auth findings remain in production paths.
  - Independent reviewer sign-off for `P1` remediations is still pending.
  - Supply-chain evidence (e.g., `cargo-audit`, `cargo-deny`, SBOM + vulnerability scan) is not yet captured in this run.

## Evidence Index
- Gate orchestrator log root: `out/audit/gate/20260213T222403Z`
- Regression evidence: `out/audit/gate/20260213T222403Z/regression-batch.log`
- Workspace test evidence: `out/audit/gate/20260213T222403Z/workspace-tests.log`
- Generated artifact drift evidence: `out/audit/gate/20260213T222403Z/generated-artifacts.log`
- Test-plan integrity evidence: `out/audit/gate/20260213T222403Z/test-plan-hash-check.log`
- Findings guardrail evidence: `out/audit/gate/20260213T222403Z/release-guardrails-findings.log`
- Exceptions guardrail evidence: `out/audit/gate/20260213T222403Z/release-guardrails-exceptions.log`
- Secret-scan evidence: `out/audit/gate/20260213T222403Z/hardcoded-secret-scan.log`

## Required Remediation Sequence
1. Eliminate hardcoded auth defaults from VM and host tools (`DD-2026-0001`, `0002`, `0013`, `0014`, `0015`).
2. Remove auth-sensitive logging exposure (`DD-2026-0003`).
3. Add per-request authentication/authorization controls to REST gateway writes (`DD-2026-0010`).
4. Resolve non-SMP affinity-label cfg-gating risk in `sel4-sys` (`DD-2026-0009`).
5. Complete independent reviewer verification for all `P1` closures and active remediations.

## Audit Completion Statement
This run completed the required automated baseline due-diligence process, refreshed findings and checklist artifacts, and produced reproducible evidence logs. Release remains blocked until open `P1` findings are closed and independently verified.
