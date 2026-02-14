<!-- Author: Lukas Bower -->
<!-- Purpose: Track release-blocking due-diligence findings, dispositions, and closure requirements. -->
<!-- Copyright 2026 Lukas Bower -->

# Due Diligence Blockers

## Gate Snapshot
- Due-diligence gate execution: `PASS` (`scripts/ci/due_diligence_gate.sh`)
- Gate evidence root: `out/audit/gate/20260213T222403Z`
- Release decision state: `FAIL`
- Last blocker review date: `2026-02-13`
- Blocking rule: any `P0/P1` finding not in `CLOSED_VERIFIED` blocks release.

## Active P0/P1 Blockers
| Finding | Severity | Disposition | Blocker | Owner | Target Date | Closure Requirement |
|---|---|---|---|---|---|---|
| `DD-2026-0001` | `P1` | `OPEN` | Hardcoded default auth token in VM console path. | `root-task-owner` | `2026-02-20` | Remove default secret in production path; add fail-fast config validation and negative tests. |
| `DD-2026-0002` | `P1` | `OPEN` | Hardcoded default auth token in host gateway. | `hive-gateway-owner` | `2026-02-20` | Require explicit secret in non-test mode; prove with integration test coverage. |
| `DD-2026-0003` | `P1` | `OPEN` | Auth token material leaks into logs. | `root-task-owner` | `2026-02-20` | Redact auth payload/token bytes and add tests to prevent regression. |
| `DD-2026-0009` | `P1` | `OPEN` | `sel4-sys` affinity invocation label is not cfg-gated for selected kernel headers. | `sel4-sys-owner` | `2026-02-20` | Gate wrapper by kernel config/available labels; prove by build matrix pass. |
| `DD-2026-0010` | `P1` | `OPEN` | REST gateway control routes allow unauthenticated writes. | `hive-gateway-owner` | `2026-02-20` | Add gateway request auth controls and deny unsafe exposure defaults. |
| `DD-2026-0013` | `P1` | `OPEN` | `coh` CLI fallback uses hardcoded default auth token. | `coh-owner` | `2026-02-20` | Remove fallback literal and require explicit token outside mock mode. |
| `DD-2026-0014` | `P1` | `OPEN` | `cohsh` CLI fallback uses hardcoded default auth token. | `cohsh-owner` | `2026-02-20` | Remove fallback literal and require explicit token configuration. |
| `DD-2026-0015` | `P1` | `OPEN` | `cohsh` TCP transport builder defaults to hardcoded auth token. | `cohsh-owner` | `2026-02-20` | Remove default token initialization and enforce explicit auth token configuration. |

## Recently Closed (This Run)
- `DD-2026-0004` -> `CLOSED_VERIFIED` (`base-shard/shard_1k.coh` pass in `out/audit/gate/20260213T222403Z/regression-batch.log`).
- `DD-2026-0005` -> `CLOSED_VERIFIED` (`cargo test --workspace` pass in `out/audit/gate/20260213T222403Z/workspace-tests.log`).
- `DD-2026-0006` -> `CLOSED_VERIFIED` (`scripts/check-generated.sh` pass in `out/audit/gate/20260213T222403Z/generated-artifacts.log`).
- `DD-2026-0011` -> `CLOSED_VERIFIED` and `DD-2026-0012` -> `CLOSED_VERIFIED` (`scripts/ci/check_test_plan.sh` pass in `out/audit/gate/20260213T222403Z/test-plan-hash-check.log`).

## Non-Blocking Findings (Tracked)
- Active `P2` findings: `DD-2026-0007`.
- Closed `P2` findings this run: `DD-2026-0006`, `DD-2026-0011`, `DD-2026-0012`.

## Notes
- The gate script defers future-dated `P1` findings and therefore passed with deferred blockers.
- Release decision remains `FAIL` per `docs/audit/DUE_DILIGENCE_PLAN.md` Section 10 because `P1` findings remain open.

## Exit Criteria
A blocker may be removed only when:
- finding disposition is updated to `CLOSED_VERIFIED` in `docs/audit/findings.csv`,
- closure evidence includes reproducible command/log path and commit SHA,
- an independent reviewer records verification in `docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md`.
