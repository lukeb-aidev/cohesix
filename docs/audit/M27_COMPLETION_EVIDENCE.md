<!-- Author: Lukas Bower -->
<!-- Purpose: Bind owner-approved M27 completion to exact evidence while preserving unexecuted physical checks and residual risk. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Milestone 27 completion — 2026-09-14

Milestone 27 — Operator Utilities: Inspect, Trace, Bundle, Diff, Attest is
**Complete by explicit owner approval on the evidence below**. This record
closes `m27-owner-review-closure` and `m27-audit-ledger-refresh` in the
[Build Plan](../BUILD_PLAN.md#27), including the disclosed remaining Pi
evidence gaps. It does not establish full physical Pi qualification.

## Owner decision and scope

After receiving the remaining Pi checks and successful QEMU results, Lukas
Bower instructed:

> I approve the milestone being marked completed on this basis. Please update the build plan and status, commit and push to main.

The approval accepts the final-image Pi boot, live operator checks and Pi
Stages 03–05 as unexecuted for M27 completion. Those checks retain their
original status; no stage marker, target result or evidence source is changed.
The earlier failed physical attempts remain failures. Future physical claims
still require the applicable [Test Plan](../TEST_PLAN.md) evidence. This
decision neither activates downstream milestones nor grants a general waiver
for later source, target, release or audit acceptance.

The earlier instruction, “Consider DD30 and Rust review signed off”, remains
bound separately by [DD30_M27_APPROVAL.toml](DD30_M27_APPROVAL.toml). DD30
remains P1 / `ACCEPTED_RISK`; dynamic fault/wake remains `UNEXECUTED`; expiry
remains 2026-10-13. That approval binds the reviewed implementation at
`7c3b82abbaf938f82f958dc40886d24fcf1c9f01`. This completion record does not
amend the protected hashes, original release waiver or finding disposition.

The owner's earlier stock-Pi exclusion applies to positive signed-device
acceptance. Optional measurement-only mode remains non-attested, and required
device-bound authority fails closed when no issuer is available. Shared TPM2
signature verification passes independently signed offline fixtures; neither
those fixtures nor QEMU results prove a live signed hardware identity. Actual
isolated TPM/DICE issuance and sealed/derived ticket admission remain the
conditional reopened M26 device task in the Build Plan.

## Tested source and target identities

- Tested source: `b54bdd2fcff97fe35c5e3a61faa4d5a761b2a1ac`.
- Source inventory digest: `5c8a7b623078baee766d214a15771ea86e37b89b6535cb8cba9f1e8c65fa0fea`.
- QEMU base artifact ID: `bca8377a892fa0522dc2ab832aad2f6f4aa4449952f3d941a9a14ae87fd0836b`.
- QEMU gated artifact ID: `72319d68c405d284b40389388ef7582fb48a69bc57b3c9e1a6250c2ca7b3a8b7`.
- Pi image ID: `d484710a6dd358ea7c1540c166cbfb4d59e227f17e6b3881540bd9773ef57255`.
- Pi raw image SHA-256: `ec621af1f477f94f39a3f4e66e20badf40aa722d07339e47e79bbbe54c60d245`.

The Pi image was built and bound to its selected manifest but never booted.
The subsequent completion commit changes documentation and its generated
inventory metadata only; it does not
rebind these tested artifacts or stage records to a new source identity.
Artifact paths below are retained local evidence under ignored `out/`, not
files distributed by a clean Git checkout.

## Qualification results

| Evidence lane | Result | Boundary |
| --- | --- | --- |
| Common Stage 01 | PASS | Host contracts, applicable Rust/Python gates and generated consistency; reused by QEMU from the exact Pi common-stage record. |
| QEMU Stages 02–05 | PASS | Selected builds, complete TCP matrix, REST/Python smoke and final due diligence. Linux-only FUSE is not applicable on this macOS run. |
| QEMU live TCP and REST operator workflows | PASS | Bounded canonical traces, complete packs, repeat inspection, empty self-diff, byte-identical offline replay, secret-canary exclusion and fail-closed unsigned attestation. |
| Pi Stage 02 | PASS | Exact selected image build and source/manifest binding only. |
| Final-image Pi boot and live operator workflows | UNEXECUTED | Serial recovery did not reach the authenticated reboot handoff; the older `9a5eb97f1` image remained reachable over TCP. |
| Pi Stages 03–05 | NOT_RUN | No final-source physical TCP/REST or Pi governance chain is claimed. |
| Native Linux ARM64 host suite | PASS, retained | All eight host tools built; offline operator checks and Mac/Linux pack inspection parity passed at `9a5eb97f1`, with production-source continuity to `b54bdd2fc` recorded. |
| Human Rust review | APPROVED | Separate exact-source DD30/M27 approval. |
| DD30 dynamic fault/wake | UNEXECUTED | P1 / ACCEPTED_RISK under the unchanged approval and expiry. |
| Live positive signed-device proof | Not established | Stock Pi positive requirement excluded; offline cryptographic verification retains its fixture proof class. |

The unexecuted Pi sequence comprises a fresh exact-image boot and settling
record; live TCP/REST M27 capture and offline composition; the complete
17-script Stage 03 matrix across four independently booted groups; Stage 04's
four REST scripts and Python smoke; then Stage 05 against that exact Pi chain.
No full throughput rerun was required for the read-only M27 utility scope.

The complete host-tool suite, `tools/cohesix-py`, generated contracts and
performance consumers were reviewed during implementation. Applicable changes
and fixtures landed together; target/runtime interfaces and throughput
workloads remain unchanged by the final collection repairs. This closure
changes only documentation, milestone disposition and generated metadata for
the new documentation inventory entry.

## Retained evidence index

The local summary is `out/m27-attestation/M27_STATUS.json`; the append-only
command history is `out/m27-attestation/task-record.md`. The table binds the
completed stage and operator records by path and SHA-256. QEMU Stage 01 reuses
the common result in the Pi state directory.

| Record | Retained path | SHA-256 |
| --- | --- | --- |
| Pi stage 1 | `out/test-plan/m27-pi4-b54b/evidence/attempts/stage-01/20260914T025957.887717Z-15975-fbf3cda68259/stage.json` | `046d75912d4d7e914eaedef95d54bcc20cf6ebafa3a304e81787786ac61308a7` |
| Pi stage 2 | `out/test-plan/m27-pi4-b54b/evidence/attempts/stage-02/20260914T031334.170653Z-47182-e1c1e5a35730/stage.json` | `c4595c3a0799a1cf41726e6c1db31e2e547259426335aaf115071c0e949a20a9` |
| QEMU stage 2 | `out/test-plan/m27-qemu-b54b/evidence/attempts/stage-02/20260914T031720.614201Z-50008-9fa976e8b598/stage.json` | `d5a4a35c5e4cc9adc9d7d58a28f6a375253dca958c65ff2ebc067e40fe07e83f` |
| QEMU stage 3 | `out/test-plan/m27-qemu-b54b/evidence/attempts/stage-03/20260914T032003.058854Z-52099-d419a99672c3/stage.json` | `02b090d8a8a30c5710bfd62356d8d6e0fdb4aa87c62e522598d9fbaf98a0b1a9` |
| QEMU stage 4 | `out/test-plan/m27-qemu-b54b/evidence/attempts/stage-04/20260914T032848.493577Z-55094-fe9ba6063707/stage.json` | `fab61b843a5d5eda32103a7d5df274e277039b29f4eadf1ad6a9127d10ab942f` |
| QEMU stage 5 | `out/test-plan/m27-qemu-b54b/evidence/attempts/stage-05/20260914T032945.407626Z-56058-663512b60e8a/stage.json` | `838a496880e09ddcec40a917ce00cb978940a6dba0fb33ed431eb797fc24b997` |
| QEMU operator tcp | `out/m27-attestation/qemub54b-operator/tcp/result.json` | `ce4ea1a178b3ed0645b20531cf0cc0abb71adf4a2acfce63ca5ee6e2911b47ab` |
| QEMU operator rest | `out/m27-attestation/qemub54b-operator/rest/result.json` | `4b1b10c142fb57732f551dd0380c393cde46ad2b9c56d1329840966a2b05b79e` |

Additional evidence under `out/m27-attestation/`:

- `host-implementation-continuity-b54b.json`, `linux-arm64-build-9a5e.json`
  and `linux-operator-9a5e/result.json`: native host evidence and exact production
  continuity. The later Rust change is the test catalog's script hash only.
- `pi-serial-owner-release.json`, `pib54b-boot1/`, `pib54b-boot2/`,
  `pib54b-boot3/` and `pi-proven-direct/serial.log`: failed serial handoffs
  and the user-directed REBOOT.md retry. No visible serial holders remained;
  the direct attach received no ACK, so no reboot was issued. Ports were released.
- `pi-common-b54b.log`, `pi-target-build-b54b.log`,
  `qemu-common-b54b.log`, `qemu-target-build-b54b.log`, `qemu-tcp-b54b.log`,
  `qemu-rest-b54b.log`, `qemu-governance-b54b.log` and
  `qemu-operator-b54b.log`: completed command logs.

## Completion-change validation

Only the required documentation checks run for this closure:

- `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`:
  regenerate the documentation inventory and its dependent graph digest for
  this new record. Runtime manifests, generated code and policies remain identical.
- `scripts/check-generated.sh`, including `scripts/ci/check_test_plan.sh`:
  generated outputs, canonical test catalog, documentation projection and
  runner integrity.
- `git diff --check` and scoped link/header checks for the edited documents.
- Read-only SHA-256 verification of the retained evidence records above.

Completed common, QEMU and native suites are reused; no new Pi or full Rust
suite is run for this documentation-only approval record. The final command
results are retained in `out/m27-attestation/completion-checks/`.

Result: PASS. Generated consistency and its Test Plan checks pass after the
documentation inventory refresh; 13 added local links/anchors, six document
headers and eight retained evidence hashes/statuses/source bindings pass.
The generated delta is exactly one non-production documentation entry and
its dependent graph digest. The initial missing-inventory failure remains
retained alongside the successful rerun.
