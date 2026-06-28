<!-- Author: Lukas Bower -->
<!-- Purpose: Track Milestone 26c documentation and script drift findings. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Doc Drift Ledger

Status: `OPEN`

| ID | Surface | Drift | State | Evidence | Required Closure |
| --- | --- | --- | --- | --- | --- |
| M26C-DOC-001 | `docs/TEST_PLAN.md` and runner scripts | Test Plan lacked target-qualified runner semantics required by 26c. | Closed for runner / QEMU full; Pi full pending | `scripts/ci/check_test_plan.sh` PASS; QEMU/Pi Stage 01 smoke PASS; QEMU Stage 01-05 PASS in `out/test-plan/m26c-qemu`; Stage 05 due diligence PASS at `out/audit/gate/20260628T015332Z` | Keep Pi 4 Stage 01-05 closure under `m26c-full-test-plan-qemu-and-pi4`; do not treat the Pi stage-only build as hardware proof. |
| M26C-DOC-002 | Canonical Mermaid diagrams | Some canonical diagrams used raw HTML line breaks. | Closed for active docs | `scripts/ci/check_mermaid_github.sh` PASS active surfaces | Keep checker in CI/test-plan surface. |
| M26C-DOC-003 | Release Mermaid snapshots | Release-derived copies still contain raw HTML labels. | Open warning | 32 warnings from Mermaid checker | Fix only through release-cut flow or record as release-derived warning. |
| M26C-DOC-004 | Pi runtime/DMA proof wording | Docs now distinguish compiler-owned DMA profile truth, stage-only target-build provenance, normalizer proof classification, live proof bundles, and Pi Stage 05 proof-artifact enforcement. | Semantics implemented / Pi live proof open | `scripts/pi4-image-build.sh`; `scripts/pi4_trace_normalize.py`; `scripts/pi4_gate_proof.sh`; `scripts/ci/test_plan_run.sh`; Runtime/DMA explorer handoff | Run the final validation batch, then close only after live Pi evidence contains `PI4_RUNTIME_DMA_PROOF=fresh-pi`, `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`, and `DRIVER_TASK_DMA_BLOCKER=none`. |
| M26C-DOC-005 | Worker public docs vs implementation | Generated worker role state, no_std worker loops, endpoint badge validation, notification badge classes, and non-MCS scheduling evidence now exist for QEMU closure. | Closed-QEMU / full future cap-bundle not claimed | `cargo test -p coh-rtc worker_runtime`; `cargo test -p worker-heart -p worker-gpu -p worker-lora`; `cargo test -p root-task --test worker_authority`; `cargo check -p worker-heart -p worker-gpu -p worker-lora --target aarch64-unknown-none` | Keep future full cap-bundle and Pi runtime/DMA proof out of QEMU worker claims. |
| M26C-DOC-006 | AI-fingerprint wording | Generic comments and marketing phrasing remain in some authored surfaces; QEMU-touched worker crate wording was cleaned while generated/release/vendored surfaces were left alone. | Closed for QEMU-touched files / broader cleanup deferred | `apps/worker-heart/src/lib.rs`; `apps/worker-gpu/src/lib.rs`; `apps/worker-lora/src/lib.rs`; `out/audit/m26c_ai_fingerprint_rg.txt` | Address broader host/doc cleanup only through accepted Phase 4 waves with characterization evidence. |

## Notes

Generated snippets, release snapshots, vendored Markdown, seL4 mirrors, and
append-only audit evidence are not style-cleanup surfaces. They require their
source generator, release-cut, vendor import, accepted reference refresh, or
append-only evidence flow.
