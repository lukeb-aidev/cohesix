<!-- Author: Lukas Bower -->
<!-- Purpose: Audit Milestone 26c canonical documentation against generated and observed as-built truth. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Docs-As-Built Audit

Status: `COMPLETE / QEMU-ALIGNED / PI4-GENET-ALIGNED`

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

## As-Built Findings

| Surface | Status | Finding | Evidence |
| --- | --- | --- | --- |
| Target-qualified Test Plan | Aligned-contract | Runner and docs now agree on `--target qemu|pi4`, `target.env`, and target-qualified markers. | `scripts/ci/check_test_plan.sh` PASS |
| Markdown inventory | Aligned | Every tracked Markdown path is classified once. | `docs/audit/M26C_MARKDOWN_INVENTORY.csv` |
| Mermaid active docs | Aligned-active | Canonical diagrams no longer use raw HTML labels flagged by the checker. | `scripts/ci/check_mermaid_github.sh` PASS active surfaces |
| Pi DMA protection profile | Aligned-compiler | `coh-rtc` now owns `dma.protection_profile`; virt resolves to `none`, Pi-family manifests resolve to `bounded-no-iommu`, and SMMU profiles are rejected until generated DMA-domain state exists. | `cargo test -p coh-rtc --lib dma`; `scripts/check-generated.sh` |
| Runtime/DMA proof states | Aligned-Pi | Pi 4 proof states are machine-checkable: the image build writes target-build provenance, the normalizer classifies absent/diagnostic/qemu-or-stale/fresh-pi states, the gate wrapper writes the live proof bundle, and Pi Stage 05 requires `PI4_RUNTIME_DMA_PROOF=fresh-pi` with `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`. Final board closure uses `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env` and does not count QEMU or stage-only image builds as live board proof. | `scripts/pi4-image-build.sh`; `scripts/pi4_trace_normalize.py`; `scripts/pi4_gate_proof.sh`; `scripts/ci/test_plan_run.sh`; `out/audit/gate/20260629T061204Z` |
| Worker implementation | Aligned-QEMU | VM worker-heart/gpu/lora now use bounded no_std loop primitives for heartbeat, receipt, lifecycle, revoke, lease, and pressure handling; GPU and LoRA remain control-plane receipt workers only. | `cargo test -p worker-heart -p worker-gpu -p worker-lora`; `cargo check -p worker-heart -p worker-gpu -p worker-lora --target aarch64-unknown-none` |
| Cap-backed worker tickets | Aligned-QEMU | Generated worker endpoint badge classes are required for implemented roles, root-task rejects metadata-only authority, and stale/wrong action/role/epoch badges fail tests. | `cargo test -p root-task --test worker_authority`; `cargo test -p coh-rtc worker_runtime` |
| Notification lifecycle | Aligned-QEMU | Generated notification badge classes exist for revoke, shutdown, lease-expiry, telemetry-pressure, and IRQ; worker loops handle notification events and root-task verifies event-specific badges. This does not claim future full cap-bundle notification isolation. | `cargo test -p worker-heart -p worker-gpu -p worker-lora`; `cargo test -p root-task --test worker_authority` |
| MCS scheduling evidence | Aligned-QEMU | Generated scheduling evidence records QEMU/non-MCS priority, domain, and bounded service-turn fallback, and validation rejects MCS budget/timeout/consumed-budget claims on non-MCS profiles. | `cargo test -p coh-rtc worker_runtime`; `cargo test -p root-task --test worker_authority` |

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
