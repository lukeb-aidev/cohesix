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

The next release follows
[27b–27g](BUILD_PLAN.md#post-26e-investment-constrained-delivery-sequence): executable
host foundation, recoverable CUDA recipes, Verified Private LoRA Release,
installation/CI, the full SwarmUI workbench, then integrated qualification. Broader
scope remains owned by [28 onward](BUILD_PLAN.md#roadmap-id-mapping). This order changes
planning, not implementation or evidence status.

Milestone [27e](BUILD_PLAN.md#27e), **Installation, Adoption and CI**, is
**Complete** as of 20 September 2026. Selected signed Mac/Linux ARM64 packages
include the explicit Python distribution sources, offline adoption guides and
CI example. The shared journey command preserves retry identity and durable
state, reports doctor boundaries and returns success only after the requested
outcome is verified. Focused package/service/lifecycle/CI tests, native host
builds and clean SDK installs pass. Installed tools reverify the accepted native
CUDA/LoRA evidence at its original identity; failed-canary rollback stays failed.
See [Adoption](ADOPTION.md) and the
[implementation record](audit/M27E_IMPLEMENTATION_RECORD.md). Native UI and
fresh assembled release qualification remain 27f/27g.

Milestone [27d](BUILD_PLAN.md#27d), **Verified Private LoRA Release**, is
**Complete** as of 19 September 2026. Genuine native import and HF training pass
comparable evaluation and actual serving promotion. A measured regression is
refused, and a forced canary failure restores the exact previous adapter and
generation while remaining a failed candidate. Focused boundary/refusal checks,
exact macOS/Linux ARM64 and QEMU builds, independent WorkerLora receipts,
cleanup and generated/Test Plan checks pass. See the
[implementation record](audit/M27D_IMPLEMENTATION_RECORD.md). Package adoption is delivered by 27e; native UI
and integrated release qualification remain later milestones.

Milestone [27c](BUILD_PLAN.md#27c), **Recoverable CUDA Recipes**, is **Complete**
as of 19 September 2026. The shared CLI/Python lifecycle persists identity and
submission intent, reconciles lost ACKs without duplicate CUDA execution, reuses
compatible verified stage outputs and retains cumulative resource accounting.
A fresh admitted native recipe, focused refusal/recovery/reuse checks, macOS and
Linux ARM64 builds, generated consistency and Test Plan checks pass. Its canonical
diagnostic case preserves a partial lease-detail capture error; no complete
release evidence, new Pi qualification or full-suite pass is claimed. See the
[implementation record](audit/M27C_IMPLEMENTATION_RECORD.md).

Milestone [27b](BUILD_PLAN.md#27b) is **Complete** as of 19 September 2026
for the selected **Executable Host Foundation**. The macOS ARM64 controller and
Linux AArch64 CUDA 13.2.2 host pass real vector-add and matrix-multiply through
QEMU admission, the host-ticket agent and bounded GPU bridge under both hardened
systemd and a digest-pinned NVIDIA container. Separately enrolled gateway,
native and Worker custodians produce verified causal chains. Native manager
identities match the signed results and measured memory/CPU/task controls;
CUDA allocation admission is explicitly not a hard GPU partition.

Focused authority/refusal, deadline/cancel/revoke, lost-ACK/restart, evidence
integrity and safe OOM checks pass at their recorded proof layers. Exact host
and QEMU builds, generated consistency and Test Plan checks pass. No full suite,
new physical Pi qualification, production Worker ticket-to-bundle binding,
Queen reboot persistence or complete use-case acceptance is claimed. See the
[implementation record](audit/M27B_IMPLEMENTATION_RECORD.md#selected-executable-foundation-closure-19-september-2026)
for exact identities, evidence reuse and the scoped compatibility review.

Earlier broad 27b implementation remains available under the updated roadmap:
identity/exporter/federation, native snapshots, FUSE and package foundations,
field-bus references, launchd/Xcode adapters, MIG discovery and generated workflow
stages. Their existing evidence stays at its original source/profile identity.
Credentialed Apple release, broader native-provider/package integration and
complete domain workflows remain unqualified under their later milestone owners.
MIG is unsupported on Merlin; CUDA, data and artifacts remain host-side.

Milestone [27a](BUILD_PLAN.md#27a) is **Complete**, approved on
14 September 2026. The scoped `m27a-host-ticket-validation-replay` restoration
found during M27b is also Complete: production Root now accepts strict native
argument/correlation objects and enforces version-1 request/result writer epochs;
focused QEMU/Merlin checks pass. The previous closure evidence is retained. Delegated REST identity, strict Queen intent replay,
host execution recovery and production-secret enforcement are implemented.
Focused host, QEMU and Pi authority/compatibility checks pass. The
[27a task record](audit/M27A_COMPLETION_EVIDENCE.md) records the owner's approval
to close without the missing M26d status-baseline comparison, candidate-F Rust
sign-off and the separate DD30 accepted risk. The comparison and dynamic
fault/wake test remain unexecuted; no performance equivalence or complete
final-source five-stage chain is claimed.

Milestones [26e](BUILD_PLAN.md#26e) and [27](BUILD_PLAN.md#27) are Complete
under their recorded owner approvals. On 14 September 2026, Lukas Bower
approved M27 closure at tested source `b54bdd2fc`: QEMU Stages 01–05 and live
TCP/REST operator checks pass; Pi Stages 01–02 pass. The final-image Pi boot,
live operator checks and Stages 03–05 remain unexecuted because serial recovery
failed. The [M27 completion record](audit/M27_COMPLETION_EVIDENCE.md) retains
the exact evidence and accepted gaps. This milestone decision does not establish
full physical qualification or activate the next milestone.

M27 delivers read-only inspect/diff utilities, canonical live trace capture and
offline replay, evidence case summaries, a thin bundle alias, and a shared
signed-evidence verifier. Stock Pi positive signed-device acceptance is excluded
by owner approval; unavailable and measurement-only evidence remain non-attested.
Human Rust review is approved. DD30 remains P1 / `ACCEPTED_RISK` under the
separate [M27 approval](audit/DD30_M27_APPROVAL.toml) through 13 October 2026;
dynamic fault/wake testing remains unexecuted.

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
| Raspberry Pi 4 | Pi firmware → U-Boot → seL4 binary image → root task is the supported hardware boot path. | M27 final-source common checks and image build pass. Fresh final-image boot, live operator and transport/governance acceptance remain unexecuted under the explicit completion approval. |
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

This snapshot describes the repository on 20 September 2026. A change that alters
one of these public capability boundaries must update this page in the same
change.
