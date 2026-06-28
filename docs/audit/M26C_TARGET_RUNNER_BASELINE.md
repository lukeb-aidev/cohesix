<!-- Author: Lukas Bower -->
<!-- Purpose: Record Milestone 26c target-qualified staged-runner baseline evidence. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Target Runner Baseline

Status: `PASS-contract / QEMU-STAGE-01-05-PASS / PI4-STAGE-01-SMOKE-PASS`

## Contract

`scripts/ci/test_plan_run.sh` now accepts `--target qemu|pi4`, writes
`target.env`, passes `TEST_PLAN_TARGET` and `COHSH_BATCH_TARGET` into each
stage, and writes target-qualified stage markers only after a successful stage
and required artifact checks.

Target-qualified PASS requires:

- `stage_01.done` through `stage_05.done`
- `stage_01.<target>.done` through `stage_05.<target>.done`
- `target.env`
- no `stage_*.incomplete`
- no files under `incomplete/`

Pi 4 Stage 03 requires a live Pi TCP console host through `COHSH_TCP_HOST` or
`COHSH_HOST`. Pi 4 Stage 04 requires an existing REST gateway URL through
`COHESIX_GATEWAY_URL`, `HIVE_GATEWAY_URL`, `COHSH_REST_URL`, or `COH_REST_URL`.
Without those prerequisites the runner fails before stage execution so QEMU
evidence cannot be mistaken for Pi 4 proof.

## Evidence

| Command | Result |
| --- | --- |
| `bash -n scripts/ci/test_plan_run.sh scripts/ci/check_test_plan.sh scripts/ci/check_mermaid_github.sh scripts/ci/render_mermaid_github.sh` | PASS |
| `scripts/ci/test_plan_run.sh --list` | PASS |
| `scripts/ci/check_test_plan.sh` | PASS |
| Invalid target negative test | PASS in runner handoff; status 2 and no stage marker |
| Pi 4 Stage 03 without host | PASS in runner handoff; status 2 and no stage marker |
| Pi 4 Stage 04 without gateway | PASS in runner handoff; status 2 and no stage marker |
| QEMU Stage 02 without Stage 01 | PASS in runner handoff; status 1 and no stage marker |
| QEMU Stage 01 smoke | PASS; `out/test-plan/m26c-runner-qemu-smoke/stage_01.done` and `stage_01.qemu.done` |
| Pi 4 Stage 01 smoke | PASS; `out/test-plan/m26c-runner-pi4-smoke/stage_01.done` and `stage_01.pi4.done` |
| QEMU full Stage 01-05 run | PASS; `out/test-plan/m26c-qemu/stage_01.done` through `stage_05.done`, matching `.qemu.done` markers, no incomplete markers, and Stage 05 due-diligence evidence at `out/audit/gate/20260628T015332Z` |

## Stage 01 Smoke Closure

The runner-lane QEMU Stage 01 smoke initially failed under shared Cargo artifact
lock contention and correctly withheld markers. The parent run reran both target
Stage 01 smoke checks after generated artifacts settled:

- `scripts/ci/test_plan_run.sh --target qemu --stage 1 --state-dir out/test-plan/m26c-runner-qemu-smoke` - PASS.
- `scripts/ci/test_plan_run.sh --target pi4 --stage 1 --state-dir out/test-plan/m26c-runner-pi4-smoke` - PASS.

This closes the Stage 01 smoke gap only. Full 26c closure still requires both
targets to pass through Stage 05 after Phase 2 blockers are closed.

## QEMU Stage 01-05 Closure

The parent run later executed the full target-qualified QEMU matrix:

- `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m26c-qemu` - PASS.
- `out/test-plan/m26c-qemu/target.env` records `TEST_PLAN_TARGET=qemu`.
- `out/test-plan/m26c-qemu` contains `stage_01.done` through `stage_05.done`
  and `stage_01.qemu.done` through `stage_05.qemu.done`.
- No `*.incomplete` markers or `incomplete/` artifacts were present in the
  QEMU state dir after the pass.
- Stage 05 due-diligence passed at `out/audit/gate/20260628T015332Z`.

This closes the QEMU side of `m26c-full-test-plan-qemu-and-pi4`. Pi 4 full
Stage 01-05 closure remains separate and still requires target-qualified Pi
hardware evidence.
