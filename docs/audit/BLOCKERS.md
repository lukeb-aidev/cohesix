<!-- Author: Lukas Bower -->
<!-- Purpose: Track release-blocking due-diligence findings, dispositions, and closure requirements. -->
<!-- Copyright 2026 Lukas Bower -->

# Due Diligence Blockers

## Gate Snapshot
- Due-diligence gate execution: `PASS` (`scripts/ci/due_diligence_gate.sh`)
- Baseline gate evidence root: `out/audit/gate/20260214T044955Z`
- M26d closure evidence roots: `out/test-plan/m26d-unsafe-remediation-qemu` and `out/test-plan/m26d-unsafe-remediation-pi4`
- Release decision state: historical baseline `PASS`; M26d P2 exception closure `PASS` for offline engineering scope
- Last blocker review date: `2026-07-16`
- Blocking rule: any `P0/P1` finding not in `CLOSED_VERIFIED` blocks release.

## Active P0/P1 Blockers
No active blockers. All findings in `docs/audit/findings.csv` are `CLOSED_VERIFIED`.

## Closed In This Run (2026-02-14)
- `DD-2026-0001`, `DD-2026-0002`, `DD-2026-0003`, `DD-2026-0007`, `DD-2026-0009`, `DD-2026-0010`, `DD-2026-0013`, `DD-2026-0014`, `DD-2026-0015`.
- Closure evidence root: `out/audit/gate/20260214T044955Z`.

## P2 Exception Closure (2026-07-16)
- Findings `DD-2026-0016`, `DD-2026-0017`, and `DD-2026-0018` are `CLOSED_VERIFIED`; exceptions `EX-2026-0016`, `EX-2026-0017`, and `EX-2026-0018` are `CLOSED`.
- Verified implementation commit: `68dd774d6ceb0706e162877f74766dd324572425`.
- Clean detached-worktree evidence: QEMU Test Plan Stages 01-05 at `out/test-plan/m26d-unsafe-remediation-qemu`, Pi hardware-independent Stages 01-02 at `out/test-plan/m26d-unsafe-remediation-pi4`, and the Stage 05 due-diligence log at `out/test-plan/m26d-unsafe-remediation-qemu/logs/stage-05-due-diligence.log`.
- The closure covers the Rust risk-ratchet, exception lifecycle, linked-runtime/HAL shared-state boundary, focused tests, workspace gates, and image packaging. It is not Pi hardware acceptance or repeated-boot WiFi proof.

## Notes
- Release decision is `PASS` per `docs/audit/DUE_DILIGENCE_PLAN.md` Section 10 (`ALL CHECKS PASSED`, no open `P0/P1`).
- Independent code and commit-scope review is complete for `DD-2026-0016` through `DD-2026-0018`; older finding rows retain their historical `independent-review-pending` state.
- Pi 4 hardware acceptance and reliable every-boot WiFi connection proof remain hardware-gated until the exact image can be exercised repeatedly on a Pi 4 with an available WiFi connection.

## Exit Criteria
A blocker may be removed only when:
- finding disposition is updated to `CLOSED_VERIFIED` in `docs/audit/findings.csv`,
- closure evidence includes reproducible command/log path and commit SHA,
- an independent reviewer records verification in `docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md`.
