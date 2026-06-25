<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Inventory Pi 4 root-owned driver cleanup and isolated runtime boundary findings. -->
<!-- Author: Lukas Bower -->
# Pi 4 Root-Owned Driver Inventory

This inventory audits the Pi 4 driver boundary against `docs/DRIVERS.md` and
the reopened Milestone 26a/26b isolated runtime model. Root-task may retain HAL
admission authority, seL4 object construction, manifest validation, console
parsing, namespace/ticket ownership, and emergency serial output. It must not
own steady USB, HDMI, GENET, CYW43, SDIO, or PCIe hardware driver logic on the
physical Pi 4 path.

## Deprecated Code Removed

| Path | Finding | Cleanup status |
| --- | --- | --- |
| `apps/root-task/src/local_seat_pi4.rs` | Retired root-resident Pi 4 local-seat implementation containing the old USB/xHCI, HDMI, and VL805 helper path. | Deleted from the worktree; CI guard now requires the path to stay absent. |
| `crates/cohesix-usb/` | Retired in-tree USB support crate with root-owned xHCI/HID/MSC logic. | Deleted from the worktree; CI guard now requires the directory and workspace member to stay absent. |
| `apps/root-task/Cargo.toml` / root `Cargo.toml` | The `usb` feature used to select a root-owned USB crate. | Current root-task `usb` feature is only a profile gate; CI guard rejects `cohesix-usb`, `usb-oxide`, or `crates/cohesix-usb` references. |
| `apps/root-task/src/local_seat.rs` | The local-seat runtime still carried fixed-layout `LocalSeat*OwnerRuntimeRecord` scaffolding with `root-runtime-pointer` non-acceptance flags. | Removed. Local-seat now keeps only prompt queue/mirror telemetry; owner-state proof comes from isolated runtime descriptors. |
| `apps/root-task/src/hal/pi4_wifi.rs` | The HAL Wi-Fi module still exposed a public `Pi4WifiState` root-owned wrapper around CYW43/SDIO debug and firmware operations. | Removed. Prompt commands now fail closed unless isolated runtime evidence supplies the required state. |
| `apps/root-task/src/hal/mod.rs` | `Cyw43Hal` still advertised direct power/reset and SDIO CMD52/CMD53 verbs. | Removed. `Cyw43Hal` now retains firmware bundle admission only; steady SDIO/CYW43 service belongs to isolated runtime descriptors. |
| `scripts/pi4_trace_normalize.py` / Pi 4 proof tests | Legacy `[local-seat] xhci root-port command-probe result=enable-slot-ok` fixtures could still be treated as command-ring success. | Normalizer now rejects local-seat command-probe success as `cmd-event-ring-timeout`; positive proof must come from linked-runtime lines or explicit proof fields. |

## Current Allowed Surfaces

| Path | Classification | Reason retained |
| --- | --- | --- |
| `configs/root_task*.toml` and `apps/root-task/src/generated/bootstrap.rs` | Linked-runtime IR/specs | All seven Pi 4 runtime specs point at `cohesix/bin/pi4-driver-*` artifacts and report `root_context_required=false`, `hardware_state_migrated=true`. |
| `apps/root-task/src/hal/driver_task.rs` | HAL/seL4 admission substrate | Root-owned TCB, CSpace, VSpace, endpoint, notification, fault, mapping, and ring setup is the admission layer. Physical Pi service must use pointer-free linked-runtime rings or fail closed. |
| `apps/root-task/src/hal/pi4_pcie.rs` | HAL-owned PCIe/VL805 admission and proof | Root prepares and proves the PCIe owner-link prerequisite; steady PCIe read/write/flush service is credited only through the linked `pcie-root` runtime owner state. |
| `apps/root-task/src/hal/pi4_wifi.rs` | HAL-owned mailbox, SDIO/card resource support, and cached diagnostics | SDIO/CYW43 service commands are submitted through linked SDIO/CYW43 runtime descriptors. The public root-owned Wi-Fi wrapper is absent, and prompt commands fail closed with runtime-required errors unless linked evidence can satisfy them. |
| `apps/root-task/src/drivers/driver_task_net.rs` | Network ring clients | GENET/CYW43/SDIO network progress is mediated by isolated runtime descriptor replay and fixed command/completion records. |
| `apps/root-task/src/local_seat.rs` | USB/HDMI ring-client prompt surface | The file owns bounded parser queues, prompt diagnostics, and linked-runtime submission. QEMU/host root-context callbacks remain compatibility-only and physical Pi checks fail closed before those callbacks can satisfy acceptance. |
| `apps/root-task/src/serial/mod.rs` | Emergency serial exception plus linked serial runtime | Early UART keeps the first shell reachable. Normal serial runtime init and owner-state proof use `pi4-driver-serial`. |

## Regression Guard

`scripts/ci/check_driver_test_coverage.py` now enforces the cleanup by checking:

- `apps/root-task/src/local_seat_pi4.rs` is absent.
- `crates/cohesix-usb/` is absent.
- root `Cargo.toml` has no `crates/cohesix-usb` or `usb-oxide` reference.
- `apps/root-task/src/local_seat.rs` has no `LocalSeatUsbOwnerRuntimeRecord`,
  `LocalSeatHdmiOwnerRuntimeRecord`, or `root-runtime-pointer` marker.
- `apps/root-task/src/hal/pi4_wifi.rs` has no public `Pi4WifiState` or
  `Cyw43HostEapolRxSource` root-owned wrapper.
- `apps/root-task/src/hal/mod.rs` has no direct CYW43 power/reset or SDIO
  command verbs on `Cyw43Hal`.

The guard still preserves the linked-runtime coverage checks for local-seat,
driver-task network, Pi 4 PCIe/Wi-Fi HAL support, and the separate
`pi4-driver-runtime` crate.
