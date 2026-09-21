<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Summarize current Cohesix implementation and evidence status without duplicating planning or run history. -->
<!-- Author: Lukas Bower -->

# Cohesix Release Status

This page is the public snapshot of what the checked-in Cohesix source declares
and what that declaration does—and does not—prove. It is intentionally short.
The [Build Plan](BUILD_PLAN.md) remains the complete record of authorized and
implemented scope, while target-qualified evidence and acceptance procedures
remain governed by the [Test Plan](TEST_PLAN.md).

Status terms such as **source**, **configured**, **target-qualified**, and
**accepted** are defined in the [Glossary](GLOSSARY.md). They are not
interchangeable.

## Release 1.1.0-beta

Release A (`1.1.0-beta`) is being qualified under
[Milestone 27g](BUILD_PLAN.md#27g). The assembled release requires complete
CUDA and private LoRA journeys through CLI, Python, CI and native SwarmUI;
a two-hour current-image Pi 4 GENET burn-in with Mac and Linux AArch64 NVIDIA
hosts; and the applicable staged [Test Plan](TEST_PLAN.md) and release gates.
Component completion does not establish assembled release acceptance.

The repaired physical-console/TCP path completed the two-hour Pi/Mac/Linux
operator burn-in, followed by passing five-stage QEMU and Pi test plans.
Extracted Mac and Linux installation checks, independently verified SD media,
fresh Pi boot, USB input and HDMI checks also pass. Full pressure qualification,
physical repeatability, remaining assembled-artifact checks and release promotion
are still open; see the [qualification record](audit/M27G_IMPLEMENTATION_RECORD.md).

The release's implemented workflows and their qualified component records are:

| Workflow | Operator guide | Component evidence |
| --- | --- | --- |
| Admitted native CUDA execution and recoverable recipes | [GPU nodes](GPU_NODES.md) | [Host foundation](audit/M27B_IMPLEMENTATION_RECORD.md), [recovery and reuse](audit/M27C_IMPLEMENTATION_RECORD.md) |
| Private LoRA import/training, evaluation, promotion and rollback | [Private LoRA release](PRIVATE_LORA_RELEASE.md) | [Native execution and recovery](audit/M27D_IMPLEMENTATION_RECORD.md) |
| Signed package installation, Python and CI | [Adoption](ADOPTION.md), [CI workflows](CI_WORKFLOWS.md) | [Package and installation checks](audit/M27E_IMPLEMENTATION_RECORD.md) |
| Native desktop operations, evidence and signed replay | [SwarmUI](SWARMUI.md), [gallery](SWARMUI_GALLERY.md) | [Native app and packaged checks](audit/M27F_IMPLEMENTATION_RECORD.md) |
| Delegated authority, failover, inspection and evidence | [Authority](M27A_AUTHORITY.md), [operator evidence](OPERATOR_EVIDENCE.md) | [Authority qualification](audit/M27A_COMPLETION_EVIDENCE.md), [operator utilities](audit/M27_COMPLETION_EVIDENCE.md) |

These records retain exact source, package, target and trust identities,
failures and accepted gaps. Fresh assembled qualification remains required;
replay and component evidence retain their original scope.

The published release remains [1.0.0-beta](../releases/RELEASE_NOTES-1.0.0-beta.md).
Its acceptance and provenance exceptions remain in the
[audit record](audit/AUDIT_REPORT_2026-09-13.md). The current
[exceptions register](audit/EXCEPTIONS.md) records DD30 as an owner-accepted
retired gap; dynamic fault/wake testing remains unexecuted.

Broader providers, agent protocols and other deferred features remain governed
by the [Build Plan](BUILD_PLAN.md#roadmap-id-mapping). Their existing source or
historical evidence does not establish release qualification.

## Capability snapshot

| Surface | Checked-in implementation | Evidence boundary |
| --- | --- | --- |
| Kernel and target profiles | Upstream seL4 16.0.0, pure-Rust `no_std` userspace, four-core SMP+MCS QEMU and Pi 4 profiles, and no operational classic-scheduler fallback. | Selected source and generated-profile truth; exact target execution and acceptance remain separate. |
| Root services | Root control, restricted fault/emergency/supervisor duties, a passive NineDoor namespace child, and an isolated active console-network child are compiler-declared. | Construction and offline validation do not prove runtime progress, fault containment, or teardown on a target. |
| Workers | Passive `worker-heartbeat`, `worker-gpu`, and `worker-lora` instances use two bounded executor lanes; QEMU and Pi each declare 1/127/128 instances. `worker-bus` remains model/session-only. | Target-qualified QEMU evidence covers the selected 256-Worker population and receipt path; the Pi configuration still requires separate fresh physical evidence. |
| Physical drivers | Pi 4 serial, display, USB, GENET, SDIO, and CYW43 paths use manifest-declared isolated runtimes admitted through HAL. | Board evidence from another source tree or image does not qualify a newly composed image. |
| QEMU | `aarch64/virt` with GICv3 is the reference target on macOS HVF and AArch64 Linux KVM. | Target-qualified evidence applies only to the exact VM artifacts and proof lanes exercised; see the [Milestone 26e result record](BUILD_PLAN.md#26e). It is not Pi 4 hardware proof. |
| Raspberry Pi 4 | Pi firmware → U-Boot → seL4 binary image → root task is the supported hardware boot path. | The selected release candidate has fresh SD/readback, boot, isolated-driver, physical-input and GENET transport evidence. Full physical repeatability and pressure acceptance remain open. |
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

This snapshot describes the repository on 22 September 2026. A change that alters
one of these public capability boundaries must update this page in the same
change.
