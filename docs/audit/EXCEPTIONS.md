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
| `EX-2026-0016` | `DD-2026-0016` | `P2` | Pi 4 driver-task isolation substrate Rust risk-ratchet delta | seL4 object allocation and isolated driver-task setup require additional explicit unsafe syscall and pointer-boundary wrappers while strongest hardware-state migration remains in progress. | Stage 5 due-diligence gate rerun must pass with the raised baseline; QEMU isolated driver-task smoke must prove vspace=isolated and pointer_free_ipc=yes; Pi hardware proof remains required before claiming full dedicated hot-path ownership. | `driver-task-owner` | `Lukas Bower` | `2026-05-22` | `2026-06-19` | `APPROVED_ACTIVE` |

## Lifecycle States
- `PROPOSED`: Captured but not approved.
- `APPROVED_ACTIVE`: Approved and not expired.
- `EXPIRED`: Past expiration date; invalid for release.
- `REVOKED`: Withdrawn before expiration.
- `CLOSED`: No longer needed due to verified remediation.
