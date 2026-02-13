<!-- Author: Lukas Bower -->
<!-- Purpose: Track due-diligence risk exceptions with owners, approvals, compensating controls, and expiration. -->
<!-- Copyright 2026 Lukas Bower -->

# Due Diligence Exceptions Register

## Exception Policy
- `P0` and `P1` findings cannot be accepted as residual risk for release.
- Any exception must reference a finding ID in `docs/audit/findings.csv`.
- Every exception must include: risk owner, approving authority, compensating controls, decision date, and expiration date.
- Exceptions expire automatically; expired exceptions force decision state `FAIL` until renewed or remediated.

## Register
| Exception ID | Related Finding | Severity | Scope | Rationale | Compensating Controls | Risk Owner | Approved By | Decision Date | Expiration Date | Status |
|---|---|---|---|---|---|---|---|---|---|---|
| `None` | `N/A` | `N/A` | `N/A` | No accepted residual risks are recorded for the current audit baseline. | `N/A` | `N/A` | `N/A` | `N/A` | `N/A` | `CLOSED` |

## Lifecycle States
- `PROPOSED`: Captured but not approved.
- `APPROVED_ACTIVE`: Approved and not expired.
- `EXPIRED`: Past expiration date; invalid for release.
- `REVOKED`: Withdrawn before expiration.
- `CLOSED`: No longer needed due to verified remediation.
