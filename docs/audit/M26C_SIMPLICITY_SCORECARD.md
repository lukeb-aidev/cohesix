<!-- Author: Lukas Bower -->
<!-- Purpose: Track Milestone 26c simplification targets and before/after evidence. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Simplicity Scorecard

Status: `QEMU-TOUCHED-UPDATED / BROADER-CLEANUP-DEFERRED`

This scorecard records only cleanup backed by current QEMU characterization and
no_std evidence. Broader host, root-task, and Pi 4 cleanup waves remain deferred
until their own preserved-contract evidence is present.

| Surface | Before Evidence | Target | After Evidence | Status |
| --- | --- | --- | --- | --- |
| Audit tooling | Missing M26C inventory/check scripts | Reproducible inventory and active Mermaid check | `scripts/ci/markdown_inventory.py`, `scripts/ci/mermaid_inventory.py`, `scripts/ci/check_mermaid_github.sh`, `scripts/ci/render_mermaid_github.sh` | Done |
| Target runner | No `--target` contract | Target-qualified state dirs and markers | `scripts/ci/test_plan_run.sh --list`; `M26C_TARGET_RUNNER_BASELINE.md` | Done-contract |
| QEMU worker runtime | Placeholder worker-loop wording and generic headers in touched worker crate surfaces | Keep comments tied to bounded no_std loop and receipt-only invariants | `apps/worker-heart/src/lib.rs`, `apps/worker-gpu/src/lib.rs`, `apps/worker-lora/src/lib.rs` | Done-QEMU |
| AI-fingerprint docs/comments | `out/audit/m26c_ai_fingerprint_rg.txt` has 152 lines | Delete/rewrite generic authored text without touching generated/release/vendored surfaces | QEMU-touched worker wording cleaned; broader findings deferred | Partial-QEMU |
| Host tools | Refactor candidates not characterized | Collapse duplication only with tests | Pending accepted wave | Deferred |
| Root-task adapters | Large boundary-sensitive modules | Split only after parity/no-std gates | Pending accepted wave | Deferred |
| HAL/network/local-seat | Complex Pi proof lanes | Extract only after runtime/DMA and hardware evidence | Pending live Pi proof | Pi-blocked |
