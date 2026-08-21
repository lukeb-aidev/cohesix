<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Summarize current Cohesix implementation and evidence status without duplicating planning or run history. -->
<!-- Author: Lukas Bower -->

# Cohesix Status

This page is the public snapshot of what the checked-in Cohesix source declares
and what that declaration does—and does not—prove. It is intentionally short.
The [Build Plan](BUILD_PLAN.md) remains the complete record of authorized and
implemented scope, while target-qualified evidence and acceptance procedures
remain governed by the [Test Plan](TEST_PLAN.md).

Status terms such as **source**, **configured**, **target-qualified**, and
**accepted** are defined in the [Glossary](GLOSSARY.md). They are not
interchangeable.

## Current development state

Milestone 26e is in progress with QEMU-first implementation and qualification.
The selected QEMU and Raspberry Pi 4 manifests describe the intended SMP+MCS
system, but neither a successful build nor a QEMU result can substitute for the
separate fresh-Pi evidence required to complete the milestone.

## Capability snapshot

| Surface | Checked-in implementation | Evidence boundary |
| --- | --- | --- |
| Kernel and target profiles | Upstream seL4 16.0.0, pure-Rust `no_std` userspace, four-core SMP+MCS QEMU and Pi 4 profiles, and no operational classic-scheduler fallback. | Selected source and generated-profile truth; exact target execution and acceptance remain separate. |
| Root services | Root control, restricted fault/emergency/supervisor duties, a passive NineDoor namespace child, and an isolated active console-network child are compiler-declared. | Construction and offline validation do not prove runtime progress, fault containment, or teardown on a target. |
| Workers | `worker-heartbeat`, `worker-gpu`, and `worker-lora` are declared executable with one slot each; `worker-bus` remains model/session-only. | Executable declaration is not a claim that a particular run created the child or that QEMU, Pi, release, or use-case acceptance passed. |
| Physical drivers | Pi 4 serial, display, USB, GENET, SDIO, and CYW43 paths use manifest-declared isolated runtimes admitted through HAL. | Board evidence from another source tree or image does not qualify a newly composed image. |
| QEMU | `aarch64/virt` with GICv3 is the reference development and regression target. | QEMU proves only the exact VM artifact and proof lane exercised; it is not Pi 4 hardware proof. |
| Raspberry Pi 4 | Pi firmware → U-Boot → seL4 binary image → root task is the supported hardware boot path. | Current 26e Pi build, flash/readback, boot, coexistence, network, and full-system acceptance require one fresh exact-image evidence chain. |
| Host tools | `cohsh`, `coh`, Hive Gateway, SwarmUI, Python, GPU, sidecar, ticket, CAS, and evidence tools run beside the target on macOS or Linux. | Host, mock, fixture, and package success cannot create target Worker, driver, or use-case acceptance. |
| GPU and AI execution | GPU drivers, CUDA/NVML, model training, inference, PEFT execution, and deployment-specific automation remain host-side. | Cohesix records bounded authority, lifecycle, telemetry, and receipts; it does not execute GPU workloads in the VM. |
| AWS/UEFI | Planned only. | No current Cohesix AWS target or production-use claim. |

## How to verify a claim

- For source and generated-profile truth, inspect the selected
  `configs/root_task*.toml` manifest, its resolved output, and generated
  snippets.
- For QEMU or Pi acceptance, use the exact staged workflow in the
  [Test Plan](TEST_PLAN.md).
- For build, flash, readback, boot, network, and console proof, follow
  [Hardware Bring-up](HARDWARE_BRINGUP.md).
- For performance claims, follow [Benchmarking](BENCHMARKS.md) and retain the
  complete result artifact.

This snapshot describes the repository on 22 August 2026. A change that alters
one of these public capability boundaries must update this page in the same
change.
