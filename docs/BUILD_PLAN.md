# Cohesix Build Plan (ARM64, Pure Rust Userspace)

The current roadmap is contained in this file: unchanged milestones 0–27g,
[Release B and milestones 28–28g](#release-b), then deferred whole-numbered
milestones 29–43. The [ownership map](#roadmap-id-mapping) resolves historical
references without changing evidence or generated identifiers. Only the new
28x release scope is committed next; later design inventories are not additional
release gates. This planning revision activates no implementation work.

This build plan records what Cohesix has implemented, what remains to be built,
and the conditions for completing each milestone. It defines scope, dependencies,
deliverables, acceptance criteria, and known limitations so users and contributors
can distinguish delivered capabilities from planned work.

| [27e](#27e) | Installation, Adoption and CI | Complete |
| [27f](#27f) | SwarmUI Community Showcase: Spectrum Workbench + Live AI Hive | Planned — next release |
| [27g](#27g) | Integrated Qualification and Next Release | In Progress — Release A qualification and two-hour Pi burn-in |
| [28](#28) | Shared Governed Jobs and Bounded Unattended Authority | Planned |
| [28a](#28a) | Useful CUDA Workloads and Reliable GPU Operations | Planned |
| [28b](#28b) | Deeper PEFT Lifecycle and Verified Serving | Planned |
| [28c](#28c) | macOS 27: Siri/App Intents, Apple AI and Metal-Backed Workflows | Planned |
| [28d](#28d) | MCP Access to Complete Selected Workflows | Planned |
| [28e](#28e) | A2A Delegation of Durable Selected Jobs | Planned |
| [28f](#28f) | NeMo Agent Toolkit Adoption Kit and Live Integration | Planned |
| [28g](#28g) | Installation, Integrated User Qualification and Release B | Planned |
| [29](#29) | Extended Formal Assurance and NIST Evidence | Deferred — explicit activation |
| [30](#30) | Broader Providers, Federation, Deployment and Domain Workflows | Deferred — explicit activation |
| [31](#31) | Semantic Object Fabric and Context Capsules | Deferred — explicit activation |
| [32](#32) | General Machine-Checked Admission and Proof Artefacts | Deferred — explicit activation |
| [33](#33) | Production Worker Binding, Driver Inventory and Quarantine | Deferred — explicit activation |
| [34](#34) | Advanced Agents, Context Optimisation and Broader NeMo Families | Deferred — explicit activation |
| [35](#35) | General Inference Gateway, Compatibility and Routing | Deferred — explicit activation |
| [36](#36) | Extended MCP/A2A Coverage and Resource Mounts | Deferred — explicit activation |
| [37](#37) | VM-Local Persistence and Reboot Recovery | Deferred — explicit activation |
| [38](#38) | Core-Local Service-Turn Scheduling | Deferred — explicit activation |
| [39](#39) | Operator-Lane Scheduling and Multi-Surface Responsiveness | Deferred — explicit activation |
| [40](#40) | Read-Only Edge Status and Attestation Tool | Deferred — explicit activation |
| [41](#41) | Pi 4 Root-Shell Hardware Status | Deferred — explicit activation |
| [42](#42) | Additional AI-Native Namespace Projections | Deferred — explicit activation |
| [43](#43) | AWS Native Deployment and ENA | Deferred — explicit activation |

---

## Post-26e Delivery Sequence <a id="post-26e-investment-constrained-delivery-sequence"></a>

The 19 September 2026 owner instruction makes numerical order implementation
order. Preserve completed 0–27a and their evidence/exception decisions. The next
release is exactly **27b -> 27c -> 27d -> 27e -> 27f -> 27g**. Each milestone
closes against its own focused contracts using only earlier completed owners;
27g qualifies the assembled release. A prompt to implement a milestone means
its complete body/tasks/checks, with no unstated conversation requirement.
Under the charter, a Planned milestone is activated by the owner's implementation
request after its listed prerequisites are complete; record In Progress before
implementation. This planning edit itself activates no runtime work.

### Release packaging

Retain **A = 1.1.0-beta**, **B = 1.2.0-beta**, **C = 1.3.0-beta** and
`Cohesix-<version>-beta-<platform>`. A delivers recoverable CUDA, Verified Private
LoRA Release, adoption/CI and the substantial native SwarmUI showcase. B is the
qualified 28–28g ecosystem release below; C remains reserved. No version is
published or reassigned by this planning change.
Historical releases and evidence keep original names, IDs and proof classes.

### Milestone ownership and historical references <a id="roadmap-id-mapping"></a>

The 23 September 2026 revision keeps 0–27g and their evidence, status and
qualification requirements unchanged. New 28–28g are the bounded Release B
implementation scopes in this file. No deferred milestone gates that release.
The old blanket assurance/provider/semantic/agent dependency chain is removed.

| Former scope (before this consolidation) | Current owner |
| --- | --- |
| 0–27g, including selected CUDA/HF foundations and narrow reopenings | Unchanged 0–27g; extend, do not repeat their accepted work. |
| 28 verification/NIST | Focused release-safety checks in 28; broader assurance in 29. |
| 28a provider catalogue | Selected CUDA in 28a, PEFT in 28b, Apple in 28c; remaining catalogue in 30. |
| 28b semantic objects/capsules | 31. |
| 28c general admission | Narrow shared admission/standing authority in 28; general proof programme in 32. |
| 28d complete production bundle/driver binding | Selected-path safety in 28; stronger full binding/inventory claim in 33. |
| 28e AI/PEFT/NeMo | Concrete PEFT in 28b, Apple compute in 28c, NeMo Agent Toolkit kit in 28f; advanced agent/context/framework/NeMo families in 34. |
| 28f inference gateway | Native serving/canary in 28b; general compatibility/routing/cache gateway in 35. |
| 28g broad MCP/A2A | Shared authority in 28, MCP in 28d, A2A in 28e, NeMo clients in 28f, release qualification in 28g; extra protocol breadth in 36. |
| 29 persistence | 37. |
| 29a service-turn scheduling | 38. |
| 29b operator-lane scheduling | 39. |
| 30 field status | 40. |
| 30a hardware diagnostics | 41. |
| 30b AI namespace projections | 42. |
| 31 AWS | 43. |

Historical task IDs and generated identifiers retain their bytes and meaning;
a reused numeric prefix cannot activate scope or upgrade evidence. The older
pre-19 September task mappings remain resolved as follows:

| Stable task or original scope | Current ownership |
| --- | --- |
| m27b provider-action/evidence/identity/integration foundation | 27b unchanged; selected extensions in 28–28c; wider catalogue in 30. |
| m27b live-reference-workflows and packaging | Accepted recipes/LoRA/install remain 27c/27d/27e; unselected domain/deployment breadth is 30. |
| m27b federation/exporters/industry-sidecars | 30; retain their existing implementation and evidence. |
| m27b-operator-manuals-and-reference-maintenance | 27b unchanged; 27e adoption remains unchanged. |
| m27c-semantic-ir-and-object-contract and former semantic workbench inspector | 31. |
| m27d-live-peft-reference-paths | 27d unchanged. |
| m27d-framework-adapters and m27d-nemo-provider-family | 34; the distinct minimal NeMo Agent Toolkit integration is 28f. |
| m27e-inference-ir-and-compatibility-contract and m27e-inference-receipts-otel-and-evidence | 35. |
| m28c-mcp-policy-ir and m28c-a2a-policy-ir | Retained full-catalogue designs in 36; selected protocol implementation is 28d/28e over 28 authority. |
| m28b-production-worker-ticket-driver-inventory and m28b_production_bundle | 33; no persisted identifier is renamed. |
| m28a-native-provider-discovery-and-actions | 30 inventory; its existing narrow launchd restoration for M27g remains valid at its original scope. |

Each deferred milestone has a current scope/activation boundary and an
expandable retained design inventory. **Only inside those labelled inventories,
numeric milestone references use the former numbering in the first table.**
Those design records preserve task ideas, detailed constraints, commands and
rationale; their former Planned/Reopened or complete-catalogue wording is not
new implementation authority or a Release B gate. At activation, reconcile the
selected remaining tasks with current code and this mapping before execution.
Do not reimplement work accepted by 28x. Current task/claim references elsewhere
must use the current heading and exact task ID, not an old number alone.

### Compatibility and validation ownership

Every implementation milestone reviews the complete host-tool suite (`coh`,
`cohsh`, gateway, ticket agent, GPU and sidecar bridges, CAS/evidence tools,
SwarmUI), `tools/cohesix-py`, compiler/generated consumers and benchmark scripts.
## Milestone 0 — Repository Skeleton & Toolchain <a id="0"></a> 
[Milestones](#Milestones)

**Status:** Complete — the repository/workspace scaffolding, build scripts, size
guard, bounded `m0-rust-1-97-1-security-refresh` task, bounded
Commands: Select focused owner tests and exact host/target builds under TEST_PLAN; run scripts/check-generated.sh and scripts/ci/check_test_plan.sh before merge.
Checks: No thresholds or proof classes change; complete integration evidence and named review support the A release, while B/C reservations remain intact.
Deliverables: Qualified 1.1.0-beta artifacts and evidence-backed adoption/overhead report.
```
## Release B — Governed CUDA, PEFT, Apple and NeMo Workflows <a id="release-b"></a>

**Status:** Planned — owner-directed scope, 23 September 2026. Milestones
28–28g together deliver **1.2.0-beta**. Release A and every 0–27g milestone,
including 27g's status, tasks, acceptance and evidence, remain unchanged.
This planning revision neither activates implementation nor approves a release.

**Product outcome:** existing NeMo agents, CUDA/PEFT workflows and Mac
applications can perform useful GPU work through Cohesix with bounded
authority, observed results and recoverable failures. They do not need another
agent framework, model platform or infrastructure stack.

The release must deliver three complete user journeys:

1. Register an approved useful CUDA workload without changing Cohesix source;
   preflight, submit, observe, verify outputs, cancel and recover safely.
2. Train or import a genuine LoRA adapter, compare it with the base and accepted
   deployment, serve a real canary, promote or roll back and inspect evidence.
3. Use those operations from NeMo through MCP/A2A and from macOS 27 through
   App Intents/Shortcuts/Siri; follow the same run in SwarmUI/CLI. Demonstrate
   genuine local MLX/Metal work as well as explicit remote CUDA operation.

### Release boundaries and common acceptance

- Reuse 27a authority, 27b providers/evidence, 27c recipes, 27d LoRA transactions,
  27e installation and 27f workbench. Extend actual gaps, not duplicate their
  registries, journals, schedulers, verifiers, deployment or recovery machinery.
- Complete every narrow milestone below. An unavailable profile, mock,
  read-only demonstration or documentation-only adapter cannot satisfy a
  required live workflow. No milestone 29–43 gates this release.
- Numerical order is the default delivery sequence, not a blanket dependency
  chain. Each milestone lists its actual contracts. Apple is not a runtime or
  installation prerequisite for NeMo; MCP and A2A remain independently usable.
- Every client uses the same authenticated, delegated action/job contract,
  current-state admission, expiry/revocation, idempotency, writer fencing,
  cumulative budgets and observed result. UI, Siri, model output, protocol
  adapters and provider metadata never mint authority or terminal receipts.
- Models, training, inference, NeMo, Apple frameworks and CUDA/Metal stay
  host-side. Pi Workers remain control-plane roles, not GPU hosts. Large
  inputs, models, datasets, checkpoints and outputs remain in bounded host
  storage/CAS; only control metadata and references cross the VM boundary.
- Record external executor credential custody and bypass limits. A lease or
  preflight estimate is not hard GPU isolation, and seL4 confinement does not
  prove a privileged external host cannot bypass Cohesix. Distinguish estimates,
  enforced limits, observed usage and unavailable measurements.
- Qualify exact Pi 4 Queen, Apple-silicon macOS 27 and Jetson Orin Nano 8GB
  reference profiles. Keep QEMU and any explicitly supplied remote/discrete GPU
  evidence separate. Verify actual SDK, OS, JetPack/L4T/CUDA, model/runtime and
  protocol versions; do not inherit assumed compatibility from old proposals.
- Ship installation, doctor diagnostics, task-oriented CLI/Python help,
  relevant SwarmUI views and failure guidance with each capability. 28g
  integrates them; it is not permission to deliver libraries without a journey.
- Use the existing TEST_PLAN catalog and changed-surface proof layers. Add
  missing focused actions through their owner during implementation. Preserve
  all applicable merge/release gates and immutable evidence rules; no mocks as
  target proof, arbitrary retries, threshold relaxation or unrelated hardware
  reruns. Review all host-tool/Python/generated/benchmark consumers, update
  affected ones atomically and record genuine non-impact.
- Standing authorisation avoids repeated human prompts within its limits;
  platform-required or selected-policy approval remains enforceable. Individual
  changes and milestone acceptance need no human sign-off. Only overall release
  publication/promotion requires the named human release owner's approval.

## Milestone 28 — Shared Governed Jobs and Bounded Unattended Authority <a id="28"></a>

**Value:** every client safely operates the same durable job without granting
unrestricted host access or repeatedly asking permission inside valid policy.
**Prerequisites:** accepted 27g and its exact authority/executor/recipe contracts.

Extend existing run identity with exact subject, action, target, input hashes,
limits, policy, state/resource generation, deadline and idempotency bindings.
Persist execution separately from pending result delivery. Supply status,
cancel, reconcile and verifiable evidence; a lost response never justifies a
second effect. Resume only from a valid runtime-supported checkpoint, otherwise
report a known terminal or explicitly uncertain outcome and an authorised plan.

Implement revocable standing scopes with expiry, cumulative budgets,
concurrency and retry/cooldown limits shared across CLI, REST, Apple, MCP and
A2A. Every effect still needs a fresh state-bound decision. Use the smallest
generated deterministic evaluator for these actions, with authoritative inputs
and dispatch-time reservation/recheck. Missing, contradictory, stale, malformed
or unverifiable inputs refuse. Retain target-side checks; host verdict strings
and model assertions are not grants. Administrative changes require separate
authority; escalation cannot approve itself. General solvers/proofs stay in
29/32, not on the critical path.

```text
Title/ID: m28-selected-job-contract
Milestone: 28 / shared governed jobs
Goal: Extend the accepted lifecycle for selected ecosystem clients without a second execution or evidence path.
Inputs: 27a–27g; coh; host-ticket-agent; cohesix Python; coh-rtc; provider and receipt contracts.
Changes: Exact job/action/input bindings; shared status/cancel/reconcile; separate pending delivery; bounded evidence and client parity.
Commands: Existing focused coh, host-ticket-agent, Python and coh-rtc TEST_PLAN actions; scripts/check-generated.sh; git diff --check.
Checks: Duplicate, lost-response, restart, wrong-target and uncertain-outcome cases preserve identity and never blindly repeat effects.
Deliverables: Shared job contract, affected clients and focused recovery evidence.

Title/ID: m28-standing-authority-and-admission
Milestone: 28 / bounded unattended authority
Goal: Authorise unattended effects only within current revocable policy and shared durable limits.
Inputs: Existing delegation, fencing, journal, policy and target checks; m28-selected-job-contract.
Changes: Narrow current-state admission; standing scopes and cumulative accounting; expiry/revocation; separate administration; clear denials.
Commands: Focused policy/executor race and restart actions; exact QEMU/Pi checks only where target behaviour changes.
Checks: Scope widening, stale generations, forged facts, budget reset, replay and cross-protocol bypass fail before effects; required approval remains enforced.
Deliverables: Tested authority primitive, policy examples and exact enforcement/non-claims.
```

**Done:** a bounded GPU action and one existing allowlisted service-recovery
action execute within standing policy, refuse invalid/stale requests and survive
interrupted result delivery with inspectable outcomes. Selected-path safety
checks are mandatory; broad formal assurance is not.

## Milestone 28a — Useful CUDA Workloads and Reliable GPU Operations <a id="28a"></a>

**Value:** users run their own approved useful workload, not only diagnostic kernels.
**Prerequisites:** 28 and accepted 27b/27c CUDA/executor/recipe foundations.

Provide a supported registration path for a digest-pinned workload package or
container, fixed entry point, typed parameters, declared inputs/outputs,
environment/secret references and bounded resources. Registration is privileged
configuration, never model-generated shell or arbitrary paths. No Cohesix source
edit is needed to add an approved workload. Reuse native process/container owners.

Expose exact device/inventory freshness, memory headroom, admission/queue state,
health/thermal observations, deadline, cancellation, terminal outcome and output
digests. Clean up only the owned job; preserve unrelated processes. Distinguish
supported checkpoint resume from reconciliation or explicit restart. Bound shared
memory and disk; test OOM mapping safely. Ship a real batch inference, embedding
or other useful CUDA/PyTorch recipe and a documented user adaptation, with actual
GPU execution. Retain advertised native/container paths, without rebuilding them.
Jetson is required; discrete GPU, MIG, distributed placement and deep profiling
are independent later claims, not silent substitutes or new baseline scope.

```text
Title/ID: m28a-approved-user-workloads
Milestone: 28a / useful CUDA execution
Goal: Run a user-configured approved workload through the existing governed executor.
Inputs: 27c recipes; 28; gpu-bridge-host; host-cuda; host-ticket-agent; coh/Python; generated profiles.
Changes: Workload registration, typed parameters, digest/entrypoint/device binding, verified outputs and useful reference recipe.
Commands: Focused CUDA/provider/recipe checks and selected live TEST_PLAN actions; scripts/check-generated.sh.
Checks: Genuine execution and verified outputs on the admitted device; arbitrary command/path/credential injection and stale inventory refuse.
Deliverables: Installable recipe, adaptation guide, CLI/Python/SwarmUI operation and live evidence.

Title/ID: m28a-cuda-operations-and-recovery
Milestone: 28a / GPU operations
Goal: Make capacity, cancellation, failure and recovery safe and understandable.
Inputs: m28a-approved-user-workloads; doctor; telemetry; executor journal; shared verifier.
Changes: Memory/health diagnostics, exact-owner cancellation, checkpoint capability and interrupted-job reconciliation.
Commands: Selected live and deterministic timeout/cancel/stale-inventory/OOM/restart/lost-response cases.
Checks: No unrelated process is killed or uncertain effect replayed; unsupported resume/isolation/telemetry is explicit.
Deliverables: Diagnostic views, recovery walkthrough and exact-profile failure evidence.
```

**Done:** a newcomer configures useful work without patching Cohesix and handles
an interruption from published instructions, obtaining verified outputs or an
explicit unresolved outcome rather than invented success.

## Milestone 28b — Deeper PEFT Lifecycle and Verified Serving <a id="28b"></a>

**Value:** genuine adapters progress from training/import to measured, reversible serving.
**Prerequisites:** 28, 28a and accepted 27d LoRA transaction; extend its journal,
registry, evaluator, activation and recovery rather than recreate them.

Use HF PEFT as the primary open training/artifact boundary. Support configurable
LoRA training and independent compatible adapter import. Add QLoRA only where a
qualified model/runtime pair needs it to fit the reference; not every PEFT method.
Pin base revision, tokenizer/config, dataset snapshot, framework/runtime and
training settings. Validate structure, sizes, hashes, format, compatibility and
provenance. Use safe formats; disable arbitrary remote-code loading in the reference.
Explicitly configure licence acceptance, gated downloads, uploads and private data.
An imported adapter cannot invent training provenance; an artifact reference is
not licence or safety certification.

Reuse real framework checkpoints where supported, distinguishing full training
resume from adapter-only restart. Record stochastic settings without promising
identical weights. Compare candidate, base and incumbent on held-out task data
using predeclared quality/resource/latency thresholds; preserve negative results.
No judge score or scan alone proves safety or quality.

Complete train/import, evaluate, scan, canary, generation-fenced promotion and
rollback through the existing phase-journalled path. Serve a real application
through one supported native runtime endpoint, with observed model/adapter
identity, readiness and inference canary. Publication or HTTP success is not a
verified deployment. Keep general API cloning/proxying/routing in 35. HF, MLX and
NeMo adapters interoperate only for actually qualified format/model/runtime pairs;
no blanket compatibility or general conversion programme.

```text
Title/ID: m28b-configurable-peft-training-and-import
Milestone: 28b / native PEFT depth
Goal: Extend accepted training/import with useful configuration and truthful checkpoint recovery.
Inputs: 27d; 28/28a; HF PEFT artifacts; existing CAS, registry, executor and validators.
Changes: Pinned recipe inputs, bounded training parameters, provenance/format checks, supported checkpoints and genuine import.
Commands: Focused PEFT transaction/provider checks and exact-reference train/import/resume actions.
Checks: Real compatible artifacts and observed outcomes required; unsafe/corrupt inputs, path races, duplicates and stale generations fail safely.
Deliverables: Configurable train/import workflow, recovery guide and genuine adapter evidence.

Title/ID: m28b-evaluate-canary-promote-rollback
Milestone: 28b / verified serving
Goal: Compare and deploy through one real application-facing runtime with reversible promotion.
Inputs: m28b-configurable-peft-training-and-import; existing 27d evaluation/serving/rollback contract.
Changes: Fixed held-out comparisons, runtime identity/readiness, real canary requests, promotion recheck and rollback views.
Commands: Focused evaluator/activation tests and live serving, rejected-candidate, failed-canary, interrupted-promotion and rollback cases.
Checks: Failed/stale candidates cannot promote; served behaviour and identity are observed; rollback restores the accepted generation without fabricated receipts.
Deliverables: Usable endpoint, comparison results, complete release guide and recoverable deployment evidence.
```

**Done:** both genuine training and independent import work; a real client uses
the canary and accepted deployment; rejection and rollback are demonstrated.
Judge depth by this lifecycle, not the number of supported frameworks.

## Milestone 28c — macOS 27: Siri/App Intents, Apple AI and Metal-Backed Workflows <a id="28c"></a>

**Value:** the Mac is a native operator interface and local AI host, not merely remote control.
**Prerequisites:** 28 and selected 28a/28b operations; no MCP/A2A/NeMo dependency.

Target macOS 27 on the supported Apple-silicon reference. Verify and pin public
Xcode/SDK APIs, OS build, device/language/region availability before implementation;
no invented private Siri API or assumption that Siri speaks MCP/A2A. Preserve
working older host tools unless a documented compatibility change is necessary.
Add the smallest native Swift App Intents integration to the existing signed app;
no SwarmUI rewrite or second desktop product. Swift remains host-side.

Expose configured hives, approved recipes, runs and deployments through App
Intents/App Shortcuts: inspect status, start an approved job, inspect results,
request cancellation and permitted promotion/rollback. Deep-link to SwarmUI.
Return durable job references promptly; long jobs survive Siri/extension exit.
Use authenticated shared operations and native secure credential storage, not
AppleScript/UI scripting or shell. Spoken text, device possession and model
confidence are not grants. Respect platform authentication/confirmation and
Cohesix policy; scope indexed entities/results and withdraw stale/revoked entries.

Require one Foundation Models assistance flow: explain a run/refusal from scoped
evidence and propose a typed approved next action. Label model interpretations,
link sources and retain deterministic/manual operation when Intelligence is
unavailable or disabled. Do not ingest Personal Context or build a generic assistant,
cloud router or arbitrary model-provider distribution platform.

Use **MLX/MLX-LM as the single required Apple compute backend**, reusing Metal
rather than custom kernels. Demonstrate small real local inference and LoRA
train/evaluate/serve/rollback over the 28b lifecycle, with actual GPU execution,
unified-memory limits, cancellation and recovery. Distinguish local MLX, remote
CUDA and Foundation Models assistance in capability probes and UI. No implicit
CPU/remote fallback or universal HF/NeMo-to-MLX format claim. MPS/Core ML and
mobile/watch/visionOS companions are deferred. Include signing/notarisation for
the chosen distribution channel; missing credentials block that validation.

```text
Title/ID: m28c-native-apple-actions
Milestone: 28c / native entry points
Goal: Expose selected governed jobs through App Intents, Shortcuts and Siri without new authority.
Inputs: Pinned public SDK; existing SwarmUI package; 28 schemas; delegated credentials; recipes/run views.
Changes: Minimal native actions/entities, secure credential handoff, scoped discovery, durable links and availability diagnostics.
Commands: Native build/signing and intent tests; public Shortcuts/live Siri walkthroughs; shared authority negatives.
Checks: Same jobs/receipts across clients; ambiguous/unauthenticated/stale input cannot act; jobs survive client exit; disabled Intelligence leaves core operation usable.
Deliverables: Packaged actions, real Siri/Shortcuts examples and exact-device availability evidence.

Title/ID: m28c-mlx-metal-provider
Milestone: 28c / local compute
Goal: Run a genuine small-model MLX inference and LoRA lifecycle through existing jobs/deployment.
Inputs: 28b; pinned MLX/MLX-LM; supported model/data; exact Mac memory and Metal capabilities.
Changes: One bounded Apple provider, explicit topology, format/runtime checks, GPU observations and resource-safe recovery.
Commands: Focused adapter checks and live Mac infer/train/evaluate/canary/rollback/cancel actions.
Checks: Local GPU execution observed; no hidden CPU/remote fallback; memory/format failures safe; no universal adapter-portability claim.
Deliverables: Installable MLX profile, complete local journey and per-backend evidence.

Title/ID: m28c-foundation-models-assistance
Milestone: 28c / evidence-grounded Apple AI
Goal: Explain operations and propose valid next actions without model authority.
Inputs: Public pinned Foundation Models SDK; scoped evidence; shared typed actions; native app.
Changes: One assistance flow, labelled interpretation/source links, typed proposals, availability/privacy posture and deterministic fallback.
Commands: Native model/tool tests; supported-device exercise; unavailable/disabled/injection/unauthorised-action cases.
Checks: Prose cannot invent operational success or execute without admission; only scoped evidence supplied; model unavailability never blocks manual operation.
Deliverables: Useful native assistance with tested limits, not another general-purpose assistant.
```

**Done:** actual supported-device Siri/Shortcuts operation, evidence-grounded
assistance and local Metal work are exercised. Unit tests or Shortcuts alone
cannot establish live Siri acceptance; unresolved platform availability is explicit.

## Milestone 28d — MCP Access to Complete Selected Workflows <a id="28d"></a>

**Value:** ordinary agents use useful Cohesix workflows without learning its namespaces.
**Prerequisites:** 28 and selected 28a/28b actions; Apple providers only when exposed.

Use a pinned MCP revision and maintained SDK where suitable. Provide the network
transport needed by NeMo and a packaged local desktop-client path, reusing transport
adapters rather than authority implementations. Curate capability discovery,
preflight/submit/status/cancel/recover, adapter comparison/promotion/rollback and
evidence tools/resources. Generate schemas/descriptions from the shared contract;
no generic arbitrary file-write or complete administrative catalogue requirement.

Publish examples, counterexamples, required authority, pending/terminal semantics,
refusal and recovery. Preserve delegated identity, shared budgets, revocation and
current admission. Bound input/concurrency, enforce transport auth/Origin where
applicable, pin negotiation/errors and scope discovery. Never forward caller
credentials to providers or execute model-returned calls. Independently disabling
MCP must not break REST, A2A or existing jobs.

```text
Title/ID: m28d-mcp-selected-workflows
Milestone: 28d / MCP integration
Goal: Let ordinary MCP clients discover and complete selected governed CUDA/PEFT jobs.
Inputs: 28; accepted providers; hive-gateway; maintained SDK; NeMo transport requirements.
Changes: Pinned transports, curated catalog/guidance, scoped auth/discovery, job mapping, independent controls and package/doctor support.
Commands: Protocol conformance, shared authority negatives and ordinary-client live submit/observe/recover/evidence walkthroughs.
Checks: No direct executors, arbitrary writes or client-authored success; a consequential workflow works inside standing authority; disabled mode stays coherent.
Deliverables: Usable integration, versioned client configuration and live recovery evidence.
```

**Done:** a standard client completes useful work from published discovery and
configuration, including refusal/recovery. Read-only success is intermediate;
full administrative parity and MCP FUSE remain deferred to 36.

## Milestone 28e — A2A Delegation of Durable Selected Jobs <a id="28e"></a>

**Value:** agents delegate long GPU/adapter jobs and reconnect to their actual result.
**Prerequisites:** 28 and selected providers; shared gateway code may be reused,
but MCP enablement/conformance is not required.

Implement one pinned A2A revision/binding using a maintained SDK: scoped Agent
Card/skills, authenticated task creation, progress/streaming, status, verifiable
artifact references, cancellation and reconnect/recovery. Back tasks with existing
durable jobs, not another scheduler, mailbox or checkpoint engine. A task does
not grant arbitrary sub-actions. Each effect needs fresh admission and shared
accounting; retry across protocols cannot reset budgets or repeat uncertain work.
Map terminal/failed/cancelled/ambiguous states from observed provider outcomes.
Arbitrary push callbacks, extra bindings and generic multi-agent coordination
stay in 36/34.

```text
Title/ID: m28e-a2a-durable-jobs
Milestone: 28e / A2A delegation
Goal: Delegate and recover selected long-running jobs through a standard peer.
Inputs: 28 job/authority/receipts; selected providers; shared gateway primitives; pinned SDK and NeMo peer.
Changes: Card/skills, task/job mapping, scoped auth, progress/artifacts, cancellation, reconnect and independent enablement.
Commands: Selected-binding conformance, shared-budget negatives and live peer delegation/reconnect/cancel/recovery.
Checks: A2A-only works; task IDs cannot widen scope or disclose another caller's evidence; restart/ambiguity never causes blind re-execution.
Deliverables: A2A service, peer configuration, lifecycle guide and independently observed outcomes.
```

**Done:** a named standard peer delegates a real CUDA/PEFT job, disconnects and
reconnects to its correct scoped outcome. Discovery or mock task completion is insufficient.

## Milestone 28f — NeMo Agent Toolkit Adoption Kit and Live Integration <a id="28f"></a>

**Value:** NeMo developers can use Cohesix inside a normal workflow, not only read a compatibility claim.
**Prerequisites:** 28, selected CUDA/PEFT providers, 28d and 28e. Apple is optional
for a NeMo deployment, not a required package or runtime dependency.

Verify and use the selected Toolkit release's native MCP/A2A clients first.
Ship a pinned kit with version constraints/lockfile, concise configuration,
authentication/secret references, typed outputs, correlation and runnable workflows.
Use a small plugin only for a demonstrated native-configuration gap; no fork,
generic wrapper framework or duplicate SDK/action catalogue.

Require two real workflows: a NeMo agent preflights/submits/inspects an approved
CUDA job through MCP; and a NeMo workflow delegates adapter evaluation/canary/
release through A2A with a failed candidate or interruption. Demonstrate bounded
unattended operation and a denied action. NeMo owns its planner/model/checkpoint
state; Cohesix owns admitted effects, operational recovery and receipts.

Expose the accepted HF PEFT training/artifact/serving path through these workflows;
it need not use NeMo Framework as trainer to be useful to NeMo agents. Qualify
NeMo-produced imports by exact format/base/runtime evidence, never branding.
Use the workflow's configured model endpoint without a new Cohesix inference
proxy. Local/remote NIM or other NeMo services are separately selected and probed,
not required alongside full NeMo Framework/Triton/Kubernetes on the 8GB Jetson.

Use native Toolkit evaluation/profiling to compare direct and Cohesix-backed
operation on the same task: setup, credential exposure, results, recovery,
control latency and evidence utility. Record bad results and unknowns; tracing
correlation never substitutes for authoritative provider outcomes. External
upstream catalog acceptance is not a release gate; publication requires owner
authorisation/credentials. Broad NeMo families remain deferred to 34.

```text
Title/ID: m28f-nemo-agent-toolkit-kit
Milestone: 28f / ecosystem entry
Goal: Expose useful Cohesix tools/jobs from a normal pinned NeMo installation without a fork.
Inputs: Verified Toolkit MCP/A2A clients; 28d/28e; selected recipes; existing Python and evidence interfaces.
Changes: Minimal integration kit, native config, scoped auth, exact version matrix, useful examples and only necessary correlation helpers.
Commands: Clean-environment install and ordinary Toolkit workflow/config tests using exact candidate/released artifacts.
Checks: No editable repository install, private setup knowledge, raw executor credentials or parallel registry; optional services explicitly unavailable when absent.
Deliverables: Versioned kit, quick start, support matrix and packaging evidence.

Title/ID: m28f-nemo-live-workflows-and-value
Milestone: 28f / live qualification
Goal: Demonstrate useful CUDA and adapter jobs, bounded autonomy and recovery through both standard clients.
Inputs: Integration kit; real providers and configured model endpoint; reference data/incumbent; native profiling/evaluation.
Changes: MCP CUDA and A2A adapter journeys, denied/failed/interrupted cases, direct comparison and portable receipt links.
Commands: Live Toolkit runs and focused failure injections in their owning TEST_PLAN layers; capture exact toolkit/protocol/provider/target versions.
Checks: Both protocols cause and observe permitted work; denial has no effect; retry/reconnect preserves identity; overhead/limitations measured rather than invented.
Deliverables: Reproducible examples, live recovery evidence and practical integration assessment.
```

**Done:** another developer uses the kit with ordinary pinned Toolkit and a
configured Cohesix deployment, reproducing both workflows from public instructions.

## Milestone 28g — Installation, Integrated User Qualification and Release B <a id="28g"></a>

**Value:** complete capabilities form an adoptable release rather than disconnected adapters.
**Prerequisites:** every required 28–28f outcome with exact-profile evidence;
no later milestone is a replacement for missing work or a new release gate.

Extend existing least-privilege install/distribution, rollback and uninstall
paths. Ship the Mac package, supported host/GPU tools, Python/NeMo kit, client
configuration and recipes. Model/data downloads are explicit, with declared
size, licence and credentials; do not hide them in installation or create a
new distribution service. Preserve required package hashes/signatures/SBOMs.

Doctor diagnoses versions, endpoint ownership, credentials, storage, GPU/model/
runtime compatibility, protocol controls and Apple availability with actionable
remedies. Secrets never appear in example arguments/logs. CLI, SwarmUI, Siri and
agents show the same job and distinguish local MLX from remote CUDA. Improve
existing task views and explanations, not the UI theme or rendering architecture.

Run all three product journeys from clean supported environments and published
instructions without source patches or developer-only setup. Qualify MCP-only,
A2A-only, both and neither, without losing existing job state. Include Mac-originated
remote CUDA/PEFT, local MLX and both NeMo flows; protocol services must work
without macOS/Apple Intelligence. Require at least one independent clean-install
walkthrough; identify whether the evaluator is a person or agent rather than
claiming unobserved community adoption.

Before integrated runs, fix acceptance budgets against the retained baseline:
installation steps/time/download size, first useful job, manual interventions,
completion/quality, refusal clarity, interrupted recovery and added control
latency. Do not relax budgets after failure. Compare output quality as well as
mechanics; no invented savings or deterministic model-output promises.

Keep host-process recovery separate from Queen reboot persistence. Queen loss
must fail closed and reconcile safely; automatic VM-local durable resumption
remains 37, not a Release B claim. A missing required API/package/credential/
hardware/evidence is a named blocker, not permission to lower the release promise.

```text
Title/ID: m28g-installation-and-integrated-adoption
Milestone: 28g / integrated user qualification
Goal: Make complete CUDA, PEFT, Apple and NeMo journeys installable from public instructions.
Inputs: Accepted 28–28f artifacts; 27e install; 27f workbench; existing doctor/release machinery.
Changes: Exact packages, minimal configuration, task views/help, actionable diagnostics and independent clean-environment walkthroughs.
Commands: Existing package/install/doctor checks and the three full journeys; applicable generated/link checks.
Checks: No repository patches, undisclosed credentials, mock outcomes or unrelated stack; topology/proof limits explicit; all protocol-control configurations coherent.
Deliverables: Installable candidate, newcomer guides, native/agent configurations and measured adoption evidence.

Title/ID: m28g-release-b-qualification
Milestone: 28g / assembled qualification
Goal: Qualify the bounded ecosystem release without reintroducing deferred catalogues.
Inputs: Exact candidate, TEST_PLAN evidence, fixed user/overhead budgets and existing evidence/exception rules.
Changes: Integrated security/failure/recovery, compatibility review, supported-claim matrix, bounded evidence and Release B notes.
Commands: Complete applicable staged, pressure/repeatability, target and release gates under TEST_PLAN; no unrelated catalogue qualification.
Checks: Required live workflows, authority negatives, recovery and budgets pass at exact source/profile identity; missing checks remain blockers; no deferred claim promoted.
Deliverables: Qualified 1.2.0-beta candidate/evidence; publish or promote only with named human release-owner approval.
```

## Deferred milestones — preserved designs, separate activation <a id="deferred-milestones"></a>

Milestones 29–43 retain the other ideas in this single build plan. None is a
28x gate or an automatic next implementation obligation. Activation requires
a named user/engineering need, bounded remaining tasks and explicit owner
instruction. Each expandable design inventory preserves earlier technical
constraints, original task IDs and former-number references resolved by the
[ownership map](#roadmap-id-mapping), not a competing live roadmap.

Retain working behaviour and evidence. Reuse requirements already satisfied by
28x rather than repeat them. Selected-path safety/correctness/recovery defects
remain repairs in their actual owner; deferral cannot waive their standards.

## Milestone 29 — Extended Formal Assurance and NIST Evidence <a id="29"></a>
[Milestones](#Milestones)

**Status:** Deferred — explicit activation; not part of Release B.

**Preserved scope:** Retain the claim register, proof-carrying manifest witnesses, bounded Secure9P/HAL/resource checks, state-machine models, CI proof evidence and NIST/OSCAL assessment. Select work for a named assurance claim or consequential defect class; narrow checks required by shipped 28x remain in 28, not here.

**Activation and completion:** Identify the concrete need, inspect retained
implementation/evidence, and specify only the unfulfilled tasks and exact
acceptance claims. Preserve the safety constraints below, but reconcile its
former-number references and superseded breadth before implementation. No
whole-catalogue or later prerequisite is inherited by 28x.

<details>
<summary>Retained design inventory — former Milestone 28; current owner 29</summary>

The following records preserve the previous detailed proposal. Their numeric
cross-references use the former-number [mapping](#roadmap-id-mapping), and their
former status/completion wording is subordinate to the current deferred scope
above. They neither reactivate work nor duplicate accepted 28x requirements.


[Milestones](#Milestones)

**Status:** Planned — after the 27g release; preserve existing implementation and evidence.

**Delivery posture:** Implement all listed assurance deliverables: claim register,
generated witnesses, Secure9P/HAL/resource checks, TLA+/PlusCal models, bounded
checking, restricted policy IR, CI verification, and NIST mapping. Each claim
retains its exact proof class and evidence; a subset cannot close the milestone.

Deliverables:
  - Checked policy-IR and proof-vocabulary foundation consumed by Milestone 28c per-intent admission.
```



</details>

## Milestone 30 — Broader Providers, Federation, Deployment and Domain Workflows <a id="30"></a>
[Milestones](#Milestones)

**Status:** Deferred — explicit activation; not part of Release B.

**Preserved scope:** Retain the wider provider catalogue, enterprise identity, Kubernetes/CEL/DRA, MIG/DCGM/CUPTI, native deployment profiles, federation, portable evidence/exporters/SIEM, industrial protocols and domain playbooks. Qualify a named adopter workflow at a time. Existing CUDA/PEFT/Apple paths from 28a–28c are reused, not reimplemented.

**Activation and completion:** Identify the concrete need, inspect retained
implementation/evidence, and specify only the unfulfilled tasks and exact
acceptance claims. Preserve the safety constraints below, but reconcile its
former-number references and superseded breadth before implementation. No
whole-catalogue or later prerequisite is inherited by 28x.

**Existing restoration remains unchanged:** the retained
`m28a-native-provider-discovery-and-actions` launchd transitional-observation
repair stays narrowly Reopened for M27g only, with its existing deadline,
single-dispatch, identity checks and parser/native evidence obligations.
This renumbering adds no requirement to 27g and does not defer that repair.

<details>
<summary>Retained design inventory — former Milestone 28a; current owner 30</summary>

The following records preserve the previous detailed proposal. Their numeric
cross-references use the former-number [mapping](#roadmap-id-mapping), and their
former status/completion wording is subordinate to the current deferred scope
above. They neither reactivate work nor duplicate accepted 28x requirements.


[Milestones](#Milestones)

**Status:** Planned — deferred breadth; existing implementation remains preserved.
The retained launchd observation defect is narrowly Reopened under
`m28a-native-provider-discovery-and-actions` for M27g restoration only.

**Prerequisites:** completed 27g and 28. This milestone extends the qualified
27b foundation, 27c recipes, 27d LoRA transaction and 27e packaging. The complete
catalogue below belongs here, including Kubernetes/controllers/CEL/DRA,
launchd/Xcode/codesign/notary/App Store Connect, Apple ML, MIG/DCGM/CUPTI,
- Semantic extraction, 28c intent admission, AI run control, inference interoperability, and MCP/A2A interop can build on proven provider and integration schemas instead of inventing action catalogs or silently treating mock workflows as live.


</details>

## Milestone 31 — Semantic Object Fabric and Context Capsules <a id="31"></a>
[Milestones](#Milestones)

**Status:** Deferred — explicit activation; not part of Release B.

**Preserved scope:** Retain immutable semantic objects/edges, compiler and history views, extractor registry, bounded queries, indexes, visibility/provenance, capsule planning/rendering and the workbench inspector. Activate for a measured retrieval/context problem not solved by existing artifact and evidence references; no current 28x workflow depends on this substrate.

**Activation and completion:** Identify the concrete need, inspect retained
implementation/evidence, and specify only the unfulfilled tasks and exact
acceptance claims. Preserve the safety constraints below, but reconcile its
former-number references and superseded breadth before implementation. No
whole-catalogue or later prerequisite is inherited by 28x.

<details>
<summary>Retained design inventory — former Milestone 28b; current owner 31</summary>

The following records preserve the previous detailed proposal. Their numeric
cross-references use the former-number [mapping](#roadmap-id-mapping), and their
former status/completion wording is subordinate to the current deferred scope
above. They neither reactivate work nor duplicate accepted 28x requirements.


[Milestones](#Milestones)

**Status:** Planned — after the 27g release; preserve existing implementation and evidence.

**Delivery posture:** Implement the complete semantic object fabric and Context
Capsules: typed objects/edges, compiler and history views, extractor registry,
bounded queries, visibility/provenance, deterministic rendering, host tools,
and conformance. All listed deliverables are required; capsule-only work is an
intermediate result.

Commands: Select focused owner tests and exact host/target builds under TEST_PLAN; run scripts/check-generated.sh and scripts/ci/check_test_plan.sh before merge.
Checks: The inspector verifies the owner records through shared libraries independently of MCP/A2A; UI caches or screenshots cannot strengthen evidence.
Deliverables: Backend-owned, qualified workbench inspector.
```

</details>

## Milestone 32 — General Machine-Checked Admission and Proof Artefacts <a id="32"></a>
[Milestones](#Milestones)

**Status:** Deferred — explicit activation; not part of Release B.

**Preserved scope:** Retain the general restricted-policy IR, authoritative-fact snapshots, seven-verdict engine, solver/evaluator correspondence, decision witnesses, independently checked proof artefacts and richer inspection. Extend the tested narrow admission from 28 only for demonstrated policy needs. Never create a competing grant issuer or weaken current-state checks.

**Activation and completion:** Identify the concrete need, inspect retained
implementation/evidence, and specify only the unfulfilled tasks and exact
acceptance claims. Preserve the safety constraints below, but reconcile its
former-number references and superseded breadth before implementation. No
whole-catalogue or later prerequisite is inherited by 28x.

<details>
<summary>Retained design inventory — former Milestone 28c; current owner 32</summary>

The following records preserve the previous detailed proposal. Their numeric
cross-references use the former-number [mapping](#roadmap-id-mapping), and their
former status/completion wording is subordinate to the current deferred scope
above. They neither reactivate work nor duplicate accepted 28x requirements.


[Milestones](#Milestones)

**Status:** Planned — after the 27g release; preserve existing implementation and evidence.

**Delivery posture:** Implement the complete admission contract and every listed
reference action and host-tool integration. Consequential actions require exact
facts, policy, grants, stale-state refusal, and receipts. Read-only inspection,
dry-run, and capsule rendering remain non-mutating operations, not alternate
milestone completion paths.

Commands: Select focused owner tests and exact host/target builds under TEST_PLAN; run scripts/check-generated.sh and scripts/ci/check_test_plan.sh before merge.
Checks: The inspector verifies the owner records through shared libraries independently of MCP/A2A; UI caches or screenshots cannot strengthen evidence.
Deliverables: Backend-owned, qualified workbench inspector.
```

</details>

## Milestone 33 — Production Worker Binding, Driver Inventory and Quarantine <a id="33"></a>
[Milestones](#Milestones)

**Status:** Deferred — explicit activation; not part of Release B.

**Preserved scope:** Retain complete Worker ticket/lease-to-live-bundle binding, ticket-free driver inventory, generation-keyed quarantine, fresh-ticket Worker restart and fresh-generation driver recovery. Reuse 26e containment and the exact current admission contract. Stronger bundle claims require this evidence; basic revoke/generation/fault safety needed by 28x cannot be deferred here.

**Activation and completion:** Identify the concrete need, inspect retained
implementation/evidence, and specify only the unfulfilled tasks and exact
acceptance claims. Preserve the safety constraints below, but reconcile its
former-number references and superseded breadth before implementation. No
whole-catalogue or later prerequisite is inherited by 28x.

<details>
<summary>Retained design inventory — former Milestone 28d; current owner 33</summary>

The following records preserve the previous detailed proposal. Their numeric
cross-references use the former-number [mapping](#roadmap-id-mapping), and their
former status/completion wording is subordinate to the current deferred scope
above. They neither reactivate work nor duplicate accepted 28x requirements.


[Milestones](#Milestones)

**Status:** Planned — after the 27g release; preserve existing implementation and evidence.

**Delivery posture:** Implement and qualify complete production Worker ticket/lease
binding, driver-inventory projection, structured quarantine, and fresh-ticket
restart. Profile selection controls runtime use; it cannot omit these
implementation or acceptance requirements.

Commands: Select focused owner tests and exact host/target builds under TEST_PLAN; run scripts/check-generated.sh and scripts/ci/check_test_plan.sh before merge.
Checks: The inspector verifies the owner records through shared libraries independently of MCP/A2A; UI caches or screenshots cannot strengthen evidence.
Deliverables: Backend-owned, qualified workbench inspector.
```

</details>

## Milestone 34 — Advanced Agents, Context Optimisation and Broader NeMo Families <a id="34"></a>
[Milestones](#Milestones)

**Status:** Deferred — explicit activation; not part of Release B.

**Preserved scope:** Retain task graphs/handoffs, durable semantic context, experiments, attention/prefix/hotset policies, broader framework/PEFT adapters, NeMo Run/customisation, Guardrails, evaluation, retrieval and serving families. Reuse 28b/28c transactions and the 28f NeMo Agent Toolkit kit. Add only a named capability the external framework and standard protocols do not already supply.

**Activation and completion:** Identify the concrete need, inspect retained
implementation/evidence, and specify only the unfulfilled tasks and exact
acceptance claims. Preserve the safety constraints below, but reconcile its
former-number references and superseded breadth before implementation. No
whole-catalogue or later prerequisite is inherited by 28x.

<details>
<summary>Retained design inventory — former Milestone 28e; current owner 34</summary>

The following records preserve the previous detailed proposal. Their numeric
cross-references use the former-number [mapping](#roadmap-id-mapping), and their
former status/completion wording is subordinate to the current deferred scope
above. They neither reactivate work nor duplicate accepted 28x requirements.


[Milestones](#Milestones)

**Status:** Planned — after the 27g release; preserve existing implementation and evidence.

**Delivery posture:** Implement the complete AI/PEFT substrate: delegated and
governed runs, durable context, task graphs/handoffs, inference strategies,
prefix/hotset reuse, all listed framework/NeMo adapters, and provider parity.
- Milestone 28f can expose a provider-neutral, auditable inference boundary over
  the accepted run, capsule, ticket, and receipt semantics.
- Milestone 30b can expose stable AI namespace roots based on proven host-side semantics rather than speculation.


</details>

## Milestone 35 — General Inference Gateway, Compatibility and Routing <a id="35"></a>
[Milestones](#Milestones)

**Status:** Deferred — explicit activation; not part of Release B.

**Preserved scope:** Retain broad Models/Chat Completions/Responses/Embeddings compatibility, provider-neutral streaming/receipts, context/cache policies, routing aliases, shadow promotion, telemetry and general SDK conformance. Activate for a client that native runtime endpoints cannot serve. The real application endpoint, canary and rollback required by 28b are already release work, not deferred.

**Activation and completion:** Identify the concrete need, inspect retained
implementation/evidence, and specify only the unfulfilled tasks and exact
acceptance claims. Preserve the safety constraints below, but reconcile its
former-number references and superseded breadth before implementation. No
whole-catalogue or later prerequisite is inherited by 28x.

<details>
<summary>Retained design inventory — former Milestone 28f; current owner 35</summary>

The following records preserve the previous detailed proposal. Their numeric
cross-references use the former-number [mapping](#roadmap-id-mapping), and their
former status/completion wording is subordinate to the current deferred scope
above. They neither reactivate work nor duplicate accepted 28x requirements.


[Milestones](#Milestones)

**Status:** Planned — after the 27g release; preserve existing implementation and evidence.

**Delivery posture:** Implement the complete pinned inference contract: Models,
Chat Completions, Responses, Embeddings, streaming/cancellation, Context Capsule
refs, receipts, policy aliases/routing, SDK conformance, telemetry, and host
integration. Fixed-model and governed requests are both required; deployment
policy selects their use and 28c admission governs consequential actions.

Commands: Select focused owner tests and exact host/target builds under TEST_PLAN; run scripts/check-generated.sh and scripts/ci/check_test_plan.sh before merge.
Checks: The inspector verifies the owner records through shared libraries independently of MCP/A2A; UI caches or screenshots cannot strengthen evidence.
Deliverables: Backend-owned, qualified workbench inspector.
```

</details>

## Milestone 36 — Extended MCP/A2A Coverage and Resource Mounts <a id="36"></a>
[Milestones](#Milestones)

**Status:** Deferred — explicit activation; not part of Release B.

**Preserved scope:** Retain exhaustive administrative/operation coverage, richer discovery/prompt catalogues, additional protocol bindings, push callbacks and the read-only MCP resource/FUSE mount. Reuse the independently usable 28d/28e services and shared standing authority from 28. No second task scheduler, authority evaluator or protocol-specific budget ledger.

**Activation and completion:** Identify the concrete need, inspect retained
implementation/evidence, and specify only the unfulfilled tasks and exact
acceptance claims. Preserve the safety constraints below, but reconcile its
former-number references and superseded breadth before implementation. No
whole-catalogue or later prerequisite is inherited by 28x.

<details>
<summary>Retained design inventory — former Milestone 28g; current owner 36</summary>

The following records preserve the previous detailed proposal. Their numeric
cross-references use the former-number [mapping](#roadmap-id-mapping), and their
former status/completion wording is subordinate to the current deferred scope
above. They neither reactivate work nor duplicate accepted 28x requirements.


[Milestones](#Milestones)

**Status:** Planned — after the 27g release; preserve existing implementation and evidence.

**Delivery posture:** Implement and independently qualify both MCP and A2A across
the selected profile's complete admitted gateway operations and use cases,
including consequential unattended execution, observation, recovery, and
Manifest-disabled protocols expose nothing. MCP-only and A2A-only deployments
are independently usable and qualified. VM grammar, Secure9P semantics, and
generated manifest authority remain unchanged.


</details>

## Milestone 37 — VM-Local Persistence and Reboot Recovery <a id="37"></a>
[Milestones](#Milestones)

**Status:** Deferred — explicit activation; not part of Release B.

**Preserved scope:** Retain bounded spool/settings semantics, QEMU block evidence, isolated Pi EMMC2 storage, safe media layout, power-loss/reboot recovery, and CYW43/shared-IRQ coexistence. Activate for a deployment that needs durable disconnected/reboot operation. Host checkpoints and safe reconciliation after Queen loss do not establish VM persistence.

**Activation and completion:** Identify the concrete need, inspect retained
implementation/evidence, and specify only the unfulfilled tasks and exact
acceptance claims. Preserve the safety constraints below, but reconcile its
former-number references and superseded breadth before implementation. No
whole-catalogue or later prerequisite is inherited by 28x.

<details>
<summary>Retained design inventory — former Milestone 29; current owner 37</summary>

The following records preserve the previous detailed proposal. Their numeric
cross-references use the former-number [mapping](#roadmap-id-mapping), and their
former status/completion wording is subordinate to the current deferred scope
above. They neither reactivate work nor duplicate accepted 28x requirements.


[Milestones](#Milestones)

**Status:** Planned as one complete QEMU and Pi persistence milestone.
Implement both target backends, bounded spool/settings semantics, containment,
recovery, and acceptance. No user-demand or host-storage-insufficiency decision
is required. Pi work consumes the exact SMP+MCS topology, current-image CYW43
coexistence record, and same-harness performance/repeatability baseline from
`m26e-mcs-smp-target-acceptance`.

storage fault injection gives the most decision-rich recovery case, and
whether an already-factored SD/MMC core ever makes the optional QEMU SDHCI lane
cheap enough to promote. None is a reason to design around Pi behavior before
the first physical canary.


</details>

## Milestone 38 — Core-Local Service-Turn Scheduling <a id="38"></a>
<a id="29a"></a>
[Milestones](#Milestones)

**Status:** Deferred — explicit activation; not part of Release B.

**Preserved scope:** Retain generated service buckets, queue/locality optimisation, sharded telemetry/spool drains, observability and exact-target pressure evidence. Activate from a measured useful-workload bottleneck. Preserve the accepted SMP+MCS topology and independent admission/reserve bounds; no speculative scheduler replacement.

**Activation and completion:** Identify the concrete need, inspect retained
implementation/evidence, and specify only the unfulfilled tasks and exact
acceptance claims. Preserve the safety constraints below, but reconcile its
former-number references and superseded breadth before implementation. No
whole-catalogue or later prerequisite is inherited by 28x.

<details>
<summary>Retained design inventory — former Milestone 29a; current owner 38</summary>

The following records preserve the previous detailed proposal. Their numeric
cross-references use the former-number [mapping](#roadmap-id-mapping), and their
former status/completion wording is subordinate to the current deferred scope
above. They neither reactivate work nor duplicate accepted 28x requirements.


[Milestones](#Milestones)

**Delivery posture:** Implement and qualify core-local service-turn scheduling on
QEMU and Pi. Establish the same-harness baseline and demonstrate bounded
locality, contention, throughput, and tail latency; an existing SLO miss is not
Checks: Host pressure stays semantic-only; QEMU proves regression stability; Pi throughput claims require fresh target logs and separated acceptance lanes; same-harness REST artifacts compare service-bucket results to the 26d rolling baseline with service-bucket counters attached.
Deliverables: Repeatable validation and benchmark lanes for core-local SMP optimization.
```



</details>

## Milestone 39 — Operator-Lane Scheduling and Multi-Surface Responsiveness <a id="39"></a>
<a id="29b"></a>
[Milestones](#Milestones)

**Status:** Deferred — explicit activation; not part of Release B.

**Preserved scope:** Retain bounded fairness, resumable output, backpressure/degradation and pressure evidence across serial, USB, authenticated TCP, HDMI, network, diagnostics and optional persistence. Use 38 only when its service buckets are actually needed. Existing liveness defects remain current repairs, not a reason to wait for this optimisation.

**Activation and completion:** Identify the concrete need, inspect retained
implementation/evidence, and specify only the unfulfilled tasks and exact
acceptance claims. Preserve the safety constraints below, but reconcile its
former-number references and superseded breadth before implementation. No
whole-catalogue or later prerequisite is inherited by 28x.

<details>
<summary>Retained design inventory — former Milestone 29b; current owner 39</summary>

The following records preserve the previous detailed proposal. Their numeric
cross-references use the former-number [mapping](#roadmap-id-mapping), and their
former status/completion wording is subordinate to the current deferred scope
above. They neither reactivate work nor duplicate accepted 28x requirements.


[Milestones](#Milestones)

**Delivery posture:** Implement and qualify operator-lane scheduling over the 29a
service buckets, covering every listed operator/background surface. Baseline
and mixed-load responsiveness evidence are required; a prior latency incident
or failed smaller fix is not an activation prerequisite.

Checks: QEMU proves semantic stability, timeout/overrun behavior, and latency bounds; Pi 4 claims require fresh target logs, preserved MCS admission/reserve evidence, and separated serial, USB, TCP, Wi-Fi/GENET, HDMI, persistence, and flash proof lanes.
Deliverables: Repeatable validation for Cohesix multi-surface responsiveness.
```



</details>

## Milestone 40 — Read-Only Edge Status and Attestation Tool <a id="40"></a>
[Milestones](#Milestones)

**Status:** Deferred — explicit activation; not part of Release B.

**Preserved scope:** Retain the standalone read-only coh-status workflow, shared inspect/attest/snapshot core, offline replay and latency checks. Activate for a field workflow not met by current doctor/CLI/SwarmUI. Reuse existing verifiers; distinguish historical evidence, measurements and genuinely authenticated live attestation.

**Activation and completion:** Identify the concrete need, inspect retained
implementation/evidence, and specify only the unfulfilled tasks and exact
acceptance claims. Preserve the safety constraints below, but reconcile its
former-number references and superseded breadth before implementation. No
whole-catalogue or later prerequisite is inherited by 28x.

<details>
<summary>Retained design inventory — former Milestone 30; current owner 40</summary>

The following records preserve the previous detailed proposal. Their numeric
cross-references use the former-number [mapping](#roadmap-id-mapping), and their
former status/completion wording is subordinate to the current deferred scope
above. They neither reactivate work nor duplicate accepted 28x requirements.


[Milestones](#Milestones)

**Delivery posture:** Deliver the standalone `coh-status` binary and its complete
read-only field workflow. Reuse 27/27f snapshot, trace, status, and attestation
internals; no separate field-demand decision is required.

Deliverables:
  - Field-tech read-only transcript fixtures and explicit docs.

```


</details>

## Milestone 41 — Pi 4 Root-Shell Hardware Status <a id="41"></a>
<a id="30a"></a>
[Milestones](#Milestones)

**Status:** Deferred — explicit activation; not part of Release B.

**Preserved scope:** Retain the passive Pi-only hw-status command, HAL-mediated allowlisted firmware queries, shared field vocabulary and fresh serial/latency evidence. Activate for a real local diagnostic need. No firmware writes, driver ownership changes or implied Wi-Fi/USB/display acceptance.

**Activation and completion:** Identify the concrete need, inspect retained
implementation/evidence, and specify only the unfulfilled tasks and exact
acceptance claims. Preserve the safety constraints below, but reconcile its
former-number references and superseded breadth before implementation. No
whole-catalogue or later prerequisite is inherited by 28x.

<details>
<summary>Retained design inventory — former Milestone 30a; current owner 41</summary>

The following records preserve the previous detailed proposal. Their numeric
cross-references use the former-number [mapping](#roadmap-id-mapping), and their
former status/completion wording is subordinate to the current deferred scope
above. They neither reactivate work nor duplicate accepted 28x requirements.


[Milestones](#Milestones)

**Delivery posture:** Deliver `hw-status`, the shared read-only status projection,
and all listed Pi field diagnostics with exact target evidence. No separate
field-demand decision is required.

Checks: Transcript, latency, and normalizer coverage prove passive behavior, bounded command timing, stable field extraction, and no regression in existing Pi 4 gates.
Deliverables: Repeatable host and Pi 4 validation for `hw-status`.
```


</details>

## Milestone 42 — Additional AI-Native Namespace Projections <a id="42"></a>
<a id="30b"></a>
[Milestones](#Milestones)

**Status:** Deferred — explicit activation; not part of Release B.

**Preserved scope:** Retain bounded job/dataset/experiment/inference/metrics and admission read projections when their actual owner contracts exist. Activate for a namespace consumer that current APIs cannot adequately serve. Use existing role-scoped grammar and canonical host-ticket effects; no semantic store, inference data plane or new authority path enters the VM.

**Activation and completion:** Identify the concrete need, inspect retained
implementation/evidence, and specify only the unfulfilled tasks and exact
acceptance claims. Preserve the safety constraints below, but reconcile its
former-number references and superseded breadth before implementation. No
whole-catalogue or later prerequisite is inherited by 28x.

<details>
<summary>Retained design inventory — former Milestone 30b; current owner 42</summary>

The following records preserve the previous detailed proposal. Their numeric
cross-references use the former-number [mapping](#roadmap-id-mapping), and their
former status/completion wording is subordinate to the current deferred scope
above. They neither reactivate work nor duplicate accepted 28x requirements.


[Milestones](#Milestones)

**Delivery posture:** Implement the complete listed AI control namespace, host/UI
projections, and conformance after its owner contracts. No adoption study or
separate namespace-demand decision is required.

Deliverables:
  - Canonical AI namespace regression pack and test-plan coverage.
```


</details>

## Milestone 43 — AWS Native Deployment and ENA <a id="43"></a>
[Milestones](#Milestones)

**Status:** Deferred — explicit activation; not part of Release B.

**Preserved scope:** Retain EC2 Arm64 feasibility/boot-resource mapping, UEFI packaging, isolated ENA, bounded outbound bootstrap/TLS/HTTP/IMDSv2, 9door mounts, AMI tooling and target-qualified recovery/performance. Validate exact platform support and cost before dependent work. Cloud proof, deliberate TCB expansion and boot-media persistence remain separate from Pi/QEMU claims.

**Activation and completion:** Identify the concrete need, inspect retained
implementation/evidence, and specify only the unfulfilled tasks and exact
acceptance claims. Preserve the safety constraints below, but reconcile its
former-number references and superseded breadth before implementation. No
whole-catalogue or later prerequisite is inherited by 28x.

<details>
<summary>Retained design inventory — former Milestone 31; current owner 43</summary>

The following records preserve the previous detailed proposal. Their numeric
cross-references use the former-number [mapping](#roadmap-id-mapping), and their
former status/completion wording is subordinate to the current deferred scope
above. They neither reactivate work nor duplicate accepted 28x requirements.


[Milestones](#Milestones)

**Status:** Planned as one complete AWS milestone, including platform
bring-up, isolated ENA, outbound bootstrap, bounded TLS/HTTP/IMDSv2 support,
9door mount, AMI tooling, recovery, and performance qualification. No separate
funding, demand, or feasibility-only authorization limits its scope. Validate
the selected EC2 platform and boot/hardware handoff first as implementation
prerequisites; unsupported hardware remains a real blocker, not acceptance.

Deliverables:
- Reproducible AMI build pipeline.
```

</details>

---

## Docs-as-Built Alignment (applies to Milestone 8 onward)

To prevent drift:

