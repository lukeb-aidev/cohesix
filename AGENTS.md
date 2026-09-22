<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define the normative Cohesix charter and route contributors to task-specific contracts. -->
<!-- Author: Lukas Bower -->
# AGENTS — Cohesix Build Charter

Cohesix is a Rust/seL4 control-plane OS for bounded, capability-scoped orchestration
and telemetry of edge GPU nodes, using a Queen / Worker hive model. This charter
sets repository-wide invariants; the linked owners supply mandatory detail for
the affected work. A prose requirement is not evidence that CI enforces it.

## Start here

1. Confirm the requested branch, worktree changes, affected component and exact
   host/target profile. Preserve unrelated edits, logs and immutable evidence;
   do not reset, clean, switch branches or overwrite artifacts to simplify a task.
2. Read the relevant [BUILD_PLAN](docs/BUILD_PLAN.md) task and selected manifest.
   Implementation must cite its exact active milestone/submilestone and task ID.
   Read applicable local instructions and the contracts below, not every document.
3. Follow [CONTRIBUTING](CONTRIBUTING.md) for atomic changes and validation, and
   [Coding Guidelines](docs/CODING_GUIDELINES.md) for Rust and test discipline.
   Reuse an applicable repository skill or maintained entry point; neither can
   waive the charter, task scope or evidence requirements.
4. Choose the validation lane before editing. Use the Test Plan's action catalog
   and changed-path selection; report commands, results, proof limits and blockers.

| Work | Required starting points |
| --- | --- |
| Host tools, Python or UI | [HOST_TOOLS](docs/HOST_TOOLS.md), affected surface contract, focused host checks in [TEST_PLAN](docs/TEST_PLAN.md) |
| Root, service or Worker runtime | [ARCHITECTURE](docs/ARCHITECTURE.md), [ROLES_AND_SCHEDULING](docs/ROLES_AND_SCHEDULING.md), exact-profile target convergence |
| Drivers, timers, DMA or physical Pi | [DRIVERS](docs/DRIVERS.md), [HARDWARE_BRINGUP](docs/HARDWARE_BRINGUP.md), fresh Pi evidence for physical claims |
| Generated contracts or protocols | Selected `configs/root_task*.toml`, resolved manifest, compiler IR and [INTERFACES](docs/INTERFACES.md) |
| Docs, policy or overall release | [CONTRIBUTING validation](CONTRIBUTING.md#5-validate-locally), [TEST_PLAN](docs/TEST_PLAN.md); no unrelated Rust build for documentation-only work |

## Authority and conflict resolution

Use the authority that owns the question; lower-level guidance cannot waive
repository invariants or act outside its domain:

1. This charter owns invariants; it and BUILD_PLAN own milestone legality.
2. The selected source/resolved manifest, seL4 build outputs and `coh-rtc`
   artifacts own generated/as-built behavior.
3. The most specific architecture, interface, driver, security, scheduling or
   operator document owns that surface's contract.
4. TEST_PLAN owns test selection, execution order, target authority, provenance,
   convergence and acceptance.

Generated output cannot activate scope or waive this charter. Surface documents
cannot contradict selected as-built truth; test documents cannot override
milestone legality. Stop and reconcile conflicting canonical authorities in the
same scoped change, rather than choosing the convenient rule. A direct task to
repair governance may change those documents atomically, not unrelated product code.

## Scope and targets

- Primary development host: Apple Silicon/macOS 26 under
  [TOOLCHAIN_MAC_ARM64](docs/TOOLCHAIN_MAC_ARM64.md). Confirm the actual environment.
- VM: QEMU `aarch64/virt`, GICv3. Hardware: Pi 4 firmware -> U-Boot -> seL4 -> root.
  UEFI/AWS work is inactive unless BUILD_PLAN authorizes it. seL4 stays external.
- `In Progress` tasks are active. `Reopened` permits only the stated defect,
  regression and evidence restoration. Cite both discovery and restoration tasks
  for downstream-discovered defects. Pending/future tasks are inactive.
- Cleanup, refactoring, preparation or future-proofing creates no scope authority.

## Durable architecture and security boundaries

- VM artifacts remain pure Rust and `no_std`: no POSIX/libc facade, in-VM CUDA/NVML,
  hidden RPC or undeclared shared-memory authority. GPU access remains host-side;
  target Workers serve only their declared ticket, lease and telemetry contracts.
- The authenticated console is the sole in-VM TCP listener. Control uses documented
  Secure9P namespaces, console grammar or compiler-declared driver/service ABIs
  with role-scoped capabilities.
- Physical discovery, mapping, DMA, IRQ and admission go through HAL. Manifest-
  declared isolated runtimes own steady-state devices; root admits, supervises
  and diagnoses. Only the declared owner may issue, complete, retry or recover
  device operations; helpers and fallbacks cannot become competing owners.
- Target elapsed time uses exported `CNTVCT_EL0` only when the selected seL4 build
  enables it, scaled by generated `TIMER_CLOCK_HZ`. No `CNTPCT_EL0`, EL0 timer
  control, dummy time or CPU-speed spin timing.
- Rootfs CPIO stays below 4 MiB. Secure9P stays 9P2000.L: `msize <= 8192`, walk
  depth <= 8, no `..`, and no fid reuse after clunk.
- Validate nested lengths, counts, offsets, arithmetic and resource limits before
  the allocation, copy, indexing or work they control. Outer bounds do not validate
  inner fields. Return typed deterministic errors; never hard-code secrets.
- Without authenticated cohsh/TCP, prioritise serial, then local-seat USB input,
  then HDMI feedback. With authenticated cohsh/TCP, give its response flush bounded
  priority without starving physical input, emergency diagnostics or fatal status.
- Under load, preserve command liveness and bounded ACK/ERR/END on every active
  surface. Reduce only nonessential mirroring, redraws, progress, verbose telemetry
  and large tails. Report bounded, rate-limited idle/busy/high-load/overload status
  and the strongest known blocker to serial/local-seat operators.
- Preserve seL4 semantics, manifest fidelity and deterministic bounds. Prevent
  stack overflow and memory corruption; reuse existing bounded diagnostics.

## Compiler truth, interfaces, and documentation

- Never hand-edit generated code, manifests, policy, scripts or documentation
  blocks. Change IR, validate, regenerate every output and update affected
  implementations, fixtures and docs together. The selected `SEL4_BUILD_DIR`
  owns kernel headers, object sizes, slot layout and configuration truth.
- Documentation describes as-built behavior, not aspiration; drift is a defect.
  Keep affected CLI/UI help, manuals and public guides aligned. Shared manual
  content has one source; only genuine surface differences justify variation.
- Console grammar, NineDoor errors, namespaces, `/proc` formats, role authority
  and generated-interface changes are breaking. Update all affected surfaces;
  bump the manifest schema when the contract is manifest/generated controlled.
- Follow [documentation and metadata rules](CONTRIBUTING.md#documentation-and-metadata).
  Explain contracts and non-obvious decisions, not syntax or invented history.
  Keep useful API/safety documentation; do not credit tools in source headers.
- `.coh` grammar is owned by [USERLAND_AND_CLI](docs/USERLAND_AND_CLI.md).
  Code changes under `releases/` increment the minor version and update directory
  and tarball names; do not rewrite immutable release evidence.

<a id="reviewable-engineering"></a>
## Code Review Rules

- Report actionable correctness, security, reliability and compatibility issues.
  Identify the failure condition, affected path and impact; distinguish demonstrated
  failures from risks. Leave mechanical style to tooling; avoid speculative rewrites.
- Optimise for correctness, maintainability and review cost, not apparent human
  authorship, code volume or detector scores. Code and repository-local rationale
  must explain ownership, state transitions, bounds and failures without chat history.
- Use the simplest idiomatic Rust preserving target constraints. Add abstractions,
  traits, macros, dependencies or types only for a current invariant, ownership
  boundary or material simplification. Explicit protocol/control-flow repetition
  is acceptable when easier to review. Narrow and justify lint allowances; use
  `cfg` for genuine profile differences, not suppression to hide unfinished work.
- Use unambiguous composite identity/deduplication/correlation encodings and
  preserve persisted meaning when formats change.
- Distinguish success, already-satisfied state, deterministic refusal and uncertain
  outcome using provider contracts and observations. Never invent transition
  evidence or blindly replay uncertain side effects.
- Keep terminal execution separate from pending recovery/result delivery. Preserve
  those obligations durably across restarts and retention; retry delivery, not effects.
- Safe APIs must uphold internal unsafe preconditions. Every unsafe block and trait
  implementation needs a precise safety argument, not a ceremonial comment. Apply
  the full [Rust safety rules](docs/CODING_GUIDELINES.md#safety-and-risk-controls),
  including Send/Sync evidence, transmute equivalence and the unchanged risk ratchet.

## Atomic work

- One authorised goal per complete change. No speculative or unrelated cleanup.
  Follow the [task record](CONTRIBUTING.md#task-record) and explain changed
  invariants and rationale; record material AI assistance there, preserving provenance.
- Only maintained workflows/gates or invoked support belong in tracked `scripts/`.
  Scratch probes belong in ignored `out/scripts/` or temporary storage. Follow
  [script lifecycle rules](CONTRIBUTING.md#script-lifecycle) before promotion.
- Any material behavior, interface, schema, bound, authority, lifecycle, evidence,
  workflow or performance change requires a complete host-tool, `tools/cohesix-py`
  and benchmark compatibility review. Use the generated inventory; record unaffected
  surfaces, and update all affected implementations, contracts, tests and docs together.

## Test Discipline

- Preserve distinct independently known contracts, not test counts. Cover relevant
  malformed/boundary inputs; never weaken expectations, add arbitrary retries or
  change truth to obtain PASS. Seek a safe minimal reproducer for defects.
- Match proof to execution: pure tests for pure contracts, host tests for host
  behavior, QEMU for seL4 VM behavior and fresh Pi evidence for physical behavior.
  Mocks cannot accept a target. Do not recreate the target in host simulations.
- Apply the complete [Test Discipline](docs/CODING_GUIDELINES.md#test-discipline)
  before changing tests; TEST_PLAN owns commands and acceptance, not those simulations.

## Convergence, acceptance, and audit closure

- Use [CONTRIBUTING validation](CONTRIBUTING.md#5-validate-locally) for iteration,
  merge, documentation-only and release obligations. All existing merge baseline,
  generated-consistency, risk and target gates remain required; relocation is no waiver.
- Non-claiming convergence may stop at the first failed proof layer; it never
  supplies acceptance. Component/milestone closure requires BUILD_PLAN checks and
  applicable TEST_PLAN evidence. Staged/release claims require the complete applicable
  staged, pressure, repeatability, hardware, due-diligence and promotion evidence
  with exact source/image/target identity. A green health-check CI is not that proof.
- Individual changes, commits, merges and component/milestone acceptance need no
  human sign-off. Reviews may be agent-led; required independent technical review,
  safety arguments, tests, evidence and scope/exception controls remain mandatory.
- Only an overall Cohesix release requires explicit approval by a named human
  release owner before publication or promotion. Approval covers the assembled
  release's exact source/artifacts, evidence, limitations and residual risks,
  not separate approval of every change. AI and automated checks cannot provide it.
- Preserve historical approvals at their original scope; never fabricate approvals
  or verification. Make evidence accessible and disclose unexecuted or blocked checks.

## Task record

Use the unchanged fields in [CONTRIBUTING's task record](CONTRIBUTING.md#task-record).
Planner, Builder and Auditor are contribution roles; Queen and Workers are system
roles. BUILD_PLAN must explicitly introduce any additional role.

## Authoritative document map

In addition to the task routes above, consult the relevant owners:
[SECURITY](docs/SECURITY.md) for trust boundaries;
[SECURE9P](docs/SECURE9P.md) and [INTERFACES](docs/INTERFACES.md) for protocols;
[BENCHMARKS](docs/BENCHMARKS.md) for measurement;
[API_GUIDELINES](docs/API_GUIDELINES.md) for public APIs.
Read linked contracts when their surface is affected; do not duplicate them here.
