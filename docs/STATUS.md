<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Help Cohesix 1.2.0 readers locate current capabilities, execution boundaries and source-bound evidence. -->
<!-- Author: Lukas Bower -->

# Cohesix 1.2.0 status

Use this page to find the right workflow and understand what its evidence means.
The installed bundle's `VERSION.txt`, `RELEASE_NOTES.md` and manifest identify
its exact contents. The [Build Plan](BUILD_PLAN.md) owns task scope and the
[Test Plan](TEST_PLAN.md) owns acceptance checks; neither a source declaration
nor a component result transfers proof to a different image or host.

## Where work runs

| Place | Cohesix role | Start here |
| --- | --- | --- |
| QEMU or Raspberry Pi 4 target | The seL4 root task supplies Queen authority; passive Heartbeat, GPU and LoRA Workers have bounded target roles. Selected child runtimes own the console, namespace or physical devices. | [Architecture](ARCHITECTURE.md), [roles](ROLES_AND_SCHEDULING.md) |
| Mac or Linux controller | `cohsh`, `coh`, Hive Gateway, SwarmUI, Python and agent clients use the target's documented control surfaces. One process owns the target TCP console at a time. | [Quickstart](QUICKSTART.md), [host tools](HOST_TOOLS.md) |
| Linux AArch64 NVIDIA executor | CUDA/NVML, model weights, native jobs and their output checks stay on the GPU host. A target Worker records control and receipts; it does not execute CUDA. | [GPU nodes](GPU_NODES.md), [private LoRA release](PRIVATE_LORA_RELEASE.md) |
| Apple Silicon Mac | Local MLX and vMLX execution stay on macOS. The selected Mac workflow can submit governed work through a separate Cohesix target. | [Mac providers](MACOS_PROVIDERS.md), [SwarmUI](SWARMUI.md) |

The QEMU and Pi manifests each declare 256 passive Worker instances across
Heartbeat, GPU and LoRA roles. A selected QEMU profile enables authenticated
MCP and A2A routes at the host gateway; the selected Pi profile disables them.
Check the installed manifest and profile before using an agent client. Host
protocol replies and target admission are distinct from independently verified
native provider outcomes. See [Host API](HOST_API.md) and the
[Glossary](GLOSSARY.md) for those terms.

## Choose a workflow

| Need | Guide | Evidence to retain |
| --- | --- | --- |
| Boot or connect to a target | [Quickstart](QUICKSTART.md), [hardware bring-up](HARDWARE_BRINGUP.md) | Exact image/profile, boot, transport and authenticated response; QEMU results do not establish Pi hardware behavior. |
| Inspect or operate a hive | [Host tools](HOST_TOOLS.md), [operator walkthrough](OPERATOR_WALKTHROUGH.md) | Original request identity, bounded response and relevant target or host observation. |
| Run a CUDA job or release an adapter | [GPU nodes](GPU_NODES.md), [private LoRA release](PRIVATE_LORA_RELEASE.md) | Native output or signed release verification as well as the admitted job and Worker receipt. Reconcile an uncertain reply under its original identity. |
| Use MCP, A2A or NeMo | [Host API](HOST_API.md), [use cases](USE_CASES.md) | Selected protocol/profile, delegated subject, original task or ticket, and native outcome verification. |
| Use the Mac MLX workflow | [Mac providers](MACOS_PROVIDERS.md), [SwarmUI](SWARMUI.md) | Local Metal/model identity and, for governed work, the separate target admission and signed native result. |

## Read evidence at its original scope

Selected component records cover the [host job foundation](audit/M28_IMPLEMENTATION_RECORD.md),
[CUDA workload](audit/M28A_IMPLEMENTATION_RECORD.md),
[private adapter](audit/M28B_IMPLEMENTATION_RECORD.md),
[Mac MLX and governed serving](audit/M28C1_IMPLEMENTATION_RECORD.md),
[MCP](audit/M28D_IMPLEMENTATION_RECORD.md),
[A2A](audit/M28E_IMPLEMENTATION_RECORD.md) and
[NeMo](audit/M28F_IMPLEMENTATION_RECORD.md). The
[Mac app and Shortcuts record](audit/M28C_APPLE_ACTIONS_RECORD.md) covers its
separate installed client path. Each record identifies the exercised source,
profile, host or target and limits; use the release's own evidence to judge an
assembled 1.2.0 distribution.

The earlier [1.1.0-beta release notes](../releases/RELEASE_NOTES-1.1.0-beta.md)
and [qualification record](audit/M27G_IMPLEMENTATION_RECORD.md) retain their
original Pi, host and package evidence, including disclosed limits. They remain
useful for history and comparison, but do not qualify a new 1.2.0 image.

To verify a specific claim, match its source and selected manifest to the
installed bundle, then use [Hardware Bring-up](HARDWARE_BRINGUP.md) for physical
proof, the [Test Plan](TEST_PLAN.md) for acceptance, and
[Benchmarks](BENCHMARKS.md) for measured performance. The
[exceptions register](audit/EXCEPTIONS.md) retains accepted historical gaps.
