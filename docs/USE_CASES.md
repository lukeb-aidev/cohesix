<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Explain where current Cohesix capabilities fit and how to assess candidate deployments. -->
<!-- Author: Lukas Bower -->
# Cohesix Use Cases

Cohesix gives an operator or AI agent something more useful than a shell and
safer than an all-powerful automation token:

> The agent proposes. Cohesix admits or refuses. A constrained host provider
> acts. The operator can inspect the request, result, and evidence separately.

That pattern matters when a camera, robot, factory line, private model host, or
small edge fleet can be changed remotely but the authority to change it should
be much smaller than the applications and GPU stacks being governed. Cohesix is
the bounded seL4 control and evidence layer; CUDA, model runtimes, Kubernetes,
service managers, domain applications, and high-volume data remain host-side.

## Four Reasons to Use Cohesix

| Real-world problem | What Cohesix changes |
| --- | --- |
| An AI operations agent needs to restart one service but must not receive a shell or cluster credential. | The request becomes one typed, allowlisted action with an independent admission decision and terminal host receipt. |
| Several workloads compete for an edge GPU and operators need to know who requested what. | Lease intent, quota, priority, Worker state, provider observation, and execution evidence remain distinguishable. |
| A private model or LoRA adapter must move from preparation to deployment without hiding the handoff in scripts. | Artifact identity, approval, activation intent, host execution, verification, and rollback can become separate reviewable stages. |
| A multi-node incident needs coordinated recovery without one controller receiving ambient authority over every node. | Each hive keeps local admission authority while bounded host relays and evidence provide a fleet view. |

These are capability patterns, not packaged sector applications. Cohesix does
not claim that a named medical, traffic, manufacturing, cloud, GPU, or
compliance environment is supported out of the box.

## Try the Current Control Model

The Python package includes nine bounded playbooks for rehearsing control and
evidence relationships before connecting a live provider. List them, then try a
mixed-fleet example without issuing any control writes:

```bash
python3 -m pip install -e tools/cohesix-py
cohesix-playbook --list
cohesix-playbook \
  --playbook mixed-closed-loop-ai-factory \
  --dry-run \
  --mock

jq '{workflow_kind, use_case_id, plan_summary, production_use_case_accepted}' \
  out/examples/playbooks/mixed-closed-loop-ai-factory/report.json
```

The expected boundary is explicit:

```json
{
  "workflow_kind": "control-model",
  "use_case_id": "multi-hive-mission-control",
  "plan_summary": {
    "approvals": 3,
    "schedule": 2,
    "leases": 1,
    "exports": 1
  },
  "production_use_case_accepted": false
}
```

This is useful because it exposes the approvals, schedules, leases, exports,
and local provider probes that a deployment must resolve. It does **not** train,
serve, deploy, or evaluate a model. Removing `--dry-run --mock` submits the
current generic control plan to the selected backend; it still does not turn the
fixture into a complete sector workflow.

## What the Nine Playbooks Actually Cover

The compiler-owned graph maps every built-in playbook to one of the six patterns
below. The Python list and report output expose the same `use_case_id` and local
provider-probe selection.

| Capability pattern | Current control-model playbooks | Honest interpretation |
| --- | --- | --- |
| The Agent Action Airlock | `jetson-critical-infra`, `mac-endpoint-compliance` | Rehearse narrow admission, scheduling, quota/export, and selected provider relationships; no sector application is executed. |
| The Self-Healing Edge Swarm | `jetson-manufacturing-safety`, `jetson-traffic-safety`, `mac-release-factory` | Rehearse health/remediation control relationships; there is no autonomous diagnosis or fail-safe recovery claim. |
| The GPU Flight Deck | `mixed-medical-edge-ai` | Rehearse GPU lease/quota/export relationships and selected provider probes; no medical workload or compliance claim. |
| Model Rollout with a Flight Recorder | No dedicated built-in playbook yet | The capability pattern is documented, but a complete staged rollout remains planned work. |
| The Private LoRA Foundry | `mac-private-peft-grid` | Rehearse WorkerLora, lease, export, and provider boundaries; training and runtime reload remain external. |
| Multi-Hive Mission Control | `mixed-closed-loop-ai-factory`, `mixed-logistics-digital-twin` | Rehearse cross-fleet control relationships; federation and domain applications still require live conformance. |

Exact transitive prerequisites are compiler-owned in
[`configs/generated/host_integration_dependency.json`](../configs/generated/host_integration_dependency.json).
The generated [support table](snippets/host_integration_dependency.md) names the
required evidence mode and owning milestone for each dependency. Python tests
now fail if a playbook's use-case mapping or selected local provider probes
drift from that graph.

Current probes observe only the machine running `cohesix-playbook`; they do not
discover a remote execution topology. For example, a Mac control rehearsal may
report its required NVIDIA provider as unavailable rather than silently
pretending that it found a Jetson. Explicit Mac-controller, remote-CUDA/Jetson,
and optional Apple-GPU executor selection belongs to the complete Milestone 27b
workflow.

## Current Capability Boundary

| Capability | Current boundary |
| --- | --- |
| Operator control | Authenticated console grammar projected by `cohsh`, `coh`, the REST gateway, Python, and other host tools. There is no independent in-VM 9P/TCP listener. |
| Authority | Role-scoped tickets, manifest-defined namespaces, bounded file operations, and explicit policy gates. |
| Orchestration | Queen control files plus profile-declared Heartbeat, GPU, and LoRA Worker roles. Configured execution is separate from target, provider, and use-case acceptance. |
| Observability | Bounded `/proc`, `/log`, Worker telemetry, driver counters, and host-projected status. Retention and durability depend on the selected profile and host integration. |
| Host integration | REST, Python, GPU inventory, model-registry descriptors, and host adapters project existing Cohesix semantics; they do not create VM authority. |
| Heavy runtimes | CUDA, NVML, Kubernetes, systemd, Docker, model training/inference, field protocols, and application data planes remain outside the VM trusted computing base. |

The selected source manifest, resolved manifest, and generated `coh-rtc` output
define the namespaces, roles, limits, and host projections in a build. See
[Status](STATUS.md) for the current evidence boundary and the
[Glossary](GLOSSARY.md) for Cohesix-specific terms.

## Six AI Hive Scenarios

Queen and Worker are control-plane roles, not language models running inside
the VM. Models and agent frameworks stay host-side and may propose intent.
Specialized Workers contribute bounded lifecycle, telemetry, lease, or receipt
state; an executable slot alone proves neither an external action nor a use
case.

### 1. The Agent Action Airlock

**Maturity: as-built admission primitives; deployment integration pattern.**

An AI operations agent requests one service, Kubernetes, GPU-lease, or model
action without receiving a general shell. Cohesix checks the role, path, schema,
bounds, and policy. A separately constrained host provider executes only an
allowlisted action and returns an observed result.

Current code provides the bounded ticket and admission primitives. A production
deployment must still prove delegated identity, writer fencing, provider
conformance, rollback, and a fail-safe terminal state. `OK` means admission, not
that the external effect happened.

### 2. The Self-Healing Edge Swarm

**Maturity: as-built heartbeat and telemetry surfaces; deployment integration
pattern.**

Heartbeat Workers and bounded target state provide health signals. A host model
may rank anomalies, but remediation returns through the Queen as an allowlisted
ticket or schedule request. The useful product is the reviewable loop from
observation to admission to host result—not a claim that the VM autonomously
diagnoses or heals the system.

Raw video, tensors, and unbounded event streams stay outside Cohesix. Alert
quality, retry limits, provider rollback, durability, and the uncertain terminal
state remain deployment responsibilities.

### 3. The GPU Flight Deck

**Maturity: as-built inventory, lease, status, and Worker GPU records;
deployment integration pattern for execution.**

`gpu-bridge-host` can publish a bounded accelerator and model view. A GPU Worker
carries scoped lifecycle, lease, telemetry, and receipt state, so the Queen can
coordinate requests without putting CUDA, NVML, weights, or raw GPU access
inside the VM.

The host executor must still enforce memory, stream, lifetime, revocation, and
device isolation and return an observed result. An `ACTIVE` lease is intent and
state evidence, not GPU isolation or workload-execution proof. See
[GPU_NODES.md](GPU_NODES.md).

### 4. Model Rollout with a Flight Recorder

**Maturity: as-built model descriptors and active identifier; deployment
integration pattern for activation and rollback.**

In the intended canary flow, an agent recommends a model, the Queen admits a
bounded activation request, and a host executor verifies the artifact, changes
the runtime, observes health, and returns an authoritative result. The key
distinction is “identifier admitted” versus “runtime reloaded and verified.”

Current primitives can represent parts of this flow, but no dedicated built-in
playbook performs the complete staged rollout. Cohesix does not store weights or
hot-reload inference; those remain host-provider responsibilities.

### 5. The Private LoRA Foundry

**Maturity: as-built executable Worker LoRA receipt path and bounded host PEFT
helpers; target and provider acceptance remain profile-dependent.**

A private pool prepares an adapter host-side, imports size-checked and hashed
metadata, and requests activation through the Queen. WorkerLora can record
bounded terminal receipts but never trains, evaluates, scans, loads, or serves a
model.

The `mac-private-peft-grid` control model is the closest current rehearsal. A
production path still needs real provider execution, artifact verification,
evaluation, rollback, and exact target evidence. See
[Python Support](PYTHON_SUPPORT.md) and
[Userland and CLI](USERLAND_AND_CLI.md).

### 6. Multi-Hive Mission Control

**Maturity: as-built manifest-driven host-ticket relay and read-only fleet
fan-in; deployment integration pattern across accepted hives.**

Several hives can present a read-only operational picture while mutation
authority remains local. A host coordinator can recommend where to investigate,
move a lease, or recover a service without receiving ambient authority over
every target.

Current host composition provides useful control-model and fan-in primitives,
but every peer still needs accepted target proof, authentication, provider
execution, failure policy, durable receipt correlation, and evidence retention.
Federation is host-side composition, not exactly-once external execution.

## Build the Missing Pieces with Us

The most valuable community work is not another sector name. It is turning one
of these control patterns into a narrow, reproducible, evidence-backed reference
journey. The Build Plan owns when each item becomes active; confirm its status
before implementation.

| Contribution | Why it matters | Planned owner |
| --- | --- | --- |
| Replace generic playbook endings with generated `preflight → admit → execute → observe → verify → recover` stages. | Makes a playbook a real workflow rather than a persuasive name around control writes. | `m27b-live-reference-workflows` in [Milestone 27b](BUILD_PLAN.md#27b) |
| Prove the portable Linux AArch64 NVIDIA path on Jetson, with explicit CUDA/NVML versions and bounded real workloads. | Gives Cohesix one compelling, reproducible edge-AI reference instead of a mock GPU story. | `m27b-jetson-orin-nano-live-conformance` in [Milestone 27b](BUILD_PLAN.md#27b) |
| Make host and Worker receipts authoritative and causally linked. | Lets operators reconstruct whether a request was admitted, executed, observed, and verified. | `m27b-authoritative-receipt-and-evidence-core` in [Milestone 27b](BUILD_PLAN.md#27b) |
| Build the visual Live AI Hive journey in SwarmUI. | Makes capability boundaries, provider truth, failures, and evidence understandable to newcomers. | `m27f-live-ai-community-showcase` in [Milestone 27f](BUILD_PLAN.md#27f) |
| Add focused provider fixtures, negative tests, and documentation. | Gives seL4, Rust, Python, GPU, and operations contributors useful pieces that can be reviewed independently. | [Test Plan](TEST_PLAN.md) and the active owning task |

Good contributions preserve Cohesix's distinctive boundary: small VM authority,
typed refusal, bounded execution, host-side data planes, and evidence that never
promotes itself. Start with [Contributing](../CONTRIBUTING.md), reproduce the
[Quickstart](QUICKSTART.md), and use [Operator Recipes](OPERATOR_RECIPES.md) to
find a workflow worth improving.

## How to Read Maturity Claims

| Term | Meaning |
| --- | --- |
| **As-built** | Present in current source or compiler-generated profile output and covered by repository tests. |
| **Evidence-backed** | As-built behavior with target-qualified evidence named in canonical audit or Test Plan records. |
| **Integration pattern** | A deployment composition using current Cohesix boundaries but requiring project-specific host software or policy. |
| **Planned** | Owned by a pending or future Build Plan task and not current behavior. |

A Worker run, package, fixture, mock, or dry-run does not promote a scenario.
Every required dependency row needs its independently correlated evidence.

## Good Fit and Poor Fit

Cohesix is a strong fit when the valuable product is a controlled operation:
explicit authority, bounded state, typed refusal, leases, replay, and audit
around a more complex host-side system.

Cohesix is not the right boundary when a workload requires any of the
following inside the VM:

- POSIX or Linux application compatibility;
- CUDA, NVML, a model trainer, or a general container runtime;
- an unbounded message broker or high-volume media/tensor data plane;
- arbitrary remote procedure calls or unauthenticated mutation;
- a network appliance with additional in-VM listeners;
- a claim of offline durability without an accepted persistent profile or
  host-side store.

## Assess a Deployment

Before adopting a pattern, answer these questions:

1. Which exact manifest profile and generated namespace authorize the flow?
2. Which component owns the data plane, and is it outside the VM where required?
3. What ticket, path, bounds, and refusal behavior govern every mutation?
4. Which host adapter performs the external side effect, and how is it
   allowlisted, cancelled, and audited?
5. Which evidence lane proves the target: repository tests, QEMU, an archived
   Pi comparator, or fresh current-image hardware?
6. What remains diagnostic, simulated, site-specific, or planned?
7. How are secrets, offline state, rollback, and recovery handled without
   widening VM authority?

If the answers are concrete, continue with the
[Operator Walkthrough](OPERATOR_WALKTHROUGH.md). Use
[TEST_PLAN.md](TEST_PLAN.md) for the required validation lane,
[BENCHMARKS.md](BENCHMARKS.md) for performance claims, and
[HARDWARE_BRINGUP.md](HARDWARE_BRINGUP.md) to keep build, flash, boot, network,
console, and benchmark proof separate.
