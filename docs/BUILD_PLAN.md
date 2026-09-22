# Cohesix Build Plan (ARM64, Pure Rust Userspace)

> **Current post-27g plan — 23 September 2026:**
> [28x and later](BUILD_PLAN_28_PLUS.md) is the canonical continuation of this
> build plan. It replaces the former prospective 28–31 bodies and their future
> mapping, catalogue gates and blanket dependencies; those retained sections are
> historical design proposals, not additive implementation requirements.
> [Current ownership mapping](BUILD_PLAN_28_PLUS.md#mapping) resolves old numbers
> and immutable task/schema identifiers. Milestones 0–27g, active qualification,
> narrow reopenings, implementation, evidence and exceptions remain unchanged.
> New 28x work is Planned; this documentation edit activates no runtime work.

This build plan records what Cohesix has implemented, what remains to be built,
and the conditions for completing each milestone. It defines scope, dependencies,
deliverables, acceptance criteria, and known limitations so users and contributors
can distinguish delivered capabilities from planned work.

| [27e](#27e) | Installation, Adoption and CI | Complete |
| [27f](#27f) | SwarmUI Community Showcase: Spectrum Workbench + Live AI Hive | Planned — next release |
| [27g](#27g) | Integrated Qualification and Next Release | In Progress — Release A qualification and two-hour Pi burn-in |
| [28](BUILD_PLAN_28_PLUS.md#28) | Shared Governed Jobs and Bounded Unattended Authority | Planned |
| [28a](BUILD_PLAN_28_PLUS.md#28a) | Useful CUDA Workloads and Reliable GPU Operations | Planned |
| [28b](BUILD_PLAN_28_PLUS.md#28b) | Deeper PEFT Lifecycle and Verified Serving | Planned |
| [28c](BUILD_PLAN_28_PLUS.md#28c) | macOS 27: Siri/App Intents, Apple AI and Metal-Backed Workflows | Planned |
| [28d](BUILD_PLAN_28_PLUS.md#28d) | MCP Access to Complete Selected Workflows | Planned |
| [28e](BUILD_PLAN_28_PLUS.md#28e) | A2A Delegation of Durable Selected Jobs | Planned |
| [28f](BUILD_PLAN_28_PLUS.md#28f) | NeMo Agent Toolkit Adoption Kit and Live Integration | Planned |
| [28g](BUILD_PLAN_28_PLUS.md#28g) | Installation, Integrated User Qualification and Release B | Planned |
| [29](BUILD_PLAN_28_PLUS.md#29) | Extended Formal Assurance and NIST Evidence | Deferred — explicit activation |
| [30](BUILD_PLAN_28_PLUS.md#30) | Full Production Bundle Binding and Quarantine Inventory | Deferred — explicit activation |
| [31](BUILD_PLAN_28_PLUS.md#31) | VM-Local Persistence and Reboot Recovery | Deferred — explicit activation |
| [32](BUILD_PLAN_28_PLUS.md#32) | Broader Enterprise, Industry and Protocol Integration | Deferred — explicit activation |
| [33](BUILD_PLAN_28_PLUS.md#33) | Semantic Objects and Context Capsules | Deferred — explicit activation |
| [34](BUILD_PLAN_28_PLUS.md#34) | Advanced Agent Orchestration and Context Optimisation | Deferred — explicit activation |
| [35](BUILD_PLAN_28_PLUS.md#35) | General Inference Gateway and Advanced Routing | Deferred — explicit activation |
| [36](BUILD_PLAN_28_PLUS.md#36) | Broader NeMo Training, Evaluation and Serving Families | Deferred — explicit activation |
| [37](BUILD_PLAN_28_PLUS.md#37) | Measured Target Scheduling and Responsiveness Improvements | Deferred — explicit activation |
| [38](BUILD_PLAN_28_PLUS.md#38) | Local Hardware Status and Additional Namespace Projections | Deferred — explicit activation |
| [39](BUILD_PLAN_28_PLUS.md#39) | AWS Deployment | Deferred — explicit activation |

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
LoRA Release, adoption/CI and the substantial native SwarmUI showcase. B/C remain
reserved for subsequent qualified deliveries; no version is silently reassigned.
Historical releases and evidence keep original names, IDs and proof classes.

### Prospective milestone and task mapping <a id="roadmap-id-mapping"></a>

Numbers below are current ownership. Historical text in completed 0–27a, audit
records, immutable releases, logs and generated identifiers uses its original
numbering. Consult this mapping; old links must not be interpreted by a reused
number alone. Current incoming links and active task bodies use the new owners.
Task suffixes are preserved under the new prefix unless this table names a split.
Existing implementation records, generated metadata and uncommitted as-built
notes retain their original task identifiers until their owner updates them; the
mapping resolves those references without changing schema or evidence bytes.

| Previous owner/task | Current owner and complete disposition |
| --- | --- |
| 0–27a | Unchanged completed scope, implementation, evidence and exceptions. |
| 27b provider-action registry, native/external executor, GPU/MIG and Jetson conformance | 27b selected executable foundation; broader native/Apple/K8s/MIG/DCGM/CUPTI extensions retain full tasks under `m28a-*`. |
| `m27b-authoritative-receipt-and-evidence-core`, read visibility and identity | `m27b-authoritative-receipt-and-evidence-core` and `m27b-scoped-authority-and-read-visibility`; 27b shared verifier and selected authority/read contract; broad enterprise/exporter integration under 28a. |
| `m27b-integration-surface-registry`, public mode gate and ecosystem matrix | 27b selected contract; 27e selected packaging/adoption; exhaustive catalogue extensions under `m28a-*`. |
| `m27b-live-reference-workflows` and ticket-centred lifecycle | 27c reusable CUDA recipe and diagnostic case; 27d LoRA recipe; all nine broader/domain playbooks under `m28a-live-reference-workflows`. |
| `m27b-packaging-deployment-profiles` | 27e selected packages/doctor/newcomer and CI workflows; additional deployment/platform shapes under `m28a-packaging-deployment-profiles`. |
| `m27b-use-case-release-gate`, provider/exporter performance | 27b focused conformance, 27g assembled release/overhead; broader use-case/exporter gates under `m28a-*`. |
| `m27b-federation-conformance`, observability-exporters, industry-sidecar-contracts | 28a with the same task suffixes; retain already working code and original evidence. |
| `m27b-operator-manuals-and-reference-maintenance` | Stable 27b task and implementation record retained; 27e owns new recipe/adoption help. |
| 27c semantic fabric and Context Capsules / `m27c-*` | 28b / `m28b-*`, complete scope plus semantic workbench inspector. |
| 27d basic run identity/checkpoints/recovery | 27c recipe foundation; advanced task graphs, agents, experiments, attention/KV, prefix/hotsets, frameworks and NeMo remain 28e. |
| 27d PEFT registry transaction and live reference | 27d native import/HF training, comparable evaluation and real serving release; broader profiles become `m28e-broader-peft-provider-transactions` and `m28e-broader-peft-reference-paths`. |
| Other 27d / `m27d-*` tasks | 28e / `m28e-*`; extend accepted 27c/27d without another executor or lifecycle. |
| 27e general inference / `m27e-*` | 28f / `m28f-*`, full API/governance/streaming/cache/receipt scope plus inference inspector. |
| 27f workbench/showcase | 27f full supported CUDA/PEFT experience; integrated release cut is 27g. |
| `m27f-semantic-inference-inspector` and formal/bundle inspectors | `m28b-semantic-workbench-inspector`, `m28f-inference-workbench-inspector`, `m28c-admission-workbench-inspector`, `m28d-production-binding-workbench-inspector`; backend-owned acceptance replaces release placeholders. |
| 28 verification/NIST / `m28-*` | 28, unchanged complete assurance scope. |
| 28a general admission / `m28a-*` | 28c / `m28c-*`. |
| 28b production Worker binding/fault lifecycle / `m28b-*` | 28d / `m28d-*`; stable generated `m28b_production_bundle` label is not renamed here. |
| 28c MCP/A2A / `m28c-*` | 28g / `m28g-*`; the [detailed contract](#28g) is preserved at a pinned Git revision. |
| 29, 29a, 29b, 30, 30a, 30b, 31 | Same IDs: edge persistence, service-turn scheduling, operator scheduling, field status, hardware diagnostics, AI namespace projections and AWS; prospective references follow new owners. |

The following task IDs are stable keys referenced by the existing compiler
input and generated graph. Their full task bodies move to the owners below;
there are no duplicate alias tasks. Their numeric prefix is historical, while
the explicit `Milestone:` field and section define current ownership. This
keeps manifest/generated bytes unchanged during the planning-only migration.

| Stable task ID | Current milestone |
| --- | --- |
| `m28c-a2a-policy-ir` | [28g](#28g) |
| `m28c-mcp-policy-ir` | [28g](#28g) |
| `m27e-inference-ir-and-compatibility-contract` | [28f](#28f) |
| `m27e-inference-receipts-otel-and-evidence` | [28f](#28f) |
| `m27d-live-peft-reference-paths` | [27d](#27d) |
| `m27d-framework-adapters` | [28e](#28e) |
| `m27d-nemo-provider-family` | [28e](#28e) |
| `m28b-production-worker-ticket-driver-inventory` | [28d](#28d) |
| `m27c-semantic-ir-and-object-contract` | [28b](#28b) |

Deferred implementation order is **28 assurance -> 28a broader providers ->
28b semantic fabric -> 28c general admission -> 28d production binding ->
28e advanced agents/NeMo -> 28f general inference -> 28g MCP/A2A -> 29
persistence -> 29a service turns -> 29b operator scheduling -> 30 field status
-> 30a hardware diagnostics -> 30b AI namespaces -> 31 AWS**. Later consumers
supply their own new target/provider evidence; extension points or conditional
future claims do not gate the earlier owner's acceptance.

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
## Milestone 28 — Formal Verification Baseline + Proof-Carrying Manifests <a id="28"></a>
[Milestones](#Milestones)

**Status:** Planned — after the 27g release; preserve existing implementation and evidence.

**Delivery posture:** Implement all listed assurance deliverables: claim register,
generated witnesses, Secure9P/HAL/resource checks, TLA+/PlusCal models, bounded
checking, restricted policy IR, CI verification, and NIST mapping. Each claim
retains its exact proof class and evidence; a subset cannot close the milestone.

Deliverables:
  - Checked policy-IR and proof-vocabulary foundation consumed by Milestone 28c per-intent admission.
```


## Milestone 28a — Broader Providers, Federation, Deployment and Domain Workflows <a id="28a"></a>
[Milestones](#Milestones)

**Status:** Planned — deferred breadth; existing implementation remains preserved.
The retained launchd observation defect is narrowly Reopened under
`m28a-native-provider-discovery-and-actions` for M27g restoration only.

**Prerequisites:** completed 27g and 28. This milestone extends the qualified
27b foundation, 27c recipes, 27d LoRA transaction and 27e packaging. The complete
catalogue below belongs here, including Kubernetes/controllers/CEL/DRA,
launchd/Xcode/codesign/notary/App Store Connect, Apple ML, MIG/DCGM/CUPTI,
- Semantic extraction, 28c intent admission, AI run control, inference interoperability, and MCP/A2A interop can build on proven provider and integration schemas instead of inventing action catalogs or silently treating mock workflows as live.

## Milestone 28b — Persistent Semantic Object Fabric + Context Capsules (Host-Side) <a id="28b"></a>
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
## Milestone 28c — Machine-Checked Intent Admission + Decision-Bound Authority <a id="28c"></a>
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
## Milestone 28d — Production Worker Ticket/Lease Binding + Driver Inventory Projection + Structured Fault Lifecycle <a id="28d"></a>
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
## Milestone 28e — Advanced Agent Orchestration, Experiments, Attention/KV and NeMo <a id="28e"></a>
[Milestones](#Milestones)

**Status:** Planned — after the 27g release; preserve existing implementation and evidence.

**Delivery posture:** Implement the complete AI/PEFT substrate: delegated and
governed runs, durable context, task graphs/handoffs, inference strategies,
prefix/hotset reuse, all listed framework/NeMo adapters, and provider parity.
- Milestone 28f can expose a provider-neutral, auditable inference boundary over
  the accepted run, capsule, ticket, and receipt semantics.
- Milestone 30b can expose stable AI namespace roots based on proven host-side semantics rather than speculation.

## Milestone 28f — Inference Interoperability + Auditable Receipts (OpenAI-Compatible Host Boundary) <a id="28f"></a>
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
## Milestone 28g — MCP/A2A Gateway Coverage + Governed Autonomous Workflows <a id="28g"></a>
[Milestones](#Milestones)

**Status:** Planned — after the 27g release; preserve existing implementation and evidence.

**Delivery posture:** Implement and independently qualify both MCP and A2A across
the selected profile's complete admitted gateway operations and use cases,
including consequential unattended execution, observation, recovery, and
Manifest-disabled protocols expose nothing. MCP-only and A2A-only deployments
are independently usable and qualified. VM grammar, Secure9P semantics, and
generated manifest authority remain unchanged.

## Milestone 29 — Bounded VM-Local Persistence: Spool Stores + Settings <a id="29"></a>
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

## Milestone 29a — Core-Local Service-Turn Scheduling (SMP Hot-Path Optimization) <a id="29a"></a>
[Milestones](#Milestones)

**Delivery posture:** Implement and qualify core-local service-turn scheduling on
QEMU and Pi. Establish the same-harness baseline and demonstrate bounded
locality, contention, throughput, and tail latency; an existing SLO miss is not
Checks: Host pressure stays semantic-only; QEMU proves regression stability; Pi throughput claims require fresh target logs and separated acceptance lanes; same-harness REST artifacts compare service-bucket results to the 26d rolling baseline with service-bucket counters attached.
Deliverables: Repeatable validation and benchmark lanes for core-local SMP optimization.
```


## Milestone 29b — Operator-Lane Scheduler + Multi-Surface Responsiveness <a id="29b"></a>
[Milestones](#Milestones)

**Delivery posture:** Implement and qualify operator-lane scheduling over the 29a
service buckets, covering every listed operator/background surface. Baseline
and mixed-load responsiveness evidence are required; a prior latency incident
or failed smaller fix is not an activation prerequisite.

Checks: QEMU proves semantic stability, timeout/overrun behavior, and latency bounds; Pi 4 claims require fresh target logs, preserved MCS admission/reserve evidence, and separated serial, USB, TCP, Wi-Fi/GENET, HDMI, persistence, and flash proof lanes.
Deliverables: Repeatable validation for Cohesix multi-surface responsiveness.
```


## Milestone 30 — Edge Local Status (Pi 4 Host Tool)  <a id="30"></a>
[Milestones](#Milestones)

**Delivery posture:** Deliver the standalone `coh-status` binary and its complete
read-only field workflow. Reuse 27/27f snapshot, trace, status, and attestation
internals; no separate field-demand decision is required.

Deliverables:
  - Field-tech read-only transcript fixtures and explicit docs.

```

## Milestone 30a — Pi 4 Root-Shell Hardware Status (`hw-status`)  <a id="30a"></a>
[Milestones](#Milestones)

**Delivery posture:** Deliver `hw-status`, the shared read-only status projection,
and all listed Pi field diagnostics with exact target evidence. No separate
field-demand decision is required.

Checks: Transcript, latency, and normalizer coverage prove passive behavior, bounded command timing, stable field extraction, and no regression in existing Pi 4 gates.
Deliverables: Repeatable host and Pi 4 validation for `hw-status`.
```

## Milestone 30b — AI-Native Namespace Surfaces (Control-Plane Only)  <a id="30b"></a>
[Milestones](#Milestones)

**Delivery posture:** Implement the complete listed AI control namespace, host/UI
projections, and conformance after its owner contracts. No adoption study or
separate namespace-demand decision is required.

Deliverables:
  - Canonical AI namespace regression pack and test-plan coverage.
```

## Milestone 31 — AWS AMI (UEFI → Cohesix, ENA, Diskless 9door)  <a id="31"></a>
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

---

## Docs-as-Built Alignment (applies to Milestone 8 onward)

To prevent drift:

