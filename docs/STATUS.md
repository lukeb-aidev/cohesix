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

[Milestone 28](BUILD_PLAN.md#28) is complete at its selected foundation scope.
The source declares
false-default, compiler-controlled MCP and A2A access switches and implements
selected REST/CLI/Python jobs with a private standing ledger for GPU submit and
service restart. Exact-source KVM target and native Linux AArch64 CUDA/service
observations passed the selected jobs and standing authority cases, including
stale request refusal and recovery of pending result delivery. The current
gateway has no MCP or A2A routes. The [M28 implementation record](audit/M28_IMPLEMENTATION_RECORD.md)
retains the evidence and limits. This component result does not change Release A
acceptance or qualify Release B.

[Milestone 28a](BUILD_PLAN.md#28a) is complete at its selected Orin scope. The source contains a
private digest-pinned workload registration path, version 2 request validation,
native GPU diagnostics and an independently checked batch-edge example. An
exact-source KVM Queen on Merlin2 admitted reference and independently
configured adaptation jobs through both systemd and Docker GPU owners. Signed
original terminals and independently verified output bytes bind each case to
the selected Orin. Cancellation, lost-response, bridge-restart and native-timeout
checks settled their reservations without replay. The [M28a implementation
record](audit/M28A_IMPLEMENTATION_RECORD.md) retains the focused evidence and
limits; this component result does not qualify Release B.

[Milestone 28b](BUILD_PLAN.md#28b) is complete for the selected private model
reference. Cohesix can train or import a small model adapter, measure it against
the running version, and reversibly promote it with a verified application
request. A pinned Linux AArch64 NVIDIA host completed genuine LoRA training,
independent compatible import, and full Trainer checkpoint resume through an
exact-source KVM Queen and WorkerLora receipt path. Held out comparison rejected
a worse adapter before load. An interrupted promotion restored its incumbent,
which another application request observed. The [M28b implementation
record](audit/M28B_IMPLEMENTATION_RECORD.md) keeps the distinct source, profile,
negative-result and network-observation limits. This is component evidence, not
Pi 4 or integrated Release B qualification.

[Milestone 28c](BUILD_PLAN.md#28c) is **Complete for the narrowed Mac developer workflow** on 25 September 2026. The [completion record](audit/M28C_COMPLETION_RECORD.md) binds the supported macOS 27 Apple M4, focused tests, selected private KVM service work, pinned model/data/adapter, installed vMLX 1.6.65 and the final Developer ID app. Spoken Siri was removed from the developer value gate; macOS user-created Shortcuts supply the useful native action path. The subsequent governed release and serving work is [Complete in 28c1](BUILD_PLAN.md#28c1) under separate admitted evidence.

[Milestone 28c1](BUILD_PLAN.md#28c1) is **Complete for the selected Mac component** on 25 September 2026. An exact-source pinned QEMU Queen admitted real Apple M4 Metal training and an imported release interruption under original ticket identities. The shared verifier reported signed `succeeded` and `recovered_failure` outcomes; the latter restored accepted generation 1. The pinned signed vMLX 1.6.65 engine served four frozen responses from the content-bound fused model, refused a changed generation, and observed the verified incumbent. The [implementation record](audit/M28C1_IMPLEMENTATION_RECORD.md) retains source, image, profile, native and graph identities, focused checks, and the physical Pi/Release B proof limits.

The installed development-signed App Intents path enrolled a delegated Keychain connection and used user-created Shortcuts to start and inspect an approved private KVM service job. `coh` resolved its original admission and confirmed result. A later held cancellation settled `refused_no_effect` before dispatch, and revocation removed the native scope choice. Foundation Models explained the scoped job and proposed a typed inspect follow-up without submitting it. The [actions record](audit/M28C_APPLE_ACTIONS_RECORD.md) retains exact identities, the initial cancellation defect and correction. These are selected component observations, not Release B qualification.

The final SwarmUI release binary and App Intents extension were Developer ID signed under Team `KB88FQXUX2`; Apple accepted notarisation submission `b14be979-3330-4770-94f1-73fce245bf6e`. The stapled app installed at `~/Applications/SwarmUI-M28c-Build2.app` passed strict signature verification and Gatekeeper assessment, and its two signed executable hashes matched the notarised stage. `pluginkit` registered only that final extension. A saved read-only status Shortcut received HTTP 403 after the earlier scope revocation; this is a live refusal, not a fresh authorised job on final bytes.

The final installed **Local MLX** panel ran bounded inference and 16-row held-out evaluation using the pinned Qwen2.5-1.5B instruction model and 48-step LoRA adapter on the observed Apple M4 Metal device. The panel displayed the model/adapter hashes, about 974 MB/1.14 GB peak allocation, a useful bounded answer and held-out loss `0.8091070055961609`, labelled **local observation, no Cohesix admission or promotion**. The Python native component's earlier real 48-step LoRA training and four frozen operational answers remain a narrow diagnostic quality result; answer templates repeat across the train/test split. The installed signed vMLX 1.6.65 engine served a disposable fused copy and reproduced four direct fused answers inside the frozen latency bound; its `g1` label is diagnostic, with source and repaired loaded hashes retained separately in the [MLX record](audit/M28C_MLX_COMPONENT_RECORD.md).

Focused Python MLX (6), vMLX (12), frontend (3), SwarmUI workbench (12), native compile, Rust formatting and generated-consistency checks passed; the installed Metal inference/evaluation and final Apple signing path were exercised. The full suite was not run. M28c adds no Pi hardware, mixed MLX/CUDA, accepted Mac deployment, broad model-quality or integrated Release B claim.

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
