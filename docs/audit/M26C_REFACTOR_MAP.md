<!-- Author: Lukas Bower -->
<!-- Purpose: Define Milestone 26c refactor candidates, preserved contracts, and authorization state. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Refactor Map

Status: `BASELINE-COMPLETE / FUTURE-WAVES-DEFERRED`

Cleanup is authorized only where the preserved contract and targeted evidence
are named below. Broad host-tool, root-task, HAL, and Pi 4 structural refactors
remain deferred outside 26c until their own characterization evidence exists.

| Candidate Surface | Classification | Owner | Preserved Contracts | Required Baseline | State |
| --- | --- | --- | --- | --- | --- |
| Markdown/Mermaid inventory tooling | Low-risk audit tooling | docs-owner | Inventory must match `git ls-files '*.md'`; active Mermaid checker must not scan ignored outputs. | `M26C_MARKDOWN_INVENTORY.*`, `M26C_MERMAID_INVENTORY.csv` | Implemented |
| Target-qualified runner | Enabling gate | runner-owner | Existing QEMU defaults, no Pi/QEMU evidence blending, no incomplete markers in PASS. | `M26C_TARGET_RUNNER_BASELINE.md` | Implemented-contract |
| QEMU Worker helper cleanup | Low-risk cleanup | worker-owner | No protocol/grammar drift; helper loops stay bounded no_std; GPU/LoRA remain receipt-only; no live Worker task is claimed. | Worker helper tests and QEMU no_std build tree; target image load/resume remains reopened | Implemented-model-only |
| AI-fingerprint cleanup | Low-risk cleanup | docs-owner | No generated/release/vendored hand edits; no behavior or grammar drift. | Post-behavior baseline plus AI audit | QEMU-touched closed / broader deferred |
| Host tool structural cleanup | Characterization-first refactor | host-tools-owner | ACK/ERR/END, REST/TCP/FUSE behavior, ticket schemas, request auth. | Host tests and post-behavior baseline | Deferred |
| Root-task runtime decomposition | Boundary-sensitive refactor | root-task-owner | Console grammar, `/proc` shapes, Secure9P semantics, append-only logs, no-std closure. | NineDoor parity and no-std trees | Deferred |
| HAL/network/local-seat decomposition | Boundary-sensitive refactor | hal-owner | HAL-only authority, boot transcripts, netstats/netstatus, Pi proof lanes. | Runtime/DMA proof and Pi staged evidence | Deferred outside 26c |

## Revert Sizing Rule

Every future accepted wave must touch one owned surface, cite a preserved
contract list, link to characterization evidence, and have a targeted test
subset. Broad "cleanup" patches are not accepted by this map.
