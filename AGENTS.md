<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define the normative Cohesix build charter, scope, and guardrails for contributors. -->
<!-- Author: Lukas Bower -->
# AGENTS — Cohesix Build Charter
You are an OS designer and expert in seL4 and Rust on aarch64.

You are building Cohesix, a control-plane operating system for highly secure orchestration and telemetry of edge GPU nodes, using a Queen / Worker hive model.

This file is the concise, repository-wide operating charter for Cohesix.
Violations block merge. Detailed contracts live in the authoritative documents
listed below; this file routes to them instead of duplicating them.

## Authority and conflict resolution

Apply the authority that owns the question; no lower-level source can waive a
repository-wide invariant or act outside its stated domain:

1. **Repository invariants:** this charter. **Scope and milestone legality:**
   this charter and
   [BUILD_PLAN.md](docs/BUILD_PLAN.md). Cite the exact active
   milestone/submilestone and task title/ID for implementation work.
2. **Generated/as-built behavior:** the selected `configs/root_task*.toml`
   source manifest, its resolved manifest, selected seL4 build outputs, and
   `coh-rtc` generated artifacts.
3. **Surface contracts:** the most specific document for the affected
   architecture, interface, driver, security, scheduling, or operator surface.
4. **Testing and evidence:** [TEST_PLAN.md](docs/TEST_PLAN.md) governs selection,
   execution order, target authority, provenance, convergence, and acceptance.

Generated output cannot waive this charter or activate milestone scope; surface
documents cannot contradict selected generated/as-built truth; test
documentation cannot override BUILD_PLAN milestone legality. If canonical
authorities disagree, stop and reconcile them in the same scoped change before
implementation or claims; do not choose whichever rule is more convenient.

A direct task to repair canonical governance or resolve a contradiction may
change the governing documents atomically. It does not authorize unrelated
product implementation.

## Scope and targets

- Primary development host: macOS 26 on Apple Silicon, using
  [TOOLCHAIN_MAC_ARM64.md](docs/TOOLCHAIN_MAC_ARM64.md).
- Target VM: QEMU `aarch64/virt` with GICv3.
- Target hardware: Raspberry Pi 4 via Pi firmware -> U-Boot -> seL4 binary
  image -> root task. UEFI/AWS work is inactive unless BUILD_PLAN authorizes it.
- Kernel: upstream seL4, external and never vendored.
- Userspace: pure Rust root task, compiler-declared isolated driver/service/
  Worker images, NineDoor, and host-side tools.
- `In Progress` tasks are active. `Reopened` tasks authorize only the defect,
  regression, and evidence closure needed to restore their stated definition of
  done. Use the most specific active task; a downstream-discovered defect cites
  both its discovery milestone and the reopened restoration task.
  Pending/future tasks are inactive.
- Do not disguise cleanup, preparation, refactoring, or future-proofing as
  milestone authority. Do not perform unrelated cleanup.

## Durable architecture and security boundaries

- VM artifacts remain `no_std`; no POSIX or libc façade, in-VM CUDA/NVML,
  hidden RPC, or undeclared shared-memory authority is permitted.
- The authenticated root-task console is the only in-VM TCP listener. Control
  uses documented Secure9P namespaces, console grammar, or compiler-declared
  driver/service ABIs with role-scoped capability authority.
- All physical device discovery, mapping, DMA, IRQ, and resource admission goes
  through HAL. Physical drivers run as manifest-declared isolated runtimes;
  root may admit, supervise, and diagnose them but cannot own their steady-state
  device path.
- A compiler-declared owner is solely responsible for physical issue,
  completion, retry, and recovery. DPCs, helpers, compatibility paths, and
  fallbacks cannot operate the same device independently.
- Hardware elapsed-time logic uses exported `CNTVCT_EL0` only when enabled by
  the selected seL4 build and scales from generated `TIMER_CLOCK_HZ`. No
  `CNTPCT_EL0`, EL0 timer-control access, dummy time, or CPU-speed spin timing.
- Rootfs CPIO remains below 4 MiB. Secure9P remains 9P2000.L with
  `msize <= 8192`, walk depth <= 8, no `..`, and no fid reuse after clunk.
- Worker behavior must be documented as built. Worker GPU access remains
  host-side; target Workers handle only declared ticket, lease, and telemetry
  contracts.
- Validate all user-controlled input and fail with typed deterministic errors.
  Never hard-code secrets.
- Scheduling budgets, namespaces, driver mechanics, and Pi evidence follow
  their owning documents in the map below.
- Without an authenticated `cohsh`/TCP session, service physical operator input
  first: serial, then local-seat USB keyboard, then HDMI feedback when present.
- With an authenticated `cohsh`/TCP session, give that primary shell bounded
  response-flush priority without starving serial/local-seat input, emergency
  diagnostics, or fatal status.
- Under load, preserve command liveness and bounded `ACK`/`ERR`/`END` on every
  active operator surface. Reduce only nonessential mirroring, redraws,
  progress breadcrumbs, verbose telemetry, and large tails.
- Keep serial and local-seat operators informed with rate-limited, bounded
  `idle`, `busy`, `high-load`, or `overload` summaries and the strongest known
  blocker; status reporting cannot create unbounded queues.
- Prefer the simplest design that preserves seL4 semantics, deterministic
  bounds, and manifest fidelity. Prevent stack overflow and memory corruption;
  reuse existing instrumentation before adding carefully bounded diagnostics.

## Compiler truth, interfaces, and documentation

- Never hand-edit generated code, manifests, policy, scripts, or generated
  documentation blocks. Change IR, validate it, regenerate every output, and
  update affected source, fixtures, and docs together.
- The selected `SEL4_BUILD_DIR` or equivalent profile build directory defines
  kernel header, object-size, slot-layout, and configuration truth.
- Documentation describes generated/as-built truth, not aspiration. Drift is a
  defect even when CI does not yet detect it.
- Changes to console grammar, NineDoor errors, namespace or `/proc` formats,
  role authority, or generated interfaces are breaking. Update all affected
  fixtures, generated artifacts, tests, and canonical docs; bump the manifest
  schema when the changed contract is manifest/generated controlled.
- Human-authored files in comment-capable formats retain concise Author,
  Purpose, and current-year Lukas Bower copyright metadata. Do not add invalid
  comments, invented fields, or sidecars to commentless formats; use existing
  package metadata or the governing documentation instead. Generated, vendored,
  and immutable release files retain their authoritative format.
- Comments and documentation must describe contracts, invariants, authority, or
  failure behavior—not generic file-summary boilerplate. Do not credit OpenAI,
  Codex, or other tools in file headers.
- `.coh` scripts follow [USERLAND_AND_CLI.md](docs/USERLAND_AND_CLI.md).
- Any code change under `releases/` increments the minor version and updates
  the release directory and tarball names.

## Atomic work

- Keep each change within one authorized goal. Partial or speculative changes
  are not mergeable.
- Tracked files under `scripts/` must implement a documented community or
  developer workflow, a canonical CI/test/evidence gate, or support invoked by
  a tracked build, release, or operator entry point. Temporary probes,
  one-off reproducers, scratch generators, and ad-hoc test wrappers belong
  under ignored `out/scripts/` (or an operating-system temporary directory),
  never in the tracked `scripts/` tree. Promote a temporary script only with
  its owning call site or documentation, focused tests where its logic merits
  them, and removal of the superseded path in the same change.
- Compile every affected implementation for its exact host/target profile.
  Documentation-only or policy-only changes run their applicable documentation,
  metadata, generated-consistency, and link checks; they do not invent a Rust
  compilation requirement.
- Add or update only the tests and target evidence required by Test Discipline
  and TEST_PLAN. Update public documentation with public behavior or interface
  changes.
- Any material Cohesix change—including generated/as-built interfaces, schemas,
  defaults, bounds, namespaces, authority, lifecycle or evidence semantics,
  supported workflows, or performance-relevant behavior—requires a same-change
  compatibility review of the complete host-tool suite, `tools/cohesix-py`
  library, and performance benchmark scripts.
- Update every affected implementation, generated contract, test or fixture,
  benchmark workload or report schema, and document together. Record reviewed
  surfaces requiring no change; unexplained cross-surface drift blocks merge.

## Test Discipline

- Tests preserve distinct, independently known contracts. Do not add a test
  merely because code changed or optimize for test count.
- Prefer small deterministic tests for parsing, bounds, arithmetic, ABI/layout,
  serialization, policy predicates, state machines, and other pure behavior.
- Match authority to execution: pure contracts use deterministic unit tests;
  host-component contracts use focused host tests; QEMU-target seL4 behavior
  requires QEMU evidence; physical Pi behavior requires fresh Pi evidence.
  Each layer proves only what it exercises.
- Do not model more target scheduling, IPC, capability, IRQ, DMA/cache, or
  driver behavior in a host test than a genuine host-testable contract needs.
  A green host simulation is not target acceptance, and target evidence does
  not replace an unexercised pure contract test.
- If host simulation and target evidence disagree, investigate the simulation
  before changing target code merely to satisfy it.
- After a target-discovered defect, add a host regression only when the cause is
  a useful deterministic host-testable invariant. Preserve the smallest
  independently understood invariant; do not recreate the full target scenario
  in mocks or require a host test for every target defect.
- Tests cannot depend on uncontrolled wall-clock time, sleeps, randomness,
  execution order, external networks, or shared mutable state/environment
  unless that behavior is the contract under test. Prefer fixed or injected
  inputs; controlled time, randomness, network, filesystem, and mocks remain
  legitimate.
- Prefer exact assertions for exact contracts. Do not loosen assertions, widen
  accepted outcomes, increase arbitrary retries/polling, or change expected
  values merely to obtain PASS. Predicates, ranges, and set membership remain
  valid when they are the contract.
- Production constants, tables, or implementation logic are not independent
  test oracles. Expected truth comes from an independent specification,
  generated contract, ABI, fixture, protocol, or other authoritative source.
- A directly affected test may be simplified, consolidated, replaced, or
  removed only when its protection is demonstrably redundant,
  implementation-coupled, misleading, obsolete, or superseded by stronger
  evidence. Canonical protocol/as-built fixtures remain authoritative unless
  their governing contract intentionally changes.

## Convergence, acceptance, and audit closure

- Non-claiming convergence diagnostics defined by TEST_PLAN may run early and
  stop at the first failed target proof layer. They never emit or replace
  acceptance evidence.
- Milestone/release claims require the complete applicable staged Test Plan,
  exact source/image/target provenance, and all required pressure,
  repeatability, hardware, due-diligence, and promotion evidence:

  ```sh
  scripts/ci/test_plan_run.sh --list
  scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/<run-id>
  scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/<run-id>
  ```

- Before merge, run `scripts/check-generated.sh` and
  `scripts/ci/check_test_plan.sh`.
- AI-assisted Rust is untrusted. Before merge, it requires command evidence,
  human reviewer sign-off, and:

  ```sh
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo check --workspace
  cargo test --workspace
  cargo audit
  cargo deny check advisories
  ```

- Every `unsafe` block has a precise `SAFETY:` invariant. Every unsafe
  `Send`/`Sync` implementation also requires concurrency evidence.
  `transmute` requires documented ABI/layout equivalence.
- Non-test `unwrap()` is prohibited unless impossible by construction.
  Non-test `expect()` is limited to invariant boundaries with a precise
  message. User input returns typed errors; do not hide failures with lossy
  defaults.
- Never hold a lock across `.await`. Control-plane channels require bounded
  backpressure; spawned tasks define ownership, cancellation, and shutdown.
- Non-test `unsafe`, `unwrap`, `expect`, and `panic!` counts cannot
  increase without a finding in `docs/audit/findings.csv` and an approved
  exception in `docs/audit/EXCEPTIONS.md`.

## Task record

Planner, Builder, and Auditor are the contribution roles; Queen and Workers are
system roles. BUILD_PLAN must explicitly introduce any additional role.

```text
Title/ID: <slug>
Milestone: <exact milestone/submilestone and task title/ID>
Goal: <one sentence>
Inputs: <artifacts, versions, paths>
Changes:
  - <file> — <summary>
Commands: <exact shell commands for the scoped host/target>
Checks: <deterministic success criteria>
Deliverables: <files, logs, doc updates>
```

## Authoritative document map

- Scope and milestone tasks: [BUILD_PLAN.md](docs/BUILD_PLAN.md)
- Architecture and TCB boundaries: [ARCHITECTURE.md](docs/ARCHITECTURE.md)
- Drivers, HAL, DMA/IRQ/cache, and timers: [DRIVERS.md](docs/DRIVERS.md)
- Physical build/flash/acceptance: [HARDWARE_BRINGUP.md](docs/HARDWARE_BRINGUP.md)
- Roles, scheduling, namespaces, and operator priority:
  [ROLES_AND_SCHEDULING.md](docs/ROLES_AND_SCHEDULING.md)
- External interfaces and breaking changes:
  [INTERFACES.md](docs/INTERFACES.md),
  [SECURE9P.md](docs/SECURE9P.md), and
  [USERLAND_AND_CLI.md](docs/USERLAND_AND_CLI.md)
- Security and threat boundaries: [SECURITY.md](docs/SECURITY.md)
- Host-tool catalog and composition: [HOST_TOOLS.md](docs/HOST_TOOLS.md)
- Performance methodology and reports: [BENCHMARKS.md](docs/BENCHMARKS.md)
- Testing and evidence: [TEST_PLAN.md](docs/TEST_PLAN.md)
- Contribution and language guidance: [CONTRIBUTING.md](CONTRIBUTING.md),
  [CODING_GUIDELINES.md](docs/CODING_GUIDELINES.md), and
  [API_GUIDELINES.md](docs/API_GUIDELINES.md)
