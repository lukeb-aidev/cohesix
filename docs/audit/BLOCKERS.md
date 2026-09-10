<!-- Author: Lukas Bower -->
<!-- Purpose: Track release-blocking due-diligence findings, dispositions, and closure requirements. -->
<!-- Copyright 2026 Lukas Bower -->

# Due Diligence Blockers

## Current release candidate (2026-09-10)

Release readiness is **FAIL**. Exact `2be878d8d` passed QEMU Stages 01–04;
Stage 05 stopped on the four open P1 findings below. `cargo audit` and
`cargo deny check advisories` passed. Complete evidence is retained under
`out/release-qualification/1.0.0-beta-8bc556850-20260909/canonical-2be878d8d-01/frozen-candidate`.
Later source or hardware checks do not rewrite that failed run.

| Finding | Implemented repair | Closure still required |
| --- | --- | --- |
| `DD-2026-0026` | `1894029de` gives the TLS base real atomic storage. Exact staged `a5ef48045` loadable sections match the symbol-bearing ELF; its TLS object is 16 aligned bytes with no overlapping object. | Independent ABI/layout review; retain physical fault attribution separately. |
| `DD-2026-0027` | `b8e293bc9` returns the new PCIe mapping after successful first publication and checks register addresses. | Independent review and exact-image Pi integrity/console evidence. |
| `DD-2026-0028` | `d35fc94f0` releases the cold PCIe bridge before status/MSI access. | Independent review and complete exact-image Wi-Fi/GENET boot and network proof. |
| `DD-2026-0029` | `db420f1c1` reloads VL805 firmware after cold reset and preserves USB failure reporting. | Independent review plus the required keyboard-present and keyboard-absent physical cases. |

The exact ELF check is at
`out/release-qualification/1.0.0-beta-8bc556850-20260909/audit-a5ef48045/tls-layout-review.json`.
These findings remain `OPEN` in `findings.csv`; a repair commit or an isolated
passing check does not close them. Final Pi acceptance, SD delivery/readback,
release packaging and human Rust review also remain separate requirements.

## Historical gate snapshots

- February baseline: `PASS`, `out/audit/gate/20260214T044955Z`.
- M26d P2 exception closure: `PASS` for offline engineering scope,
  `out/test-plan/m26d-unsafe-remediation-qemu` and
  `out/test-plan/m26d-unsafe-remediation-pi4`.
- Blocking rule: any P0/P1 finding outside `CLOSED_VERIFIED` blocks release.

## Closed In This Run (2026-02-14)
- `DD-2026-0001`, `DD-2026-0002`, `DD-2026-0003`, `DD-2026-0007`, `DD-2026-0009`, `DD-2026-0010`, `DD-2026-0013`, `DD-2026-0014`, `DD-2026-0015`.
- Closure evidence root: `out/audit/gate/20260214T044955Z`.

## P2 Exception Closure (2026-07-16)
- Findings `DD-2026-0016`, `DD-2026-0017`, and `DD-2026-0018` are `CLOSED_VERIFIED`; exceptions `EX-2026-0016`, `EX-2026-0017`, and `EX-2026-0018` are `CLOSED`.
- Verified implementation commit: `68dd774d6ceb0706e162877f74766dd324572425`.
- Clean detached-worktree evidence: QEMU Test Plan Stages 01-05 at `out/test-plan/m26d-unsafe-remediation-qemu`, Pi hardware-independent Stages 01-02 at `out/test-plan/m26d-unsafe-remediation-pi4`, and the Stage 05 due-diligence log at `out/test-plan/m26d-unsafe-remediation-qemu/logs/stage-05-due-diligence.log`.
- The closure covers the Rust risk-ratchet, exception lifecycle, linked-runtime/HAL shared-state boundary, focused tests, workspace gates, and image packaging. It is not Pi hardware acceptance or repeated-boot WiFi proof.

## Notes
- The February baseline decision was `PASS` per `docs/audit/DUE_DILIGENCE_PLAN.md` Section 10 (`ALL CHECKS PASSED`, no open `P0/P1`).
- Independent code and commit-scope review is complete for `DD-2026-0016` through `DD-2026-0018`; older finding rows retain their historical `independent-review-pending` state.
- Pi 4 hardware acceptance and reliable every-boot WiFi connection proof remain hardware-gated until the exact image can be exercised repeatedly on a Pi 4 with an available WiFi connection.

## Exit Criteria
A blocker may be removed only when:
- finding disposition is updated to `CLOSED_VERIFIED` in `docs/audit/findings.csv`,
- closure evidence includes reproducible command/log path and commit SHA,
- an independent reviewer records verification in `docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md`.
