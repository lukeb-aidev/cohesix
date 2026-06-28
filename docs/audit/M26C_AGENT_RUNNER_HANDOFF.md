<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Record the Milestone 26c target-qualified Test Plan runner lane handoff. -->
<!-- Author: Lukas Bower -->

# M26C Agent Runner Handoff

Title/ID: `m26c-target-qualified-runner-baseline`

Goal: Make target-qualified staged-run semantics real before 26c cleanup or structural decomposition depends on them.

Inputs:
- `AGENTS.md`
- `docs/BUILD_PLAN.md` Milestone 26c, especially `m26c-target-qualified-runner-baseline`
- `docs/TEST_PLAN.md`
- `scripts/ci/test_plan_run.sh`
- `scripts/ci/check_test_plan.sh`
- `scripts/ci/test_plan_stage_*.sh`
- `scripts/cohsh/run_regression_batch.sh`
- `scripts/cohsh/REST_regression_batch.sh`

Files touched:
- `scripts/ci/test_plan_run.sh` - added `--target qemu|pi4`, target metadata, stage preflight rules, target-qualified markers, target-specific artifact checks, and final target PASS validation.
- `scripts/ci/check_test_plan.sh` - added runner-contract validation for target-qualified PASS, metadata, marker, and Pi 4 prerequisite language.
- `docs/TEST_PLAN.md` - documented target-qualified PASS semantics, target matrix, state-dir metadata, target markers, and Pi 4 Stage 03/04 prerequisites.
- `docs/audit/M26C_AGENT_RUNNER_HANDOFF.md` - this lane handoff.

Contract implemented:
- The runner accepts `--target qemu|pi4`; legacy invocations default to `qemu`.
- The runner writes `target.env` into the shared state dir and refuses to reuse a state dir for a different target.
- Every stage receives `TEST_PLAN_TARGET=<target>` and `COHSH_BATCH_TARGET=<target>`.
- The runner writes `stage_XX.<target>.done` only after the stage succeeds and required target artifacts exist.
- Full target-qualified PASS requires generic `stage_01.done` through `stage_05.done`, target-qualified `stage_01.<target>.done` through `stage_05.<target>.done`, `target.env`, and no `stage_*.incomplete` or `incomplete/` records.
- Pi 4 Stage 03 fails before the stage starts unless `COHSH_TCP_HOST` or `COHSH_HOST` is set to a non-loopback live Pi 4 TCP console, unless `TP_PI4_ALLOW_LOOPBACK=1` explicitly documents a local tunnel.
- Pi 4 Stage 04 fails before the stage starts unless an existing REST gateway URL is supplied through `COHESIX_GATEWAY_URL`, `HIVE_GATEWAY_URL`, `COHSH_REST_URL`, or `COH_REST_URL`.

Commands and results:
- `bash -n scripts/ci/test_plan_run.sh` - PASS.
- `bash -n scripts/ci/check_test_plan.sh` - PASS.
- `scripts/ci/check_test_plan.sh` - PASS (`test plan integrity checks ok`).
- `scripts/ci/test_plan_run.sh --list` - PASS; printed stages 1-5 plus qemu/pi4 target matrix and target metadata markers.
- `scripts/ci/test_plan_run.sh --target bogus --stage 1 --state-dir out/test-plan/m26c-runner-bogus` - expected FAIL status 2; invalid target rejected before state evidence.
- `scripts/ci/test_plan_run.sh --target pi4 --stage 4 --state-dir out/test-plan/m26c-runner-pi4-stage4-no-gateway` - expected FAIL status 2; no `stage_04*` marker produced.
- `scripts/ci/test_plan_run.sh --target pi4 --stage 3 --state-dir out/test-plan/m26c-runner-pi4-stage3-no-host` - expected FAIL status 2; no `stage_03*` marker produced.
- `scripts/ci/test_plan_run.sh --target qemu --stage 2 --state-dir out/test-plan/m26c-runner-qemu-stage2-no-stage1` - expected FAIL status 1; no `stage_02*` marker produced.
- `scripts/ci/test_plan_run.sh --target qemu --stage 1 --state-dir out/test-plan/m26c-runner-qemu-smoke` - FAIL in `generated-artifacts`: `scripts/check-generated.sh` reached `cargo run -p coh-rtc` after waiting on the Cargo artifact lock, then the cargo process was terminated with signal 15. The updated `test-plan-hash-check` step passed first, and the runner withheld both `stage_01.done` and `stage_01.qemu.done`.
- `git diff --check -- scripts/ci/test_plan_run.sh scripts/ci/check_test_plan.sh docs/TEST_PLAN.md` - PASS.
- `git diff --no-index --check /dev/null docs/audit/M26C_AGENT_RUNNER_HANDOFF.md` - PASS for whitespace (expected diff exit 1 because the file is new).

Parent follow-up after Cargo artifact contention cleared:
- `scripts/ci/test_plan_run.sh --target qemu --stage 1 --state-dir out/test-plan/m26c-runner-qemu-smoke` - PASS; wrote `stage_01.done` and `stage_01.qemu.done`.
- `scripts/ci/test_plan_run.sh --target pi4 --stage 1 --state-dir out/test-plan/m26c-runner-pi4-smoke` - PASS; wrote `stage_01.done` and `stage_01.pi4.done`.

Artifact paths:
- `out/test-plan/m26c-runner-qemu-smoke/target.env`
- `out/test-plan/m26c-runner-qemu-smoke/logs/stage-01-integrity.log`
- `out/test-plan/m26c-runner-pi4-smoke/target.env`
- `out/test-plan/m26c-runner-pi4-smoke/logs/stage-01-integrity.log`
- `out/test-plan/m26c-runner-pi4-stage3-no-host/target.env`
- `out/test-plan/m26c-runner-pi4-stage4-no-gateway/target.env`
- `out/test-plan/m26c-runner-qemu-stage2-no-stage1/target.env`

Blockers and residual gaps:
- The initial QEMU Stage 01 smoke failure was caused by shared Cargo artifact contention and produced no target-qualified PASS evidence.
- Parent follow-up reran QEMU and Pi 4 Stage 01 after the contention cleared; both runs passed and wrote target-qualified markers.
- Stage scripts were intentionally left unchanged for this lane. Target qualification is enforced by the runner wrapper, and stages receive `TEST_PLAN_TARGET` and `COHSH_BATCH_TARGET` through the environment.
- Pi 4 Stage 03/04 remain preflight-gated on live Pi TCP/gateway prerequisites and cannot be replaced by QEMU evidence.
- Out-of-lane worktree changes observed and intentionally skipped: `docs/HOST_TOOLS.md`, `docs/NETWORK_CONFIG.md`, `docs/USE_CASES.md`, `docs/audit/M26C_MARKDOWN_INVENTORY.csv`, `docs/audit/M26C_MARKDOWN_INVENTORY.md`, `docs/audit/M26C_MERMAID_INVENTORY.csv`, `scripts/ci/check_mermaid_github.sh`, `scripts/ci/markdown_inventory.py`, `scripts/ci/mermaid_inventory.py`, and `scripts/ci/render_mermaid_github.sh`.

Lane status: PASS for the target-qualified runner/check/docs contract and Stage 01 target smoke evidence.
