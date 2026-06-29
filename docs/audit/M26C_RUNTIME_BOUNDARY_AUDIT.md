<!-- Author: Lukas Bower -->
<!-- Purpose: Record Milestone 26c host/VM runtime boundary audit findings. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Runtime Boundary Audit

Status: `QEMU-CLOSED / PI4-GENET-CLOSED-FOR-26C`

## Boundary Statement

Cohesix VM-side code remains `no_std` and may not import host transport,
filesystem, process, GPU, PEFT, or provider capabilities into the VM closure.
Host `std` tools may project documented Secure9P/console semantics but do not
create new authority paths.

## Current Evidence

| Area | As-Built State | Status |
| --- | --- | --- |
| Root-task/NineDoor VM boundary | Separate VM root-task adapter and host NineDoor implementation remain distinct; Stage 05 QEMU due-diligence and final Pi Stage 05 evidence are closed. | PASS-QEMU+PI4 |
| GPU boundary | CUDA/NVML remain host-side; VM worker-gpu emits control receipts only through bounded no_std loop helpers. | PASS-QEMU |
| LoRA/PEFT boundary | Training/TensorRT/PEFT remain host-side; VM worker-lora emits control receipts only through bounded no_std loop helpers. | PASS-QEMU |
| Driver runtime boundary | Isolated runtime and HAL proof surfaces exist; compiler-owned DMA profile truth resolves Pi-family profiles to `bounded-no-iommu`; final GENET board proof produced `PI4_RUNTIME_DMA_PROOF=fresh-pi` and `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`. | PASS-PI4-GENET |
| Worker loops | worker-heart, worker-gpu, and worker-lora have bounded no_std loop primitives and target builds; kernel entrypoints drive those loops instead of placeholder-only semantics. | PASS-QEMU |
| Endpoint-cap authority | Generated endpoint badge ranges are compiler-owned for implemented worker roles; root-task rejects metadata-only authority and stale/wrong badges. This is endpoint-badge evidence, not a claim of full future cap-bundle authority. | PASS-QEMU |
| Notification lifecycle | Generated notification badge classes are compiler-owned and worker loops handle revoke/shutdown/lease/pressure events; full future cap-bundle notification isolation is not claimed. | PASS-QEMU |
| MCS budget evidence | Generated scheduling evidence records non-MCS priority/domain/service-turn fallback and rejects MCS claims on non-MCS profiles. | PASS-QEMU |

## No-Std Gate Evidence

Existing QEMU closure evidence from the saved gate logs, not rerun in this
docs-only update:

- `out/test-plan/m26c-qemu/stage_05.done`
- `out/test-plan/m26c-qemu/logs/stage-05-due-diligence.log` records `PASS due-diligence-gate`.
- `out/audit/gate/20260628T015332Z/` records PASS for `cargo clippy --workspace --all-targets -- -D warnings`, `cargo check --workspace`, `cargo test -p secure9p-codec`, `cargo test -p tests`, `cargo test --workspace`, `scripts/check-generated.sh`, `scripts/ci/check_test_plan.sh`, and the QEMU regression batch.
- `cargo check -p root-task --target aarch64-unknown-none --no-default-features --features "cohesix-dev"`
- `cargo tree -p root-task --target aarch64-unknown-none -e normal --no-default-features --features "cohesix-dev" > out/audit/m26c_root_task_tree_qemu.txt`

Pi 4 and isolated runtime boundary evidence is now closed for 26c by the final
GENET run:

- `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env`
- `out/test-plan/m26c-pi4-live/stage_03.done`
- `out/test-plan/m26c-pi4-live/stage_04.done`
- `out/test-plan/m26c-pi4-live/stage_05.done`
- `out/audit/gate/20260629T061204Z/` records PASS for `cargo clippy --workspace --all-targets -- -D warnings`, `cargo check --workspace`, `cargo test -p secure9p-codec`, `cargo test -p tests`, `cargo test --workspace`, `cargo audit`, `cargo deny check advisories`, `scripts/check-generated.sh`, `scripts/ci/check_test_plan.sh`, and regression-batch reuse.
