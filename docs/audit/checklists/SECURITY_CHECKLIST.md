<!-- Author: Lukas Bower -->
<!-- Purpose: Checklist for security assurance, supply-chain checks, and evidence capture during due diligence. -->
<!-- Copyright 2026 Lukas Bower -->

# Security Checklist

## Run Metadata
- Audit date: `2026-02-14`
- Commit SHA: `22cd5017d060c3439b6f7fc4f70717f329134803`
- Auditor: `automation-agent`
- Independent reviewer: `TBD`
- Evidence root: `out/audit/gate/20260214T044955Z`

## Security Checks
- [x] No hardcoded secrets in audited production codepaths (`apps/root-task/src`, `apps/hive-gateway/src`, `apps/coh/src`, `apps/cohsh/src`).
- [x] Auth/session establishment does not log sensitive material in reject paths.
- [x] Ticket/role validation enforced before sensitive operations.
- [x] User input handling rejects malformed and out-of-bounds values.
- [x] Negative-path tests exist for auth failures and malformed requests.
- [x] Policy gate and action queue semantics are enforced and auditable.
- [x] Release-facing auth defaults in audited components require explicit configuration.
- [ ] Dependency vulnerability scans completed (`cargo-audit`, `cargo-deny`, or equivalent) and evidence captured.
- [x] Secret scan completed (`hardcoded-secret-scan`) and evidence captured.
- [ ] SBOM generated (`syft` or equivalent) and vulnerability scan results recorded.
- [x] Exceptions reviewed in `docs/audit/EXCEPTIONS.md`; no expired active exceptions.

## Evidence References
- Security evidence paths:
  - `out/audit/gate/20260214T044955Z/hardcoded-secret-scan.log`
  - `out/audit/gate/20260214T044955Z/release-guardrails-findings.log`
  - `out/audit/gate/20260214T044955Z/release-guardrails-exceptions.log`
  - `out/audit/gate/20260214T044955Z/workspace-tests.log`
  - `out/audit/gate/20260214T044955Z/regression-batch.log`
- Command logs:
  - `scripts/ci/due_diligence_gate.sh`
  - `scripts/cohsh/run_regression_batch.sh`
- Related finding IDs:
  - `DD-2026-0001`, `DD-2026-0002`, `DD-2026-0003`, `DD-2026-0010`, `DD-2026-0013`, `DD-2026-0014`, `DD-2026-0015`

## Sign-off
- Auditor decision: `PASS`
- Independent reviewer decision: `TBD`
- Decision date: `2026-02-14`
- Notes: `Security blockers in the due-diligence register are closed; supply-chain and SBOM checks remain recommended additive controls outside the baseline gate.`
