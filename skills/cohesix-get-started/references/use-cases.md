<!-- Author: Lukas Bower -->
<!-- Purpose: Map researched user projects and published 1.2.0 features to operating skills. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Useful Cohesix 1.2.0 projects

Use the installed release's matching contracts for operations. Web links to
upstream projects establish context; they do not qualify a Cohesix provider.

## Choose a first useful project

Start with one job and a result you can check. The [skill catalogue](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/skills/INDEX.md)
includes example requests, prerequisites and loading instructions for all
13 workflows. [Get started](../../cohesix-get-started/SKILL.md) chooses a
path from the hardware and deployment you already have.

The primary sources below were checked on **1 October 2026**. They establish
real application patterns and native-tool capabilities. The proposed Cohesix
journeys are our application of those patterns; they are not reports of users
deploying Cohesix or claims of an existing integration with those products.

| User need and research | A useful first Cohesix deliverable | Skills to use |
| --- | --- | --- |
| Experiment privately with a local assistant or extraction task. Apple's [MLX-LM](https://github.com/ml-explore/mlx-lm) supports local text generation and model fine-tuning on Apple Silicon. | Compare a pinned base and compatible adapter on held-out examples, retaining Metal identity, quality and memory observations. No Queen is needed for local exploration. | [Host setup](../../cohesix-ai-host-setup/SKILL.md) → [MLX workbench](../../cohesix-mlx-workbench/SKILL.md) |
| Prepare image batches for an edge vision pipeline. NVIDIA's [defect-inspection example](https://developer.nvidia.com/blog/automatic-defect-inspection-using-the-nvidia-end-to-end-deep-learning-platform/) describes manufacturing image inspection on Jetson. | Run the shipped grayscale Sobel batch with two distinct user inputs and independently verify every output pixel. A useful bounded preprocessing step precedes any separately supplied detector. | [Workload authoring](../../cohesix-workload-authoring/SKILL.md) → [GPU operations](../../cohesix-gpu-operations/SKILL.md) |
| Keep a camera or video-analysis service observable. [Jetson Platform Services](https://docs.nvidia.com/jetson/jps/inference-services/overview.html) supplies video detection, tracking and language-model services. | Inspect one service, authorize one enrolled lifecycle action if needed, and observe a real application response afterward. Installing its video stack and qualifying a site remain separate work. | [Inspect](../../cohesix-inspect/SKILL.md) → [Edge recovery](../../cohesix-edge-recovery/SKILL.md) → [Evidence](../../cohesix-evidence/SKILL.md) |
| Let a research or operations agent use tools without acquiring broad host credentials. NVIDIA's [AI-Q](https://docs.nvidia.com/aiq-blueprint/latest/index.html) is a self-hostable research backend; [NeMo Agent Toolkit](https://github.com/NVIDIA/NeMo-Agent-Toolkit) supplies agent profiling, evaluation and MCP/A2A integrations. | Connect the pinned Cohesix kit, discover one scoped CUDA/PEFT job, follow its original ID through a disconnect, and compare direct versus governed work when evaluation is requested. AI-Q itself is not bundled. | [Agent delegation](../../cohesix-agent-delegation/SKILL.md) or [NeMo workflows](../../cohesix-nemo-workflows/SKILL.md) |
| Improve a task with a small private model update. Hugging Face's [LoRA guide](https://huggingface.co/docs/peft/main/en/task_guides/lora_based_methods) explains parameter-efficient adaptation. | Freeze the base, dataset and candidate, compare held-out quality, and prove the generation reached by a real client on the selected admitted Linux release path. Local Mac training uses its own observation route. | [Private adapter release](../../cohesix-private-adapter-release/SKILL.md); [MLX workbench](../../cohesix-mlx-workbench/SKILL.md) for Mac exploration |
| Choose a model across development and deployment machines. The [Hugging Face download guide](https://huggingface.co/docs/huggingface_hub/guides/download) supports fetching explicit revisions. | Record candidates, immutable artifact identity and host-specific quality/capacity. Distribution needs a separately advertised, qualified byte-transfer path; a model reference does not deliver weights. | [Model rollout](../../cohesix-model-rollout/SKILL.md) |
| Follow delegated work after an agent disconnects. [A2A's specification](https://a2a-protocol.org/latest/specification/) describes task retrieval and event resubscription. | Reconcile the original Cohesix task, retain unresolved legs and build a source-linked incident case. Fan-in reads can show several hives while each keeps its own mutation authority. | [Agent delegation](../../cohesix-agent-delegation/SKILL.md) → [Fleet operations](../../cohesix-fleet-operations/SKILL.md) → [Evidence](../../cohesix-evidence/SKILL.md) |

If you only need native training, inference or service management, those tools
may already meet the goal. Cohesix adds value when permission, shared-resource
admission, original-job recovery or evidence needs to remain explicit.

## How 1.2.0 features reach users

Review the installed release's [status](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/STATUS.md) and manifest before acting.
The table covers user-facing workflows without treating optional components
or future scope as universal deployment support.

| 1.2.0 surface | Skill coverage and observable result |
| --- | --- |
| Signed Mac `.pkg`, Ubuntu ARM64 controller/SwarmUI `.deb` packages, portable archives and optional QEMU | **Get started / host setup:** verify the selected package and publisher, use matching tools, check the actual host, and preserve state through documented upgrade/removal. Models, CUDA and guest assets need their own preparation. |
| QEMU/Pi Queen, bounded Worker admission, scheduler, leases and telemetry | **Inspect / fleet operations:** show identity, freshness, READY state, reservations, pressure and blockers through the existing gateway. A 256-Worker profile is a bound/declaration; a user's current readiness and performance require observation. |
| CLI, REST, Python, SwarmUI and optional FUSE state view | **Inspect / GPU operations / evidence:** use the same selected namespace and request identity. FUSE is optional; mounting a reference does not transfer model bytes. |
| Registered CUDA work, native owner, cancellation and durable host journals | **Workload authoring / GPU operations:** prepare the user's package and verifier, submit once, follow the original job and independently check its output. |
| Selected MCP/A2A jobs and cumulative standing authority | **Agent delegation:** discover caller-visible jobs, preflight exact scope, submit once and recover the same ID. Read/recovery quota is distinct from new-admission budget. |
| Pinned NeMo kit with native clients, model planners, evaluation and profiling | **NeMo workflows:** prove client behavior and completed evaluation rows, then check the provider independently. The selected kit is Linux AArch64. |
| Mac Shortcuts, Keychain connection and optional Foundation Models explanation | **Agent delegation / host setup:** enroll the selected connection, use a user-created Shortcut and inspect the same durable job. An explanation has no action authority. |
| SwarmUI Local MLX and Python LoRA work | **MLX workbench:** local inference, bounded training and held-out comparison on Metal. Admitted Mac release/serving custody remains planned 28c1. |
| Selected private PEFT release, canary, promotion and rollback | **Private adapter release / model rollout:** correlate frozen inputs and native phases, observe the served generation and verify the exact outcome. Verified cross-host weight distribution requires its separately qualified path. |
| Native service providers and per-hive federation | **Edge recovery / fleet operations:** perform the selected authorized action and retain each hive's local decision and observations. Sector applications remain external. |
| Evidence packs, timelines, comparison and signed causal verification | **Evidence:** deliver a reviewable case with retained source identities, missing observations and the applicable trust check. Capture and signed verification prove different things. |
