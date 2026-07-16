<!-- Author: Lukas Bower -->
<!-- Purpose: Track Milestone 26c simplification targets and before/after evidence. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Simplicity Scorecard

Status: `COMPLETE / DOC-SUITE-PASS / BROADER-CLEANUP-DEFERRED`

This scorecard records cleanup backed by 26c characterization and staged-run
evidence. Broader host, root-task, and Pi 4 cleanup waves remain deferred
outside 26c until their own preserved-contract evidence is present.

| Surface | Before Evidence | Target | After Evidence | Status |
| --- | --- | --- | --- | --- |
| Audit tooling | Missing M26C inventory/check scripts | Reproducible inventory and active Mermaid check | `scripts/ci/markdown_inventory.py`, `scripts/ci/mermaid_inventory.py`, `scripts/ci/check_mermaid_github.sh`, `scripts/ci/render_mermaid_github.sh` | Done |
| Target runner | No `--target` contract | Target-qualified state dirs and markers | `scripts/ci/test_plan_run.sh --list`; `M26C_TARGET_RUNNER_BASELINE.md` | Done-contract |
| QEMU Worker helper surfaces | Placeholder wording and generic headers in touched Worker crates | Keep comments tied to bounded no_std helper/model invariants without claiming task execution | `apps/worker-heart/src/lib.rs`, `apps/worker-gpu/src/lib.rs`, `apps/worker-lora/src/lib.rs`; no Worker image load/resume path exists | Done-wording / runtime-reopened |
| AI-fingerprint docs/comments | `out/audit/m26c_ai_fingerprint_rg.txt` has 152 lines | Delete/rewrite generic authored text without touching generated/release/vendored surfaces | QEMU-touched worker wording cleaned; broader findings deferred | Partial-QEMU |
| Host tools | Refactor candidates not characterized | Collapse duplication only with tests | Pending accepted wave | Deferred |
| Root-task adapters | Large boundary-sensitive modules | Split only after parity/no-std gates | Pending accepted wave | Deferred |
| HAL/network/local-seat | Complex Pi proof lanes | Extract only after runtime/DMA and hardware evidence | Final Pi GENET proof exists; extraction remains outside 26c | Deferred |
| README-linked narrative suite | 11,227 lines across `README.md` and 16 focused narrative documents | Remove repetition while retaining generated mirrors, contracts, runbooks, proof boundaries, and enough foundational context for new readers | 5,492 lines; 5,735 fewer lines (51.1% reduction). The preservation review restored 858 lines of source-backed contracts after the first 4,634-line pass. | Done-preserved |
| Full comparable suite | 23,246 lines including the 12,019-line historical `BUILD_PLAN.md` ledger | Improve the active entry point and contracts without rewriting historical authorization records | 17,594 lines; 5,652 fewer lines (24.3% reduction); historical plan detail retained. | Done-preserved |
| Operator recipes and perimeter | Advanced operator material had no single owner; linked contribution, current-source, security, and toolchain guides contained stale or duplicated guidance | Add task-oriented depth without bloating the ordered walkthrough, and make every linked perimeter guide current | 1,312 lines across new `OPERATOR_RECIPES.md` and the four rewritten perimeter guides; each surface has one stated purpose and source-backed commands. | Done-additive |
| Suite diagrams | 18 diagrams across the active README-linked suite; 84 tracked blocks repository-wide | Keep diagrams only where they clarify an owned boundary or sequence | Ten diagram blocks across nine suite documents; 76 tracked blocks; all 76 rendered | Done |
| Contract ownership | Architecture, interface, protocol, role, driver, host, and operator explanations overlapped | One owning document per contract with explicit source maps and cross-references | Ownership tables in `README.md`; exact compiler-generated mirrors retained only where tests require them | Done |

The full-suite reduction is intentionally smaller because `BUILD_PLAN.md` is a
normative authorization and historical status ledger. The remediation changed
its current scope and stale product-facing wording but retained its audit
history instead of treating it as general narrative prose.

The preservation follow-up deliberately increased the first-pass line count.
That is not regression: the added material is the only source-backed copy of a
public contract or a runnable task recipe. Cross-references, generated snippets,
and the ownership map still prevent those contracts from spreading into
multiple narrative mirrors.
