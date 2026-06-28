<!-- Author: Lukas Bower -->
<!-- Purpose: Consolidate Milestone 26c multi-agent lane handoffs and status decisions. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Agent Handoffs

Status: `QEMU-IMPLEMENTED / PI4-HARDWARE-OPEN`

Milestone 26c requires multi-agent execution. This index records the lanes used
for the current run and whether their evidence is sufficient to advance the
milestone gate.

| Lane | Agent | Scope | Files Touched | Status | Handoff / Evidence |
| --- | --- | --- | --- | --- | --- |
| Runner | `019f08b4-39cf-7d51-9caa-9ff12cb09bb8` | Phase 0 target-qualified staged runner | `scripts/ci/test_plan_run.sh`, `scripts/ci/check_test_plan.sh`, `docs/TEST_PLAN.md`, `docs/audit/M26C_AGENT_RUNNER_HANDOFF.md` | PASS-contract / Stage 01 smoke closed | `docs/audit/M26C_AGENT_RUNNER_HANDOFF.md`; `docs/audit/M26C_TARGET_RUNNER_BASELINE.md` |
| Docs provenance | `019f08b4-5355-7a82-8ad4-8ee35e5ca0a6` | Phase 1 Markdown/Mermaid/provenance inspection | none by agent | FAIL-readiness | Findings incorporated into inventory, Mermaid audit, drift ledger, and blocker ledger |
| Runtime/DMA | `019f08b4-8733-7b82-9403-acc7b40b95d0`; `019f0d04-3c17-70b1-b199-ca797fd9cf03` | Pi 4 runtime/DMA proof and DMA protection profile inspection | none by agents | SEMANTICS-IMPLEMENTED / PI4-LIVE-OPEN | Findings drove `PI4_RUNTIME_DMA_PROOF`, proof bundle, and Stage 05 Pi proof-artifact gates |
| Worker/cap/lifecycle/MCS | `019f08b4-6c0e-7c92-9fe0-4ae2051a2ce8`; `019f0d04-f56b-7440-b0e7-0316a944a221` | Worker architecture, endpoint caps, notifications, MCS evidence inspection and stale-doc closure | `M26C_RUNTIME_BOUNDARY_AUDIT.md`, `M26C_NINEDOOR_PARITY_MATRIX.md`, `M26C_REFACTOR_OWNERSHIP.md` | PASS-QEMU / PI4-PROOF-OPEN | QEMU stale failures were reclassified; future cap-bundle and Pi proof remain open |
| Compiler DMA profile | parent run | `m26c-dma-protection-profile-truth` | `tools/coh-rtc/src/ir.rs`, `tools/coh-rtc/src/codegen/{rust.rs,docs.rs}`, `configs/root_task*.toml`, generated artifacts, docs/audit ledgers | PASS-profile / proof-gap | `cargo test -p coh-rtc --lib dma`; `scripts/check-generated.sh` |
| QEMU closure/fix pass | parent run + subagents | Fresh QEMU/Pi builds, QEMU Stage 01-05 closure, and QEMU defect repair | runtime, root-task, cohsh scripts/tests, test-plan scripts/docs, audit registers | PASS-QEMU / Pi hardware pending | `out/test-plan/m26c-qemu`; `out/audit/gate/20260628T015332Z`; `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml` |

## Planner Decision

QEMU Phase 2 behavior-changing work is implemented and the QEMU post-behavior
baseline is frozen. Broad Phase 4 cleanup remains constrained to QEMU-scoped
surfaces with named characterization. Pi 4 HAL/network/local-seat cleanup and
full 26c closure remain blocked until live runtime/DMA proof and target-qualified
Pi Stage 01-05 pass.

## Commands Observed In Parent Run

- `scripts/ci/check_test_plan.sh` - PASS.
- `scripts/ci/test_plan_run.sh --list` - PASS.
- `scripts/ci/check_mermaid_github.sh --markdown-list out/audit/m26c_markdown_inventory.txt` - PASS with 32 release-snapshot warnings.
- `scripts/ci/render_mermaid_github.sh --markdown-list out/audit/m26c_markdown_inventory.txt --out out/audit/m26c-mermaid-rendered` - PASS extraction; `mmdc` unavailable.
- `cargo test -p secure9p-codec` - PASS.
- `cargo test -p coh-rtc --lib dma` - PASS.
- `scripts/check-generated.sh` - PASS.
- `scripts/ci/test_plan_run.sh --target qemu --stage 1 --state-dir out/test-plan/m26c-runner-qemu-smoke` - PASS.
- `scripts/ci/test_plan_run.sh --target pi4 --stage 1 --state-dir out/test-plan/m26c-runner-pi4-smoke` - PASS.
- `scripts/cohesix-build-run.sh --clean --transport tcp --no-run --sel4-build "$PWD/seL4/SMP_build" --out-dir out/cohesix --profile release --root-task-features cohesix-dev --cargo-target aarch64-unknown-none` - PASS fresh QEMU build.
- `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml` - PASS fresh Pi 4 stage-only build.
- `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m26c-qemu --stage 1` through `--stage 5` - PASS; full rerun also reported completed stages 5.
- `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m26c-qemu --stage 5` - PASS after ratchet/exception repair; due-diligence log root `out/audit/gate/20260628T015332Z`.
- Current edit batch adds `DRIVER_TASK_DMA_PROOF`, `PI4_RUNTIME_DMA_PROOF`, runtime/DMA proof bundles, and Pi Stage 05 proof-artifact enforcement; final validation is intentionally batched after edits.

## Residual Gaps

- Full Pi 4 Stage 01-05 closure remains pending live target prerequisites and
  cannot be replaced by the stage-only Pi image build.
- Live Pi proof must include `PI4_RUNTIME_DMA_PROOF=fresh-pi`,
  `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`, and
  `DRIVER_TASK_DMA_BLOCKER=none`.
