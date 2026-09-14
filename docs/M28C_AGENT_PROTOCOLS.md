<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Specify M28c protocol controls, agent usage contracts, implementation requirements, and acceptance gates. -->
<!-- Author: Lukas Bower -->

# M28c — Agent Protocol Contracts and Acceptance

This is the detailed future-work contract incorporated by
[Milestone 28c](BUILD_PLAN.md#28c). BUILD_PLAN retains milestone status, scope,
dependencies, task IDs, and completion authority. This document cannot activate
implementation work, waive repository invariants, or claim as-built support.
Its requirements must remain aligned with that milestone and its owning
architecture, interface, security, and testing contracts.

**Shared contracts**

### Manifest enablement and global disable

The following are planned compiler-owned settings, not current manifest syntax:

```toml
[gateway.agent_protocols]
enabled = false

[gateway.mcp]
enabled = false

[gateway.a2a]
enabled = false
```

- Effective MCP enablement is `gateway.agent_protocols.enabled &&
  gateway.mcp.enabled`; A2A uses the same master switch and its own flag.
  Missing settings resolve to false. Master false dominates retained child
  settings; both false, MCP-only, A2A-only, and both enabled are supported.
- CLI/environment/config overrides may narrow this generated ceiling only.
  Direct stdio launch, remote requests, mount options, discovery, negotiation,
  and agent tools cannot enable a manifest-disabled protocol. An invalid or
  mismatched selected generated policy fails closed before protocol startup.
- A disabled protocol registers no handlers, protocol-only listeners, public
  or extended Agent Cards, catalog endpoints, streams, subscriptions, or push
  callbacks and starts no protocol background work. Its credentials are not
  resolved solely for that protocol. A dedicated disabled stdio invocation
  exits with a bounded diagnostic on stderr before accepting protocol input.
  Existing REST routes and independent host tools retain their own enablement.
- Record effective flags and manifest fingerprint in bounded gateway/doctor
  diagnostics. Reject attempts to override disabled settings deterministically;
  HTTP requests to absent routes cannot activate an adapter or alternate path.
- Apply a changed manifest through the supported configuration/restart lifecycle.
  Do not claim hot reload unless implemented and tested. On disable transition,
  stop new protocol admission and terminate protocol streams/callback workers.
  Already accepted tickets retain the underlying executor's durable lifecycle;
  disabling a transport neither cancels nor replays their side effects. Use
  separately authorized cancellation or grant revocation and existing status
  surfaces to manage that work. A2A-only operation never depends on MCP being on.
- Bump the manifest schema when these fields are implemented, regenerate all
  selected profiles and consumers, and validate schema even for disabled
  configuration. Dependency readiness checks apply to effective enablement;
  disabling both protocols must not require installing their optional services.

### Complete function and use-case coverage

- Extend the 27b registry projection to inventory every supported gateway
  operation: metadata and bounds, namespace reads and admitted append/batch
  writes, Queen/Worker lifecycle and scheduling controls, GPU leases, AI/PEFT
  lifecycle, provider actions, policy/approval and administrative operations,
  cancellation/recovery, and audit/evidence/replay wherever already supported.
  Each entry retains its owning contract and selected-profile maturity.
  Future or disabled functions are recorded as such without inventing support.
- Every admitted entry has an MCP tool/resource mapping and an A2A
  operation/skill mapping when that protocol is enabled. Many operations may
  compose into a typed A2A skill; semantic parity does not require one endpoint
  per tool. Schema/docs endpoints may map to native metadata or linked
  resources. The ledger records any native protocol limitation, an equivalent
  in-protocol composition where possible, and the owning rationale/evidence.
  Unmapped admitted functionality blocks coverage acceptance. Exclusions cannot
  be justified solely by a demo catalog, a human-only preference, or write risk.
- Generic append, batch, and host-ticket tools validate the exact underlying
  action and target against the same generated schema, policy, and admission
  as named tools. No raw-write wrapper bypasses a missing action policy.
- Keep full implementation coverage separate from caller-visible discovery.
  Apply 27b visibility rules before constructing payloads; tools and skills are
  filtered by current identity, grants, deployment readiness, and supported
  protocol capabilities. Safe unavailable/authorization-needed explanations
  must not reveal another subject's resources or privileged policy details.
- Registry coverage includes end-to-end use cases, not only action names:
  discover/inspect, acquire capacity, execute/observe, evaluate/canary/promote,
  cancel/resume/rollback, bounded service remediation, and evidence/replay.
  Include every supported use-case row and its applicable stages; do not
  require optional inference or semantic dependencies for unrelated controls.

### Standing authorization and per-action admission

- Reuse 27a delegated identity, 27b action policy, 27d durable run state, and
  28a admission for an explicit standing authorization bound to a subject,
  workflow/run, action set, targets, policy version, expiry, and revocation
  state. Bounds include resource/cost ceilings where applicable, cumulative
  operations, concurrency, retry/cooldown limits, and delegation depth.
  Subdelegation can only attenuate the original authority. Credentials travel
  through authenticated transport/configuration, never model-visible arguments.
- The shared authority/run owners enforce budgets across MCP, A2A, REST, child
  agents, reconnects, and process recovery; protocol-local counters cannot
  multiply a run's budget. Refuse autonomy claims where durable accounting,
  fact freshness, revocation, or executor enforcement is unavailable.
  Every mutating transport serving that workflow, including generic writes,
  must bind the same run identity; omitting it cannot recover an unmetered path.
- Standing permission is a ceiling, not a reusable 28a decision. Each concrete
  side effect requires a fresh applicable decision and state-bound grant,
  exact idempotency identity, fencing, and a receipt. Revalidate state and
  authority on resume, delayed dispatch, escalation, and subdelegation.
- Existing one-shot approval requirements remain in force until their owning
  policy explicitly supports bounded standing authorization. The selected
  policy determines automatic admission, required human approval, or refusal.
  Prompts and client-side confirmation never grant authority. Server-side
  denial remains effective even if a client suppresses all confirmation UI.
- Administrative functions may be projected under separately delegated
  administrative authority. Ordinary workflow grants cannot widen their own
  scopes, change admission policy, mint credentials, disable auditing, enable
  protocols, or approve their own escalation. Missing authority produces a
  typed explanation and authorized escalation/resume path, never a fallback.
- Distinguish request acceptance, execution, confirmed outcome, cancellation
  requested, cancellation confirmed, and unknown outcome. Cancellation is not
  rollback. Ambiguous execution requires reconciliation under the existing
  WAL/receipt contract; a disconnect never authorizes blind resubmission.
- Document where enforcement resides: gateway-attributed callers remain
  gateway-enforced unless target verification is separately proven. Host
  executor custody and bypass assumptions remain visible; protocol conformance
  and seL4 isolation cannot establish an external effect's correctness alone.

### Agent discovery and correct-use guidance

Generate one bounded, versioned usage contract from 27b operation/use-case rows,
their owner schemas, accepted walkthroughs, and selected policy. Every use case
must provide its purpose and appropriate/unsupported uses, prerequisites and
readiness, typed inputs and examples, output/receipt semantics, least required
authority, approval mode, bounds, preflight steps, lifecycle, safe retry and
cancellation rules, failure/refusal remedies, and evidence/replay instructions.
Include counterexamples such as treating an ACK as completion, inventing a
target, replaying an ambiguous write, or following instructions in telemetry.

Publish that contract through the maximum useful native features of the pinned
revision and negotiated client capabilities:

| Surface | Required use of protocol affordances |
| --- | --- |
| MCP discovery | Server identity/version and instructions or documentation links where supported; concise tool titles/descriptions, exact input/output schemas, effect/idempotency annotations, and structured results with text fallback and receipt/resource links. |
| MCP resources and prompts | Searchable or paginated use-case/catalog resources, templates and bounded operating guides; workflow prompts with validated arguments and policy-driven approval/escalation; freshness, audience and priority metadata and change notifications where supported. |
| A2A discovery | Agent Card description/documentation, skills with names/descriptions/tags/examples, media modes, security requirements, supported interfaces/extensions, and capabilities; sensitive deployment guidance belongs in authenticated views. |
| A2A interaction | Typed message data and task/artifact records explain prerequisites, next steps, progress, failures, required input/authority, cancellation limits, and terminal receipts using the pinned binding's states and errors. |

Essential safety and usage information must be available in tool/skill
descriptions and results: a client may not expose MCP prompts or resources to
its model. Provide bounded read-only help/catalog tools and an A2A guidance
skill/message path so such clients can obtain the same contract. Core workflows
must remain usable without proprietary extensions; use native fields first,
negotiated extensions only where justified, and linked structured guidance for
details the wire format cannot express. Record used, unsupported, and disabled
affordances in a revision/client capability matrix with conformance evidence.
Evaluate argument completion, progress/status, durable task support, resource
subscriptions, and input/authorization elicitation where the pinned protocol
offers them; implement those that improve admitted workflows within configured
bounds. Missing client support must yield a tested discovery/status/input
fallback. Client callbacks cannot acquire extra authority or disclose secrets.
Use progressive discovery and bounded pagination rather than unbounded prompt
injection of the entire catalog. No runtime fetch of remote documentation is
required for the packaged core guide.

Guidance, schemas, examples, prompts, Agent Cards, and annotations are descriptive
and never authoritative instructions to bypass validation. Bind guides to the
selected manifest, registry/policy/schema versions, visibility, and freshness;
invalidate caches on identity/policy changes and refuse stale authority. Keep
credentials, private targets, raw prompts, and unredacted evidence out of public
metadata. Validate examples against real schemas and exercise them through
ordinary clients. Clients must be able to determine the correct sequence and
handle refusal using only published protocol guidance; this is a tested
usability contract, not a claim that every model will follow instructions.

Protocol references for implementation-time revision selection:
[MCP tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools),
[MCP resources](https://modelcontextprotocol.io/specification/2026-07-28/server/resources),
[MCP prompts](https://modelcontextprotocol.io/specification/2025-11-25/server/prompts),
and [A2A specification](https://a2a-protocol.org/latest/specification/).
These references describe available affordances across revisions; they do not
select a mixed wire contract. Pin and validate one complete revision per
protocol, recording supported features and client limitations before acceptance.

**Non-Goals (Explicit)**
- No in-VM MCP endpoint, MCP listener, MCP filesystem root, or MCP-specific root-task parser.
- No new console verbs, no new 9P verbs, no ACK/ERR/END grammar changes, and no hidden RPC behind MCP tool names.
- No direct execution of `systemctl`, `docker`, `kubectl`, CUDA/NVML, PEFT, or NeMo provider APIs from the MCP server. Side effects go through delegated REST and/or `/host/tickets/spec`.
- No OpenAI-compatible endpoint, inference proxy, provider call, or model-output
  tool execution inside MCP/A2A. Inference submissions map to accepted 27d
  `infer.run` admission and the 27e gateway; model-produced tool calls remain
  inert data until separately validated and authorized.
- No duplicate semantic graph, vector index, capsule planner, prompt archive, or
  inference receipt schema in `hive-gateway`; 28c consumes accepted 27c and
  27e libraries and immutable refs.
- No MCP tool that bypasses role-scoped tickets, policy approval, writer-epoch fencing, host-ticket allowlists, or evidence exports.
- No MCP/A2A-local policy evaluator, fact-authority classifier, grant minter, or
  admission cache transferable across intents. Protocol metadata and model
  text remain untrusted intent inputs.
- No model-controlled prompt or MCP client metadata is trusted as authorization. Tool descriptions, prompts, and annotations are documentation only.
- No CUDA/NVML, PEFT, NeMo, Kubernetes, systemd, or Docker code enters the VM TCB.
- No implicit translation from arbitrary FUSE writes into MCP `tools/call`. Write-capable Cohesix mounts continue to use existing console/REST `ECHO` semantics and the existing append-only control files.
- No in-VM A2A endpoint, no A2A-specific root-task queue, no A2A peer mesh, no opaque inter-agent mailbox, and no direct A2A-to-provider execution path.
- No A2A push notification callback is accepted without SSRF-safe URL validation, explicit allowlist policy, per-task auth material, bounded retry policy, and audit evidence.

**Deliverables**

### 1) MCP protocol endpoint and lifecycle in `hive-gateway`
**Purpose:** Let standard MCP hosts connect to Cohesix without client-specific shims while keeping the gateway's loopback/auth defaults.

Implementation requirements:
- Apply the shared manifest enablement contract before creating any transport,
  resolving MCP credentials, or advertising discovery. Runtime flags can only
  select or restrict manifest-enabled transports and endpoints.
- Add an MCP server mode to `apps/hive-gateway` with:
  - stdio transport for local MCP hosts that launch the gateway as a subprocess,
  - Streamable HTTP endpoint for remote-capable MCP clients, sharing the gateway's loopback-only default and non-loopback risk override,
  - protocol revision, JSON Schema dialect, authorization mode, and capability negotiation pinned in `docs/HOST_API.md` and generated gateway metadata,
  - exact revision-specific version/request metadata, session handling where
    supported, explicit termination behavior, and deterministic unsupported-version errors,
  - `tools`, `resources`, and `prompts` capabilities with paginated discovery where needed,
  - optional list-change notifications only when the implementation has deterministic change detection.
- Remote MCP transport must validate `Origin`, require gateway request auth, and require delegated tickets for mutating tools. A production non-loopback profile must implement the authorization contract of the pinned MCP revision or explicitly document and test a narrower compatibility mode; a preconfigured loopback bearer token alone is not generic remote MCP authorization conformance.
- Stdio mode must read credentials only from environment/config, never from prompts or tool arguments.
- MCP stdout/stdin must carry only valid MCP JSON-RPC messages; logs go to stderr or the existing gateway log path.
- Streamable HTTP mode must support the accepted request/response content types, bounded SSE streams when enabled, explicit cancellation handling, and no broadcast of one client's server messages to another client.
- The gateway must expose enough server metadata for common MCP clients and inspectors to identify the server, protocol revision, tool names, resource URI scheme, and auth requirements.
- Populate the native discovery and guidance fields required by the shared
  agent-usage contract. A negotiated feature limitation must yield the documented
  help/result fallback without weakening authorization or requiring a client fork.

As-built leverage:
- Reuse `hive-gateway` broker queues, request-auth checks, loopback binding policy, OpenAPI bounds, and existing `cohsh` REST transport code.

---

### 2) Resource catalog over existing Cohesix paths and accepted host artifacts
**Purpose:** Give MCP clients context without giving them a new authority or
storage model.

Implementation requirements:
- Expose bounded `cohesix://guide/*` usage-contract and coverage resources from
  generated 27b records and accepted operator guidance, with schema/manifest
  fingerprints and visibility checks. These are host catalog artifacts and do
  not require a semantic store or imply a VM namespace.
- Define namespace-backed `cohesix://namespace/*` resource URIs that map
  one-to-one to existing bounded reads:
  - `/proc/boot`, `/proc/root/*`, `/proc/9p/*`, `/proc/lease/*`, `/proc/schedule/*`, `/proc/spool/*`, `/proc/attest/*`
  - `/gpu/*`, `/gpu/models/*`, `/gpu/telemetry/schema.json`
  - `/host/tickets/status`, `/host/tickets/deadletter`, and provider status under `/host/systemd/*`, `/host/docker/*`, and `/host/k8s/*`
  - evidence-pack and timeline summaries when Milestone
    27/27a/27c/27d/27e evidence is available
  - NeMo capability, guardrail, evaluator, and provider receipt summaries only when the 27d optional provider family is enabled.
- Define host-artifact resource families only when their owner milestones are
  accepted:
  - `cohesix://semantic/snapshot/<snapshot-id>/object/<object-id>/<view>` and
    bounded edge/query summaries backed by the 27c read-only semantic core,
  - `cohesix://context/capsule/<capsule-id>` and its manifest/render receipt
    backed by the 27c capsule verifier/renderer,
  - `cohesix://inference/receipt/<receipt-id>` and bounded status/timeline views
    backed by the 27e receipt verifier and evidence projection.
- Namespace-backed resource reads must use only `LS`, `CAT`, or `TAIL` through
  the existing gateway/session machinery and must enforce manifest-derived
  path, line, byte, and walk-depth bounds. Host-artifact resources must use
  only the accepted read-only 27c/27e libraries over immutable ids and must
  not acquire repository paths, provider credentials, run admission, or write
  authority.
- Resource templates may expose common path families, but template expansion must reject `..`, absolute host filesystem paths, overlong components, and undeclared provider roots.
- Resource contents must be redacted with the same rules as evidence packs: no raw tickets, auth tokens, provider credentials, or secret refs.
- Resource metadata must expose source milestone, schema version, artifact hash,
  visibility class, provenance/receipt ref, and freshness without claiming
  host-artifact data is a kernel-visible 9P object.

As-built leverage:
- Reuse `coh evidence pack`, `coh evidence timeline`, generated Cohesix path
  defaults, the existing `/host` provider status surfaces, and the accepted
  `cohesix-semantic-core` and `cohesix-inference-core` read/verify APIs.

---

### 3) Tool catalog for real Cohesix operations
**Purpose:** Support useful MCP automation while preserving the Cohesix write path.

Implementation requirements:
- The families below illustrate the catalog; the shared coverage ledger defines
  completeness. Include every admitted gateway control, administrative action,
  batch operation, lifecycle/recovery operation, and metadata/evidence surface
  under its exact authority. Include read-only help/use-case discovery for
  clients that cannot consume resources or prompts.
- Read-only tools:
  - `cohesix.fs.ls`, `cohesix.fs.cat`, `cohesix.fs.tail`
  - `cohesix.semantic.inspect` and `cohesix.semantic.query` over an admitted
    27c snapshot id,
  - `cohesix.context.inspect`, `cohesix.context.render`, and
    `cohesix.context.verify` over an admitted immutable capsule id,
  - `cohesix.inference.status` and `cohesix.inference.receipt` over admitted
    27e ids,
  - `cohesix.cuda.inventory` for bounded host CUDA/NVIDIA capability and GPU inventory summaries
  - `cohesix.evidence.timeline` for bounded evidence/timeline summaries.
- Mutating or side-effect-capable tools must produce existing Cohesix writes only:
  - `cohesix.host_ticket.submit` appends the generated action-selected
    `host-ticket/v1` or `host-ticket/v2` record to `/host/tickets/spec`.
    `receipt_mode=none` uses v1; a receipt-bearing GPU/PEFT action uses v2 and
    must carry the exact admitted Worker binding fields required by 26e. A
    caller cannot choose a weaker schema than the provider/action registry
    requires.
  - `cohesix.gpu.lease_grant`, `cohesix.gpu.lease_renew`, and `cohesix.gpu.lease_release` map to existing GPU lease actions.
  - `cohesix.peft.export`, `cohesix.peft.import`, `cohesix.peft.activate`, and `cohesix.peft.rollback` map to existing PEFT ticket/action flows and 27d transaction receipts.
  - `cohesix.inference.submit` maps a fixed, generated request schema and
    optional accepted Context Capsule ref through 28a admission to the
    existing 27d `infer.run` host-ticket action and 27e inference gateway. It
    never calls a provider directly or executes model-produced tool calls.
  - `cohesix.nemo.probe`, `cohesix.nemo.infer`, `cohesix.nemo.guardrails`, and `cohesix.nemo.evaluate` map to 27d optional provider actions or deterministically return unavailable when NeMo is not enabled.
  - `cohesix.k8s.cordon`, `cohesix.k8s.drain`, and `cohesix.k8s.lease_sync` map to existing K8s host-ticket actions.
  - `cohesix.systemd.status_check`, `cohesix.systemd.start`, `cohesix.systemd.stop`, and `cohesix.systemd.restart` map to existing systemd host-ticket actions.
  - `cohesix.docker.status_check`, `cohesix.docker.stop`, and `cohesix.docker.restart` map to existing Docker host-ticket actions.
- MCP tool schemas and A2A skill schemas must derive from the shared manifest/provider action and integration-surface registry. Provider action names, target selectors, dry-run flags, idempotency keys, receipt fields, Worker tier, external-executor requirement, and availability state must not be hand-maintained separately for the two protocols.
- Every mutating schema additionally derives its exact 28a intent schema,
  required facts, policy id, grant ceiling, freshness/recheck mode, and decision
  receipt. The gateway maps the call/task into that typed intent, consumes the
  accepted `admission_id`, then submits only the existing host-ticket or control
  action.
- Discovery omits or marks typed unavailable any operation whose
  27b/27c/28a/27d/27e dependency row or admission policy is not accepted in the selected profile. A
  client cannot select `live` mode to override missing semantic, capsule,
  provider, executor, inference, or receipt evidence.
- Every tool schema must be generated or checked against manifest/provider policy:
  - bounded string lengths,
  - explicit enum values for actions and providers,
  - no free-form shell command field,
  - id/idempotency-key/writer-epoch requirements for mutating calls,
  - target path validation using existing Cohesix path rules.
- Tool results must return structured MCP output plus a text fallback containing the Cohesix receipt id, ticket id, action, target, state path, and evidence refs. They must not expose raw tickets or provider credentials.
- Describe preconditions, required scope and approval mode, consequential
  effects, idempotency limits, completion evidence, and safe follow-up actions
  in each generated tool contract. Return typed refusal/remediation information
  without suggesting unsafe retries or escalation outside the caller's scope.

As-built leverage:
- Reuse `host-ticket-agent` executors, `coh peft`, `host-cuda`, generated policy defaults, delegated REST identity, writer-epoch fencing, and evidence/timeline redaction.

---

### 4) Prompt templates for governed operator and autonomous workflows
**Purpose:** Make the shared usage contract actionable for MCP clients.

Implementation requirements:
- Add prompt templates that assemble existing tools/resources for common Cohesix tasks:
  - semantic impact review from an immutable snapshot and Context Capsule,
  - inference receipt and context-selection audit without raw prompt retention,
  - CUDA capacity triage before a GPU lease,
  - PEFT export/import/promotion/rollback review,
  - NeMo provider readiness and guardrail/evaluator receipt review,
  - K8s cordon/drain with lease and evidence checks,
  - systemd service recovery with Docker workload status,
  - Docker remediation with post-action evidence collection.
- Prompts name exact Cohesix actions, prerequisites, expected receipts, and
  recovery paths. They support unattended execution inside an existing standing
  authorization and request human approval only when the underlying selected
  policy requires it or additional authority is needed. A prompt cannot replace
  a missing approval, 28a decision, or server-side authorization check.
- Cover all admitted use-case rows, including operational administration,
  cancellation/resume, rollback, and evidence collection where supported;
  maintain examples and counterexamples from the same generated usage contract.
- Prompt text must not embed secrets, tickets, endpoint auth, or unbounded host paths.
- Prompt outputs are guidance only; only existing Cohesix tickets, receipts, and evidence determine state.

As-built leverage:
- Reuse Milestone 27 operator utilities, 27a audit/replay/fencing, 27c
  semantic/capsule artifacts, 27d run envelopes/checkpoints, 27e inference
  receipts, and existing host-ticket provider receipts.

---

### 5) A2A Agent Card and task facade over existing runs
**Purpose:** Let external A2A peers delegate bounded Cohesix operational tasks and observe status/artifacts without making A2A a coordination plane.

Implementation requirements:
- Add an A2A-compatible HTTP facade in `hive-gateway` behind the shared manifest
  master/per-protocol controls, existing request auth, loopback default,
  non-loopback exposure override, rate limits, and broker backpressure.
- Record the accepted A2A revision and its exact version-metadata location,
  endpoint paths, binding, media types, extension policy, unsupported-version
  errors, and streaming/push support in `docs/HOST_API.md` and generated metadata.
- Pin one accepted binding/revision mapping in generated policy. JSON-RPC, HTTP+JSON, and gRPC method or endpoint names must not be mixed across protocol revisions, and fixtures must be regenerated when that mapping changes.
- Publish an Agent Card from `/.well-known/agent-card.json` when A2A is enabled; alternate generated paths may exist only as additional configured aliases. The card must advertise only enabled Cohesix skills, authentication requirements, endpoint interfaces, and safe capability summaries; it must not expose raw tickets, secrets, host paths, or executor internals.
- Provide the authenticated extended Agent Card endpoint only when policy enables it, and ensure the extended card obeys stricter access checks than the public discovery card.
- Generate Card/skill descriptions, tags, worked use-case examples, media modes,
  security requirements, documentation links, and optional capability claims
  from the shared usage contract. Provide authenticated guidance through A2A
  itself, including when MCP is disabled; never require an MCP-only resource
  URI to learn a required input, authorization condition, or recovery step.
- A2A skills map to the same real-world operational families as MCP tools:
  semantic/capsule inspection, inference submission/status/receipt, CUDA/GPU
  inventory and leases, PEFT export/import/activate/rollback, optional NeMo
  probe/infer/guardrail/evaluator actions, K8s cordon/drain/lease sync, systemd
  status/start/stop/restart, Docker status/stop/restart, and evidence/timeline
  inspection.
- Extend those families to the entire admitted gateway coverage ledger,
  including control/administrative and recovery functions. A skill can compose
  multiple existing actions, but each action retains its own admission and
  receipt. No free-form peer request can widen a typed skill's authority.
- A2A `SendMessage` and `SendStreamingMessage` operations (or the exact
  generated equivalents for the pinned binding/revision) create or resume 27d
  run/task envelopes only after fixed skill/action/input schema validation.
  Semantic inputs are immutable 27c snapshot/capsule refs; inference actions
  map through 28a admission to the existing 27d action and 27e gateway. Free-form natural
  language is never translated directly into host or provider execution.
- A2A `GetTask`, `ListTasks`, `CancelTask`, `SubscribeToTask`,
  push-notification configuration, and streaming update operations (or their
  generated binding equivalents) are projections of existing
  semantic/capsule refs, run/checkpoint/evidence records, 27e inference
  receipts, host-ticket receipt state, and gateway audit state. Cancellation
  may append a validated Cohesix cancel/control request when one exists; it
  must not kill provider executors directly.
- A2A artifacts are bounded, redacted references to immutable semantic objects,
  Context Capsules, inference receipts, evidence packs, timelines, checkpoint
  summaries, provider receipts, and MCP/Cohesix resource refs. Large files,
  raw prompts/outputs, secrets, raw ticket material, and provider credentials
  are never embedded in artifacts.
- A2A push notification configs are disabled by default. If enabled, they require SSRF-safe URL validation, generated allowlists, per-task auth material, bounded retry/backoff, signed or authenticated delivery where configured, and audit evidence for every callback attempt.
- Map missing input and additional authorization to the selected binding's
  native states/errors with a bounded explanation and safe continuation.
  Credentials use the declared secure authorization channel. A task state,
  peer message, or approval text does not itself satisfy an authorization gate.
  Reconnect, resume, delegation, and cancellation preserve original identity,
  shared budgets, exact action correlation, and observed outcome semantics.

As-built leverage:
- Reuse Milestone 27c semantic/capsule artifacts, 27d run envelopes,
  checkpoints and provider receipts, 27e inference receipts,
  `host-ticket-agent` state, gateway request auth, and delegated REST identity.

---

### 6) Security, audit, and confused-deputy controls
**Purpose:** Keep model-controlled MCP and A2A calls inside Cohesix's existing capability discipline.

Implementation requirements:
- Mutating MCP tools require:
  - gateway request auth,
  - delegated capability ticket with matching path/action scope,
  - id/idempotency_key,
  - writer_epoch when the target profile enables fencing,
  - policy approval where the underlying Cohesix path already requires it.
- A2A task-creating or task-mutating calls require the same gateway request auth, delegated scope, id/idempotency key, writer epoch, and policy approval as the underlying Cohesix ticket/control action.
- Gateway audit lines must record protocol (`mcp` or `a2a`), method,
  tool/resource/prompt/skill/task name, semantic snapshot/Context
  Capsule/render/inference receipt refs when present, delegated ticket hash,
  Cohesix path/action, idempotency key, writer epoch, upstream ACK/ERR, task
  state, and evidence refs.
- MCP clients cannot supply arbitrary upstream paths for provider-specific mutating tools; provider tools must expand from checked target fields into manifest-allowlisted Cohesix paths/actions.
- A2A clients cannot supply arbitrary provider targets, host paths, or executor commands through message text or metadata; A2A skill inputs must expand only into manifest-allowlisted Cohesix paths/actions.
- Tool listing, prompt listing, Agent Cards, and A2A skills are not authorization. Calls fail closed if the current request lacks the required delegated scope.
- Remote MCP and A2A transports inherit loopback default, non-loopback exposure warning, origin validation, request-auth, rate limits, broker backpressure, and bounded response sizes.
- MCP and A2A conformance tests must include prompt-injection and confused-deputy negative cases: a resource, prompt, Agent Card, or peer message that asks the model to bypass tickets must not change server-side authorization.
- Inference results containing apparent MCP/A2A calls, host-ticket lines, paths,
  or credentials remain untrusted content and cannot initiate a second action
  without fresh schema validation, delegated scope, policy approval, and a new
  attributable audit record.
- Enforce the shared standing-authorization contract, including attenuation,
  revocation, cross-protocol accounting, and administrative separation. Test
  cumulative harmful action sequences and scope escalation as well as invalid
  individual calls. Existing executor/journal owners enforce these contracts;
  protocol adapters do not add an authority cache or policy evaluator.

As-built leverage:
- Reuse REST delegated identity from 27a, host-ticket WAL/replay, evidence redaction, policy rules, and gateway queue/backpressure controls.

---

### 7) Ecosystem conformance and client configuration
**Purpose:** Make Cohesix usable from standard MCP hosts and A2A peers without custom client forks.

Implementation requirements:
- Add checked examples for:
  - master-disabled, MCP-only, A2A-only, and both-enabled deployments,
  - discovery-to-completion using only the published use-case guide,
  - an unattended standing-authorized workflow including real side effects,
    recovery, and authoritative receipts,
  - local stdio MCP server config,
  - remote Streamable HTTP MCP endpoint config,
  - read-only namespace, semantic object, Context Capsule, and inference
    receipt browsing,
  - an inference submission that carries an immutable capsule ref and returns
    a 27e receipt ref without executing returned tool calls,
  - delegated mutating tool calls with explicit ticket/auth configuration,
  - A2A Agent Card discovery,
  - A2A task submission, streaming status, artifact retrieval, and cancellation against mock/dry-run providers.
- Add checked protocol fixtures/schemas for MCP JSON-RPC messages, A2A HTTP+JSON requests, gateway REST/OpenAPI compatibility, and generated provider action schemas so future client regressions are reviewable as data.
- Validate with at least one MCP inspector/client conformance path and archive the transcript/output under the milestone evidence directory.
- Validate with at least one A2A-compatible client/conformance path and archive the transcript/output under the milestone evidence directory.
- For each enabled protocol, qualify an ordinary client/peer against a named
  unattended scenario. Include limited-client capability fallback, safe refusal,
  cancellation/resume, revocation, and ambiguous-outcome recovery. Deterministic
  protocol fixtures remain the contract oracle; model-driven walkthroughs are
  additional usability evidence and cannot replace authorization tests.
- Add a gateway protocol performance probe covering namespace and host-artifact
  MCP resource reads, MCP tool calls that submit host tickets, A2A task
  creation/status streaming, and backpressure/refusal paths. Record semantic
  object/capsule/inference receipt adapter overhead separately from namespace
  and protocol overhead. Compare authority-bearing paths against REST
  read/write behavior from the accepted 27a gateway authority baseline; do not
  treat this as Pi hardware throughput proof unless the gateway probe exposes
  an upstream runtime regression.
- Document how MCP clients should treat Cohesix resources, tools, prompts, approval prompts, and errors.
- Document how A2A peers should treat Cohesix Agent Cards, skills, task status, artifacts, push notification limits, and errors.
- Expose deterministic error mapping from Cohesix `ERR` lines and REST gateway errors into MCP errors without losing the original Cohesix reason.
- Expose deterministic error mapping from Cohesix `ERR` lines, host-ticket refusals, and REST gateway errors into A2A task/error states without losing the original Cohesix reason.

As-built leverage:
- Reuse `docs/HOST_API.md`, `docs/API_GUIDELINES.md`, `docs/HOST_TOOLS.md`, `resources/openapi/hive-gateway.yaml`, and existing gateway status counters.

---

### 8) `coh mount` interoperability: REST primary, MCP context view optional
**Purpose:** Keep `coh mount --rest-url` as the direct gateway-backed namespace mount, while adding a useful MCP-facing filesystem view only where MCP resource discovery brings additional value.

Implementation requirements:
- Preserve the existing `coh mount --rest-url` behavior as the canonical FUSE view over Cohesix namespaces through `hive-gateway`; it remains the path for normal file-shaped reads and append-only writes.
- Add an optional MCP resource mount mode only if the MCP server exposes a resource/tool/prompt catalog that a local filesystem consumer cannot get from the existing mount without speaking MCP:
  - `coh mount --mcp-url <endpoint> --read-only --at <path>` mounts MCP-admitted context, not the full Cohesix namespace.
  - The mounted tree exposes bounded MCP resources, resource templates, tool schemas, prompt templates, and evidence/resource links as files.
  - Resource file reads call MCP `resources/list`, `resources/templates/list`, and `resources/read`; tool and prompt catalog files are generated from `tools/list`, `prompts/list`, and `prompts/get`.
  - The tree must make the backing type explicit for every resource/tool entry:
    namespace path/action for `LS`/`CAT`/`TAIL`/`ECHO` or
    `/host/tickets/spec`, or immutable artifact id/schema/hash and owner
    milestone for 27c semantic/capsule and 27e inference receipt resources.
    Usage-guide/catalog files carry their generated 27b/28c contract version
    and selected manifest fingerprint.
- The MCP mount is read-only by default and in the milestone acceptance path. Writes, renames, chmod, symlink creation, and host filesystem path escapes fail deterministically with no MCP `tools/call`.
- If a later task proposes write-capable MCP mount nodes, it must be a separate
  breaking-risk review and may append only the generated action-selected
  `host-ticket/v1` or `host-ticket/v2` record with delegated ticket,
  idempotency key, writer epoch, policy approval, and local operator
  confirmation. Receipt-bearing actions require v2; a mount write cannot
  request, downgrade, or synthesize the schema/receipt mode. It must never map
  arbitrary file writes to arbitrary MCP tools.
- A2A does not get a FUSE mode in this milestone. A2A task status and artifact links may appear as read-only MCP/evidence files, but task creation remains an A2A HTTP operation or an existing Cohesix ticket/control write.
- `coh doctor` and mount validation should report whether the REST mount, MCP resource mount, both, or neither are available, and should distinguish FUSE availability from MCP protocol availability.
- `coh doctor` must also distinguish namespace-resource readiness from
  semantic store/capsule verifier and inference receipt-verifier readiness; one
  missing optional artifact family does not make unrelated resources appear
  healthy or unavailable.
- MCP resource mount caches must be bounded, TTL-governed, and invalidated on MCP list-change notifications when enabled; stale cache reads must be marked as stale rather than silently presented as live state.

As-built leverage:
- Reuse `coh mount` FUSE validators, REST mount exclusivity, `CohAccess` read helpers, gateway MCP resource catalog, and evidence redaction rules.

---

### 9) Operator walkthrough and docs-as-built alignment
**Purpose:** Keep operator-facing guidance accurate as MCP, A2A, gateway REST, and mount modes become adjacent surfaces.

Implementation requirements:
- Audit and update `docs/OPERATOR_WALKTHROUGH.md` so the happy path, prerequisites, command ordering, failure handling, and expected evidence match the as-built gateway/MCP/A2A/mount behavior.
- Audit and update related canonical docs in the same milestone work:
  - `docs/HOST_API.md` for REST, MCP, and A2A endpoint/auth behavior,
  - `docs/HOST_TOOLS.md` for `coh mount --rest-url`, optional `coh mount --mcp-url`, `hive-gateway`, semantic/capsule and inference receipt resources, A2A Agent Card/task facade, host-ticket-agent, GPU bridge, and sidecar workflows,
  - `docs/API_GUIDELINES.md` for transport choice and MCP-vs-A2A-vs-REST-vs-filesystem guidance,
  - `docs/USERLAND_AND_CLI.md` for operator-visible commands and grammar-stability wording,
  - `docs/INTERFACES.md` for path/action mappings and refusal semantics,
  - `docs/ARCHITECTURE.md` for host-only MCP/A2A projections and the VM/host boundary,
  - `docs/SECURITY.md` for delegated ticket, prompt-injection, confused-deputy, redaction, and non-loopback exposure guidance,
  - `docs/TEST_PLAN.md` for the MCP, A2A, and mount evidence matrix.
- The audit must start from as-built code and generated truth:
  - `apps/hive-gateway/src/**`,
  - `apps/coh/src/mount.rs`,
  - `apps/coh/src/doctor.rs`,
  - `apps/host-ticket-agent/src/executors/**`,
  - `docs/snippets/*`,
  - generated manifests and `coh-rtc` outputs.
- Documentation must distinguish:
  - direct TCP `cohsh` proof,
  - REST/gateway proof,
  - gateway-backed `coh mount --rest-url`,
  - optional read-only MCP resource mount,
  - read-only 27c semantic/capsule and 27e inference receipt projections
    versus namespace-backed resources,
  - the separate 27e OpenAI-compatible endpoint versus MCP inference
    submission/status/receipt tools,
  - MCP tools/prompts that submit Cohesix tickets rather than executing host commands directly,
  - A2A Agent Card discovery, task submission, streaming status, artifact retrieval, and refusal behavior.
- Generated snippets and derived docs must be refreshed through `coh-rtc` or their owning generator; hand-editing generated blocks is invalid.

As-built leverage:
- Reuse 26c docs-as-built audit discipline, existing host-tool docs, generated snippets, and the 28c MCP/A2A conformance evidence.

**Commands**
- `cargo test -p hive-gateway`
- `cargo test -p hive-gateway --test agent_protocol_controls`
- `cargo test -p hive-gateway --test agent_usage_contract`
- `cargo test -p hive-gateway --test agent_autonomy`
- `cargo test -p hive-gateway --test mcp_protocol`
- `cargo test -p hive-gateway --test mcp_resources`
- `cargo test -p hive-gateway --test mcp_tools`
- `cargo test -p hive-gateway --test mcp_prompts`
- `cargo test -p hive-gateway --test mcp_security`
- `cargo test -p hive-gateway --test gateway_action_registry`
- `cargo test -p hive-gateway --test a2a_protocol`
- `cargo test -p hive-gateway --test a2a_tasks`
- `cargo test -p hive-gateway --test a2a_security`
- `cargo test -p coh --test mount_mcp`
- `cargo test -p host-ticket-agent`
- `cargo test -p coh --test evidence_pack`
- `cargo test -p coh --test evidence_timeline`
- `cargo test -p coh-rtc`
- `scripts/ci/gateway_perf_probe.sh --scenario mcp-a2a-protocols --state-dir out/bench/m28c-gateway-protocols`
- `git diff --check -- docs/BUILD_PLAN.md docs/OPERATOR_WALKTHROUGH.md docs/HOST_API.md docs/HOST_TOOLS.md docs/API_GUIDELINES.md docs/USERLAND_AND_CLI.md docs/INTERFACES.md docs/ARCHITECTURE.md docs/SECURITY.md docs/TEST_PLAN.md`
- `scripts/check-generated.sh`
- `scripts/cohsh/run_regression_batch.sh`
- `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m28c-qemu-gateway-agents`

These are planned implementation/acceptance commands. The selected Test Plan
also records ordinary-client transcripts and authoritative external-execution
receipts for `m28c-unattended-workflow-acceptance`; a passing host test or QEMU
run alone cannot establish live provider execution. Documentation-only roadmap
changes use documentation, metadata, and generated-consistency checks.

<a id="checks-definition-of-done"></a>
**Checks (Definition of Done)**
- Master and per-protocol disabled configurations satisfy the shared enablement
  contract across every transport/launch path, route, discovery surface, stream,
  callback, and optional MCP mount. The master overrides retained true child
  flags; CLI/env overrides cannot widen it. Missing/invalid configuration and
  attempted indirect activation fail closed. Independent REST/host operation
  remains available; disable-transition evidence accounts for accepted work.
- The complete gateway inventory has no unaccounted operations. Every admitted
  operation and supported use-case stage has an equivalent mapping/composition
  in each enabled protocol; blocked/future rows and any native protocol limits
  retain explicit reasons. Every advertised live action has its exact owning
  authority, dependency, and execution evidence. An excluded action cannot
  disappear from the coverage denominator merely because its adapter is missing.
- Generated usage guidance covers appropriate and unsupported uses, schemas,
  examples/counterexamples, authority/approval, bounds, lifecycle, failure,
  recovery, and evidence. Ordinary clients can discover and execute the checked
  scenario using this guidance alone, including tool-only MCP and A2A-only
  configurations. Examples validate against owner schemas; stale or conflicting
  guidance fails the consistency gate.
- Every enabled protocol has a named unattended consequential workflow from
  discovery/preflight through admission, execution, observation, recovery, and
  authoritative receipt. No per-action human interaction occurs inside the
  accepted standing policy. Exact-policy escalation, refusal, expiry/revocation
  during a run, reconnect/restart, attenuation, cumulative budget exhaustion,
  cancellation, and ambiguous execution have deterministic negative/recovery
  evidence. Read-only, mock, or dry-run success alone cannot close this gate.
- Shared authority checks prevent cross-protocol or child-agent budget renewal,
  self-approval, and policy/manifest widening. Resume rechecks current state;
  retries do not duplicate effects. Cancellation requested, confirmed canceled,
  rollback, and unknown outcome remain distinguishable in receipts and tasks.
- MCP lifecycle, discovery, cancellation, version/session handling, tool/resource/
  prompt methods, and negotiated optional features pass the exact pinned
  revision's contract and recorded client capability matrix. Regenerate fixtures
  when a revision changes these methods or headers; never mix revisions.
- A2A Agent Card discovery, revision-specific version metadata, optional
  authenticated extended Card, and generated message/task/stream/push/artifact
  mappings pass the exact pinned revision and binding recorded in the docs.
- `crates/cohsh-core/fixtures/grammar.sha256` and generated `docs/snippets/cohsh_grammar.md` remain unchanged unless a separately approved breaking grammar milestone changes them.
- Every namespace-backed MCP read maps to existing `LS`, `CAT`, or `TAIL`.
  Every semantic/capsule or inference-receipt read maps to the accepted
  read-only 27c or 27e core over an immutable id. Every MCP write maps to
  existing `ECHO` into a documented Cohesix control file or
  `/host/tickets/spec`; no host-artifact adapter mints authority.
- Gateway metadata/bounds/schema and usage-guide reads project their existing
  host/generated owner contract; they do not manufacture VM namespace paths.
- Every A2A task maps to accepted 27c semantic/capsule refs where context is
  used, an existing 27d run/checkpoint/evidence record, a 27e inference
  receipt where inference is used, and, when mutating, an existing Cohesix
  host-ticket/control action. No A2A message text or metadata becomes
  authorization.
- Each protocol's read-only discovery, usage guidance, visibility, and auth
  acceptance passes before its mutating workflow acceptance. A2A-only profiles
  do not require MCP runtime enablement or MCP acceptance evidence.
- Read-only MCP acceptance is not sufficient evidence for mutating tools, A2A
  task creation, inference/provider action execution, or VM Worker/driver
  authority. Each mutating acceptance artifact must name its
  27a/27b/28a authority/admission inputs and applicable 27c/27d/27e
  context, run, and receipt inputs,
  matching 26e live-task evidence where applicable, and 28b evidence only for
  production Worker ledger binding, complete driver-inventory projection, or
  structured quarantine/restart claims.
- Read-only MCP and A2A artifact/resource acceptance includes negative tests for public, ticket-scoped, and admin-only read visibility; ticket/provider/evidence/audit reads for the wrong delegated identity fail before payload construction.
- MCP tool schemas and A2A skill schemas are generated from the same
  provider/integration graph and reference the same accepted semantic/capsule
  and inference schemas; parity tests fail if semantic/context, inference,
  CUDA/GPU, PEFT, NeMo, K8s, systemd, Docker, FUSE/read projection, federation,
  evidence, Worker tier, external-executor requirement, or availability state
  drifts between protocols.
- MCP/A2A discovery and execution respect the selected use-case row. Missing live dependencies yield omission or typed unavailable/refused results, and protocol success never changes the row's maturity classification.
- No MCP tool directly invokes host executors, shell commands, CUDA/NVML calls, PEFT filesystem mutation, NeMo endpoints, `systemctl`, `docker`, or `kubectl` outside the existing Cohesix adapters.
- No A2A skill directly invokes host executors, shell commands, CUDA/NVML calls, PEFT filesystem mutation, NeMo endpoints, `systemctl`, `docker`, or `kubectl` outside the existing Cohesix adapters.
- No MCP tool or A2A skill calls an inference provider directly, reimplements
  the OpenAI-compatible surface, or executes model-produced tool calls; an
  inference submission is admitted through 27d and observed through a verified
  27e receipt.
- Semantic/capsule inspection, inference submission/receipt, CUDA/GPU, PEFT,
  NeMo, K8s, systemd, and Docker scenarios have deterministic mock tests and at
  least one live-safe dry-run/conformance transcript appropriate to their claim
  class.
- Mutating tools and A2A task actions fail without delegated scope and leave no side effects; duplicate mutating calls with the same id/idempotency key do not duplicate side effects.
- Remote MCP and A2A endpoints validate `Origin`, enforce auth, respect loopback defaults, and return bounded protocol errors under gateway backpressure.
- Evidence packs and timelines can reconstruct the MCP call or A2A task,
  semantic snapshot/capsule/render refs, delegated ticket hash, inference
  admission/receipt ref, underlying Cohesix path/action, provider receipt, and
  final state without raw prompt/output or secret leakage.
- Standard MCP clients can discover resources/tools/prompts and call read-only tools without Cohesix-specific patches.
- Standard A2A clients can discover the Agent Card, submit a dry-run task, observe status/artifacts, and handle refusals without Cohesix-specific patches.
- Existing `coh mount --rest-url` semantics remain unchanged, including REST mount exclusivity and append-only write behavior.
- `coh mount --mcp-url` exposes only MCP-admitted resources/tool schemas/prompt templates/evidence links, is read-only in the acceptance path, and never invokes MCP tools during filesystem metadata or write operations.
- There is no A2A FUSE mode; A2A task/artifact state appears through gateway protocol responses and read-only evidence/resource projections only.
- MCP-mounted resource contents match the corresponding MCP `resources/read` output and, for Cohesix namespace-backed resources, the corresponding REST/console read within documented bounds.
- MCP-mounted semantic/capsule and inference receipt resources verify against
  the same immutable hashes and schemas as the direct 27c/27e host tools;
  their presence never claims those artifacts exist in the VM namespace.
- A2A artifacts and push notification attempts are bounded, redacted, policy-gated, and reconstructable from audit/evidence without raw secret leakage.
- Gateway protocol performance evidence shows MCP resource/tool and A2A task/artifact paths stay bounded relative to the 27a gateway authority baseline; any full Pi/QEMU benchmark is triggered only by evidence of upstream runtime-path regression.
- `docs/OPERATOR_WALKTHROUGH.md` and related canonical docs describe the as-built transport, mount, MCP, A2A, host-ticket, provider, and evidence behavior without claiming implemented support before code/tests/generated outputs exist.

**Compiler touchpoints**
- `coh-rtc` adds schema-versioned `gateway.agent_protocols.enabled`,
  `gateway.mcp.enabled`, and `gateway.a2a.enabled`, all false by default;
  emits the effective enablement conjunctions and selected manifest fingerprint
  into every gateway/profile consumer; and rejects runtime policy widening.
  Master false dominates child flags without requiring optional providers.
- `coh-rtc` emits one complete operation/use-case coverage and usage-contract
  projection from the 27b registry and accepted owner schemas. It includes
  mapping/composition/exclusion reasons, readiness and visibility, native
  protocol/client affordances, schemas/examples, authority/approval mode,
  lifecycle/recovery/evidence guidance, versioning, and cache invalidation.
  Validate example/schema parity and reject unaccounted admitted operations.
- Standing-authority schemas and budgets remain owned by shared 27a/27b/27d/28a
  contracts. Generated protocol policy references their exact action, subject,
  run, attenuation, revocation, and accounting rules rather than redefining them.
- `coh-rtc` emits `gateway.mcp.*` policy:
  - enabled transports (`stdio`, `streamable_http`),
  - accepted MCP protocol revision,
  - JSON Schema dialect and authorization mode for the accepted revision,
  - revision-specific version/request metadata, session/termination policy,
    cancellation policy, and stream enablement/bounds,
  - endpoint path,
  - resource URI roots and path allowlists,
  - tool allowlists and provider action mappings,
  - prompt template ids,
  - usage-guide/help mappings, schema and example refs, effect annotations,
    client capability fallbacks, visibility/cache scope, and freshness policy,
  - MCP resource-mount enablement, read-only requirement, cache TTL, and synthetic tree bounds,
  - per-tool max input/output bytes,
  - delegated-ticket and writer-epoch requirements,
  - redaction and evidence-export flags.
- Generated MCP policy references, without redefining:
  - accepted 27c semantic snapshot/object/view/edge, Context Capsule, render,
    visibility, and immutable-id schemas,
  - accepted 27e inference request/admission/receipt ids, read visibility, and
    status/submit action mappings.
- Manifest validation rejects semantic/capsule resources whose schemas,
  visibility classes, store profiles, or immutable-id bounds do not match
  accepted 27c outputs, and rejects inference resources/tools whose
  admission, receipt, provider, or content-retention contract does not match
  accepted 27d/27e outputs.
- Manifest validation rejects MCP enablement when Milestone 27a delegated write identity or required audit/replay/fencing prerequisites are disabled for mutating tools.
- Manifest validation rejects NeMo MCP tools unless the 27d optional NeMo provider family and parity checks are enabled.
- `coh-rtc` emits protocol-neutral `gateway.provider_actions.*` and
  `gateway.integration_surfaces.*` projections derived from the Milestone 27b
  graph for every operation exposed through MCP tools or A2A skills, with
  referenced owner schemas from 27c and 27e where semantic/capsule or
  inference operations are present. They include action ids, target schema
  refs, dry-run support, idempotency requirements, writer-epoch requirements,
  receipt schema refs, Worker/executor/package dependencies, observed
  availability, use-case refs, and evidence-export behavior. They are
  generated compatibility metadata, not a second registry, semantic store,
  inference protocol, or authority source.
- Manifest validation rejects any MCP tool or A2A skill whose operation/action
  mapping is absent from the shared registry or whose underlying owner schema
  diverges between protocols. Task compositions preserve each action's schema.
- `coh-rtc` emits `gateway.a2a.*` policy:
  - enabled endpoint/binding,
  - accepted A2A protocol revision,
  - binding-specific operation and endpoint mappings for that revision,
  - revision-specific version metadata and extension policy,
  - Agent Card path, provider metadata, skill ids, and interface declarations,
  - skill guidance/examples/security/media modes, documentation/artifact links,
    input/authority-required mappings, and client capability fallbacks,
  - task, artifact, stream, and push-notification bounds,
  - skill allowlists and provider action mappings,
  - per-skill max input/output bytes,
  - delegated-ticket, idempotency, and writer-epoch requirements,
  - redaction, evidence-export, and callback allowlist flags.
- Manifest validation rejects A2A enablement when required Milestone
  27a/27b/28a delegated authority, registry or admission, audit/replay, or fencing
  prerequisites are disabled for selected mutating skills. Validate 27d durable
  run/task state for tasks and 27c/27e context/inference dependencies only where
  used. Disabled protocols and unrelated operations do not acquire those
  optional dependencies.
- Manifest validation rejects NeMo A2A skills unless the 27d optional NeMo provider family and parity checks are enabled.
- Generated docs refresh:
  - `docs/HOST_API.md`
  - `docs/API_GUIDELINES.md`
  - `docs/HOST_TOOLS.md`
  - `docs/INTERFACES.md`
  - `docs/ARCHITECTURE.md`
  - `docs/SECURITY.md`
  - `docs/TEST_PLAN.md`
  - `docs/USERLAND_AND_CLI.md`
- Human-authored as-built docs refreshed in:
  - `docs/OPERATOR_WALKTHROUGH.md`
