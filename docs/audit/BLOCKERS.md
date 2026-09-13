<!-- Author: Lukas Bower -->
<!-- Purpose: Track release-blocking due-diligence findings, dispositions, and closure requirements. -->
<!-- Copyright 2026 Lukas Bower -->

# Due Diligence Blockers

## Current release candidate (2026-09-13)

Release readiness remains **FAIL**. The [current audit report](AUDIT_REPORT_2026-09-13.md)
records scoped closure of `DD-2026-0026`, the approved EX20–23 renewal through
2026-10-13, and the four remaining P1 findings. Exact `22e3d08ff` QEMU
Stages 01–04 passed; their original Stage 05 failure is preserved. Subsequent
focused checks retain their actual implementation and artifact identities.
Stages 01–04 are not being rerun during this closure work, per user instruction.

| Finding | Implemented repair | Closure still required |
| --- | --- | --- |
| `DD-2026-0027` | First-publication/address repair and independent pure review pass; two exact `25dc10813` GENET boots preserve all eight integrity samples and console proof. | Fresh integrity and console records for the repaired candidate image. Later image records cannot inherit the `25dc10813` samples. |
| `DD-2026-0028` | Cold reset, firmware completion and ordered timer paths have independent source/emitted review. The boot-audit reserve repairs demonstrated WiFi log eviction. | Complete same-image WiFi/GENET boot, network and retained reset/status/MSI/endpoint/firmware records. |
| `DD-2026-0029` | Firmware reload and bounded USB failure reporting have independent source review. | Required keyboard-present and keyboard-absent machine cases on the repaired implementation. Historical absence evidence and assumed human observations do not supply those machine states. |
| `DD-2026-0030` | `3746e659f` repairs borrowed outputs, critical-TCB syscall storage and child IPC ownership. Focused Mac/native tests and independent source/emitted checks pass. | Required exact QEMU/Pi fault/wake evidence; the unexecuted debugger error-path diagnostic remains incomplete after an automated safety stop. |

`DD-2026-0026` is `CLOSED_VERIFIED` for its storage/ABI defect: independent
review of exact `8c050a0aaf7b8d0d074e9fb12fc91ef45ae60b39` verifies a single
16-byte, 16-aligned TLS object at `0x924b00`, without writable-symbol overlap.
The report binds that symbol to the staged image and records source/test
evidence. Historical physical-fault attribution remains unproved; each later
selected image still requires its own layout check.

DD27–30 remain `OPEN`. Pi recovery requires a physical power-cycle to observed
U-Boot; the latest passive UART intake received no bytes. Final Pi acceptance,
SD delivery/readback, release packaging and human review of new Rust remain
separate requirements. No target date, severity or acceptance threshold was
relaxed.

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
