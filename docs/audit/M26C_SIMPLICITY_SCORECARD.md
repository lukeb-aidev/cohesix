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
| QEMU worker runtime | Placeholder worker-loop wording and generic headers in touched worker crate surfaces | Keep comments tied to bounded no_std loop and receipt-only invariants | `apps/worker-heart/src/lib.rs`, `apps/worker-gpu/src/lib.rs`, `apps/worker-lora/src/lib.rs` | Done-QEMU |
| AI-fingerprint docs/comments | `out/audit/m26c_ai_fingerprint_rg.txt` has 152 lines | Delete/rewrite generic authored text without touching generated/release/vendored surfaces | QEMU-touched worker wording cleaned; broader findings deferred | Partial-QEMU |
| Host tools | Refactor candidates not characterized | Collapse duplication only with tests | Pending accepted wave | Deferred |
| Root-task adapters | Large boundary-sensitive modules | Split only after parity/no-std gates | Pending accepted wave | Deferred |
| HAL/network/local-seat | Complex Pi proof lanes | Extract only after runtime/DMA and hardware evidence | Final Pi GENET proof exists; extraction remains outside 26c | Deferred |
| README-linked narrative suite | 11,227 lines across `README.md` and 16 focused narrative documents | Remove repetition while retaining generated mirrors, contracts, runbooks, proof boundaries, and enough foundational context for new readers | 4,634 lines; 6,593 fewer lines (58.7% reduction) | Done |
| Full README-linked suite | 23,246 lines including the 12,019-line historical `BUILD_PLAN.md` ledger | Improve the active entry point and contracts without rewriting historical authorization records | 16,730 lines; 6,516 fewer lines (28.0% reduction); historical plan detail retained | Done |
| Suite diagrams | 18 diagrams across the active README-linked suite; 84 tracked blocks repository-wide | Keep one diagram only where it clarifies an owned boundary or sequence | Nine suite diagrams; 75 tracked blocks; all 75 rendered | Done |
| Contract ownership | Architecture, interface, protocol, role, driver, host, and operator explanations overlapped | One owning document per contract with explicit source maps and cross-references | Ownership tables in `README.md`; exact compiler-generated mirrors retained only where tests require them | Done |

The full-suite reduction is intentionally smaller because `BUILD_PLAN.md` is a
normative authorization and historical status ledger. The remediation changed
its current scope and stale product-facing wording but retained its audit
history instead of treating it as general narrative prose.
