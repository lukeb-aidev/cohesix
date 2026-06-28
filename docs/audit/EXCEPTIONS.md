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
| `EX-2026-0016` | `DD-2026-0016` | `P2` | Pi 4 driver-task isolation substrate Rust risk-ratchet delta | seL4 object allocation and isolated driver-task setup required additional explicit unsafe syscall and pointer-boundary wrappers while strongest hardware-state migration remained in progress. | Superseded by `EX-2026-0017`, which records the current 26c due-diligence ratchet baseline and keeps the Stage 05 gate active. | `driver-task-owner` | `Lukas Bower` | `2026-05-22` | `2026-06-19` | `CLOSED` |
| `EX-2026-0017` | `DD-2026-0017` | `P2` | Milestone 26c due-diligence Rust risk-ratchet baseline refresh | Current 26c QEMU closure runs expose accumulated non-test risk-pattern counts that must be frozen before any cleanup/refactor wave can claim zero-regression status. | Stage 05 due-diligence must pass against the refreshed baseline `unsafe=804`, `unwrap=469`, `expect=1063`, `panic=147`; future non-test increases require a new finding and approved exception before merge; Phase 4 cleanup remains blocked until characterization and post-behavior baselines are valid. | `audit-owner` | `Lukas Bower` | `2026-06-28` | `2026-07-28` | `APPROVED_ACTIVE` |

## Lifecycle States
- `PROPOSED`: Captured but not approved.
- `APPROVED_ACTIVE`: Approved and not expired.
- `EXPIRED`: Past expiration date; invalid for release.
- `REVOKED`: Withdrawn before expiration.
- `CLOSED`: No longer needed due to verified remediation.
