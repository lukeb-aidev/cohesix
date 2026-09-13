<!-- Author: Lukas Bower -->
<!-- Purpose: Bind scoped DD26, DD27 and DD29 closures and approved exception renewals to evidence while preserving outstanding Stage 05 blockers. -->
<!-- Copyright 2026 Lukas Bower -->

# Cohesix Audit Report (2026-09-13)

Decision: **FAIL / release blocked**. DD26, DD27 and DD29 are closed for their
independently reviewed scoped defects. DD28 and DD30 remain OPEN P1 findings. This report
records scoped evidence closure; it does not announce a new staged PASS.

Scope: Milestone 26e
`m26e-driver-runtime-mcs-port-and-cyw43-coexistence`, discovered during
`m26e-mcs-smp-target-acceptance`. The selected implementation tested for the
DD26 artifact check is `8c050a0aaf7b8d0d074e9fb12fc91ef45ae60b39`.
The audit starts from clean `385d5fdc46c80d6fe1b590f4555ee3b0b0763954`, whose
only change from that implementation is the cohsh Worker-script test fingerprint.
The post-burn-in update below reviews final source `719043fad` and actual Pi
implementation `61f7bcd1f`; it supersedes the earlier outstanding-proof snapshot.
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

## Earlier outstanding-proof snapshot (before completed burn-in repairs)

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
These statements describe that earlier snapshot, not the post-burn-in state below.

## Post-burn-in closure review

Title/ID: `stage5-post-burn-in-evidence-closure`. Milestone and discovery task
remain the M26e tasks named above. The goal is to close only findings whose
missing evidence is now available, preserving completed Stages 01–04 and burn-in.
The clean main source reviewed is
`719043fadc26cf9ac5e826ea68c65d45b49b750f`. Its final change after tested Pi source
`61f7bcd1f057277151b46e884339076a7e49bb59` affects only host evidence timeline
parsing/tests and documentation; the reviewed Pi implementation is unchanged.

The burn-in evidence root is
`out/burn-in/20260913T002623Z/budget-8m-20260913T0637Z` in the main checkout.
`CURRENT.md`, `RUN_RECORD.md` and `REVIEW.md` record the completed focused
repairs and previously uncovered tools. The original timed maintenance failure
after 86 minutes and 43 jobs remains unchanged. Subsequent maintenance refusals,
containment, QUIESCED, both FUSE remounts and resumed CUDA/PEFT work passed.
No full burn-in or completed stage was repeated for this review. The Pi was
last observed ONLINE with empty active leases; the earlier recovery requirement
is superseded by the actual successful boot and workload evidence.

### DD27: first-publication/address defect closed

Independent automated reviewer `/root/review_tls_pcie_publication` recommends
`CLOSED_VERIFIED` in
`independent/dd27-burn-in-evidence-review-719043fad-01/review.json`, SHA-256
`73676bd5c769532c4ef17ac530c9f8e19576cd5d32cba9c1753d602aac1b4c2b`.
Its `verify_existing_evidence.py` independently extracts the sealed root ELF
from the actual Pi image, SHA-256
`5f3457c2a0798ec28bca7f7511b90e8eeb7eb628f43844823939d3661a9aacbf`, image ID
`9643bd91cf1cb7a071d5d2525f2b58ca50695e70088b4b000dbf01b8f034c6cc`.
All eight acquired root-text cuts match actual ELF FNV-1a32 `0x0001c885` and
word34 `0x91002100` in order. The capture-time, pre-PCIe receipt is complete
and no audit-capture failure is present. Image/RAM CRC, exact serial BUILD and
the raw-result boot-log hash bind that boot; the host pack manifest is not
misrepresented as a Pi image identity.

The raw workload completed 1024 unpaced requests on one connection without
retry/reconnect at 670.726 requests/s and p95 4.595 ms. Later authenticated
Linux Gateway evidence exported 14 paths with zero missing/errors. The review
rehashes existing pure checks and verifies the five publication/address/mapping
functions are unchanged through final source. These records satisfy the scoped
integrity and console criterion; no second-boot or instruction-cache claim is made.

### DD29: firmware/USB defect and physical cases closed

The same independent reviewer recommends `CLOSED_VERIFIED` in
`independent/dd29-mixed-evidence-closure-review-719043fad-01/review.json`, SHA-256
`2b2788b075ced4ac8e26fadd53977abea1deada8edf2b29625a899b85bcbe750`.
Six relevant files are unchanged from the independently reviewed implementation
through `61f7bcd1f` and `719043fad`. The acquired Queen log shows firmware
notification, completion and the 20 ms hardware wait, complete proof, keyboard
enumeration, first HID report and command readiness with no pending recovery.
No typed physical key is inferred from those machine observations.

Lukas Bower explicitly confirmed: “Keyboard absent has been tested and confirmed
by me.” The exact statement is retained in
`validation/post-burn-in-stage5-check-20260913T100408Z/keyboard-absent-human-attestation.json`.
This is HUMAN-ATTESTED physical testing. Its image, test time and machine log
are unspecified; none is invented. The finding requires present and absent
physical cases without mandating a machine transcript for absence. The two
stated evidence bases satisfy that scoped criterion. No repeat is required,
and no same-image absence capture or full repeatability matrix is claimed.

### Remaining blockers and retained checks

DD28 remains OPEN: the complete `61f7bcd1f` GENET reset/firmware receipt and
network work are available, but same-image WiFi boot/network proof is missing.
DD30 remains OPEN: normal Worker lifecycle and maintenance policy refusals do
not supply restricted critical-TCB kernel-error/fault/wake proof. The automated
cybersecurity stop described above still supplies no execution evidence and
was not retried. These are the remaining P1 blockers; keyboard absence and
human Rust approval are resolved.

`timeline-merge-checks/results.json` under the burn-in root records PASS for
all eight required checks on `719043fad`: format, Clippy, workspace check/test,
audit, deny advisories, generated consistency and Test Plan integrity.
`human-review-signoff.json` records Lukas Bower's approval of all six burn-in
fixes; earlier Stage 5 fixes were also approved. `publication-result.json`
records their push to main through `719043fad`. The current user reconfirmed
approval of all fixes. Approval and source publication are complete.

The current focused assessment is
`validation/post-burn-in-stage5-check-20260913T100408Z`. It retains the initial
four-finding predicate result, the passing exception register check, current
evidence hashes and the subsequent two-finding closure update. Only affected
audit metadata, generated consistency, Test Plan integrity and diff/link checks
are selected for this documentation update. Existing full-source checks are
reused. No Stage 05 PASS marker or final release claim is emitted.

Changes are confined to the findings register and its audit/checklist consumers.
The complete host-tool suite, Python SDK, generated runtime interfaces and
performance benchmark surfaces were reviewed for compatibility: no behavior,
schema, ABI, workload or implementation changes are needed for this record update.

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
