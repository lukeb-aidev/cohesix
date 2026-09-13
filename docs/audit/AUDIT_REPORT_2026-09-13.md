<!-- Author: Lukas Bower -->
<!-- Purpose: Bind verified DD26–29 closures and the explicit DD30 release-owner waiver to evidence and canonical staged acceptance. -->
<!-- Copyright 2026 Lukas Bower -->

# Cohesix Audit Report (2026-09-13)

Finding decision for **1.0.0-beta: DD26–29 CLOSED_VERIFIED; DD30 P1 /
ACCEPTED_RISK under the explicit release-owner waiver EX-2026-0030**. The
DD30 dynamic fault/wake test remains unexecuted. The final Stage 05 verdict
belongs to the dedicated release acceptance run on the final committed source,
with both approved decisions explicitly selected and validated; this report
does not substitute for that run. The approved carry-forward preserves the
older Stage 01–04 identities and discloses the missing complete new-source chain.

Scope: Milestone 26e
`m26e-driver-runtime-mcs-port-and-cyw43-coexistence`, discovered during
`m26e-mcs-smp-target-acceptance`. The selected implementation tested for the
DD26 artifact check is `8c050a0aaf7b8d0d074e9fb12fc91ef45ae60b39`.
The audit starts from clean `385d5fdc46c80d6fe1b590f4555ee3b0b0763954`, whose
only change from that implementation is the cohsh Worker-script test fingerprint.
The post-burn-in update reviews source `719043fad` and actual Pi implementation
`61f7bcd1f`. The final finding review below adds fresh WiFi evidence on that same
Pi image and the owner decision on reviewed source `6d7c16e4a`. These updates
supersede the earlier outstanding-proof snapshots. The later host-only compatibility repair below changes no target runtime
implementation.

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
Stages 01–04 not be retraced unless absolutely necessary. At this earlier
snapshot no new full plan had been launched. The later failed attempt and
explicit carry-forward approval are recorded below.

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

## Earlier post-burn-in closure review (before DD28 proof and DD30 owner decision)

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

## Final finding closure and release-owner decision

Title/ID: `stage5-final-findings-and-owner-waiver`. Milestone: M26e
`m26e-driver-runtime-mcs-port-and-cyw43-coexistence`, discovered during
`m26e-mcs-smp-target-acceptance`, including its explicit release 1.0.0-beta
DD30 owner-decision restoration task. Goal: close DD28 using independent
same-image physical evidence and apply the owner's narrowly scoped DD30
acceptance without inventing dynamic test results or altering source provenance.

### DD28: same-image WiFi and GENET proof closed

The fresh WiFi evidence is
`completion-20260913T102047Z/dd28-wifi-01`. The canonical U-Boot/TFTP RAM loader
completed all three transfer and post-reset CRC checks, then booted the same
`61f7bcd1f057277151b46e884339076a7e49bb59` implementation and image used for the
completed GENET evidence. Image SHA-256 is
`5f3457c2a0798ec28bca7f7511b90e8eeb7eb628f43844823939d3661a9aacbf`; image ID is
`9643bd91cf1cb7a071d5d2525f2b58ca50695e70088b4b000dbf01b8f034c6cc`.
After the passive settle, the first raw WiFi workload passed all 64 requests
without retry/reconnect: 40.541 requests/s and p95 33.862 ms against the
unchanged WiFi thresholds. The subsequent authenticated Queen-log collection
completed AUTH/ATTACH/CAT/END/QUIT/EOF with 1379 body frames and 160446 bytes,
then the separate serial probe returned PONG. Both en0/en8 packet intervals
are retained and hashed in `packet-capture-binding.json`.

Independent review
`independent/dd28-dual-mode-evidence-review-6d7c16e4a-01/review.json`, SHA-256
`b23918dd37a5049a5006db8d1670f053dc5acf490cec20cb167d5ea61ff069f7`, rehashes
49 inputs, validates the image and root-text cuts, retained reset/wait/completion
proof, both network lanes, raw results, real CAT frames and EOF, and serial
liveness. Aggregate complete-proof records and unchanged fail-closed source
establish the scoped PCIe checks; unretained register values are not invented.
The recommendation is DD28 `CLOSED_VERIFIED`. RAM-loader elapsed time is not
ordinary SD boot timing, and this finding closure makes no new SD-delivery,
boot-time, second-boot or full repeatability claim.

### DD30: owner accepts the unexecuted dynamic test

Lukas Bower explicitly stated: “There is no debugger, dd30 was a one-off issue.
Mark it as a pass, we are ready for release”. The exact statement is retained
in `completion-20260913T102047Z/dd30-release-owner-decision.json` and the tracked
[DD30_RELEASE_WAIVER.toml](DD30_RELEASE_WAIVER.toml). It accepts the remaining
fault/wake evidence gap for this release. DD30 remains P1 / `ACCEPTED_RISK`;
its verified-closure date and closure evidence remain empty. The target test
is **UNEXECUTED**, not a passing dynamic test.

Source, selected ABI, pure-test and emitted-code review of the IPC repair
remain available. The current review is
`independent/dd30-current-evidence-review-6d7c16e4a-01/review.json`, SHA-256
`97aa61923ad311221b7eb558f07a13474fba7cfc11c1825608165f728befb443`.
The same directory's `physical-pi-capability-gap.json`, SHA-256
`ac8fbbb9df0378951f1eefce74bca40c8dd97398176037539a1bb23cc914a4fb`, distinguishes
ordinary root-control Worker shutdown and maintenance policy refusals from a
restricted kernel fault. There is no supported serial/TCP fault-injection
command, U-Boot cannot fault a running seL4 task, and the user confirms no
debugger is connected. The earlier diagnostic stopped by automatic approval
review was not repeated or bypassed.

EX-2026-0030 is specific to `1.0.0-beta` and
`dd30-restricted-ipc-dynamic-fault-wake`. Its conservative administrative expiry
is 2026-10-13, aligned to the current release qualification window; the owner
did not supply an expiry date for this waiver. The validator requires the exact
approval, matching finding and exception records, owner, release, dates and
active status, and all ten protected production-file hashes at the original
reviewed commit. Current files must match those hashes or the exact separately
approved host-only successor described below. Explicit `DD_RELEASE_ID=1.0.0-beta` is
required for release admission. Missing, changed, withdrawn, expired or
out-of-scope approval fails closed. P0 and every other unclosed P1 remain
blocking; this DD30 decision changes no runtime, severity or threshold. The
separate carry-forward decision below owns the staged provenance exception.

### Earlier current-source qualification plan (superseded by owner approval below)

The source-context audit is
`independent/stage5-current-source-reuse-6d7c16e4a-01/review.json`, SHA-256
`049e05e6c071114439dde87d3c26456115dd8be26b79a49ce09df9f3eaa4c6e6`.
It finds no accepted current-source Stage 01–04 chain. The retained
`stage5-closure-22e3d08ffb5d-01` chain remains valid historical evidence, but
canonical verification rejects its changed source/input context. Material IPC,
Worker maintenance and host FUSE/policy changes followed that chain. Therefore
fresh canonical stages on the final clean commit are necessary; no marker is
copied, source hash rewritten, or reuse predicate relaxed. The completed
burn-in and human keyboard-absence evidence are preserved without repetition.

The final invocation uses the Mac `qemu_smp_production` profile, its selected
seL4 output, the pinned HVF QEMU executable, and explicit release context:

```sh
scripts/ci/test_plan_run.sh --list
DD_RELEASE_ID=1.0.0-beta scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/<final-source-run>
```

The canonical run will retain immutable evidence under
`completion-20260913T102047Z/validation/`, including the
exact clean commit, complete environment selection with secrets omitted,
commands, stage attestations and final verdict. A Stage 05 PASS is valid only
when that canonical run actually completes; it denotes release acceptance
with this disclosed residual risk, not execution of the waived test.

### Changes, checks and compatibility

The atomic change updates the findings/exception records, waiver, canonical
milestone/Test Plan/audit policy and their checklist consumers. A shared
lifecycle validator preserves the focused and full due-diligence gate paths,
with deterministic regressions for ordinary lifecycle rules and the narrow
waiver's invalidation cases. Focused lifecycle tests, shell syntax and exception/blocker checks (including
wrong or absent release selection) are retained in
`validation/dd30-release-waiver-lifecycle-01`. Compiler regeneration, generated
consistency, Test Plan integrity and metadata/diff/link checks are retained in
`completion-20260913T102047Z/checks`. The
compiler regenerates its source inventory and dependent integration digest;
generated files are not hand-edited.

The complete host-tool suite, `tools/cohesix-py` library, runtime interfaces,
performance harnesses, workloads and report schemas were reviewed for
compatibility. This governance change adds no runtime, API, ABI, namespace,
policy-default, workload or benchmark-schema change; those implementations
require no edits. The audit disposition vocabulary is unchanged. The new
release-waiver record is consumed only by the canonical due-diligence gate.
No Rust implementation is changed, and prior approved Rust/source checks
remain preserved with their exact provenance.

## Approved carry-forward and host-only compatibility repair

Title/ID: `stage5-release-owner-carry-forward-host-model`. Milestone and discovery
remain the active M26e restoration tasks stated above, with the explicit approved
carry-forward task in BUILD_PLAN. Goal: complete release Stage 05 without
repeating completed suites, preserving authentic predecessor identities and the
reviewed exact host-only repair. Inputs are the four original `22e3d08ff`
attestations, later scoped fix/target/burn-in receipts, source `aeaa8edb2`, the
reviewed two-file patch and Lukas Bower's subsequent explicit approval.

The fresh `aeaa8edb2` attempt completed workspace, UI, QEMU-feature and preceding
host checks, then failed in the Pi host-feature suite. The first failure was
`cyw43_supervisor_drives_production_pair_restart_one_operation_per_outer_turn`.
The host `CallWithMRs` fixture echoes its request label; the repaired object
wrapper interpreted the Suspend request label 11 as an error. The assertion
panic left recovery state active and poisoned the module lock, producing a
cascade and a later deadline-test loop whose guard counted only admitted turns.
The initial standalone failure, isolated deadline/group controls, ordered
predecessor diagnosis and process sample are retained under
`completion-20260913T102047Z/checks`. The owned stalled host-test process was
terminated; its original failed canonical attempt remains unchanged.

Independent causal review is
`independent/pi-feature-host-suspend-causal-review-aeaa8ed-01/review.json`, SHA-256
`11d2e85c371437a519bea49768c7887199ac7c8ed35b9160835887d7b469c0e5`.
The two-file repair changes only the typed host object model in `sel4.rs` and
the deadline fixture in `event/mod.rs`. The four typed host operations use their
existing models; unknown endpoint labels still echo. The target fast-register
call is unchanged. The fixture enforces its existing admitted/outer-turn
relationship on every iteration, retaining the 25 ms and 192-turn limits.
No recovery condition is cleared merely to make a test pass.

Independent source review
`completion-20260913T102047Z/independent-review/host-kernel-object-model-01/review.json`,
SHA-256 `31f84049561cabce88182450be0e977838eeb9adc17b7728701beeb6fd652cf2`,
verifies the normalized target helper is identical and the event production
prefix is byte-identical. Patch SHA-256 is
`54c1ea4bc418b41400c450f47de8e354c3c21aa5e4181748602fb6bfdf85a738`.
The previously failed Pi host-feature suite then passed all **2388 tests** with
zero failures. The repaired crate's normal-profile Clippy, formatting and Rust
risk gate passed. An additional noncanonical feature-Clippy exploration emitted
487 diagnostics at unchanged sites; its failure is retained separately, and
unrelated feature-lint cleanup was not added to the release task. No completed
workspace or UI suite was repeated after the no-duplicate instruction.

Lukas Bower approved the exact review packet and answered “Approved”. The
immutable decision is
`completion-20260913T102047Z/release-carry-forward-review/owner-approval.json`,
SHA-256 `009fdb80aaf6be0206636bd5886337bcbc9d84101260a3521e7afaa44f2befc1`.
The packet approves the Rust repair, release-only reuse of original Stage 01–04
records plus later scoped fix evidence, and the exact DD30 successor binding.
[RELEASE_1_0_0_BETA_CARRY_FORWARD.toml](RELEASE_1_0_0_BETA_CARRY_FORWARD.toml)
binds that decision, proposal, independent review, original attestation/context
hashes, the closure-file scope and 18 named later evidence records. Their exact
hashes and original verdicts are required, including the timed burn-in failure
and DD30's partial proof; final review receipts cannot replace them. Its expiry is the conservative existing
2026-10-13 administrative limit. The original DD30 decision and protected
hashes remain intact; the single host-only successor is checked explicitly.

The dedicated `scripts/ci/release_stage5_acceptance.py` path validates the
immutable predecessor graph and exact clean final source, checks current unique
governance, and emits `release-stage5-acceptance.json` with
`PASS_WITH_RESIDUAL_RISK` only on actual success. It creates no replacement
Stage 01–04 `.done` files and does not disable the ordinary current-context
verifier. Current `cargo audit` and `cargo deny check advisories` both passed
at 11:23 UTC; receipts under `completion-20260913T102047Z/unique-stage5-governance`
retain their exact successful commands and logs for this same acceptance
operation, with Cargo.lock and tool bindings required before reuse.

The accepted residual risks are the unexecuted DD30 dynamic fault/wake test and
the missing complete current-source Stage 01–04 chain. The original records,
failed attempts and evidence authorities stay visible. Release-bundle source
and content integrity, delivered media, other target proof and performance
thresholds remain outside this waiver. A separate packaging task owns release
artifacts and documentation; this closure does not relabel its inputs.

Compatibility review covers the complete host-tool suite, Python SDK and
performance harnesses: no public API, namespace, ABI, operator workflow,
benchmark workload or result-schema change is needed. Only the host test model,
test failure behavior and explicit release-governance workflow change. The
compiler-owned outputs retain their authoritative generation procedure.
