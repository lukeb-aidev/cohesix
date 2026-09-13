<!-- Author: Lukas Bower -->
<!-- Purpose: Bind the scoped DD26 closure and approved exception renewals to evidence while preserving outstanding Stage 05 blockers. -->
<!-- Copyright 2026 Lukas Bower -->

# Cohesix Audit Report (2026-09-13)

Decision: **FAIL / release blocked**. DD26 is closed for its independently
verified TLS storage/ABI defect. DD27–30 remain OPEN P1 findings. This report
records scoped evidence closure; it does not announce a new staged PASS.

Scope: Milestone 26e
`m26e-driver-runtime-mcs-port-and-cyw43-coexistence`, discovered during
`m26e-mcs-smp-target-acceptance`. The selected implementation tested for the
DD26 artifact check is `8c050a0aaf7b8d0d074e9fb12fc91ef45ae60b39`.
The audit starts from clean `385d5fdc46c80d6fe1b590f4555ee3b0b0763954`, whose
only change from that implementation is the cohsh Worker-script test fingerprint.
No runtime implementation changes are included in this audit update.

## Evidence root and DD26 closure

All relative evidence paths below are under:

```text
/Users/lukasbower/GitHub/cohesix/out/release-qualification/1.0.0-beta-8bc556850-20260909/continuation-a45a3d9cc-20260910T083610Z/stage5-closure-20260910T124827Z
```

Independent automated reviewer `/root/review_tls_pcie_publication` reviewed
the source, existing test receipts and emitted artifact independently of the
repair implementer. `independent/tls-pcie-publication.json` and its companion
Markdown report establish real `AtomicUsize` storage, C layout, alignment,
single-crate definition/accessors, Release/Acquire pointer publication, and
the unsafe accessor's exclusive-image obligation. A zero-sized runtime
definition no longer exists. The stored pointer's atomic publication does not
grant mutable ownership of its target.

The latest artifact refresh is
`independent/pi4-ipc-owner-emitted-8c050a0aa-01/review.json`; its exact image is
`fe083489691a86129906e8ee64784ac7c680cbb92b2f2be6bb15a97da1861ca7`, with SHA-256
`c61c94b285f450730f259d76c295859418399b580c36a0266176dee47ac3230f`.
The staged root is bound through the actual CPIO member, allocated sections
and load segments to the symbol-bearing ELF. Its single TLS object occupies
16 bytes at `0x924b00`, aligned to 16, without writable-symbol overlap.
`independent/pi4-8c050a0aa-artifact-review.json`, SHA-256
`57b0fd180f4c701a7d5caa7a4d9c6fc31ef1fad6517df83e15cf46762ad6bb24`,
records the machine-readable artifact verdict.

The DD26 closure recommendation retains exact source-span comparisons and
rehashes 22 existing evidence files:
`independent/dd26-scoped-closure-recommendation-385d5fdc4-01/review.json`, SHA-256
`0195de2918cdd52fc33d07cb44a3a64d5ee6493c270c4335c5af48e36cb2357a`.
Reproduction commands for the original artifact and emitted-code checks are
retained in the exact Pi artifact review's `verification_commands`; the checked scripts are
`verify_pi_artifact.py` and `verify_pi_emission.py` beside that report.
Existing `tls_storage_tests::exported_tls_word_has_storage_and_c_layout`
receipts and independent extracted-layout checks are reused without rerunning
Stages 01–04. These establish the scoped defect closure. Historical physical
fault attribution remains unproved, and any subsequently selected image still
requires its own layout check.

## Renewal and retained validation

Lukas Bower explicitly renewed EX20–23 through 2026-10-13. Approval is retained
in `continuation-20260912T212227Z/exception-renewal-approval.json`, SHA-256
`497578be08681f923979e6aa397c9a6be649fca6fb99cc10b23b6671a24e1159`.
Commit `724580aa62a848a8356159b9a7f0f86d23993c78` applied the reviewed scopes and
dates. `validation/exception-renewal-apply-01/result.json` records the passing
exception-register, generated-contract and Test Plan checks. P1 conditions
and compensating controls remain binding.

`validation/qemu-stage5-closure-22e3d08ffb5d-01/SUMMARY.md` preserves exact
Stages 01–04 PASS. Its original Stage 05 failure and subsequent collect-all
diagnostic remain unchanged. Later focused evidence includes
`validation/qemu-worker-convergence-8c050a0aa-01` (ordinary Worker lifecycle
PASS), `validation/native-base-tcp-8c050a0aa-01` (Jetson KVM base matrix and ten
scripts PASS), and the scoped IPC tests/source/emitted reviews. The KVM guest
was cross-built on the Mac; its client was built natively on Linux AArch64.
None of these results relabels a predecessor attestation.

The failed `8c050` broad-run attempt is also preserved. Its Worker-script
fingerprint mismatch was corrected in `385d5fdc4`; all six focused catalogue
tests passed in `validation/worker-script-catalog-01`. The user directed that
Stages 01–04 not be retraced unless absolutely necessary. No new full plan
was launched after that instruction. Final canonical context validation remains
required when the actual remaining blockers have been resolved.

## Outstanding proof and delivery boundary

DD27 requires fresh candidate root-text/console evidence. DD28 requires
complete retained PCIe observations and WiFi/GENET boot/network proof. DD29
requires the actual repaired-image keyboard-present and keyboard-absent
machine cases. The Pi currently needs physical recovery to observed U-Boot:
`continuation-20260912T212227Z/serial-intake-05/result.json` records ten seconds
with zero received or transmitted bytes. No candidate boot is inferred.
Human observations are assumed PASS as instructed, separately from missing
machine records.

DD30 still needs the required exact QEMU/Pi fault/wake evidence.
`validation/qemu-ipc-owner-gdb-3746e659f-01/stop-report.json`, SHA-256
`ddd6cffd321fcfcbe02f760f62dde295bbf0f6061428c92944778492789e7cc2`,
preserves the diagnostic stopped by automated cybersecurity review before
fault injection or error/retry execution. It was not retried. Debugger
capability preflight and ordinary lifecycle PASS do not satisfy this gap.

Fresh Pi acceptance, pressure/repeatability, SD delivery/readback and normal
boot, extracted release-bundle verification, human review of new Rust, and
promotion remain separate requirements. No merge, push or release occurred.

## Stage 05 predicate repair and audit-update checks

The findings predicate previously deferred an open P1 whose `target_date`
was in the future. That would exclude DD30 until its due date, contradicting
Section 10 of the due-diligence plan. The gate now blocks every unclosed
P0/P1 regardless of that scheduling date; severity, dates and exceptions are
unchanged. The focused `--check-blocking-findings` entry point exercises the
same predicate as Stage 05. Deterministic lifecycle regressions cover past,
future, absent and malformed dates, selected unclosed dispositions, verified
closures and the separate P2 lifecycle boundary.

This audit/gate delta changes no host-tool, Python SDK, generated runtime contract,
benchmark workload, report schema, ABI or runtime behavior. Those complete
surface groups were reviewed; none requires an implementation change here.
The local audit-update record retains the patch, source identity and exact
commands for focused lifecycle tests, shell syntax, register validation,
generated-contract/Test Plan checks and diff validation. No broad host or
target suite was selected for this audit/gate repair.
The new report is admitted by regenerating the compiler-owned source inventory
and dependent host-integration digest. The first generated check's missing
inventory entry remains recorded separately from the corrected result.
