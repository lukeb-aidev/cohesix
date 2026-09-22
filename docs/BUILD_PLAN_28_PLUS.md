<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define the focused post-27g ecosystem release and the explicitly deferred roadmap. -->
<!-- Author: Lukas Bower -->

# Cohesix Build Plan — 28x and Later

## Authority, status and preservation

**Owner-directed planning revision: 23 September 2026. All implementation below is Planned, not activated or accepted by this edit.** This is the canonical continuation of [BUILD_PLAN](BUILD_PLAN.md) after 27g, under [AGENTS](../AGENTS.md). It replaces the former prospective 28–31 bodies, their all-catalogue completion requirements, their blanket dependencies, and conflicting future-scope instructions in linked specifications. It is not an additive backlog layered over those requirements.

Milestones 0–27g, including active qualification, narrow reopenings, existing implementation, tests, evidence, exceptions and release approvals, are unchanged. Inspect the actual checkout before implementation; retain useful work and repair a selected-path regression rather than recreating accepted foundations. Historical task IDs, generated schema identifiers, artifact bytes and release records retain their meaning. The [mapping](#mapping) resolves reused numbers; it does not rename persisted records.

Release A remains 1.1.0-beta. This plan targets the assembled 28x delivery at **Release B, 1.2.0-beta**, without publishing it or changing Release A. Release C, 1.3.0-beta, remains reserved. Only overall release publication/promotion requires named human release-owner sign-off; component and milestone review follows the existing charter. Runtime approval requirements remain selected-policy controls, not mandatory human prompts for every operation.

## Product outcome

**Let existing NeMo agents, CUDA/PEFT workflows and Mac applications use Cohesix to perform useful GPU work with bounded authority, observable results and recoverable failures.** Users should not have to adopt another agent framework, model platform or infrastructure stack.

The primary audience is engineers running private NVIDIA AI workloads and Apple-silicon developers who want local Apple compute, remote CUDA execution, or both. NeMo means a concrete integration with **NVIDIA NeMo Agent Toolkit**, not a requirement to implement the entire NeMo product family. MCP exposes tools and evidence; A2A delegates durable jobs. Siri/App Intents provide native Apple entry points. All use the same Cohesix actions, policy and results.

The release must deliver three complete journeys:

1. **Useful CUDA from an existing workflow:** register an approved real workload, check capacity, submit it, follow progress, retrieve verified outputs, cancel or recover after interruption.
2. **Private adapter release:** train or import a real LoRA adapter, compare it with the accepted deployment, canary it through a real runtime, promote or roll back, and inspect the decision and evidence.
3. **Native and agent access:** discover and initiate those operations through NeMo/MCP/A2A and macOS 27 App Intents/Shortcuts/Siri, then follow the same run in SwarmUI or the CLI. A small local MLX workflow demonstrates genuine Metal-backed execution rather than Mac-as-remote-control alone.

“Seamless” is an acceptance objective to measure, not a compatibility or adoption claim made by this plan.

## Scope rules and common acceptance

- Reuse 27a authority, 27b provider/executor/evidence contracts, 27c recipes, 27d LoRA transactions, 27e installation and 27f workbench. Extend their actual gaps; do not create parallel registries, journals, schedulers, evaluators or receipt formats.
- Complete each narrow milestone below. A placeholder, mock, unavailable profile or documentation-only adapter cannot satisfy a required implementation. Optional deployment capabilities may be unavailable, but that is not evidence that their claimed live implementation works.
- Numerical order is the default delivery sequence, not a blanket prerequisite chain. Dependencies are the exact contracts listed in each milestone. Apple is not a runtime dependency of NeMo; A2A does not require MCP to be enabled. No 29+ milestone gates 28x. Implementation requests activate only their explicitly named scope after its real prerequisites; record In Progress before work.
- Retain one useful operator journey: plan/preflight, submit, status/watch, explain, cancel, recover, compare/promote/rollback where applicable, and evidence verification. Prefer existing commands and SDK types. UI, Siri, model output and protocol adapters are clients, never new authority owners.
- Enforce authentication, delegated scope, current-state checks, expiry, idempotency, writer fencing, cumulative budgets and authoritative observed outcomes on every selected path. A selected-path correctness or security defect cannot be deferred by changing a support label.
- Keep the control/data boundary explicit. Pi Workers are Cohesix control-plane roles, not GPU hosts. CUDA, Metal/MLX, training, inference, models, datasets, native Apple frameworks and NeMo remain host-side. Large artifacts remain in existing bounded host storage/CAS; the VM receives bounded control records and references only.
- Document the external executor's credential custody and bypass posture. seL4 confinement does not prove that a privileged Linux/macOS administrator or external runtime cannot bypass Cohesix. State only the enforcement boundary actually tested.
- Reference qualification uses the Pi 4 Queen, a Mac with Apple silicon running macOS 27, and the maintained Jetson Orin Nano 8GB; QEMU supplies a documented evaluation route with distinct evidence. Capture actual OS/SDK/JetPack/L4T/CUDA/runtime versions and hardware capabilities. Do not assume a JetPack version, datacentre feature or large NeMo component is available because it appears in an older plan. Remote GPU hosts are explicit, never silently substituted for local execution.
- Bound shared memory, disk use, queues, execution time and retained evidence. Report what is an admission estimate, enforced limit, observed usage or unavailable measurement. No false hard GPU-isolation claim from a scheduler lease or preflight memory check.
- Build installation, doctor diagnostics, published examples and the relevant SwarmUI view with each capability. 28g integrates and qualifies them; it must not discover that several milestones supplied only libraries.
- Use existing TEST_PLAN actions, changed-path selection and proof layers. Add missing focused actions to that catalog during implementation. Reuse immutable evidence only under its exact source/profile rules. Never require a full hardware rerun for unrelated host presentation changes, or substitute a host mock for a physical claim.
- Each implementation task records changes to, or explicit non-impact on, host tools, Python, REST/FUSE, SwarmUI, packaging, generated contracts and benchmarks. Apply schema changes through coh-rtc and update only affected consumers. Do not expand that review into new feature scope.

## Milestone index <a id="Milestones"></a>

| Milestone | Required outcome | Status |
| --- | --- | --- |
| [28](#28) | Shared governed jobs and bounded unattended authority | Planned |
| [28a](#28a) | Useful CUDA workloads and reliable GPU operations | Planned |
| [28b](#28b) | Deeper PEFT lifecycle and verified serving | Planned |
| [28c](#28c) | macOS 27: Siri/App Intents, Apple AI and Metal-backed workflows | Planned |
| [28d](#28d) | MCP access to complete selected workflows | Planned |
| [28e](#28e) | A2A delegation of durable selected jobs | Planned |
| [28f](#28f) | NeMo Agent Toolkit adoption kit and live integration | Planned |
| [28g](#28g) | Installation, integrated user qualification and Release B | Planned |

## 28 — Shared Governed Jobs and Bounded Unattended Authority <a id="28"></a>

**Value:** one safe execution contract beneath every client, without repeated manual approval inside valid standing authorisation.

**Prerequisites:** accepted 27g and the exact retained authority/executor/recipe contracts it qualifies.

### Required scope

Extend the existing recipe/run identity only where needed to represent the selected CUDA, PEFT, Apple and delegated jobs. Bind caller, action, immutable workload/model inputs, target, resource limits, policy identity, current resource generation, deadline and idempotency key to the same durable job and receipt chain. Persist execution state and pending result delivery separately; a lost response must not trigger a second effect.

Implement revocable standing authorisation for a bounded subject/action/target set, expiry, cumulative budget, concurrency and retry/cooldown limits. Every effect still requires an exact current-state admission decision. Share accounting across CLI, REST, Apple, MCP and A2A so changing protocol, restarting a gateway or retrying cannot reset a budget. Administration and policy modification require separate authority. Escalation cannot authorise itself.

Use the smallest generated deterministic policy evaluator needed for the selected actions. Missing, contradictory, stale, malformed, unsupported or unverifiable inputs refuse execution with actionable reason codes. Recheck or reserve relevant state at dispatch; a host verdict string or model assertion is not target authority. Retain all existing target checks and add only the bounded target integration needed for those decisions. Full solver machinery, a general policy language and proof-carrying-intent branding belong to 29.

Provide exact job status, cancellation, safe reconciliation, and evidence verification. Report uncertain external outcomes explicitly; do not claim universal exactly-once execution. Resume a workload only from a valid checkpoint supported by its runtime. Otherwise reconcile terminal state or propose an explicitly authorised restart.

### Tasks

```text
Title/ID: m28-selected-job-contract
Milestone: 28 / shared governed jobs
Goal: Extend the accepted recipe/job and evidence contract for the selected ecosystem clients without introducing a second lifecycle.
Inputs: 27a–27g; apps/coh; apps/host-ticket-agent; tools/cohesix-py; tools/coh-rtc; existing provider and receipt contracts.
Changes: Shared job references, action schemas, exact target/input bindings, status/cancel/reconcile operations, pending-delivery handling and bounded evidence.
Commands: Existing focused coh, host-ticket-agent, Python and coh-rtc checks selected through TEST_PLAN; scripts/check-generated.sh; git diff --check.
Checks: Duplicate, lost-response, restart, wrong-target and uncertain-outcome cases preserve one job identity and do not blindly repeat effects; clients agree on state.
Deliverables: Versioned shared job contract, affected client updates and focused failure/recovery evidence.

Title/ID: m28-standing-authority-and-admission
Milestone: 28 / bounded unattended authority
Goal: Permit useful unattended operation only inside current revocable policy and shared durable limits.
Inputs: Existing delegation, fencing, journal, policy and target checks; m28-selected-job-contract.
Changes: Narrow state-bound admission, standing scopes, durable cumulative accounting, expiry/revocation, administrative separation and clear denials.
Commands: Focused policy/authority/executor checks and race/restart negatives in TEST_PLAN; exact QEMU/Pi actions only where target behaviour changes.
Checks: Scope widening, stale generations, forged facts, budget reset, replay and cross-protocol bypass fail before the side effect; policy-required approval remains enforceable.
Deliverables: Tested unattended-authority primitive, exact enforcement claims, denial guidance and policy examples.
```

**Done:** a bounded GPU operation and a selected existing service-recovery action execute under valid standing authority, refuse invalid/stale requests, survive interrupted result delivery, and produce inspectable outcomes. Compact checks of the touched safety invariants are required; broad formal verification is not.

## 28a — Useful CUDA Workloads and Reliable GPU Operations <a id="28a"></a>

**Value:** users can run their own approved useful GPU work, not just canned vector-add/matrix-multiply demonstrations.

**Prerequisites:** 28 and accepted 27b/27c CUDA execution and recipe foundations.

### Required scope

Provide one supported way to register a digest-pinned workload package or container with a fixed entry point, typed parameter schema, declared inputs/outputs, environment and secret references, and bounded resources. Registration is privileged configuration, not an escape hatch for model-generated shell commands. Reuse the existing executor and native process/container owners. Do not require users to modify Cohesix source to add an approved workload.

Make device choice, inventory freshness, memory headroom, queue/admission state, job ownership, timeout, cancellation and actual terminal outcome visible. Preserve native GPU identity and output digests. Bound host/GPU memory use and clean up the exact process/container on cancellation without killing unrelated work. Checkpoint/resume is capability-reported; unsupported jobs are not described as resumable.

Ship one real batch inference, embedding or other useful CUDA/PyTorch recipe beyond the existing diagnostic kernels, plus a documented user-workload adaptation. Exercise native and container execution only to the extent those are advertised supported paths; retain working paths and evidence rather than reimplementing them. Jetson is the required edge reference; an explicitly supplied discrete-GPU host qualifies only its own profile.

Use existing CUDA/NVML and native telemetry for actionable health, memory and thermal diagnostics. MIG, distributed training/scheduling, automatic fleet placement and deep DCGM/CUPTI instrumentation are not baseline requirements.

```text
Title/ID: m28a-approved-user-workloads
Milestone: 28a / useful CUDA execution
Goal: Run a user-configured approved CUDA workload through the existing governed job path.
Inputs: 27c recipes; 28 job/admission contract; gpu-bridge-host; host-cuda; host-ticket-agent; coh; Python; generated profiles.
Changes: Workload registration/configuration, typed inputs, digest/entrypoint binding, exact GPU dispatch, output verification and a useful reference recipe.
Commands: Focused existing CUDA/provider/recipe checks and selected live reference actions under TEST_PLAN; scripts/check-generated.sh.
Checks: Genuine CUDA execution and verified outputs occur on the admitted device; arbitrary command/path/credential injection and stale-device requests fail.
Deliverables: Installable recipe, registration guide, CLI/Python/SwarmUI operation and live evidence.

Title/ID: m28a-cuda-operations-and-recovery
Milestone: 28a / GPU operation and recovery
Goal: Make capacity, failure, cancellation and recovery understandable and safe on the maintained GPU host.
Inputs: m28a-approved-user-workloads; existing doctor, telemetry, executor journal and receipt verifier.
Changes: Bounded memory admission, explicit resource/health observations, exact-owner cancellation, checkpoint capability and interrupted-job reconciliation.
Commands: Selected live and deterministic fault cases for timeout, cancellation, stale inventory, OOM mapping, bridge restart and lost response.
Checks: No unrelated process is killed, no uncertain job is blindly replayed, and unsupported resume/isolation/telemetry capabilities are reported honestly.
Deliverables: Recovery walkthrough, diagnostic views and exact-profile failure evidence.
```

**Done:** a newcomer adapts and runs a real workload without editing Cohesix, then handles a deliberate interruption using the published tools and obtains independently verified outputs or an explicit unresolved outcome.

## 28b — Deeper PEFT Lifecycle and Verified Serving <a id="28b"></a>

**Value:** a real adapter moves from training or import to a measured, reversible deployment without bespoke glue.

**Prerequisites:** 28, 28a and accepted 27d private LoRA transaction. Extend that transaction; do not rebuild its journal, registry or activation path.

### Required scope

Keep Hugging Face PEFT as the primary open training/artifact contract. Support a configurable LoRA training job and import of an independently produced compatible adapter. Add QLoRA only for the selected model/runtime combination where it demonstrably enables the required reference within available memory; do not make all PEFT methods required.

Pin base-model revision, tokenizer/configuration, dataset snapshot, training configuration and framework versions. Validate adapter structure, sizes, hashes, compatibility and provenance. Use safe artifact formats and disable arbitrary remote-code loading in the reference profile. Gated downloads, licence acceptance, external uploads and private-dataset access require explicit operator configuration. A recorded licence/provenance reference is not legal certification.

Train, evaluate, scan, import, canary, promote and roll back through the existing phase-journalled lifecycle. A supplied adapter follows the same checks but does not fabricate training provenance. Reuse real framework checkpoints where available, distinguishing training resume from an adapter-only restart. Record stochastic settings without promising identical trained weights.

Compare candidate, base and currently accepted deployment on a fixed held-out task dataset, with predeclared task-quality and resource/latency criteria. Keep a candidate that fails those criteria out of production. An LLM-as-judge score, scan or guardrail verdict alone is not proof of safety or quality. Record a failed comparison as a legitimate outcome, not a reason to change the threshold.

Complete one supported serving path with an application-usable endpoint, exact model/adapter identity, health/readiness, a real inference canary, generation-fenced promotion and rollback. Reuse the accepted serving runtime's native interface. Do not build a general inference proxy, multi-provider router or universal API clone. No published model entry or HTTP success alone counts as a verified deployment.

Provide documented adapters at the artifact/runtime boundary for later MLX and NeMo consumers. A NeMo-exported or MLX adapter is accepted only for an actually qualified format/model pair; neither brand nor common LoRA terminology proves portability. Broad conversion machinery remains deferred.

```text
Title/ID: m28b-configurable-peft-training-and-import
Milestone: 28b / native PEFT lifecycle depth
Goal: Extend the accepted LoRA transaction to useful configurable training, genuine external import and supported checkpoint recovery.
Inputs: 27d transaction; 28/28a authority and CUDA; HF PEFT artifact contract; existing CAS, registry, executor and evidence validators.
Changes: Version-pinned recipe inputs, bounded training parameters, provenance/format validation, supported checkpoints and import with explicit missing provenance.
Commands: Focused PEFT transaction/provider tests plus exact-reference train/import/resume actions in TEST_PLAN.
Checks: Real artifacts, compatible base/tokenizer/runtime and observed framework outcomes are required; corrupt inputs, unsafe deserialisation, path races, duplicates and stale generations fail safely.
Deliverables: Configurable training/import workflow, truthful checkpoint semantics and genuine adapter evidence.

Title/ID: m28b-evaluate-canary-promote-rollback
Milestone: 28b / verified serving
Goal: Compare and deploy adapters through one real application-facing runtime with reversible promotion.
Inputs: m28b-configurable-peft-training-and-import; existing 27d evaluator, serving reload/canary and rollback contracts.
Changes: Held-out baseline/candidate/incumbent comparison, declared thresholds, runtime identity, canary requests, promotion recheck and rollback/recovery views.
Commands: Existing focused evaluator/activation tests and selected live serving, failed-canary, interrupted-promotion and rollback actions.
Checks: Failed or stale candidates cannot promote; real served behaviour and identity are observed; rollback restores the accepted generation without fabricated receipts.
Deliverables: End-to-end adapter release guide, usable endpoint, comparison results and recoverable release evidence.
```

**Done:** both training and independent import paths are demonstrated; a real client consumes the canary and accepted deployment; a deliberately rejected candidate and a rollback are exercised. Deep PEFT support is judged by that lifecycle, not the number of frameworks listed.

## 28c — macOS 27: Siri/App Intents, Apple AI and Metal-Backed Workflows <a id="28c"></a>

**Value:** a Mac is both a natural operator surface and a useful local AI host, while remote NVIDIA execution remains explicit.

**Prerequisites:** 28 and the selected 28a/28b operations. MCP, A2A and NeMo are not prerequisites for native Apple operation.

### Required scope

Target a pinned public macOS 27/Xcode SDK and supported Apple-silicon reference. Preserve working older host tools unless a documented compatibility change is necessary. Add the smallest native Swift App Intents integration to the existing signed application/package; do not rewrite SwarmUI or create a second desktop product. Swift stays host-side and does not alter pure-Rust VM requirements.

Expose stable entities for configured hives, approved recipes, runs and deployments, and a curated action set: inspect status, start an approved job, inspect a result, request cancellation and request a permitted promotion/rollback. Use App Intents/App Shortcuts and appropriate system discovery. Deep-link to the existing workbench for detail. Long jobs return a durable run reference promptly; they must not depend on a Siri conversation or extension remaining alive.

Invoke existing authenticated Cohesix operations, not AppleScript/UI scripting or arbitrary shell. Bind the logged-in app/user session to delegated credentials held through native secure storage. Spoken text, device possession, Siri confidence and Spotlight content are not grants. Require any platform-mandated authentication/confirmation and selected Cohesix policy; no voice-only bypass. Scope indexed entities/results, redact sensitive content and withdraw stale/revoked entries.

Implement one useful Foundation Models integration: explain a run/refusal from scoped evidence and propose an approved next action through typed tool inputs. Model-produced interpretations remain labelled and non-authoritative. Keep deterministic status and manual operation available when Apple Intelligence is disabled, unavailable, offline or unsupported. No assumption that Siri speaks MCP/A2A, exposes an unrestricted agent API, or can use arbitrary Cohesix model weights. A generic Language Model provider package, cloud routing and Personal Context ingestion are not required.

Make **MLX/MLX-LM the single required Apple compute backend**, using its Metal support rather than custom kernels. Run one small real local inference and LoRA train/evaluate/serve/rollback journey over the 28b lifecycle. Capture the actual model/runtime and GPU execution evidence, unified-memory budget, cancellation and failure behaviour. Preserve distinct MLX and HF/NeMo formats; do not claim universal adapter interchange. MPS and Core ML are alternatives for later demand, not additional mandatory backends.

Keep three capabilities distinct: Mac controlling remote CUDA, Mac performing local MLX/Metal work, and Apple Foundation Models providing in-app assistance. Capability probes report each separately. Native macOS distribution must include signing/notarisation where required by the chosen channel; missing credentials leave that validation blocked, not silently waived. An App Store launch and iOS/watch/visionOS companions are deferred.

```text
Title/ID: m28c-native-apple-actions
Milestone: 28c / macOS 27 native entry points
Goal: Make selected governed workflows discoverable and usable through App Intents, Shortcuts and Siri without duplicating the UI or authority layer.
Inputs: Public pinned Apple SDK; existing SwarmUI package; 28 job/action schemas; delegated client credentials; selected recipes and run views.
Changes: Minimal native integration, entities/actions, secure credential handoff, durable job links, scoped discovery and availability diagnostics.
Commands: Pinned native build/signing and App Intents tests, published Shortcuts and live Siri walkthroughs, plus shared action-authority negatives.
Checks: Required actions reach the same job/receipt path; ambiguous voice input, unauthenticated or stale requests do not act; long jobs survive client exit; disabled Intelligence does not break core operation.
Deliverables: Packaged native actions, working Siri/Shortcuts examples and exact SDK/device/availability evidence.

Title/ID: m28c-mlx-metal-provider
Milestone: 28c / local Apple compute
Goal: Execute one genuine local MLX inference and LoRA lifecycle through Cohesix's existing job and deployment contract.
Inputs: 28b transaction; pinned MLX/MLX-LM; small supported model/dataset; exact Mac memory and Metal capability.
Changes: Single bounded Apple provider, explicit local/remote selection, format/runtime checks, native execution observations and resource-safe recovery.
Commands: Focused adapter tests and live Mac inference/train/evaluate/canary/rollback/cancel actions under TEST_PLAN.
Checks: Local GPU work is observed, not inferred from a Mac label; no implicit CPU or remote fallback; memory and unsupported-format refusals are safe; no universal HF-to-MLX portability claim.
Deliverables: Installable MLX profile, complete small-model journey and per-backend evidence.

Title/ID: m28c-foundation-models-assistance
Milestone: 28c / evidence-grounded Apple AI
Goal: Explain selected operations and propose valid next actions using Apple's public model/tool interface without granting model authority.
Inputs: Pinned Foundation Models SDK; scoped job/evidence reads; shared typed action schemas; native app integration.
Changes: One evidence-grounded assistance flow, typed proposals, source links, explicit availability/privacy posture and deterministic fallback.
Commands: Native model/tool tests, live supported-device exercise and disabled/unavailable/injection/unauthorised-action cases.
Checks: Generated prose cannot invent job success or execute without admission; only authorised evidence is supplied; model unavailability does not block CLI, UI or Shortcuts.
Deliverables: Useful native assistance and published limits, not a new general-purpose assistant.
```

**Done:** the selected macOS 27 device demonstrates actual Siri/Shortcuts operation, evidence-grounded assistance and local Metal-backed work. Shortcuts or a unit test alone cannot establish live Siri acceptance; a local unavailable state is reported honestly until the supported path is exercised.

## 28d — MCP Access to Complete Selected Workflows <a id="28d"></a>

**Value:** standard agent clients can safely use Cohesix without learning its internal namespaces or writing bespoke orchestration.

**Prerequisites:** 28 and the specific 28a/28b actions exposed. Apple-only actions depend on their accepted 28c provider; basic MCP does not.

Implement a pinned MCP revision through a thin host-side projection of existing actions and scoped reads. Use a maintained SDK where suitable. Support the standard network transport required by NeMo and a packaged local launch path for desktop clients; use an existing transport adapter rather than separate authority implementations.

Expose a curated, task-oriented catalog for capability discovery, workload preflight/submit/status/cancel/recover, adapter comparison/promotion/rollback, and evidence inspection. Schemas and descriptions come from the shared action contract, not a second generic file-write API. Document correct use, required authority, examples, terminal versus pending state and recovery. Publish only accepted selected operations; do not expose all administrative controls for completeness.

Preserve caller identity, shared budgets, revocation, current-state admission and job correlation. Implement bounded messages/concurrency, transport authentication, Origin checks where applicable, version negotiation, scope-safe discovery and deterministic errors. Follow the selected protocol's auth requirements; never forward a caller's credentials to a model/provider. Treat tool results and model-returned calls as untrusted data.

```text
Title/ID: m28d-mcp-selected-workflows
Milestone: 28d / MCP integration
Goal: Let ordinary MCP clients discover and complete selected governed CUDA/PEFT operations through the shared job contract.
Inputs: 28 action/authority contract; selected 28a/28b providers; hive-gateway; maintained MCP SDK; NeMo client transport requirements.
Changes: Pinned transports, curated tools/resources/guidance, scoped discovery/auth, job mapping, independent enable/disable and package/doctor support.
Commands: Protocol conformance, shared authority negatives and an ordinary-client live submit/observe/recover/evidence walkthrough in TEST_PLAN.
Checks: No direct executor calls, generic arbitrary writes or client-authored success; disabling MCP leaves REST and accepted jobs coherent; a real consequential workflow completes under bounded standing authority.
Deliverables: Usable MCP integration, versioned client configuration and live failure/recovery evidence.
```

**Done:** a standard client completes useful work from public discovery and configuration, including refusal/recovery, without private repository instructions. Read-only success is an intermediate checkpoint. Complete administrative API parity and MCP-backed FUSE are deferred to 32.

## 28e — A2A Delegation of Durable Selected Jobs <a id="28e"></a>

**Value:** a NeMo or other agent can delegate a long GPU or adapter-release job and reconnect to its progress and result instead of maintaining a fragile conversation.

**Prerequisites:** 28 and selected job providers. Reuse shared gateway code from 28d where useful; MCP enablement or conformance is not an A2A prerequisite.

Implement one pinned A2A revision/binding with an Agent Card advertising only accepted skills, authenticated task creation, status, progress/streaming, artifact references, cancellation and reconnect/recovery. Back every task with the existing durable Cohesix job. Use an official/maintained SDK rather than implementing the protocol from first principles.

Preserve delegated subject, skill/action limits, job idempotency and the shared 28 budget. A task is not a blanket grant to create arbitrary sub-actions. Each real effect has current admission; cross-protocol retry does not reset accounting or produce a new execution accidentally. Define terminal, failed, cancelled and ambiguous outcomes from observed provider state. Keep task artifacts bounded, scoped and verifiable. Do not add a new planner, inter-agent mailbox or checkpoint engine.

```text
Title/ID: m28e-a2a-durable-jobs
Milestone: 28e / A2A delegation
Goal: Delegate and recover selected long-running Cohesix jobs through a standard A2A peer.
Inputs: 28 job/authority/receipt contract; selected providers; shared gateway primitives; pinned A2A SDK and NeMo peer.
Changes: Agent Card/skills, task/job mapping, authentication, progress/artifacts, cancellation, reconnect and independent enablement.
Commands: Selected-binding conformance, shared-accounting negatives and live standard-peer delegation/reconnect/cancel/recovery actions in TEST_PLAN.
Checks: A2A-only operation works; task IDs cannot widen authority or expose another caller's evidence; restart and uncertain completion do not trigger blind re-execution.
Deliverables: A2A service, published peer configuration, task lifecycle guide and independently observed live outcomes.
```

**Done:** a named standard peer delegates a real CUDA or PEFT job, disconnects, reconnects and obtains the correct scoped outcome. Arbitrary push callbacks, multiple wire bindings and generic multi-agent coordination are deferred to 32/34, not required to close this milestone.

## 28f — NeMo Agent Toolkit Adoption Kit and Live Integration <a id="28f"></a>

**Value:** Cohesix is directly usable inside a NeMo developer's existing workflow, not merely described as compatible with NVIDIA tools.

**Prerequisites:** 28, the selected CUDA/PEFT providers, 28d and 28e. Apple is an optional execution/client profile, not a required installation dependency of the NeMo kit.

Use NeMo Agent Toolkit's native MCP and A2A clients first. Ship a pinned integration kit with version constraints/lockfile, concise configuration, authentication/secret-reference setup, typed outputs, tracing correlation and runnable workflows. A small plugin/function group is allowed only for a demonstrated gap that native configuration cannot solve; no NeMo fork, generic wrapper framework or duplicate Cohesix SDK.

Require two real reference workflows: (1) a NeMo agent uses MCP to preflight, submit and inspect an approved CUDA job; (2) a NeMo workflow delegates an adapter evaluation/canary/release job through A2A, follows its result, and exercises interruption or a deliberately failed candidate. At least one flow demonstrates bounded unattended operation and one a denied action. NeMo keeps its own planner/model/checkpoint state; Cohesix owns admitted effects, operational recovery and receipts.

Make HF PEFT artifacts and the accepted 28b training/serving path available to those workflows. This is useful NeMo ecosystem support even when the trainer is HF PEFT rather than NeMo Framework. Document those boundaries precisely. Import of NeMo-produced artifacts requires the same format/base/runtime validation and exact-profile evidence; it does not imply all NeMo checkpoints are interchangeable.

Support the NeMo workflow's configured model endpoint without requiring a Cohesix inference proxy. A local or remote NIM/NeMo service may be selected where independently available; credentials, model capability and host compatibility must be checked. The default integration cannot require NIM, full NeMo Framework, Triton, a Kubernetes cluster or a large NVIDIA model on the 8GB Jetson.

Use the NeMo-native evaluation/profiling facilities for an integration comparison, not a new benchmarking product. Compare direct versus Cohesix-backed operation on the same useful task: setup steps, authority exposure, completion/recovery, added control latency and evidence usefulness. Native traces may carry job/receipt IDs but never replace authoritative outcomes. A bad result must remain visible.

```text
Title/ID: m28f-nemo-agent-toolkit-kit
Milestone: 28f / NeMo ecosystem entry
Goal: Make supported Cohesix tools and durable jobs available from a normal NeMo Agent Toolkit installation without a fork.
Inputs: Pinned NeMo Agent Toolkit MCP/A2A clients; 28d/28e services; 28a/28b recipes; existing Python package and evidence APIs.
Changes: Minimal installable integration kit, native configuration, scoped auth, exact version matrix, useful examples and correlation helpers only where needed.
Commands: Clean-environment kit installation and ordinary NeMo configuration/workflow tests using released or exact candidate artifacts.
Checks: No editable repository install, private setup knowledge, raw executor credentials or parallel action registry is required; unavailable optional NVIDIA services are explicit.
Deliverables: Versioned NeMo kit, quick start, support matrix and packaging evidence.

Title/ID: m28f-nemo-live-workflows-and-value
Milestone: 28f / useful NeMo qualification
Goal: Prove useful CUDA and adapter workflows, bounded autonomy and recovery from NeMo's standard clients.
Inputs: m28f-nemo-agent-toolkit-kit; selected real providers/model endpoint; reference dataset and baseline deployment; native profiling/evaluation tools.
Changes: MCP CUDA and A2A adapter journeys, denial/interruption/candidate-failure cases, direct-versus-governed comparison and portable receipt links.
Commands: Live NeMo workflow runs and focused failure injections in their owning TEST_PLAN layers; record exact toolkit/protocol/provider/target versions.
Checks: Both protocols cause and observe real permitted work; denied work has no effect; reconnect/retry preserves job identity; measured overhead and limitations are reported without invented savings.
Deliverables: Reproducible NeMo examples, live result/recovery evidence and a practical integration assessment.
```

**Done:** another developer can use the kit with an ordinary pinned NeMo installation and configured Cohesix deployment. The kit is not gated on acceptance into NVIDIA's upstream catalog; external publication/listing requires the normal owner credentials and authorisation. Broad NeMo Run/Customizer/Megatron/Guardrails/serving-family integration belongs to 36.

## 28g — Installation, Integrated User Qualification and Release B <a id="28g"></a>

**Value:** the pieces form an adoptable tool rather than a set of individually passing adapters.

**Prerequisites:** required 28–28f outcomes and exact selected-profile evidence. No later milestone is a substitute for missing required work or a reason to enlarge this release.

Extend the accepted installation/distribution paths, with least-privilege setup and rollback/uninstall guidance. Ship the Mac package, supported host/GPU components, Python/NeMo integration kit, client configuration and useful recipe assets. Downloads of models/data are separate explicit operations with declared size/licence/credentials; do not hide them in installation. Use existing signed release assets/package channels before creating another distribution service.

Doctor must diagnose endpoint ownership, versions, credentials, host/GPU capability, model/runtime compatibility, storage, protocol enablement and Apple availability with actionable next steps. Credentials never appear in example arguments or logs. The CLI, SwarmUI, Siri and agents must show the same run/result and distinguish local MLX from remote CUDA. Improve the existing workbench for these journeys, not its visual theme or rendering architecture.

Qualify the three product journeys from clean supported environments and published instructions, without source edits or developer-only setup. Separately test MCP-only, A2A-only, both-enabled and both-disabled service configurations. Demonstrate Mac-originated remote CUDA/PEFT work, local MLX work, and both NeMo protocol flows; the standalone protocol services must not require macOS or Apple Intelligence.

Measure installation steps/time and bytes downloaded, time to first useful job, operator interventions, task completion, refusal clarity, recovery after interruption, and added control overhead. Fix acceptance budgets before the integrated runs using the retained baseline and intended workload; do not loosen them after a failed run. Compare workload output/quality as well as mechanics. At least one independent clean-install walkthrough must not rely on the implementer's private knowledge; identify whether the evaluator was a person or an agent rather than fabricating community adoption.

Keep host-process recovery distinct from Queen reboot durability. Required 28x operations must fail closed and reconcile safely after Queen loss; automatic VM-local persistent resumption is a later 31 claim. No marketing statement may imply it was delivered here.

```text
Title/ID: m28g-installation-and-integrated-adoption
Milestone: 28g / integrated user qualification
Goal: Make the selected CUDA, PEFT, Apple and NeMo journeys installable and operable from public instructions.
Inputs: Accepted 28–28f artifacts; 27e install paths; 27f workbench; existing doctor and release machinery.
Changes: Exact packages, minimal setup/configuration, task-oriented UI/help, complete walkthroughs, availability diagnostics and clean-environment evidence.
Commands: Existing packaging/install/doctor checks and independent clean-environment execution of the three product journeys; applicable generated/link checks.
Checks: No repository patch, undisclosed credential, mock result or unrelated stack is needed; local/remote modes and proof limits are visible; both protocol enablement and disabled configurations behave correctly.
Deliverables: Installable release candidate, newcomer guides, protocol/Apple/NeMo configuration and measured adoption results.

Title/ID: m28g-release-b-qualification
Milestone: 28g / assembled release qualification
Goal: Qualify the bounded ecosystem release without reintroducing deferred catalogues.
Inputs: Exact release candidate; selected TEST_PLAN evidence; fixed user/overhead budgets; historical evidence/exception rules.
Changes: Integrated failure/security/recovery tests, compatibility review, accepted-claim matrix, bounded evidence bundle and Release B notes.
Commands: Complete applicable existing staged/pressure/repeatability/target/release gates under TEST_PLAN; no unrelated catalogue qualification.
Checks: Required real workflows, authority negatives, interruption recovery and compatibility budgets pass at exact source/profile identity; unexecuted checks remain blockers, and no deferred claim is promoted.
Deliverables: Qualified 1.2.0-beta candidate and evidence; publication/promotion only after named human release-owner approval.
```

## Deferred Whole-Number Milestones

These are deliberately **Deferred, not automatically next to implement**. Each needs a named use case or engineering finding, a bounded updated task plan and explicit owner activation. The old detailed proposal is a source for that review, not an obligation to implement its entire catalogue. Existing working behaviour and safety obligations remain supported at their actual evidence level.

### 29 — Extended Formal Assurance and NIST Evidence <a id="29"></a>

Former 28 verification breadth and the general proof-oriented part of former 28c: complete manifest witness programme, broad Secure9P/HAL/state-machine proofs, general solver/evaluator correspondence, proof artefacts and version-pinned NIST LOW/OSCAL assessment. Re-entry requires a named assurance claim, reviewer/adopter need or consequential defect class. Keep narrow safety checks required by shipped 28x actions in 28; do not defer them here. No certification-by-crosswalk claim.

### 30 — Full Production Bundle Binding and Quarantine Inventory <a id="30"></a>

Former 28d's comprehensive Worker ticket/lease-to-live-cap ledger, complete ticket-free driver inventory, structured quarantine and fresh-generation recovery qualification. Retain the 26e construction/teardown design and legacy `m28b_production_bundle` evidence identity. Re-entry requires a deployment or claim needing that stronger binding. The basic generation, revoke and fault safety required by a selected 28x action remains mandatory in 28x.

### 31 — VM-Local Persistence and Reboot Recovery <a id="31"></a>

Former 29: bounded spool/settings persistence, QEMU block backend, isolated Pi EMMC2 storage, safe media layout, power-loss/reboot testing and CYW43/shared-IRQ coexistence. Activate for an actual disconnected/reboot-durable deployment. Host checkpoint storage is not this capability. Preserve all original physical safety/containment requirements when activated; do not casually merge storage and Wi-Fi ownership.

### 32 — Broader Enterprise, Industry and Protocol Integration <a id="32"></a>

Unselected former 28a catalogue: Kubernetes controllers/CEL/DRA, enterprise identity variants, MIG and deep DCGM/CUPTI evidence, additional deployment shapes, federation durability, extra exporters/SIEM, industrial/healthcare/space protocols, Apple release-factory/endpoint-management and the nine broad domain playbooks. Also unneeded former 28g breadth: full administrative protocol parity, MCP resource FUSE, additional bindings and arbitrary push delivery. Activate one named integration at a time; do not reinstate an all-providers completion gate. Preserve accepted FUSE, native service and field-bus work.

### 33 — Semantic Objects and Context Capsules <a id="33"></a>

Former 28b: semantic graph, compiler/ownership/call views, extractor registry, history, incremental indexes, capsule planner/renderer and dedicated inspectors. Activate only when an identified context/retrieval failure is not adequately solved by existing artifact and evidence references, with a measured usefulness baseline. It is not a prerequisite for policy, protocols, Apple or NeMo.

### 34 — Advanced Agent Orchestration and Context Optimisation <a id="34"></a>

Former 28e's generic task graphs, inter-agent handoffs, experiments, attention strategy, prefix/hotset lifecycle, broad framework callbacks and context optimisation. External frameworks retain these jobs in 28x. Activate for a concrete unsolved cross-framework coordination or cost problem; do not infer value from fewer tokens alone.

### 35 — General Inference Gateway and Advanced Routing <a id="35"></a>

Former 28f's comprehensive Models/Chat Completions/Responses/Embeddings compatibility, general streaming proxy, broad receipts/provider matrix, policy aliases, shadow-routing promotion, cross-provider strategy and exact-result caching. Also broad Foundation Models Language Model provider distribution if demanded. Activate only when native runtime endpoints and thin existing client integrations cannot satisfy a named client. Keep served-adapter verification and the useful endpoint required by 28b in 28b.

### 36 — Broader NeMo Training, Evaluation and Serving Families <a id="36"></a>

Former 28e's comprehensive NeMo Run, Customizer, Megatron/Automodel training/conversion, Guardrails execution rails, retrieval/evaluation services and NIM/Triton/TensorRT-LLM variants beyond the selected integration. Qualify each on hardware where it is actually supported; remote/datacentre work is not an 8GB Jetson acceptance claim. Add a native adapter only where standard MCP/A2A plus the existing job/artifact contracts do not meet a named NeMo workflow. No duplicate planner, policy engine or receipt trust root.

### 37 — Measured Target Scheduling and Responsiveness Improvements <a id="37"></a>

Former 29a/29b: core-local service-turn optimisation and operator-lane/multi-surface responsiveness. Activate from measured bottlenecks in a useful workload, preserve the accepted scheduler and deterministic liveness contracts, and require exact-target regression evidence. Existing responsiveness defects are repairs, not permission to wait for this milestone.

### 38 — Local Hardware Status and Additional Namespace Projections <a id="38"></a>

Former 30/30a/30b: expanded edge status tools, `hw-status` and AI-native read projections. Activate when existing doctor, CLI or workbench views fail a concrete diagnostic/agent need. No new AI write plane or unsupported hardware claim.

### 39 — AWS Deployment <a id="39"></a>

Former 31: AWS AMI, UEFI/ENA and exact cloud-target qualification. Activate for an actual hosted evaluation or deployment need that the supported local/QEMU path cannot serve. Retain explicit costs, target evidence, networking/security and release packaging; no cloud claim from local tests.

## Old-to-New Ownership Map <a id="mapping"></a>

| Previous prospective owner | Current disposition |
| --- | --- |
| 0–27g and their narrow retained-path reopenings | Unchanged scope/status/evidence; the current release remains their owner. |
| 28 verification | Selected safety checks in 28; remaining assurance and NIST in 29. |
| 28a providers/workflows | Selected CUDA in 28a; PEFT serving in 28b; native Apple/MLX in 28c; all unselected provider/domain/federation/exporter breadth in 32. |
| 28b semantic fabric/capsules | 33. Existing CAS/evidence references remain existing foundations, not a new semantic implementation. |
| 28c admission | Narrow shared action admission and standing authority in 28; general proof/solver programme in 29. |
| 28d production binding/inventory/quarantine | Selected-path safety remains 28; complete stronger bundle/driver claim in 30. Legacy generated IDs retain their historical bytes. |
| 28e AI/PEFT/NeMo | Concrete PEFT extensions in 28b, local Apple in 28c, NeMo Agent Toolkit integration in 28f; advanced orchestration/context in 34 and broader NeMo/framework families in 36. |
| 28f inference | Required native serving/canary in 28b and native Apple assistance in 28c; general gateway/API/routing/cache breadth in 35. |
| 28g MCP/A2A | Standing authority in 28; curated MCP in 28d; durable A2A in 28e; NeMo client integration in 28f; integrated release in 28g. Extra protocol/admin/mount/push breadth in 32. |
| 29 persistence | 31. |
| 29a/29b scheduling | 37. |
| 30/30a/30b status and namespaces | 38. |
| 31 AWS | 39. |

A current prospective link must name this file and its current anchor. Older task IDs, source comments, historical specification links and immutable evidence resolve through this map; they do not silently activate new scope because a number was reused. At implementation, reconcile the affected generated ownership/test selectors through their existing source owners; do not hand-edit generated metadata in this planning change.

## External Basis and Version Discipline

These sources informed scope, not claims of Cohesix implementation. Checked 23 September 2026; pin exact versions and public APIs again when implementing.

- NVIDIA's [3 September acquisition announcement](https://blogs.nvidia.com/blog/nvidia-to-acquire-hugging-face/) and [2 September agreement filing](https://www.sec.gov/Archives/edgar/data/1045810/000104581026000078/nvda-20260902.htm): the transaction was agreed, with closing expected in the first half of 2027. Do not assume completed ownership, merged APIs, changed licences or automatic NeMo/HF format compatibility.
- Apple's [macOS 27 developer changes](https://developer.apple.com/macos/whats-new/) and [Apple Intelligence developer overview](https://developer.apple.com/apple-intelligence/): public App Intents and Foundation Models are the integration starting points. [Siri AI availability](https://www.apple.com/newsroom/2026/09/siri-ai-a-profoundly-more-capable-and-personal-assistant-is-here/) is a supported-device/language/region/beta constraint to test, not a reason to invent private APIs.
- [MLX](https://mlx-framework.org/) supplies Apple-silicon/Metal computation and maintained language-model examples. Reuse the selected supported stack, not new Cohesix kernels.
- [HF PEFT](https://huggingface.co/docs/peft/index) is the selected open adapter/training boundary. [NeMo Megatron Bridge](https://docs.nvidia.com/nemo/megatron-bridge/latest/) illustrates why cross-framework conversion and verification must be explicit rather than assumed; broad conversion is deferred.
- NeMo Agent Toolkit [MCP client](https://docs.nvidia.com/nemo/agent-toolkit/latest/build-workflows/mcp-client.html), [A2A integration](https://docs.nvidia.com/nemo/agent-toolkit/latest/components/integrations/a2a.html) and [third-party plugins](https://docs.nvidia.com/nemo/agent-toolkit/latest/extend/third-party-plugins.html) establish native integration paths. The checked latest documentation identifies Toolkit 1.8; acceptance must pin an actual compatible release, not depend on a moving `latest` URL.
