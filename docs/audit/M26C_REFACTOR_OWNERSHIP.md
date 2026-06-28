<!-- Author: Lukas Bower -->
<!-- Purpose: Assign Milestone 26c refactor and audit ownership lanes. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Refactor Ownership

Status: `QEMU-CLOSED / PI4-PROOF-OPEN`

| Lane | Owner | Files / Surfaces | Current State |
| --- | --- | --- | --- |
| Runner | runner-owner | `scripts/ci/test_plan_run.sh`, stage contract docs | Closed-QEMU |
| Docs/audit | docs-owner | `docs/audit/M26C_*`, canonical docs, inventory scripts | Active docs-only synchronization |
| Compiler IR | compiler-owner | `tools/coh-rtc/src/**`, `configs/root_task*.toml`, generated outputs | Closed-QEMU for worker/cap/notification/non-MCS evidence; Pi proof remains open |
| Runtime/DMA | runtime-dma-owner | `apps/root-task/src/hal/**`, `apps/pi4-driver-runtime/**`, `crates/pi4-driver-abi/**` | Pi runtime/DMA proof open |
| Worker loops | worker-owner | `apps/worker-heart/**`, `apps/worker-gpu/**`, `apps/worker-lora/**` | Closed-QEMU for implemented heartbeat/GPU/LoRA loops; worker-bus deferred |
| Endpoint caps | capability-owner | `apps/root-task/src/event/**`, `worker_authority.rs`, `ninedoor.rs`, worker apps | Closed-QEMU for generated endpoint-badge authority; full future cap-bundle authority not claimed |
| Notifications | lifecycle-owner | `tools/coh-rtc`, root-task lifecycle/event/HAL, worker apps | Closed-QEMU for generated notification badges and worker-loop lifecycle events |
| Scheduling/MCS | scheduling-owner | `tools/coh-rtc`, root-task HAL/lifecycle, docs scheduling | Closed-QEMU as explicit non-MCS fallback; consumed MCS budget evidence not claimed |
| Host tools | host-tools-owner | `apps/coh`, `apps/cohsh`, `cohsh-core`, `host-ticket-agent`, `gpu-bridge-host`, `hive-gateway`, `tools/cohesix-py` | Existing QEMU parity evidence only; no new authority path or future host-tool scope authorized here |
| Root-task decomposition | root-task-owner | `apps/root-task/src/ninedoor.rs`, `event/**`, `console/**`, `log_buffer.rs` | Not authorized yet |
| HAL/network/local-seat cleanup | hal-owner | `apps/root-task/src/hal/**`, `net/**`, `drivers/**`, `local_seat.rs` | Pi 4 live hardware proof open; cleanup not authorized here |

QEMU worker-loop, endpoint-badge, notification, and non-MCS scheduling items are
closed for Milestone 26c QEMU evidence. No owner may use that closure as Pi 4
acceptance, isolated Pi runtime/DMA proof, or full future cap-bundle authority.
