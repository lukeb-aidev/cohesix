<!-- Author: Lukas Bower -->
<!-- Purpose: Audit Milestone 26c canonical documentation against generated and observed as-built truth. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Docs-As-Built Audit

Status: `COMPLETE / README-LINKED-SUITE-REMEDIATED / AS-BUILT-ALIGNED`

## Audited Inputs

- `docs/BUILD_PLAN.md` Milestone 26c
- `docs/TEST_PLAN.md`
- `docs/HARDWARE_BRINGUP.md`
- `docs/ARCHITECTURE.md`
- `docs/INTERFACES.md`
- `docs/SECURITY.md`
- `docs/WORKER_TICKETS.md`
- `docs/ROLES_AND_SCHEDULING.md`
- `configs/root_task.toml`
- `configs/root_task_pi4_uboot_aarch64.toml`
- `apps/root-task/src/generated/mod.rs`
- `tools/coh-rtc/src/ir.rs`
- `scripts/ci/test_plan_run.sh`
- `scripts/ci/check_test_plan.sh`
- `README.md` and all 17 directly linked canonical Markdown documents
- `docs/HOST_API.md` and `resources/openapi/hive-gateway.yaml`
- `docs/audit/M26C_MARKDOWN_INVENTORY.csv`
- `docs/audit/M26C_MERMAID_INVENTORY.csv`
- `docs/audit/M26C_MERMAID_GITHUB_RENDER_AUDIT.md`

## As-Built Findings

| Surface | Status | Finding | Evidence |
| --- | --- | --- | --- |
| Target-qualified Test Plan | Aligned-contract | Runner and docs now agree on `--target qemu` or `--target pi4`, `target.env`, and target-qualified markers. | `scripts/ci/check_test_plan.sh` PASS |
| Markdown inventory | Aligned | All 203 tracked Markdown paths are classified once in the regenerated CSV and report. | `docs/audit/M26C_MARKDOWN_INVENTORY.csv`; `docs/audit/M26C_MARKDOWN_INVENTORY.md` |
| Mermaid active docs | Aligned-active | Canonical diagrams no longer use raw HTML labels flagged by the checker. Inventory status is finalized as 43 `pass` and 32 release-only `warning-render-pass` rows; all 75 rendered. | `scripts/ci/check_mermaid_github.sh`; `docs/audit/M26C_MERMAID_GITHUB_RENDER_AUDIT.md` |
| REST OpenAPI | Aligned | `max_bytes` remains required for `CAT` and `TAIL`; optional `lines=1..256` belongs only to `TAIL`. The duplicated narrative schema was replaced with a link-backed quick reference. | `resources/openapi/hive-gateway.yaml`; `apps/hive-gateway/src/main.rs`; `docs/HOST_API.md`; `cargo test -p hive-gateway` |
| Reopened 26b DPC status | Aligned-current | Reciprocal notification/IRQ topology, the bounded DPC ring, and isolated-runtime service are implemented; repeated current-image Wi-Fi functional proof remains open. | `docs/BUILD_PLAN.md`; `docs/DRIVERS.md`; Pi manifest and driver-runtime source |
| Pi DMA protection profile | Aligned-compiler | `coh-rtc` now owns `dma.protection_profile`; virt resolves to `none`, Pi-family manifests resolve to `bounded-no-iommu`, and SMMU profiles are rejected until generated DMA-domain state exists. | `cargo test -p coh-rtc --lib dma`; `scripts/check-generated.sh` |
| Runtime/DMA proof states | Aligned-Pi | Pi 4 proof states are machine-checkable: the image build writes target-build provenance, the normalizer classifies absent/diagnostic/qemu-or-stale/fresh-pi states, the gate wrapper writes the live proof bundle, and Pi Stage 05 requires `PI4_RUNTIME_DMA_PROOF=fresh-pi` with `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`. Final board closure uses `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env` and does not count QEMU or stage-only image builds as live board proof. | `scripts/pi4-image-build.sh`; `scripts/pi4_trace_normalize.py`; `scripts/pi4_gate_proof.sh`; `scripts/ci/test_plan_run.sh`; `out/audit/gate/20260629T061204Z` |
| Worker implementation | Aligned-QEMU | VM worker-heart/gpu/lora now use bounded no_std loop primitives for heartbeat, receipt, lifecycle, revoke, lease, and pressure handling; GPU and LoRA remain control-plane receipt workers only. | `cargo test -p worker-heart -p worker-gpu -p worker-lora`; `cargo check -p worker-heart -p worker-gpu -p worker-lora --target aarch64-unknown-none` |
| Cap-backed worker tickets | Aligned-QEMU | Generated worker endpoint badge classes are required for implemented roles, root-task rejects metadata-only authority, and stale/wrong action/role/epoch badges fail tests. | `cargo test -p root-task --test worker_authority`; `cargo test -p coh-rtc worker_runtime` |
| Notification lifecycle | Aligned-QEMU | Generated notification badge classes exist for revoke, shutdown, lease-expiry, telemetry-pressure, and IRQ; worker loops handle notification events and root-task verifies event-specific badges. This does not claim future full cap-bundle notification isolation. | `cargo test -p worker-heart -p worker-gpu -p worker-lora`; `cargo test -p root-task --test worker_authority` |
| MCS scheduling evidence | Aligned-QEMU | Generated scheduling evidence records QEMU/non-MCS priority, domain, and bounded service-turn fallback, and validation rejects MCS budget/timeout/consumed-budget claims on non-MCS profiles. | `cargo test -p coh-rtc worker_runtime`; `cargo test -p root-task --test worker_authority` |

## README-Linked Suite Remediation (2026-07-15)

| Documentation lane | Files | As-built authority | Final evidence |
| --- | --- | --- | --- |
| Entry point and scope | `README.md`, `docs/BUILD_PLAN.md` | Active milestone state, selected manifests, and target-qualified evidence | Current status and proof boundaries are explicit; 26c returned to `Complete` only after the final gates passed. |
| Core contracts | `ARCHITECTURE`, `INTERFACES`, `SECURE9P`, `ROLES_AND_SCHEDULING`, `DRIVERS` | Source, selected manifests, resolved output, and generated snippets | Transport, namespace, role, scheduling, HAL, and driver-runtime ownership are separated and cross-referenced. |
| Operator and host | `USERLAND_AND_CLI`, `HOST_TOOLS`, `API_GUIDELINES`, `PYTHON_SUPPORT`, `FAILURE_MODES`, `OPERATOR_WALKTHROUGH` | Current CLI help, fixtures, source, OpenAPI, and generated policy mirrors | Direct and gateway ownership, compiled gateway bounds versus target boot truth, provider scheduling, mock persistence, REST authority, and `TAIL lines` placement are stated as built. |
| Platform and product | `USE_CASES`, `HARDWARE_BRINGUP`, `BOOT_REFERENCE`, `GPU_NODES`, `BENCHMARKS` | Current Test Plan state, historical 26c evidence, and 26d proof boundaries | Implemented capability, historical proof, current-image proof, simulation, and planned work are no longer conflated. |
| Mermaid | Nine active suite diagrams; 75 tracked blocks | Diagram owners plus the same source and evidence used by their documents | GitHub compatibility PASS; Mermaid CLI 11.16.0 rendered 75 of 75 blocks. The 32 warnings belong only to immutable release snapshots and also rendered. |
| Suite integrity | `README.md` plus 17 linked documents | Repository paths and GitHub heading anchors | 266 local links resolved; headers, H1 structure, fences, Markdown linting, and diff checks passed. |

This remediation changed authored documentation, audit inventories, and the
embedded OpenAPI description. The OpenAPI edit documents existing handler
behavior; no runtime behavior, generated artifact, release snapshot, or
hardware acceptance changed.

## Required Follow-Up

1. Keep full cap-bundle notification authority out of 26c QEMU and Pi claims; current
   closure is generated badge classes, root-task verification, and no_std
   worker-loop handling.
2. Re-run `cargo run -p coh-rtc`, `scripts/check-generated.sh`, and
   `scripts/ci/check_test_plan.sh` after generated truth changes.

## Final Pi 4 Alignment

- Serial: `/Users/lukasbower/pi4-serial-20260629-135454.log`.
- Pcap: `/Users/lukasbower/tcpdump-usb-eth-20260629-135504.pcap`.
- Runtime/DMA proof: `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env`.
- TCP proof: `out/test-plan/m26c-pi4-live/cohsh-tcp-proof-genet-latest.txt`.
- Target-qualified state dir: `out/test-plan/m26c-pi4-live`, with Stage 01-05
  `.done` and `.pi4.done` markers and no incomplete markers.
- Stage 05 due-diligence root: `out/audit/gate/20260629T061204Z`.
