<!-- Author: Lukas Bower -->
<!-- Purpose: Track Milestone 26c as-built blockers, owners, evidence, and closure state. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C As-Built Blockers

Status: `QEMU-IMPLEMENTED / PI4-HARDWARE-OPEN`

Milestone 26c QEMU implementation gaps are closed for the worker/capability,
notification, non-MCS scheduling, and QEMU validation lanes. Full milestone
closure still requires live Pi 4 Stage 01-05 evidence and a fresh Pi
runtime/DMA proof bundle. Deferred items must not be cited as satisfied
evidence.

## Current Gate Summary

| Gate | Status | Evidence |
| --- | --- | --- |
| Target-qualified runner contract | PASS | `scripts/ci/test_plan_run.sh --list`; `scripts/ci/check_test_plan.sh`; `docs/audit/M26C_AGENT_RUNNER_HANDOFF.md` |
| Markdown inventory | PASS | `docs/audit/M26C_MARKDOWN_INVENTORY.csv`; diff against `git ls-files '*.md'` passed |
| Active Mermaid compatibility | PASS with release warnings | `scripts/ci/check_mermaid_github.sh --markdown-list out/audit/m26c_markdown_inventory.txt` |
| Secure9P codec blocker probe | PASS | `cargo test -p secure9p-codec` |
| DMA protection profile truth | PASS | `cargo test -p coh-rtc --lib dma`; `scripts/check-generated.sh` |
| Runtime/DMA proof closure | SEMANTICS-IMPLEMENTED / PI4-LIVE-OPEN | `scripts/pi4_trace_normalize.py` emits `PI4_RUNTIME_DMA_PROOF`; `scripts/pi4_gate_proof.sh --require-driver-task-proof` requires `fresh-pi` and `counter-qualified`; Stage 05 Pi requires a proof artifact |
| Worker/cap/notification/MCS implementation | PASS-QEMU | Worker loops, generated endpoint badges, notification badges, and non-MCS scheduling evidence are implemented for QEMU closure |
| Post-behavior baseline freeze | QEMU-FROZEN / PI4-OPEN | QEMU post-behavior baseline can be frozen; Pi live runtime/DMA and target-qualified Stage 01-05 remain open |
| Full QEMU staged Test Plan | PASS | `out/test-plan/m26c-qemu` has Stage 01-05 `.done` and `.qemu.done` markers with no incomplete markers; Stage 05 evidence `out/audit/gate/20260628T015332Z` |
| Full Pi 4 staged Test Plan | BLOCKED | Fresh Pi 4 image build passed, but target-qualified Pi 4 Stage 01-05 hardware evidence is not present |

## Blocking Items

| ID | Severity | Owner | State | Evidence Command / Source | Dependency Impact | Closure Requirement |
| --- | --- | --- | --- | --- | --- | --- |
| M26C-BLOCK-001 | P0 | runner-owner | closed | `scripts/ci/test_plan_run.sh --list`; `scripts/ci/check_test_plan.sh`; `scripts/ci/test_plan_run.sh --target qemu --stage 1 --state-dir out/test-plan/m26c-runner-qemu-smoke`; `scripts/ci/test_plan_run.sh --target pi4 --stage 1 --state-dir out/test-plan/m26c-runner-pi4-smoke` | Target-qualified runner contract and Stage 01 markers exist for QEMU and Pi 4. | Full Stage 01-05 target runs remain part of M26C-BLOCK-010 after Phase 2 blockers close. |
| M26C-BLOCK-002 | P0 | runtime-dma-owner | open / semantics implemented | `scripts/pi4_trace_normalize.py`; `scripts/pi4_gate_proof.sh`; `scripts/pi4-image-build.sh`; `scripts/ci/test_plan_run.sh`; Runtime/DMA explorer handoff | 26c now has target-build and live-proof semantics, but no fresh live Pi hardware proof bundle is present. | Run live Pi proof with `PI4_RUNTIME_DMA_PROOF=fresh-pi`, `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`, `DRIVER_TASK_DMA_BLOCKER=none`, and target-qualified Pi Stage 01-05. |
| M26C-BLOCK-003 | P0 | compiler-owner | closed | `cargo test -p coh-rtc --lib dma`; `scripts/check-generated.sh`; `rg -n "DmaProtectionProfile|bounded-no-iommu|smmu" tools/coh-rtc/src configs apps/root-task/src/generated docs/snippets/root_task_manifest.md` | Compiler now owns `dma.protection_profile`, but this does not close runtime proof or malicious-device DMA confinement. | Keep `none` for virt profiles, `bounded-no-iommu` for Pi-family profiles, and reject SMMU profiles until generated per-device DMA-domain state exists. |
| M26C-BLOCK-004 | P0 | worker-owner | closed-QEMU | `cargo test -p worker-heart -p worker-gpu -p worker-lora`; QEMU worker-runtime code | Implemented heartbeat/GPU/LoRA worker loops replace placeholder-only QEMU behavior. | Keep worker-bus deferred and do not cite this as Pi runtime/DMA proof. |
| M26C-BLOCK-005 | P0 | capability-owner | closed-QEMU | `cargo test -p root-task --test worker_authority`; `cargo test -p coh-rtc worker_runtime` | Generated endpoint-badge authority is enforced for implemented roles. | Future full cap-bundle authority remains out of 26c QEMU scope. |
| M26C-BLOCK-006 | P0 | lifecycle-owner | closed-QEMU | Worker loop tests; `worker_authority` notification badge checks | Generated notification badges and worker-loop lifecycle events exist for QEMU. | Live Pi notification evidence and future full cap-bundle isolation are not claimed. |
| M26C-BLOCK-007 | P0 | scheduling-owner | closed-QEMU | `cargo test -p coh-rtc worker_runtime`; `cargo test -p root-task --test worker_authority` | Generated non-MCS scheduling evidence is profile-qualified and MCS claims are rejected on non-MCS profiles. | Consumed MCS budget evidence remains future/profile-specific. |
| M26C-BLOCK-008 | P1 | docs-owner | open | `scripts/ci/check_mermaid_github.sh --markdown-list out/audit/m26c_markdown_inventory.txt` warnings | Release snapshot diagrams retain raw HTML labels, but release snapshots are update-by-release-flow only. | Fix through release-cut flow or keep recorded as release-derived warning; do not hand-edit snapshots for style. |
| M26C-BLOCK-009 | P1 | docs-owner | open | `out/audit/m26c_ai_fingerprint_rg.txt` | AI-fingerprint audit has findings, including generic file-purpose headers and "world-class" wording. | Classify each finding as generated, accepted-specific, rewrite, delete, release-derived, vendored, or deferred before cleanup. |
| M26C-BLOCK-010 | P1 | validation-owner | open / QEMU closed | QEMU Stage 01-05 PASS in `out/test-plan/m26c-qemu`; Stage 05 due diligence PASS at `out/audit/gate/20260628T015332Z`; fresh Pi 4 stage-only image build PASS | QEMU validation has accepted evidence; Pi 4 target-qualified Stage 01-05 and fresh runtime/DMA hardware proof remain open. | Run one final batched QEMU validation after this edit batch, then run Pi 4 Stage 01-05 with live target evidence and `PI4_RUNTIME_DMA_PROOF_FILE`. |

## Non-Blocking Context

- QEMU port `31339` appears in documented QEMU self-test hostfwd paths and in
  `apps/root-task/src/net/stack.rs`; no hidden alternate in-VM service was
  established by this audit.
- HAL/MMIO searches still show legacy QEMU virtual drivers and physical serial
  code. These require the Phase 1 runtime-boundary and Phase 3 no-std/HAL gates
  before any structural cleanup.
- Fresh QEMU build and QEMU Stage 01-05 close the QEMU validation lane only.
  They do not satisfy Pi 4 runtime/DMA or fresh hardware proof blockers.
