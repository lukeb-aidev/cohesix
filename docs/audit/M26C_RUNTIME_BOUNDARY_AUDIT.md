<!-- Author: Lukas Bower -->
<!-- Purpose: Record Milestone 26c host/VM runtime boundary audit findings. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Runtime Boundary Audit

Status: `WORKER-EXECUTION-REOPENED / PI4-GENET-HISTORICAL-CLOSED-FOR-26C`

## Boundary Statement

Cohesix VM-side code remains `no_std` and may not import host transport,
filesystem, process, GPU, PEFT, or provider capabilities into the VM closure.
Host `std` tools may project documented Secure9P/console semantics but do not
create new authority paths.

## Current Evidence

| Area | As-Built State | Status |
| --- | --- | --- |
| Root-task/NineDoor VM boundary | Separate VM root-task adapter and host NineDoor implementation remain distinct; Stage 05 QEMU due-diligence and final Pi Stage 05 evidence are closed. | PASS-QEMU+PI4 |
| GPU boundary | CUDA/NVML remain host-side; worker-gpu helper code models bounded receipts but no Worker TCB is launched. | PASS-HOST-BOUNDARY / MODEL-ONLY |
| LoRA/PEFT boundary | Training/TensorRT/PEFT remain host-side; worker-lora helper code models bounded receipts but no Worker TCB is launched. | PASS-HOST-BOUNDARY / MODEL-ONLY |
| Driver runtime boundary | Isolated runtime and HAL proof surfaces exist; compiler-owned DMA profile truth resolves Pi-family profiles to `bounded-no-iommu`; final GENET board proof produced `PI4_RUNTIME_DMA_PROOF=fresh-pi` and `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`. | PASS-PI4-GENET |
| Worker loops | worker-heart, worker-gpu, and worker-lora have bounded no_std helper primitives and some target build artifacts; root-task does not load or resume them as Worker tasks. | REOPENED / MODEL-ONLY |
| Endpoint-cap authority | Current profiles disable endpoint caps. Reserved badge ranges are compiler-owned schema data, not minted or delivered capabilities. | REOPENED / DISABLED |
| Notification lifecycle | Current profiles disable Worker notifications. Helper event handling does not prove notification-object creation, cap delivery, or target handling. | REOPENED / DISABLED |
| MCS budget evidence | The generated non-MCS record rejects MCS claims but is metadata only; no Worker scheduling context or applied TCB settings exist. | NOT-CLAIMED |

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
