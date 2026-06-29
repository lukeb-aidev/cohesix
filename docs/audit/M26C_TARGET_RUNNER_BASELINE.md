<!-- Author: Lukas Bower -->
<!-- Purpose: Record Milestone 26c target-qualified staged-runner baseline evidence. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Target Runner Baseline

Status: `PASS-contract / QEMU-STAGE-01-05-PASS / PI4-STAGE-01-05-PASS`

## Contract

`scripts/ci/test_plan_run.sh` now accepts `--target qemu|pi4`, writes
`target.env`, passes `TEST_PLAN_TARGET` and `COHSH_BATCH_TARGET` into each
stage, and writes target-qualified stage markers only after a successful stage
and required artifact checks.

The runner also supports focused iteration through `--iteration` /
`TEST_PLAN_ITERATION=1`. Iteration writes `stage_XX.inputs.sha256`,
`stage_XX.iteration`, and `stage_XX.<target>.iteration`; it does not write
generic or target-qualified PASS markers. Later stages check stored input
fingerprints before reusing previous stage evidence when fingerprints exist.

Target-qualified PASS requires:

- `stage_01.done` through `stage_05.done`
- `stage_01.<target>.done` through `stage_05.<target>.done`
- `target.env`
- `stage_01.inputs.sha256` through `stage_05.inputs.sha256`
- no `stage_*.incomplete`
- no files under `incomplete/`

Pi 4 Stage 03 requires a live Pi TCP console host through `COHSH_TCP_HOST` or
`COHSH_HOST`. Pi 4 Stage 04 requires an existing REST gateway URL through
`COHESIX_GATEWAY_URL`, `HIVE_GATEWAY_URL`, `COHSH_REST_URL`, or `COH_REST_URL`.
Without those prerequisites the runner fails before stage execution so QEMU
evidence cannot be mistaken for Pi 4 proof.

Stage 03 subgroup selectors such as `COHSH_BATCH_GROUPS=base` are iteration-only
and write INCOMPLETE in final mode. Stage 05 may validate and reuse fresh Stage
03 regression evidence from the same state dir; standalone due diligence remains
exhaustive unless `DD_REUSE_REGRESSION_BATCH_FROM` is supplied explicitly.

## Evidence

| Command | Result |
| --- | --- |
| `bash -n scripts/ci/test_plan_run.sh scripts/ci/check_test_plan.sh scripts/ci/check_mermaid_github.sh scripts/ci/render_mermaid_github.sh` | PASS |
| `scripts/ci/test_plan_run.sh --list` | PASS |
| `scripts/ci/check_test_plan.sh` | PASS |
| Stage input fingerprint smoke (`tp_stage_input_fingerprint 1/3/5`) | PASS |
| Stage 03 subgroup without `--iteration` | PASS negative test; status 1, `stage_03.incomplete` written, no PASS marker |
| Invalid target negative test | PASS in runner handoff; status 2 and no stage marker |
| Pi 4 Stage 03 without host | PASS in runner handoff; status 2 and no stage marker |
| Pi 4 Stage 04 without gateway | PASS in runner handoff; status 2 and no stage marker |
| QEMU Stage 02 without Stage 01 | PASS in runner handoff; status 1 and no stage marker |
| QEMU Stage 01 smoke | PASS; `out/test-plan/m26c-runner-qemu-smoke/stage_01.done` and `stage_01.qemu.done` |
| Pi 4 Stage 01 smoke | PASS; `out/test-plan/m26c-runner-pi4-smoke/stage_01.done` and `stage_01.pi4.done` |
| QEMU full Stage 01-05 run | PASS; `out/test-plan/m26c-qemu/stage_01.done` through `stage_05.done`, matching `.qemu.done` markers, no incomplete markers, and Stage 05 due-diligence evidence at `out/audit/gate/20260628T015332Z` |
| Pi 4 full Stage 01-05 run | PASS; `out/test-plan/m26c-pi4-live/stage_01.done` through `stage_05.done`, matching `.pi4.done` markers, no incomplete markers, final GENET runtime/DMA proof `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env`, and Stage 05 due-diligence evidence at `out/audit/gate/20260629T061204Z` |

## Stage 01 Smoke Closure

The runner-lane QEMU Stage 01 smoke initially failed under shared Cargo artifact
lock contention and correctly withheld markers. The parent run reran both target
Stage 01 smoke checks after generated artifacts settled:

- `scripts/ci/test_plan_run.sh --target qemu --stage 1 --state-dir out/test-plan/m26c-runner-qemu-smoke` - PASS.
- `scripts/ci/test_plan_run.sh --target pi4 --stage 1 --state-dir out/test-plan/m26c-runner-pi4-smoke` - PASS.

This closed the Stage 01 smoke gap. The final closure run below records both
targets through Stage 05 after the Phase 2 blockers were closed.

## QEMU Stage 01-05 Closure

The parent run later executed the full target-qualified QEMU matrix:

- `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m26c-qemu` - PASS.
- `out/test-plan/m26c-qemu/target.env` records `TEST_PLAN_TARGET=qemu`.
- `out/test-plan/m26c-qemu` contains `stage_01.done` through `stage_05.done`
  and `stage_01.qemu.done` through `stage_05.qemu.done`.
- No `*.incomplete` markers or `incomplete/` artifacts were present in the
  QEMU state dir after the pass.
- Stage 05 due-diligence passed at `out/audit/gate/20260628T015332Z`.

This closes the QEMU side of `m26c-full-test-plan-qemu-and-pi4`.

## Pi 4 Stage 01-05 Closure

The final Pi 4 state dir now contains a target-qualified full pass. Stage 01
and Stage 02 markers were already present from the current Pi validation state;
the final request refreshed only Stage 03, Stage 04, and Stage 05 against the
same live GENET board state:

- `out/test-plan/m26c-pi4-live/target.env` records `TEST_PLAN_TARGET=pi4`.
- `out/test-plan/m26c-pi4-live` contains `stage_01.done` through
  `stage_05.done` and `stage_01.pi4.done` through `stage_05.pi4.done`.
- No `*.incomplete` markers or `incomplete/` artifacts were present after the
  Stage 05 pass.
- Stage 03 ran against `192.168.10.50:31337` with
  `PI4_RUNTIME_DMA_PROOF_FILE=out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env`.
- Stage 04 ran through `hive-gateway` at `http://127.0.0.1:48080` with request
  auth token `m26c-pi4-rest-token`.
- Stage 05 due diligence passed at `out/audit/gate/20260629T061204Z`.

This closes the Pi 4 side of `m26c-full-test-plan-qemu-and-pi4` without using
QEMU or older board evidence as a substitute.
