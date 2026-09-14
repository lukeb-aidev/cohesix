<!-- Author: Lukas Bower -->
<!-- Purpose: Track release-blocking due-diligence findings, dispositions, and closure requirements. -->
<!-- Copyright 2026 Lukas Bower -->

# Due Diligence Blockers

## Current release candidate (2026-09-13)

The finding-level blockers are resolved for explicitly selected release
`1.0.0-beta`: DD26–29 are `CLOSED_VERIFIED`, and DD30 remains `P1` /
`ACCEPTED_RISK` under Lukas Bower's release-specific decision. The
[current audit report](AUDIT_REPORT_2026-09-13.md) binds the evidence and
[DD30 waiver](DD30_RELEASE_WAIVER.toml). Final Stage 05 acceptance is determined
by the dedicated release acceptance entry, with the explicitly approved
carry-forward and residual risk recorded.
No target fault test is represented as passed by that decision.

| Finding | Disposition and evidence | Remaining boundary |
| --- | --- | --- |
| `DD-2026-0026` | Independently verified TLS storage/ABI repair and exact `8c050a0aa` image layout. | Historical physical-fault attribution remains unproved; later selected images retain their own layout obligation. |
| `DD-2026-0027` | Actual `61f7bcd1f` ELF matches all eight acquired integrity cuts; raw console work and independent review pass. | Scoped publication/address defect closure. |
| `DD-2026-0028` | Both exact `61f7bcd1f` RAM lanes retain ordered reset/waits/firmware completion and pass network work. WiFi also passes authenticated CAT/QUIT/EOF and serial liveness. | RAM-transfer timing does not establish ordinary boot timing, SD delivery or repeatability. |
| `DD-2026-0029` | Independently reviewed firmware/USB repair, machine-observed presence and Lukas Bower's explicitly confirmed keyboard-absence test. | Absence is human-attested with unspecified image/time/log; no repeat is required. |
| `DD-2026-0030` | Source/ABI/pure/emitted IPC repair review passes; Lukas Bower accepts the remaining dynamic fault/wake gap under `EX-2026-0030`. | Dynamic fault/wake testing remains unexecuted. Only matching `DD_RELEASE_ID=1.0.0-beta`, active approval and original or explicitly approved successor file hashes admit this residual risk through 2026-10-13. |

The original runtime fixes are human-approved and published through `719043fad`;
the later host-only repair is independently reviewed and explicitly approved.
The final acceptance record binds its published successor commit.
The Pi's fresh WiFi boot uses the same actual `61f7bcd1f` image as the completed
GENET burn-in. Completed burn-in and historical stage results remain preserved.
The earlier Stage 01–04 chain is bound to `22e3d08ff`; the canonical verifier
rejects ordinary reuse for the later approved source. Lukas Bower explicitly
approved release-only carry-forward of those original records plus later scoped
fix evidence under [the bound policy](RELEASE_1_0_0_BETA_CARRY_FORWARD.toml).
No old marker is copied or relabelled. The failed fresh attempt is preserved;
its host-only repair passed all 2388 affected Pi feature tests. Completed suites
and burn-in are not repeated.

The DD30 owner decision is the sole change to the earlier P1 acceptance rule;
the subsequent owner-approved carry-forward is the sole staged provenance exception.
Other findings, severity, performance thresholds, target/source provenance and
release-delivery requirements remain binding. The frozen-source canonical
release Stage 05 artifact records the final execution verdict separately from this
pre-execution findings register.

## Milestone 27 review (2026-09-14)

Schema 1.20 adds the compiler-owned live trace duration bound and refreshes
host projections. Current pack-v1/timeline-v1 artifacts retain compatibility;
case-v1 and attestation-result-v1 remain non-authoritative host records.
`cargo test -p tests --test audit_ledgers` checks active exception/finding
cross-references and production risk partition arithmetic. The existing
findings, exception approvals, and risk ceilings were reviewed and remain
unchanged; this change grants no new risk exception.

The missing signed-evidence wire/trust contract is now implemented by
`cohesix-attestation` and documented in [ATTESTATION.md](../ATTESTATION.md).
The verifier accepts independently signed offline fixtures, rejects replay,
wrong measurements/keys and malformed evidence, and preserves source/proof
class. Root no longer labels public configuration hashes as TPM/DICE evidence.
The owner excludes the stock Pi from positive signed-device acceptance;
optional measurement-only mode cannot attest production ticket keys. Actual
isolated TPM/DICE issuance and sealed/derived ticket admission remain the
reopened M26 device task. M27 target/operator validation is still pending.

The four stale release test assertions were corrected to their existing
argument-driven and exact tracked-tree archive contracts. The archive test
now exercises export-ignore handling while proving untracked secrets stay
excluded and the source attributes remain unchanged. Those tests and the
previously failing Pi compiler-provenance check pass. The pinned GNU compiler,
seL4 Python environment, mkimage and a pristine canonical QEMU production build
are restored. The previous five-failure staged attempt remains historical under
`out/test-plan/m27-live-trace-qemu`; new qualification uses a fresh state root.
No historical marker or risk ceiling has been changed. DD30's release-specific
waiver does not grant M27 acceptance.

The exact `20c07df72` continuation passes QEMU Stages 01 and 02, including
2,454 Python tests and 138 subtests (one skip). Stage 03 built both variants but
Homebrew QEMU 11.0.3 aborted in HVF initialization before seL4 started. The
validated external QEMU 10.1.0 executable retains its recorded hash and valid
signature and passes the canonical four-core startup smoke; fresh target
acceptance remains required. Pi Stage 02 now invokes the canonical Pi image
builder and verifies its retained selected-manifest/source/image binding;
124 focused workflow/catalog/image tests pass. The initial passive/empty-line
Pi serial observations contained no bytes; the retained WiFi address then
accepted authenticated reads, and UART `help` returned the root prompt.
That discovery proves access to the older schema-1.18 running image, not a
fresh M27 image boot.

The subsequent clean `5cb112d24` run passes the complete Stage 01 suite and Pi
Stage 02. Its image `558f2ea36a007ea72f0f07c2f2cfba99c096bafff3fe66533c84b8757bcbca63`
passed a fresh GENET RAM boot with exact BUILD, pre/post-reset artifact CRCs and
the passive settling interval. No SD write or packet-capture proof is claimed.
QEMU Stages 01–04 pass with the validated 10.1.0 binary. Stage 05 fails solely
at the DD30 register guard: the truthful attestation log change alters the
protected `kernel.rs` hash. The original waiver is unchanged; M27 has no
accepted-risk extension. The native Linux ARM64 build of all eight host tools
also passes for that exact source.

Fresh Pi collection found two host defects: reading the lease-ID directory as
a file and probing an unadvertised optional spool root. The correction uses
the bounded `/proc` inventory, preserves empty-directory semantics, and stores
nested listings in separate `.listing` leaves. Its focused tests pass, and a
separately identified candidate host binary passes TCP and REST pack/inspect/
diff/replay composition against that retained Pi boot and a fresh QEMU boot
of its sealed default artifact. This candidate evidence
does not replace a complete staged chain for the final corrected source.

## Historical gate snapshots

- Exact `2be878d8d`: QEMU Stages 01–04 PASS, Stage 05 FAIL on then-open DD26–29;
  `out/release-qualification/1.0.0-beta-8bc556850-20260909/canonical-2be878d8d-01/frozen-candidate`.
- February baseline: `PASS`, `out/audit/gate/20260214T044955Z`.
- M26d P2 exception closure: `PASS` for offline engineering scope,
  `out/test-plan/m26d-unsafe-remediation-qemu` and
  `out/test-plan/m26d-unsafe-remediation-pi4`.
- Historical blocking rule: any P0/P1 finding outside `CLOSED_VERIFIED` blocked release. The current DD30-only owner waiver is documented above.

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
A verified blocker may be removed only when:
- finding disposition is updated to `CLOSED_VERIFIED` in `docs/audit/findings.csv`,
- closure evidence includes reproducible command/log path and commit SHA,
- an independent reviewer records verification in `docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md`.

The release-specific DD30 acceptance follows the separate explicit owner-waiver
contract in [EXCEPTIONS.md](EXCEPTIONS.md); it does not change the finding to
`CLOSED_VERIFIED` or supply missing target execution.
