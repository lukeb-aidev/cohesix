<!-- Author: Lukas Bower -->
<!-- Purpose: Track release-blocking due-diligence findings, dispositions, and closure requirements. -->
<!-- Copyright 2026 Lukas Bower -->

# Due Diligence Blockers

## Current release candidate (2026-09-13)

Release readiness remains **FAIL**. The [current audit report](AUDIT_REPORT_2026-09-13.md)
records scoped closure of `DD-2026-0026`, `DD-2026-0027` and `DD-2026-0029`,
the approved EX20–23 renewal through 2026-10-13, and two remaining P1 findings.
Exact `22e3d08ff` QEMU
Stages 01–04 passed; their original Stage 05 failure is preserved. Subsequent
focused checks retain their actual implementation and artifact identities.
Stages 01–04 are not being rerun during this closure work, per user instruction.

| Finding | Implemented repair | Closure still required |
| --- | --- | --- |
| `DD-2026-0028` | Cold reset, firmware completion and ordered timer paths have independent source/emitted review. The exact `61f7bcd1f` GENET boot now retains the ordered reset and firmware observations and passes raw network work. | Complete WiFi boot/network proof and retained PCIe observations for the same image. The completed GENET evidence remains valid. |
| `DD-2026-0030` | `3746e659f` repairs borrowed outputs, critical-TCB syscall storage and child IPC ownership. Focused Mac/native tests and independent source/emitted checks pass. | Required exact QEMU/Pi fault/wake evidence; the unexecuted debugger error-path diagnostic remains incomplete after an automated safety stop. |

`DD-2026-0026` is `CLOSED_VERIFIED` for its storage/ABI defect: independent
review of exact `8c050a0aaf7b8d0d074e9fb12fc91ef45ae60b39` verifies a single
16-byte, 16-aligned TLS object at `0x924b00`, without writable-symbol overlap.
The report binds that symbol to the staged image and records source/test
evidence. Historical physical-fault attribution remains unproved; each later
selected image still requires its own layout check.

`DD-2026-0027` is `CLOSED_VERIFIED`: independent review binds all eight fresh
root-text samples to the actual sealed `61f7bcd1f` ELF and verifies successful
console work. `DD-2026-0029` is `CLOSED_VERIFIED` from independent repair review,
machine-observed keyboard presence and Lukas Bower's explicit confirmation that
keyboard absence was tested. The absence case is human-attested; no machine
log, test timestamp or image identity is inferred. No repeat is required.

DD28 and DD30 remain `OPEN`. The completed focused burn-in recovered the Pi,
last observed ONLINE with empty active leases on `61f7bcd1f`. All fixes are
human-approved and pushed through `719043fad`; the current final-source checks
passed. Stages 01–04 and the completed burn-in were not repeated. Final staged
acceptance and release delivery remain separate from these scoped closures.
No target date, severity or acceptance threshold was relaxed.

## Historical gate snapshots

- Exact `2be878d8d`: QEMU Stages 01–04 PASS, Stage 05 FAIL on then-open DD26–29;
  `out/release-qualification/1.0.0-beta-8bc556850-20260909/canonical-2be878d8d-01/frozen-candidate`.
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
