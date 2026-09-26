<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Explain practical Release B Cohesix use cases, community needs, operating skills and proof boundaries. -->
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

This guide describes what to do with a **matching Release B installation**:
useful CUDA jobs, private adapter releases, Mac MLX work and agent clients
that share one governed job and evidence model. Check the installed version,
selected profile and [Status](STATUS.md) before citing present availability;
this guide is not a release certificate. The examples are deployment patterns,
not claims that Cohesix supplies a finished medical, traffic, manufacturing
or robotics application.

The basic distinction is simple: Cohesix decides and records **whether a
specific action may run**. The selected Mac, Jetson or other prepared host
runs the model, service or container. The result needs observation and, when
claimed as verified, the applicable independent evidence check.

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

An installed agent can discover selected operations over MCP, or delegate a
durable job over A2A. A person can use CLI, Python or a Mac Shortcut for the
same underlying action. With [NeMo Agent Toolkit](BUILD_PLAN.md#28f), a useful
journey is: inspect available work, request a CUDA job or PEFT release, see a
real denial for an out-of-scope action, disconnect, then recover the original
result. Each client keeps the same subject, budget and job identity. Cohesix
does not replace NeMo's planner or model endpoint; tool and task responses do
not independently certify the native outcome.

Use the [agent delegation skill](../skills/cohesix-agent-delegation/SKILL.md)
to select MCP versus A2A, scope a request and reconcile a lost response.

### 2. The Self-Healing Edge Swarm

**Situation.** Several camera, robot, or production-line hosts report degraded
health. The team needs a common view and a safe way to restore one service
without letting a classifier or dashboard acquire broad control.

**Cohesix's role.** Heartbeat Workers and bounded telemetry show where to look.
A host-side model can rank likely faults. Any remediation returns through a
specific ticket or schedule request, with an observed provider result and a
visible unresolved state when recovery is uncertain. Raw video, tensors, and
high-volume event streams remain in the application data plane.

Start with fresh target and host observations, then request one allowlisted
recovery action and observe the application afterward. Autonomous diagnosis,
site-specific fail-safe behavior and recovery quality still need evidence for
that site. The manufacturing and traffic playbooks below rehearse control
relationships; they do not run a safety application. Use the
[edge recovery skill](../skills/cohesix-edge-recovery/SKILL.md) for the
before/action/after workflow.

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

The Release B reference uses selected, digest-pinned workloads and independently
checked useful work on a Jetson Orin through systemd and Docker. A different
NVIDIA device, driver, container or executor profile needs its own compatibility
and native-result check. See [GPU Nodes](GPU_NODES.md) and the
[GPU operations skill](../skills/cohesix-gpu-operations/SKILL.md).

### 4. Model Rollout with a Flight Recorder

**Situation.** A team must decide which model or adapter should serve next.
A developer explores a candidate on an Apple Silicon Mac, then compares it
with a CUDA-backed deployment on a Jetson. They need to know which host did
each stage, whether any private artifact moved, and which generation a real
client reached after promotion or rollback.

**Cohesix's role.** A governed rollout names the provider and host
for each stage, checks capability and capacity, preserves one job lineage, and
shows local Metal observations separately from verified remote CUDA outcomes.
A model reference is not a file transfer. An accepted model choice is not a
verified deployment. If distribution is selected, the host data path must
deliver the bytes and verify the full destination hash before a separate
activation decision.

The Mac path distinguishes local MLX/Metal exploration from an admitted Mac
release. On an advertised profile, the admitted path can train, compare,
serve a canary and recover the accepted generation, with optional
[vMLX](https://github.com/jjang-ai/vmlx) serving bound to that generation.
vMLX serving, vMLX as an MCP client and a separate A2A peer using its model
endpoint are different integrations. A model response cannot certify a Cohesix
job. Mixed MLX/CUDA work and verified weight distribution are usable only
where the installed profile advertises and qualifies those paths. There is
still no dedicated built-in Python playbook for a complete model rollout;
the [model rollout skill](../skills/cohesix-model-rollout/SKILL.md) gives an
operator a concrete comparison, transfer and serving checklist.

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

The [private LoRA guide](PRIVATE_LORA_RELEASE.md) covers native training or
import, evaluation, canary and verified rollback. A registry pointer is not
proof of serving. The selected model, adapter and runtime pair must be
compatible; the guide's reference result does not qualify arbitrary LoRA,
QLoRA or MLX combinations. Use the
[private adapter release skill](../skills/cohesix-private-adapter-release/SKILL.md)
to freeze the comparison and follow a real client to the accepted generation.

### 6. Multi-Hive Mission Control

**Situation.** An operator manages a Mac, a Jetson, and several edge hives.
They want one read-only view of pressure and failures, but each hive must keep
authority over its own mutations.

**Cohesix's role.** Host-side fan-in gives a fleet picture. A bounded ticket
relay can ask a selected hive to act while local admission still decides.
Receipts retain the source and destination relationship; a relay cannot promise
exactly-once behavior for an unobserved external effect. Leases, quotas, and
provider evidence remain separate in the display.

Host-side composition and the Python playbooks provide starting points. Each
live site still needs target, authentication, provider, failure and retention
checks. The mixed factory and logistics names below describe possible uses of
the pattern, not shipped industry integrations. Use the
[fleet operations skill](../skills/cohesix-fleet-operations/SKILL.md) to keep
each hive's freshness, authority and result visible.

### Review an incident or a change after the fact

**Situation.** A rollout appears to have succeeded, but a user saw the old
model, a service restarted twice, or a recovery request timed out.

**Cohesix's role.** Capture bounded status and traces, compare before and after
evidence packs, and build a source-linked timeline for incident, maintenance,
rollout, or federation review. Keep the request, admission, provider observation,
and signed result separate. A retained replay helps explain an event; it does
not turn an old host observation into fresh target or hardware proof.

This [operator workflow](OPERATOR_EVIDENCE.md) is useful on its own and is the
evidence path behind the other scenarios. Use the
[evidence skill](../skills/cohesix-evidence/SKILL.md) to build a case without
turning a partial capture into a success claim.

## What recent community discussions tell us

This is a snapshot of public engineering conversations checked on
**26 September 2026**. A thread shows a reported problem or design question;
it does not mean every deployment has that problem or that Cohesix fixes the
underlying library. It tells an operator what to observe and test.

| Community and where to join | Recent discussion | What that means for a Cohesix use case |
| --- | --- | --- |
| Apple [MLX](https://github.com/ml-explore/mlx), [MLX-LM issues](https://github.com/ml-explore/mlx-lm/issues), and [discussions](https://github.com/ml-explore/mlx-lm/discussions) | An [MLX-LM serving report](https://github.com/ml-explore/mlx-lm/issues/1834) describes requests hanging while the HTTP process still answers health-like calls; a [conversion proposal](https://github.com/ml-explore/mlx-lm/issues/1842) examines large-checkpoint disk limits. | Observe a real inference canary and memory/capacity on the selected Mac. Do not treat a live process, model listing, or conversion plan as a serving or transfer result. |
| [vMLX issues](https://github.com/jjang-ai/vmlx/issues) and [release notes](https://github.com/jjang-ai/vmlx/releases) | Users have reported [front-end cancellation that left work running](https://github.com/jjang-ai/vmlx/issues/100) and [model-specific tool-call parsing problems](https://github.com/jjang-ai/vmlx/issues/226); recent releases discuss MCP errors, caching, and cancellation. | Prove the client actually invokes the admitted operation, handles refusal, and reconnects under the original identity. Test vMLX as a named client; its model output remains a proposal. |
| [NVIDIA Jetson/CUDA forums](https://forums.developer.nvidia.com/c/robotics-edge-computing/jetson-systems/jetson-orin-nano/632) | Developers compare [PyTorch container and Orin compute-capability behavior](https://forums.developer.nvidia.com/t/pytorch-container-26-06-py3-missing-compute-capability-8-7-kernels-for-jetson-orin-nano/375642) and [GPU allocation with another runtime](https://forums.developer.nvidia.com/t/pytorch-cudacachingallocator-nvml-assertion-when-sharing-cuda-context-with-llama-cpp-on-orin-nano-8-gb-jetpack-6-2-2/370049). The first warning was clarified by NVIDIA as harmless for that case. | Pin the actual JetPack, container, CUDA, device, and workload combination; use real execution and resource observation instead of inferring support or isolation from labels and warnings. |
| Hugging Face [PEFT and LoRA issues](https://github.com/huggingface/peft/issues) and [forums](https://discuss.huggingface.co/) | September reports cover [adapter combinations that cannot be reloaded](https://github.com/huggingface/peft/issues/3737) and [merges that change some adapter outputs](https://github.com/huggingface/peft/issues/3761). A [serving discussion](https://discuss.huggingface.co/t/case-study-serving-a-qwen-2-5-32b-raft-adapter-finance-on-zerogpu/172207) asks whether to merge a QLoRA adapter for deployment. | Record base and adapter identity, reject unsupported combinations, compare actual outputs, and verify the served generation before declaring a release successful. |
| [NeMo Agent Toolkit issues](https://github.com/NVIDIA/NeMo-Agent-Toolkit/issues), [docs](https://docs.nvidia.com/nemo/agent-toolkit/latest/), and [NVIDIA NeMo forum](https://forums.developer.nvidia.com/c/ai-data-science/nvidia-nemo/715) | A [per-user MCP plus A2A configuration report](https://github.com/NVIDIA/NeMo-Agent-Toolkit/issues/2162) and a [schema/result fidelity discussion](https://github.com/NVIDIA/NeMo-Agent-Toolkit/issues/2138) show why protocol labels alone do not prove an agent workflow. | Test the pinned Toolkit's native clients with real delegated credentials, typed results, denial, and reconnect. Trace links help explain work but cannot replace the provider's verified outcome. |

Across these conversations, four checks matter: did useful CUDA work really
run; did a private adapter improve and reach a real client; which work was
local Metal versus remote CUDA; and could the agent recover after refusal or
a lost response? Model runners and data stay on their hosts. Each model and
runtime combination still needs its own compatibility check.

## Try the nine control-model playbooks

The Python package contains nine named playbooks for exploring the approvals,
schedules, leases, exports and local provider probes involved in these ideas.
They are **control-model rehearsals**. The skills above guide real operating
work against a matching deployment. From a source checkout:

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
helps design a deployment; it does not satisfy those dependencies or confer
sector acceptance.

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
6. Which operations are qualified on this exact installed profile, and which
   examples are only fixtures or design patterns?

Start with the [Quickstart](QUICKSTART.md), the [Operator Walkthrough](OPERATOR_WALKTHROUGH.md),
or the [Private LoRA guide](PRIVATE_LORA_RELEASE.md). The [Build Plan](BUILD_PLAN.md)
owns milestone scope, [Status](STATUS.md) records current evidence,
and the [Test Plan](TEST_PLAN.md) defines the proof needed for a claim.

## Bring a real workflow

A useful contribution is one narrow journey another engineer can reproduce:
an approved workload, fixed inputs and quality measure, a native provider
observation, a refusal or interruption, and an honest result. The repo-managed
skills under [`skills/`](../skills/) give starting procedures; they do not
activate capabilities or replace the installed guide and operator policy.
For a new Mac or NVIDIA environment, start with
[AI host setup](../skills/cohesix-ai-host-setup/SKILL.md). For an unfamiliar
Queen, start with [read-only inspection](../skills/cohesix-inspect/SKILL.md).

Good additions include a reproducible model/runtime pair, a negative case
that shows a refusal clearly, a better receipt or recovery view, or a sector
integration with its own provider and safety evidence. The
[Build Plan](BUILD_PLAN.md) owns scope and the [Test Plan](TEST_PLAN.md) owns
proof. See [Contributing](../CONTRIBUTING.md) and
[Operator Recipes](OPERATOR_RECIPES.md) for the current entry points.
