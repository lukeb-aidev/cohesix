<!-- Author: Lukas Bower -->
<!-- Purpose: Assign Milestone 26c refactor and audit ownership lanes. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Refactor Ownership

Status: `WORKER-EXECUTION-REOPENED / OTHER-FUTURE-WAVES-DEFERRED`

| Lane | Owner | Files / Surfaces | Current State |
| --- | --- | --- | --- |
| Runner | runner-owner | `scripts/ci/test_plan_run.sh`, stage contract docs | Closed-QEMU+Pi |
| Docs/audit | docs-owner | `docs/audit/M26C_*`, canonical docs, inventory scripts | Active docs-only synchronization |
| Compiler IR | compiler-owner | `tools/coh-rtc/src/**`, `configs/root_task*.toml`, generated outputs | Corrected to non-executable Worker roles with endpoint and notification claims disabled; live execution remains reopened |
| Runtime/DMA | runtime-dma-owner | `apps/root-task/src/hal/**`, `apps/pi4-driver-runtime/**`, `crates/pi4-driver-abi/**` | Pi GENET runtime/DMA proof closed for 26c |
| Worker loops | worker-owner | `apps/worker-heart/**`, `apps/worker-gpu/**`, `apps/worker-lora/**` | Helper/model loops and build artifacts exist; target image load/resume is reopened |
| Endpoint caps | capability-owner | `apps/root-task/src/event/**`, `worker_authority.rs`, `ninedoor.rs`, worker apps | Current profiles disable endpoint authority; live cap mint/delivery/invocation is reopened |
| Notifications | lifecycle-owner | `tools/coh-rtc`, root-task lifecycle/event/HAL, worker apps | Current profiles disable Worker notifications; live object/cap delivery and handling is reopened |
| Scheduling/MCS | scheduling-owner | `tools/coh-rtc`, root-task HAL/lifecycle, docs scheduling | Non-MCS record is metadata only; applied Worker scheduling and any MCS profile evidence remain open |
| Host tools | host-tools-owner | `apps/coh`, `apps/cohsh`, `cohsh-core`, `host-ticket-agent`, `gpu-bridge-host`, `hive-gateway`, `tools/cohesix-py` | Existing QEMU parity evidence only; no new authority path or future host-tool scope authorized here |
| Root-task decomposition | root-task-owner | `apps/root-task/src/ninedoor.rs`, `event/**`, `console/**`, `log_buffer.rs` | Not authorized yet |
| HAL/network/local-seat cleanup | hal-owner | `apps/root-task/src/hal/**`, `net/**`, `drivers/**`, `local_seat.rs` | Pi 4 live hardware proof closed for 26c; cleanup still not authorized here |

QEMU helper-loop and session-model tests remain characterization evidence, but
they do not close Worker execution, endpoint-cap delivery, lifecycle
notifications, or applied scheduling. Final Pi GENET runtime/DMA, TCP, REST,
and Stage 05 evidence remain historical board evidence for 26c and do not close
those Worker lanes.
