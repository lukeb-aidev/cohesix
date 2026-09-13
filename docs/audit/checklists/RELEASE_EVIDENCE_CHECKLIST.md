<!-- Author: Lukas Bower -->
<!-- Purpose: Checklist for release evidence completeness, independence verification, and final due-diligence decision. -->
<!-- Copyright 2026 Lukas Bower -->

# Release Evidence Checklist

## Release candidate 1.0.0-beta (2026-09-13)

- [x] Preserve the exact `2be878d8d` QEMU Stages 01–04 PASS and Stage 05 FAIL logs.
- [x] Preserve completed native Jetson benchmarks and exact receipt/completion logs.
- [x] Bind the TLS storage layout check to the staged `a5ef48045` Pi image.
- [x] Preserve exact `22e3d08ff` Stages 01–04 PASS and its original Stage 05 FAIL.
- [x] Record approved EX20–23 renewal through 2026-10-13 and its passing register check.
- [x] Close the DD26 storage/ABI defect with independent automated reviewer
  `/root/review_tls_pcie_publication`, exact `8c050a0aaf7b8d0d074e9fb12fc91ef45ae60b39`
  source/image evidence and the references in the [current report](../AUDIT_REPORT_2026-09-13.md).
- [x] Close DD27 with independent `/root/review_tls_pcie_publication` review of
  actual `61f7bcd1f` image bytes, eight retained integrity samples and console work.
- [x] Close DD29 with that reviewer's unchanged-repair review, machine-observed
  keyboard presence and Lukas Bower's explicit human-attested keyboard-absence test.
  No machine trace, image identity or test timestamp is assigned to the attestation.
- [x] Close DD28 with independent exact-image dual-mode reset/firmware/network proof.
- [x] Record Lukas Bower's DD30 release-specific risk acceptance, preserving P1 severity
  and the unexecuted dynamic fault/wake test; validate EX30 and protected source hashes.
- [x] Preserve all eight passing final-source checks at `719043fad` and completed
  focused burn-in repair evidence without rerunning Stages 01–04 or burn-in.
- Final frozen-source release completion is recorded by the acceptance artifact.
  The owner explicitly approved the bound
  predecessor-stage carry-forward and exact host-only repair; the dedicated
  release record retains the accepted evidence risk and original identities.
- [ ] Verify physical Pi SD delivery/readback and required fresh boots.
- [ ] Verify the extracted Mac, native Linux and Pi release bundles.
- [x] Record Lukas Bower's approval of all fixes and publication through `719043fad`.

Finding decision: **verified closures plus explicit DD30 residual risk**. Final
Stage 05 acceptance requires the dedicated release acceptance result under
[the approved carry-forward](../RELEASE_1_0_0_BETA_CARRY_FORWARD.toml).
See [BLOCKERS.md](../BLOCKERS.md).
The DD26–29 decisions close their scoped defects. Human Rust sign-off
is recorded separately. Existing Stages 01–04 retain their source
bindings; none has been copied or relabelled for a successor commit.
The dated snapshots below describe their original runs only.

## Historical Run Metadata
- Audit window: `2026-02-14T04:38:51Z` (full baseline run)
- Baseline commit SHA: `22cd5017d060c3439b6f7fc4f70717f329134803`
- Auditor: `automation-agent`
- Independent reviewer: `TBD`
- Gate evidence root: `out/audit/gate/20260214T044955Z`

## Required Evidence Pack
- [x] `scripts/ci/due_diligence_gate.sh` output captured.
- [x] `scripts/check-generated.sh` output captured and passing.
- [x] `scripts/ci/check_test_plan.sh` output captured and reviewed.
- [x] `scripts/cohsh/run_regression_batch.sh` output captured and reviewed.
- [x] `docs/audit/findings.csv` updated with current dispositions, owners, and closure evidence fields.
- [x] `docs/audit/BLOCKERS.md` updated with current open `P0/P1` blockers.
- [x] `docs/audit/CONTROL_TRACEABILITY.md` updated with control-to-evidence links for this run.
- [x] `docs/audit/EXCEPTIONS.md` reviewed; no expired active exceptions.
- [x] `docs/audit/checklists/ARCHITECTURE_CHECKLIST.md` completed and signed.
- [x] `docs/audit/checklists/SECURITY_CHECKLIST.md` completed and signed.
- [x] `docs/audit/AUDIT_REPORT_2026-02-14.md` produced for this run.

## Decision Guardrails
- [x] No open `P0/P1` findings in `docs/audit/findings.csv`.
- [x] Any `P2` accepted risks have owner and expiration in `docs/audit/EXCEPTIONS.md`.
- [x] All closures include reproducible command/log evidence and commit SHA.
- [ ] Independent reviewer verified all `P0/P1` remediations.

## Release Decision
- Decision: `PASS`
- Decision date: `2026-02-14`
- Decision authority: `Due-diligence automation run (pending human independent review sign-off)`
- Residual risk summary: `No open blockers; independent reviewer verification remains pending for newly closed findings.`
- Follow-up deadlines: `Independent reviewer sign-off before release publication authority action.`

## Prior Snapshot (2026-02-13 Baseline)
- Decision: `FAIL`
- Decision date: `2026-02-13`
- Decision owner: `Audit run pending human sign-off`
- Residual risks: `Open P1 findings (auth defaults/logging/cfg-gating)`

## Milestone 26d P2 Exception Closure (2026-07-16)

- [x] Immutable remediation implementation is committed at `68dd774d6ceb0706e162877f74766dd324572425`.
- [x] Independent `p2_final_review` and `commit_scope_audit` reviews reported `PASS`.
- [x] Scanner v4 current counts passed at global `691/38/240/96`, linked-runtime/HAL `144/0/2/0`, and outside-component `547/38/238/96`.
- [x] Exact `cf8f9ee30` historical replay passed at global `693/38/240/96`, linked-runtime/HAL `146/0/2/0`, and outside-component `547/38/238/96`.
- [x] Lifecycle validation passed 19 tests and the exception-register gate passed with DD16-DD18 `CLOSED_VERIFIED` and EX16-EX18 `CLOSED`.
- [x] Focused tests passed: 466 linked-runtime tests, 1425 Pi-feature root-task tests, and 25 driver-ABI tests.
- [x] Workspace formatting, clippy, check, test, `cargo audit`, `cargo deny check advisories`, generated-artifact, test-plan integrity, secret-scan, and aarch64 checks passed.
- [x] `scripts/cohesix-build-run.sh --no-run --cargo-target aarch64-unknown-none` produced a 2,542,080-byte CPIO below the 4 MiB guard.
- [x] Clean detached QEMU Test Plan Stages 01-05 passed at `out/test-plan/m26d-unsafe-remediation-qemu`.
- [x] Clean detached Pi hardware-independent Test Plan Stages 01-02 passed at `out/test-plan/m26d-unsafe-remediation-pi4`.
- [ ] Live Pi 4 boot, boot-paired pcap, and repeated every-boot WiFi acceptance remain hardware-gated and are not claimed by this closure.

### Scoped Decision

- Decision: `P2 exception closure PASS (offline engineering scope)`
- Decision date: `2026-07-16`
- Decision authority: `Independent code and commit-scope review plus clean exact-commit staged gates`
- Residual boundary: `No active P2 exception remains for DD16-DD18; Pi hardware acceptance, repeated-boot WiFi proof, and global release human sign-off are outside this scoped decision.`
