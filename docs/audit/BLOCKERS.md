<!-- Author: Lukas Bower -->
<!-- Purpose: Track release-blocking due-diligence findings, dispositions, and closure requirements. -->
<!-- Copyright 2026 Lukas Bower -->

# Due Diligence Blockers

## Gate Snapshot
- Release decision state: `FAIL`
- Last blocker review date: `2026-02-13`
- Blocking rule: any `P0/P1` finding not in `CLOSED_VERIFIED` blocks release.

## Active P0/P1 Blockers
| Finding | Severity | Disposition | Blocker | Owner | Target Date | Closure Requirement |
|---|---|---|---|---|---|---|
| `DD-2026-0001` | `P1` | `OPEN` | Hardcoded default auth token in VM console path. | `root-task-owner` | `2026-02-20` | Remove default secret in production path; add fail-fast config validation and negative tests. |
| `DD-2026-0002` | `P1` | `OPEN` | Hardcoded default auth token in host gateway. | `hive-gateway-owner` | `2026-02-20` | Require explicit secret in non-test mode; prove with integration test coverage. |
| `DD-2026-0003` | `P1` | `OPEN` | Auth token material leaks into logs. | `root-task-owner` | `2026-02-20` | Redact auth payload/token bytes and add tests to prevent regression. |
| `DD-2026-0004` | `P1` | `OPEN` | Regression shard policy flow fails. | `tests-owner` | `2026-02-20` | Align fixture with policy namespace model; rerun `cargo test -p tests`. |
| `DD-2026-0005` | `P1` | `OPEN` | Workspace tests fail on macOS due section annotation incompatibility. | `sel4-runtime-owner` | `2026-02-20` | Make section annotation target-aware and restore `cargo test --workspace`. |
| `DD-2026-0008` | `P1` | `PENDING_VERIFY` | Regression batch script SMP path remediation committed, full gate verification pending. | `build-scripts-owner` | `2026-02-14` | Execute regression batch end-to-end and attach logs proving stable SMP default behavior. |
| `DD-2026-0009` | `P1` | `OPEN` | `sel4-sys` affinity invocation label is not cfg-gated for selected kernel headers. | `sel4-sys-owner` | `2026-02-20` | Gate wrapper by kernel config/available labels; prove by build matrix pass. |
| `DD-2026-0010` | `P1` | `OPEN` | REST gateway control routes allow unauthenticated writes. | `hive-gateway-owner` | `2026-02-20` | Add gateway request auth controls and deny unsafe exposure defaults. |

## Non-Blocking Findings (Tracked)
- `P2` items remain in `docs/audit/findings.csv` and cannot be ignored in release rationale.
- Active `P2` findings: `DD-2026-0006`, `DD-2026-0007`, `DD-2026-0011`, `DD-2026-0012`.

## Exit Criteria
A blocker may be removed only when:
- finding disposition is updated to `CLOSED_VERIFIED` in `docs/audit/findings.csv`,
- closure evidence includes reproducible command/log path and commit SHA,
- an independent reviewer records verification in `docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md`.
