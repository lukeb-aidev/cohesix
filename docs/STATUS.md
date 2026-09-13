<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Summarize current Cohesix implementation and evidence status without duplicating planning or run history. -->
<!-- Author: Lukas Bower -->

# Cohesix 1.0.0-beta Status

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

The 1.0.0-beta release consists of native Mac and Linux ARM64 host bundles and
a separate Pi 4 SD-image bundle. The [release notes](../releases/RELEASE_NOTES-1.0.0-beta.md)
describe changes since 0.9.0-beta. The 0.9.0-beta packages remain in `releases/`; earlier packages remain at their Git tags.

As of 13 September 2026, audit findings DD26–29 are `CLOSED_VERIFIED`. DD30
remains P1 / `ACCEPTED_RISK` for this release under the owner's source-bound
waiver; dynamic fault/wake testing remains unexecuted. The
[audit report](audit/AUDIT_REPORT_2026-09-13.md) records the evidence and limits.
Release Stage 5 is accepted as `PASS_WITH_RESIDUAL_RISK` at `5be3ca588`.
The owner approved carrying forward the original Stage 1–4 records and later
scoped fix evidence; the current source's Stage 1–4 suite was not rerun. The
original timed burn-in failure remains recorded alongside its focused repairs.
The owner requested fresh release builds without additional tests. Bundle
metadata records `NOT_RUN` for those artifacts while retaining exact source,
native-profile and content integrity checks.

## Capability snapshot

| Surface | Checked-in implementation | Evidence boundary |
| --- | --- | --- |
| Kernel and target profiles | Upstream seL4 16.0.0, pure-Rust `no_std` userspace, four-core SMP+MCS QEMU and Pi 4 profiles, and no operational classic-scheduler fallback. | Selected source and generated-profile truth; exact target execution and acceptance remain separate. |
| Root services | Root control, restricted fault/emergency/supervisor duties, a passive NineDoor namespace child, and an isolated active console-network child are compiler-declared. | Construction and offline validation do not prove runtime progress, fault containment, or teardown on a target. |
| Workers | Passive `worker-heartbeat`, `worker-gpu`, and `worker-lora` instances use two bounded executor lanes; QEMU and Pi each declare 1/127/128 instances. `worker-bus` remains model/session-only. | Target-qualified QEMU evidence covers the selected 256-Worker population and receipt path; the Pi configuration still requires separate fresh physical evidence. |
| Physical drivers | Pi 4 serial, display, USB, GENET, SDIO, and CYW43 paths use manifest-declared isolated runtimes admitted through HAL. | Board evidence from another source tree or image does not qualify a newly composed image. |
| QEMU | `aarch64/virt` with GICv3 is the reference target on macOS HVF and AArch64 Linux KVM. | Target-qualified evidence applies only to the exact VM artifacts and proof lanes exercised; see the [Milestone 26e result record](BUILD_PLAN.md#26e). It is not Pi 4 hardware proof. |
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

This snapshot describes the repository on 13 September 2026. A change that alters
one of these public capability boundaries must update this page in the same
change.
