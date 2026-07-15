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
| `EX-2026-0016` | `DD-2026-0016` | `P2` | Pi 4 driver-task isolation substrate Rust risk-ratchet delta | seL4 object allocation and isolated driver-task setup required additional explicit unsafe syscall and pointer-boundary wrappers while strongest hardware-state migration remained in progress. | Superseded by `EX-2026-0017`; the lifecycle is pending exact remediation-commit verification rather than relying on the earlier supersession commit. | `driver-task-owner` | `Lukas Bower` | `2026-05-22` | `2026-06-19` | `REVOKED` |
| `EX-2026-0017` | `DD-2026-0017` | `P2` | Milestone 26c due-diligence Rust risk-ratchet baseline refresh | The 26c QEMU closure froze the path-filtered scanner counts before the cfg-test false-positive defect was identified. | Superseded by `EX-2026-0018`; the lifecycle is pending exact remediation-commit verification rather than relying on the earlier supersession commit. | `audit-owner` | `Lukas Bower` | `2026-06-28` | `2026-07-28` | `REVOKED` |
| `EX-2026-0018` | `DD-2026-0018` | `P2` | Linked-runtime CYW43/SDIO production unsafe delta | Linux-aligned CYW43/SDIO ownership required explicit volatile shared-ring, DPC, MMIO, and seL4 notification boundaries in the isolated runtime and HAL. | Scanner v4 freezes production at global `unsafe=691`, `unwrap=38`, `expect=240`, `panic=96`, linked-runtime/HAL `unsafe=144`, `unwrap=0`, `expect=2`, `panic=0`, and outside-component `unsafe=547`, `unwrap=38`, `expect=238`, `panic=96`; exact-commit clean gates are mandatory before closure. | `driver-task-owner` | `Lukas Bower` | `2026-07-14` | `2026-07-28` | `APPROVED_ACTIVE` |

## Lifecycle States
- `PROPOSED`: Captured but not approved.
- `APPROVED_ACTIVE`: Approved and not expired.
- `EXPIRED`: Past expiration date; invalid for release.
- `REVOKED`: Withdrawn before expiration.
- `CLOSED`: No longer needed due to verified remediation.
