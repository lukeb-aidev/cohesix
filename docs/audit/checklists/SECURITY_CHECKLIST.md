<!-- Author: Lukas Bower -->
<!-- Purpose: Checklist for security assurance, supply-chain checks, and evidence capture during due diligence. -->
<!-- Copyright 2026 Lukas Bower -->

# Security Checklist

## Run Metadata
- Audit date: `2026-02-13`
- Commit SHA: `b89a7cf333aa3bac70dde338817a718fdacdc0fc`
- Auditor: `automation-agent`
- Independent reviewer: `TBD`
- Evidence root: `out/audit/gate/20260213T222403Z`

## Security Checks
- [ ] No hardcoded secrets in production codepaths.
- [ ] Auth/session establishment does not log sensitive material.
- [ ] Ticket/role validation enforced before sensitive operations.
- [x] User input handling rejects malformed and out-of-bounds values.
- [ ] Negative-path tests exist for auth failures and malformed requests.
- [x] Policy gate and action queue semantics are enforced and auditable.
- [ ] Release artifacts contain no secret defaults or debug bypass toggles.
- [ ] Dependency vulnerability scans completed (`cargo-audit`, `cargo-deny`, or equivalent) and evidence captured.
- [x] Secret scan completed (`gitleaks` or equivalent) and evidence captured.
- [ ] SBOM generated (`syft` or equivalent) and vulnerability scan results recorded.
- [x] Exceptions reviewed in `docs/audit/EXCEPTIONS.md`; no expired active exceptions.

## Evidence References
- Security evidence paths:
  - `out/audit/gate/20260213T222403Z/hardcoded-secret-scan.log`
  - `out/audit/gate/20260213T222403Z/release-guardrails-findings.log`
  - `out/audit/gate/20260213T222403Z/release-guardrails-exceptions.log`
  - `out/audit/gate/20260213T222403Z/regression-batch.log`
- Command logs:
  - `scripts/ci/due_diligence_gate.sh`
  - `scripts/cohsh/run_regression_batch.sh`
- Related finding IDs:
  - `DD-2026-0001`, `DD-2026-0002`, `DD-2026-0003`, `DD-2026-0010`, `DD-2026-0013`, `DD-2026-0014`, `DD-2026-0015`

## Sign-off
- Auditor decision: `FAIL`
- Independent reviewer decision: `TBD`
- Decision date: `2026-02-13`
- Notes: `Security gate execution passed with deferred future-dated P1 findings, but release security criteria remain unmet until open auth/secret findings are remediated and independently verified.`
