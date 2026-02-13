<!-- Author: Lukas Bower -->
<!-- Purpose: Checklist for security assurance, supply-chain checks, and evidence capture during due diligence. -->
<!-- Copyright 2026 Lukas Bower -->

# Security Checklist

## Run Metadata
- Audit date:
- Commit SHA:
- Auditor:
- Independent reviewer:

## Security Checks
- [ ] No hardcoded secrets in production codepaths.
- [ ] Auth/session establishment does not log sensitive material.
- [ ] Ticket/role validation enforced before sensitive operations.
- [ ] User input handling rejects malformed and out-of-bounds values.
- [ ] Negative-path tests exist for auth failures and malformed requests.
- [ ] Policy gate and action queue semantics are enforced and auditable.
- [ ] Release artifacts contain no secret defaults or debug bypass toggles.
- [ ] Dependency vulnerability scans completed (`cargo-audit`, `cargo-deny`, or equivalent) and evidence captured.
- [ ] Secret scan completed (`gitleaks` or equivalent) and evidence captured.
- [ ] SBOM generated (`syft` or equivalent) and vulnerability scan results recorded.
- [ ] Exceptions reviewed in `docs/audit/EXCEPTIONS.md`; no expired active exceptions.

## Evidence References
- Security evidence paths:
- Command logs:
- Related finding IDs:

## Sign-off
- Auditor decision: `PASS` | `FAIL`
- Independent reviewer decision: `PASS` | `FAIL`
- Decision date:
- Notes:
