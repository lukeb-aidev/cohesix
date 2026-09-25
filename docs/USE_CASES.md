<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Explain practical Cohesix use cases, community needs, and the current and planned capability boundaries. -->
<!-- Author: Lukas Bower -->
# Cohesix Use Cases

A team has a model on a Mac, a CUDA workload on a Jetson, and an operator who
needs to know what actually ran. An agent can suggest a change, but it should not
need a shell on either machine to make that change.

Cohesix gives the team a narrow control path:

> Propose an action → check who may do it and within what limits → run it on the
> right host → inspect the observed result → recover or roll back when necessary.

The Cohesix Queen runs on a small seL4 system. Its Workers track bounded
control, lease, telemetry, and receipt state. CUDA, MLX, model weights, training
data, inference servers, containers, and application data remain on their
ordinary hosts. Cohesix is useful when **the decision to run or change something
must be smaller, clearer, and easier to audit than the system doing the work**.
A request being accepted is not proof that its external effect happened.

This page describes real engineering problems and the Cohesix patterns that
address them. It distinguishes what ships in [Release A](STATUS.md), what is
complete only in current development source, and what [Release B](BUILD_PLAN.md#release-b)
still has to qualify. The examples are deployment patterns, not claims that
Cohesix ships a finished medical, traffic, manufacturing, or robotics product.

| Maturity | How to read it here |
| --- | --- |
| Release A | Shipped workflow with the exact evidence and limits recorded in [Status](STATUS.md). |
| Current development source | Implemented and checked at a named component scope; it is not an installed Release B result. |
| Release B goal | A release-wide journey that still needs installed, integrated qualification under [28g](BUILD_PLAN.md#28g), even when individual components are complete. |

## What you can build

### 1. The Agent Action Airlock

**Situation.** An operations agent notices a failed service or a ready adapter.
Giving it a machine-wide credential would let it do far more than restart that
service or request a release. A bounded Kubernetes action could use the same
pattern after its own provider integration.

**Cohesix's role.** Give the agent a typed, allowlisted action with a subject,
scope, deadline, budget, and explicit refusal. A host provider performs the
approved effect and reports what it observed. The operator can distinguish the
proposal, admission, native result, and verified outcome. If an answer is lost,
the agent inspects the original job instead of blindly retrying the action.

Current CLI, REST, and Python paths use Cohesix's existing control model.
[Milestone 28d](BUILD_PLAN.md#28d) has checked selected authenticated MCP tools
for native Jetson CUDA and PEFT work, including refusal and recovery after a
lost response. [Milestone 28e](BUILD_PLAN.md#28e) has separately checked
selected A2A CUDA and PEFT tasks, original-task recovery, and a NeMo native
client's card, lookup, and cancellation behavior. These are component results,
not an installed Release B journey. The full
[NeMo Agent Toolkit](BUILD_PLAN.md#28f) integration still needs live proof: an
ordinary Toolkit workflow should submit useful CUDA work, delegate a PEFT
release, see a denial, and reconnect to the actual result. Each client must
preserve the same authority, accounting, and job identity. Cohesix does not
replace NeMo's planner or model endpoint, and a tool or task response is not
an independent success receipt.

### 2. The Self-Healing Edge Swarm

**Situation.** Several camera, robot, or production-line hosts report degraded
health. The team needs a common view and a safe way to restore one service
without letting a classifier or dashboard acquire broad control.

**Cohesix's role.** Heartbeat Workers and bounded telemetry show where to look.
A host-side model can rank likely faults. Any remediation returns through a
specific ticket or schedule request, with an observed provider result and a
visible unresolved state when recovery is uncertain. Raw video, tensors, and
high-volume event streams remain in the application data plane.

The health and admission primitives exist today. Autonomous diagnosis,
site-specific fail-safe behavior, and recovery quality need a real deployment
and its own evidence. The named manufacturing and traffic playbooks below
rehearse control relationships; they do not run a safety application.

### 3. The GPU Flight Deck

**Situation.** A team wants to run batch inference or embedding work on a
Jetson while other jobs share the device. They need to know which model and
inputs were approved, whether the GPU was available, which process actually
ran, and whether its outputs are the expected ones.

**Cohesix's role.** An operator registers an approved, pinned workload with a
fixed entry point, typed inputs, and resource limits. The Queen admits a job;
a constrained host executor runs it on the selected NVIDIA device. The operator
can inspect device freshness, queue and lease state, native execution, output
hashes, and cancellation or recovery. An active lease is a control decision, not
a claim of hard GPU isolation.

**Where it stands.** [Release A](STATUS.md) includes the native CUDA foundation
and recoverable recipes. [Milestone 28a](BUILD_PLAN.md#28a) adds selected,
source-complete workload registration and independently checked useful work on
the Orin through systemd and Docker. Release B still needs installed, integrated
qualification. Other NVIDIA GPUs and deployment profiles need their own proof.
See [GPU Nodes](GPU_NODES.md).

### 4. Model Rollout with a Flight Recorder

**Situation.** A team must decide which model or adapter should serve next.
A developer explores a candidate on an Apple Silicon Mac, then compares it
with a CUDA-backed deployment on a Jetson. They need to know which host did
each stage, whether any private artifact moved, and which generation a real
client reached after promotion or rollback.

**Cohesix's role.** The planned Release B journey names the provider and host
for each stage, checks capability and capacity, preserves one job lineage, and
shows local Metal observations separately from verified remote CUDA outcomes.
A model reference is not a file transfer. An accepted model choice is not a
verified deployment. If distribution is selected, the host data path must
deliver the bytes and verify the full destination hash before a separate
activation decision.

Cohesix already has bounded model descriptors and an active identifier, while
Release A's LoRA path adds a recoverable adapter transaction. There is no
dedicated built-in playbook for a complete model rollout. Release B aims to
show a real client reaching the exact canary or restored generation.

[Milestone 28c](BUILD_PLAN.md#28c) has checked local MLX/Metal work and native
Shortcuts and App Intents on a selected Mac. [Milestone 28c1](BUILD_PLAN.md#28c1)
has separately checked admitted Mac training, recovery after an interrupted
release, and generation-fenced serving through a pinned
[vMLX](https://github.com/jjang-ai/vmlx) engine. Those are selected component
results, not broad Mac or Release B qualification. vMLX's model gateway and
its MCP client play different roles: the separate vMLX MCP client path was not
qualified by 28d, and model text cannot certify a Cohesix job.
[Milestone 28e](BUILD_PLAN.md#28e) checked selected Jetson A2A jobs; a
separate agent peer using a vMLX model endpoint remains unqualified. Neither
path makes vMLX itself a Cohesix executor or an A2A peer. Mixed MLX/CUDA
composition, vMLX as an MCP client or as a model endpoint for an A2A peer,
and optional weight distribution still need their own live acceptance.

### 5. The Private LoRA Foundry

**Situation.** A team fine-tunes a small adapter for a local task. Training
finished, but that alone does not answer whether the adapter improves the
application, matches the intended base model, or is the version currently
serving users.

**Cohesix's role.** Keep the base revision, adapter, dataset snapshot, runtime,
and request identity together. Train or import through an approved host path;
compare the candidate with the base and incumbent on held-out data; check the
artifact; stage it; observe a real inference canary; then promote or roll back
the exact serving generation. A failed comparison stays visible. A lost response
is followed under its original operation identity so it cannot quietly start a
second effect. Private weights and data stay on the selected host unless a
separately authorised transfer is made.

**Where it stands.** [Release A](STATUS.md) includes the verified private LoRA
transaction and its [operator guide](PRIVATE_LORA_RELEASE.md).
[Milestone 28b](BUILD_PLAN.md#28b) has checked configurable PEFT training and
import, held-out comparison, a real application request to the promoted
generation, and recovery to the incumbent after an interrupted promotion on a
selected NVIDIA/KVM component. The installed, integrated Release B journey
still needs qualification. LoRA, QLoRA, MLX, and other adapter formats are
compatible only where the exact model and runtime pair has been tested.

### 6. Multi-Hive Mission Control

**Situation.** An operator manages a Mac, a Jetson, and several edge hives.
They want one read-only view of pressure and failures, but each hive must keep
authority over its own mutations.

**Cohesix's role.** Host-side fan-in gives a fleet picture. A bounded ticket
relay can ask a selected hive to act while local admission still decides.
Receipts retain the source and destination relationship; a relay cannot promise
exactly-once behavior for an unobserved external effect. Leases, quotas, and
provider evidence remain separate in the display.

Current host composition and the Python playbooks provide starting points.
Each live site still needs target, authentication, provider, failure, and
retention checks. The mixed factory and logistics names below describe possible
applications of the pattern, not shipped industry integrations.

### Review an incident or a change after the fact

**Situation.** A rollout appears to have succeeded, but a user saw the old
model, a service restarted twice, or a recovery request timed out.

**Cohesix's role.** Capture bounded status and traces, compare before and after
evidence packs, and build a source-linked timeline for incident, maintenance,
rollout, or federation review. Keep the request, admission, provider observation,
and signed result separate. A retained replay helps explain an event; it does
not turn an old host observation into fresh target or hardware proof.

This [Release A operator workflow](OPERATOR_EVIDENCE.md) is useful on its own
and is also the evidence path behind the other scenarios.

## What recent community discussions tell us

This is a snapshot of public engineering conversations checked on
**26 September 2026**. A thread is evidence of a reported problem or design
question, not proof that every user has it or that Cohesix fixes the underlying
library. The Release B fit is a proposed control or verification benefit.

| Community and where to join | Recent discussion | What that means for a Cohesix use case |
| --- | --- | --- |
| Apple [MLX](https://github.com/ml-explore/mlx), [MLX-LM issues](https://github.com/ml-explore/mlx-lm/issues), and [discussions](https://github.com/ml-explore/mlx-lm/discussions) | An [MLX-LM serving report](https://github.com/ml-explore/mlx-lm/issues/1834) describes requests hanging while the HTTP process still answers health-like calls; a [conversion proposal](https://github.com/ml-explore/mlx-lm/issues/1842) examines large-checkpoint disk limits. | Observe a real inference canary and memory/capacity on the selected Mac. Do not treat a live process, model listing, or conversion plan as a serving or transfer result. |
| [vMLX issues](https://github.com/jjang-ai/vmlx/issues) and [release notes](https://github.com/jjang-ai/vmlx/releases) | Users have reported [front-end cancellation that left work running](https://github.com/jjang-ai/vmlx/issues/100) and [model-specific tool-call parsing problems](https://github.com/jjang-ai/vmlx/issues/226); recent releases discuss MCP errors, caching, and cancellation. | Prove the client actually invokes the admitted operation, handles refusal, and reconnects under the original identity. Test vMLX as a named client; its model output remains a proposal. |
| [NVIDIA Jetson/CUDA forums](https://forums.developer.nvidia.com/c/robotics-edge-computing/jetson-systems/jetson-orin-nano/632) | Developers compare [PyTorch container and Orin compute-capability behavior](https://forums.developer.nvidia.com/t/pytorch-container-26-06-py3-missing-compute-capability-8-7-kernels-for-jetson-orin-nano/375642) and [GPU allocation with another runtime](https://forums.developer.nvidia.com/t/pytorch-cudacachingallocator-nvml-assertion-when-sharing-cuda-context-with-llama-cpp-on-orin-nano-8-gb-jetpack-6-2-2/370049). The first warning was clarified by NVIDIA as harmless for that case. | Pin the actual JetPack, container, CUDA, device, and workload combination; use real execution and resource observation instead of inferring support or isolation from labels and warnings. |
| Hugging Face [PEFT and LoRA issues](https://github.com/huggingface/peft/issues) and [forums](https://discuss.huggingface.co/) | September reports cover [adapter combinations that cannot be reloaded](https://github.com/huggingface/peft/issues/3737) and [merges that change some adapter outputs](https://github.com/huggingface/peft/issues/3761). A [serving discussion](https://discuss.huggingface.co/t/case-study-serving-a-qwen-2-5-32b-raft-adapter-finance-on-zerogpu/172207) asks whether to merge a QLoRA adapter for deployment. | Record base and adapter identity, reject unsupported combinations, compare actual outputs, and verify the served generation before declaring a release successful. |
| [NeMo Agent Toolkit issues](https://github.com/NVIDIA/NeMo-Agent-Toolkit/issues), [docs](https://docs.nvidia.com/nemo/agent-toolkit/latest/), and [NVIDIA NeMo forum](https://forums.developer.nvidia.com/c/ai-data-science/nvidia-nemo/715) | A [per-user MCP plus A2A configuration report](https://github.com/NVIDIA/NeMo-Agent-Toolkit/issues/2162) and a [schema/result fidelity discussion](https://github.com/NVIDIA/NeMo-Agent-Toolkit/issues/2138) show why protocol labels alone do not prove an agent workflow. | Test the pinned Toolkit's native clients with real delegated credentials, typed results, denial, and reconnect. Trace links help explain work but cannot replace the provider's verified outcome. |

These conversations make the Release B priorities concrete: a useful CUDA job,
a private adapter whose deployed quality is checked, a Mac path that labels
local versus remote work, and agent clients that can survive refusal and lost
responses. Model runners and data stay on their hosts; each model and runtime
combination needs its own compatibility check.

## Try the nine control-model playbooks

The Python package contains nine named playbooks that let a contributor inspect
the approvals, schedules, leases, exports, and local provider probes involved
in these ideas. From a source checkout:

    python3 -m pip install -e tools/cohesix-py
    cohesix-playbook --list
    cohesix-playbook --playbook mixed-closed-loop-ai-factory --dry-run --mock
    jq '{workflow_kind, use_case_id, plan_summary, production_use_case_accepted}' \
      out/examples/playbooks/mixed-closed-loop-ai-factory/report.json

The dry run's report identifies a control model and says
"production_use_case_accepted": false. It does not train, serve, evaluate,
deploy, or control a factory. Removing --dry-run --mock submits the generic
control plan to the selected backend; it still does not create a complete
sector workflow. Local probes inspect the machine running the playbook, so a
Mac rehearsal cannot silently discover a remote Jetson.

| Use-case pattern | Built-in playbooks | What they rehearse |
| --- | --- | --- |
| Agent Action Airlock | mac-endpoint-compliance, jetson-critical-infra | Narrow admission, scheduling, and provider relationships. |
| Self-Healing Edge Swarm | mac-release-factory, jetson-manufacturing-safety, jetson-traffic-safety | Health and remediation control relationships. |
| GPU Flight Deck | mixed-medical-edge-ai | Lease, quota, export, and selected provider probes; no medical workload or compliance claim. |
| Model Rollout with a Flight Recorder | No dedicated built-in playbook yet. | The staged activation idea; a complete built-in rollout remains to be added. |
| Private LoRA Foundry | mac-private-peft-grid | Worker LoRA, lease, export, and provider boundaries; not a training result. |
| Multi-Hive Mission Control | mixed-closed-loop-ai-factory, mixed-logistics-digital-twin | Cross-hive control relationships; not accepted factory or logistics applications. |

The compiler-owned [dependency graph](../configs/generated/host_integration_dependency.json)
and generated [support table](snippets/host_integration_dependency.md)
identify the playbook dependencies and evidence modes. The playbook catalogue
is a way to explore a deployment, not a shortcut past those dependencies.

## How to decide whether Cohesix fits

Cohesix fits when the hard part is deciding **who may change what**, keeping a
job within declared limits, and finding out what happened after an interruption.
It is especially useful where a powerful host runtime must remain outside a
small control plane.

It is a poor fit if the proposed solution needs POSIX applications, CUDA or
Metal kernels, raw model weights, a trainer, a general container runtime,
high-volume media streams, arbitrary RPC, or an extra network listener inside
the seL4 VM. Offline durability requires an accepted persistent target profile
or host store; an in-memory Worker alone cannot promise it.

Before adopting a scenario, ask:

1. Which exact target and host profiles are in use, and which provider owns the
   real data plane?
2. What action, subject, bounds, and refusal authorize each mutation?
3. What proves that the external effect happened, beyond admission or HTTP
   success?
4. If a reply is lost or a process restarts, how is the original job found?
5. Where do private inputs, model bytes, credentials, and retained evidence go?
6. What is verified on this target now, and what is only a fixture, pattern, or
   Release B goal?

Start with the [Quickstart](QUICKSTART.md), the [Operator Walkthrough](OPERATOR_WALKTHROUGH.md),
or the [Private LoRA guide](PRIVATE_LORA_RELEASE.md). The [Build Plan](BUILD_PLAN.md)
owns milestone scope, [Status](STATUS.md) records current evidence,
and the [Test Plan](TEST_PLAN.md) defines the proof needed for a claim.

## Build the next reference journey with us

The best contribution is one narrow workflow another engineer can reproduce:
an approved workload, a fixed dataset and quality measure, a real provider
observation, a failure case, and an honest result. Release B's concrete owners
are [28a CUDA work](BUILD_PLAN.md#28a), [28b PEFT serving](BUILD_PLAN.md#28b),
[28c Apple compute](BUILD_PLAN.md#28c), [28c1 governed Mac rollout](BUILD_PLAN.md#28c1),
[28d MCP](BUILD_PLAN.md#28d),
[28e A2A](BUILD_PLAN.md#28e), [28f NeMo](BUILD_PLAN.md#28f), and
[28g installation and qualification](BUILD_PLAN.md#28g).

Useful independent work includes better provider fixtures and negative cases,
readable receipt and recovery views, installed CLI/Python/SwarmUI help,
repeatable Jetson and Mac examples, and a sector integration that supplies its
own safety and target evidence. Broader provider catalogues and domain
applications have separate deferred ownership in the Build Plan. See
[Contributing](../CONTRIBUTING.md) and [Operator Recipes](OPERATOR_RECIPES.md)
for the current entry points.
