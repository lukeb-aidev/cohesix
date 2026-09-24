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

Release A (`1.1.0-beta`) completes [Milestone 27g](BUILD_PLAN.md#27g).
The integrated CUDA and private LoRA journeys, CLI, Python, CI and native
SwarmUI were exercised during a measured two-hour Pi 4 GENET, Mac and Linux
AArch64 NVIDIA operator run. The applicable staged [Test Plan](TEST_PLAN.md)
passed after that burn-in. Subsequent changes received focused source and
physical checks; the unchanged staged and pressure records retain their exact
source identities rather than being relabelled as new-image runs.

The physically installed candidate Pi SD image passed independent raw
readback, fresh numbered U-Boot-menu boots with GENET static, GENET DHCP and
Wi-Fi DHCP, packaged authenticated TCP checks, and a final GENET cold boot.
The final archive's raw SD image differs in FAT metadata but contains the same
27 embedded files; its exact raw bytes were not written and read back. Mac and
Linux archives passed independent extracted installation checks. The final
three-archive verifier therefore has no passing result. The HDMI output was
visually checked through the capture card. Earlier physical USB keypress and
Wi-Fi repeatability records remain at their original image identities; this
release does not claim a new 512-entry-image Wi-Fi repeatability series. Lukas
Bower accepted these disclosed release limits after selecting the final GENET
cold boot; they do not extend earlier Wi-Fi hardware acceptance to the new image. The
[qualification record](audit/M27G_IMPLEMENTATION_RECORD.md) identifies each
proof, limitation and carried-forward result.

The release's implemented workflows and their qualified component records are:

| Workflow | Operator guide | Component evidence |
| --- | --- | --- |
| Admitted native CUDA execution and recoverable recipes | [GPU nodes](GPU_NODES.md) | [Host foundation](audit/M27B_IMPLEMENTATION_RECORD.md), [recovery and reuse](audit/M27C_IMPLEMENTATION_RECORD.md) |
| Private LoRA import/training, evaluation, promotion and rollback | [Private LoRA release](PRIVATE_LORA_RELEASE.md) | [Native execution and recovery](audit/M27D_IMPLEMENTATION_RECORD.md) |
| Signed package installation, Python and CI | [Adoption](ADOPTION.md), [CI workflows](CI_WORKFLOWS.md) | [Package and installation checks](audit/M27E_IMPLEMENTATION_RECORD.md) |
| Native desktop operations, evidence and signed replay | [SwarmUI](SWARMUI.md), [gallery](SWARMUI_GALLERY.md) | [Native app and packaged checks](audit/M27F_IMPLEMENTATION_RECORD.md) |
| Delegated authority, failover, inspection and evidence | [Authority](M27A_AUTHORITY.md), [operator evidence](OPERATOR_EVIDENCE.md) | [Authority qualification](audit/M27A_COMPLETION_EVIDENCE.md), [operator utilities](audit/M27_COMPLETION_EVIDENCE.md) |

These records retain exact source, package, target and trust identities,
failures and accepted gaps. Replay and component evidence retain their original
scope. The previous [1.0.0-beta](../releases/RELEASE_NOTES-1.0.0-beta.md)
acceptance and provenance exceptions remain in the
[audit record](audit/AUDIT_REPORT_2026-09-13.md). The current
[exceptions register](audit/EXCEPTIONS.md) records DD30 as an owner-accepted
retired gap; dynamic fault/wake testing remains unexecuted.

Broader providers, agent protocols and other deferred features remain governed
by the [Build Plan](BUILD_PLAN.md#roadmap-id-mapping). Their existing source or
historical evidence does not establish release qualification.

[Milestone 28](BUILD_PLAN.md#28) is in progress. The source now declares
false-default, compiler-controlled MCP and A2A access switches and implements
selected REST/CLI/Python jobs with a private standing ledger for GPU submit and
service restart. The current gateway has no MCP or A2A routes. A mixed-source
QEMU/native CUDA preflight has a confirmed GPU result; the required
exact-source live GPU/service cases and milestone acceptance remain pending in
the [M28 implementation record](audit/M28_IMPLEMENTATION_RECORD.md). This work
does not change Release A acceptance or qualify Release B.

## Capability snapshot

| Surface | Checked-in implementation | Evidence boundary |
| --- | --- | --- |
| Kernel and target profiles | Upstream seL4 16.0.0, pure-Rust `no_std` userspace, four-core SMP+MCS QEMU and Pi 4 profiles, and no operational classic-scheduler fallback. | Selected source and generated-profile truth; exact target execution and acceptance remain separate. |
| Root services | Root control, restricted fault/emergency/supervisor duties, a passive NineDoor namespace child, and an isolated active console-network child are compiler-declared. | Construction and offline validation do not prove runtime progress, fault containment, or teardown on a target. |
| Workers | Passive `worker-heartbeat`, `worker-gpu`, and `worker-lora` instances use two bounded executor lanes; QEMU and Pi each declare 1/127/128 instances. `worker-bus` remains model/session-only. | Target-qualified QEMU evidence covers the selected 256-Worker population and receipt path; the Pi configuration still requires separate fresh physical evidence. |
| Physical drivers | Pi 4 serial, display, USB, GENET, SDIO, and CYW43 paths use manifest-declared isolated runtimes admitted through HAL. | Board evidence from another source tree or image does not qualify a newly composed image. |
| QEMU | `aarch64/virt` with GICv3 is the reference target on macOS HVF and AArch64 Linux KVM. | Target-qualified evidence applies only to the exact VM artifacts and proof lanes exercised; see the [Milestone 26e result record](BUILD_PLAN.md#26e). It is not Pi 4 hardware proof. |
| Raspberry Pi 4 | Pi firmware → numbered U-Boot menu → seL4 binary image → root task is the supported hardware boot path. | The installed candidate image passed raw SD readback, GENET static/DHCP, Wi-Fi DHCP and packaged TCP checks; the final archive has identical embedded files but lacks exact-byte card readback. Earlier keyboard and Wi-Fi repeatability evidence remains source-bound. |
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

This snapshot describes the repository on 24 September 2026. A change that alters
one of these public capability boundaries must update this page in the same
change.
