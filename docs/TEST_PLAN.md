<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define risk-based test selection, target-first convergence, and immutable acceptance evidence. -->
<!-- Author: Lukas Bower -->

# Test Plan

This is the execution and acceptance guide for the complete Cohesix stack.
Use the [action catalog](#canonical-action-catalog) for executable checks and
the [26e enforcement map](#milestone-26e-executable-enforcement) for the
stage, collector, and promotion gates that own milestone closure. Test bodies
and evidence validators own detailed assertions; do not maintain a parallel
prose inventory of individual tests.
Current implementation status belongs in [STATUS.md](STATUS.md); task scope,
candidate chronology, and qualified result history belong in
[BUILD_PLAN.md](BUILD_PLAN.md). Do not append another run diary here.

## Coverage without duplicate execution

Select tests by the contract at risk, its real execution surface, and the claim
being made. Record the affected surfaces, catalog actions, expected positive
and negative outcomes, exact profiles, and any evidence still missing. A
successful command or test count is not a coverage argument.

| Contract at risk | Required kind of evidence | Closure owner |
| --- | --- | --- |
| Parsing, bounds, arithmetic, serialization, ABI/layout, policy, replay, pure state transitions | Small deterministic tests with independent expected truth | Stage 01 and AGENTS Test Discipline |
| Source/generated/profile/image agreement | Generated consistency, exact target compilation, validated image and launch/readback identity | Target-entry integrity; Stages 01/02; hardware runbook |
| seL4 capabilities, IPC/MCS, service/Worker startup, SMP, faults and recovery | Real READY and operation; bounded negative/recovery path; post-recovery state and liveness | QEMU/Pi target gates and 26e component/root/system evidence |
| Physical ownership, MMIO, IRQ, DMA/cache, timers, USB/HDMI, GENET/CYW43 | Fresh exact-image Pi boot, real device operation and operator observation | Conditional F; independent performance and repeatability gates |
| Console/TCP, Secure9P, REST and gateway | Exact protocol terminals, refusal/error paths, reconnect/close and bounded backpressure on a live target | Stages 03/04; Conditional A where selected |
| Shipped host tools, Python, tickets, CAS, GPU/PEFT receipts and providers | Focused host contracts plus exact-version live integration for supported modes; real providers for provider claims | Conditional B/B1; generated host integration graph; Python projection |
| SwarmUI | Deterministic presentation/replay plus separately identified packaged/live workflow checks | UI tier; host integration and release qualification |
| Capacity, latency, coexistence, sustained load and delivery | Qualified workload, before/after state, pressure/repeatability, extracted bundle execution and governance | Conditional B2/D/E/F/G as applicable; Stage 05 |

- Follow [AGENTS Test Discipline](../AGENTS.md#test-discipline): preserve useful
  pure-contract tests even when a target also exercises the path. Add a host
  regression after a target defect only when its cause is an independently
  understood deterministic invariant. Do not recreate the whole target in mocks.
- Test the applicable boundary and refusal cases, not only success: malformed
  or oversized input, wrong/stale authority or generation, exhaustion, partial
  construction/publication, timeout/fault, cancellation and teardown. Require
  typed failures, bounded resource use, no unintended authority or side effects,
  and continued service where the contract requires recovery. Terminal
  containment must remain terminal; do not demand a nonexistent restart path.
- The catalog owns command selection. A named regression identifies an
  obligation, not an additive command after its owning complete suite.
  Repeat a suite only for a distinct feature/profile, required fresh boot,
  workload, host platform, changed input, or explicitly required repeatability.
  Keep the broad Rust pre-merge/audit gates; this rule does not waive them.
- Reuse only validator-approved immutable evidence under the resume contract.
  Shared source bytes do not make Mac/HVF and Linux/KVM builds, QEMU and Pi,
  Wi-Fi and GENET, or first-install and saved-policy boots interchangeable.
  Stage 05's current advisory checks still run on every acceptance refresh.
- Review all affected host tools, `tools/cohesix-py`, generated contracts,
  benchmark workloads/report schemas, fixtures, and docs in the same change.
  Record reviewed surfaces requiring no change. Use [HOST_TOOLS.md](HOST_TOOLS.md)
  and the generated integration graph as the inventory, not a partial tool list.
- When hardware, a provider, or a required host platform is unavailable, report
  BLOCKED or unexecuted for that claim. A mock, replay, help/version smoke, or
  another target cannot fill the gap. No unrelated test cleanup or test-count
  target is authorized by this plan.

## Development order: target first, acceptance complete

During active target development, obtain real QEMU or Pi 4 evidence as early as
safely possible. Use host/unit tests to preserve pure contracts and codify
host-testable target-discovered invariants. Use the complete staged pipeline
when producing acceptance evidence. This is a sequencing correction, not a reduction in test
coverage, provenance, or release rigor.

Two workflows are deliberately separate:

- **Convergence evidence** is a development and diagnosis aid. It is emitted as
  `NON-CLAIMING TARGET DIAGNOSTIC`, uses the distinct
  `cohesix-test-plan-convergence/v1` schema, and cannot write Stage PASS
  attestations or Milestone 26e acceptance records. It does not require an
  earlier Stage 01, 02, 03, 04, or 05 attestation.
- **Acceptance evidence** is the only basis for a milestone or release claim.
  The existing Stage 01-05 runner, target-specific attestations, immutable
  artifacts, pressure/repeatability evidence, due diligence, and conditional
  claim tiers remain authoritative and fail closed.

A convergence PASS never satisfies, bypasses, or promotes an acceptance gate.
Run the full acceptance workflow again against the exact final source, image,
target, profile, and topology before making a claim.

Milestone 26e `bi`, `caps mcs`, and `smp mcs` are convergence diagnostics.
Parser/fixture and `mcs_operator_inspection` tests prove bounded rendering and
snapshot behavior. The `caps mcs` records must preserve every field within the
77-byte Pi linked-HDMI fallback width. QEMU proves only QEMU; Pi observations
require a fresh exact image. Neither diagnostic may call
`seL4_SchedContext_Consumed`.

### Target-first convergence entry point

Use the separate runner during active Milestone 26e work:

```bash
# Explicit focus is preferred when a dirty tree spans several surfaces.
scripts/ci/test_plan_converge.sh \
  --target qemu \
  --focus root-mcs \
  --path apps/root-task/src/kernel.rs

# Reuse a previously built immutable out/cohesix launch set.
scripts/ci/test_plan_converge.sh \
  --target qemu \
  --focus worker \
  --launch-existing

# Let the declarative trigger paths choose the focus for a bounded change set.
scripts/ci/test_plan_converge.sh \
  --target qemu \
  --changed-from origin/main
```

`--focus` accepts `root-mcs`, `ninedoor`, `console-network`, `worker`,
`pi4-driver`, `live-transport`, `python-sdk`, `swarmui`,
`test-plan-tooling`, or `docs`; use `--list-focus` for the generated inventory.
Automatic selection fails rather than silently choosing QEMU for a Pi-first
path or choosing a focus for an unmatched path. The selected actions come from
`configs/test_plan_actions.toml`; there is no second command catalog.

A selected focus proves only its changed path. For changes spanning independent
surfaces, retain the full changed-path list and run the additional relevant
focuses; an explicit `--focus` does not waive another surface's canary or final
claim-tier requirements. Review the selection before spending a target boot.

Every run creates a fresh directory below `out/test-plan-convergence/` and
records the Git commit, complete dirty-tree/source digest, catalog digest,
target, exact profile, focus, changed paths, selected action IDs, session/run
ID, action logs and hashes, target observation, built-image and immutable
identity hashes where applicable, UART/serial path and hash, result
(`PASS`, `FAIL`, or `BLOCKED`), first failed proof layer, and optional
`--hypothesis`/`--note`. Results are immutable candidate observations with
`claiming=false` and `promotion_eligible=false`.

### Changed-path convergence routing

The convergence focus and action metadata below are generated from the same
catalog used by acceptance. The first authoritative evidence is selected by
the most specific matching focus; broad host closure is not added before a
target canary merely because it is part of final acceptance.

<!-- test-plan-convergence:start -->
| Focus | Target | First authoritative evidence | Exact profile |
| --- | --- | --- | --- |
| `pi4-driver` | pi4 | one exact-image Pi boot, touched service/device liveness, one live operation, UART liveness, and no unexpected target fault | `pi4_production / configs/root_task_pi4_uboot_aarch64.toml` |
| `worker` | qemu | canonical QEMU boot, real Worker READY, and one bounded startup/teardown/restart recovery operation | `qemu_smp_production / configs/root_task.toml` |
| `ninedoor` | qemu | canonical QEMU boot, isolated NineDoor READY, and one real 9P operation | `qemu_smp_production / configs/root_task.toml` |
| `console-network` | qemu | canonical QEMU boot, isolated console READY v6, and the fixed one-socket HELP/NETSTATS/SMP/CACHELOG matrix | `qemu_smp_production / configs/root_task.toml` |
| `root-mcs` | qemu | canonical QEMU boot, root steady state, one real target operation, and no unexpected seL4 fault | `qemu_smp_production / configs/root_task.toml` |
| `live-transport` | qemu, pi4 | one authenticated operation over the changed live target transport | `selected target production profile` |
| `python-sdk` | qemu, pi4 | focused Python SDK tests | `host-only Python SDK` |
| `swarmui` | qemu, pi4 | focused SwarmUI package tests | `host-only SwarmUI` |
| `test-plan-tooling` | qemu, pi4 | focused test-plan tooling tests and catalog/document consistency | `test-plan tooling` |
| `docs` | qemu, pi4 | documentation metadata and generated-contract consistency | `generated documentation contracts` |
<!-- test-plan-convergence:end -->

Representative results are normative:

- root task, MCS, IPC, capability, Worker, or service-isolation changes start
  with a QEMU target canary;
- Pi boot, MMIO, DMA, IRQ, timer, cache, driver-runtime ABI, networking, or
  physical ownership changes start with a Pi 4 target canary;
- TCP, REST, gateway, or cohsh changes perform one live operation against the
  selected real target;
- Python SDK and SwarmUI changes run their focused host suite; and
- documentation-only changes run documentation/generated consistency checks.

### Target-entry integrity versus broad host closure

Stage 01 remains intact for compatibility and final acceptance, but its
responsibilities have two different positions during development:

**Target-entry integrity** is the minimum safe pre-target set: validate the
generated contracts used by changed target code, validate the selected
feature/profile, compile the exact target release configuration, run only
cheap required ABI/layout checks, and optionally run one narrow test when it
directly protects the target-entry contract.

**Broad host closure** remains mandatory for acceptance but normally follows a
successful target canary during active target work: workspace-wide tests and
Clippy, complete root/runtime feature suites, SwarmUI, mock `coh`, Python SDK
and examples, unrelated drivers, broad regressions, and dependency/risk/
governance closure. A host-only focus may run its focused host test first
because the host surface itself is the changed execution path.

### QEMU convergence proof order

For root-task behavior, MCS, isolation, Workers, capability/IPC/scheduling,
fault handling, SMP, image construction, or startup, run only:

1. generated contracts and selected configuration required by the change;
2. exact `qemu_smp_production` target compilation;
3. genuinely required cheap ABI/layout/static checks;
4. the immutable canonical Milestone 26e QEMU image boot;
5. `Cohesix console ready` root steady state;
6. the changed service READY marker, or real `WORKER_TASK_READY` for a Worker;
7. one real operation through the changed target path;
8. absence of unexpected seL4 faults, capability errors, scheduler failures,
   timeouts, or runtime panics;
9. one bounded budget/timeout/fault-recovery operation when the change affects
   scheduling or recovery, selected with `--operation-script` when the default
   probe is not the changed recovery path; then
10. the smallest appropriate regression guard; use a host guard only for a
    useful deterministic host-testable invariant.

Stop at the first failed layer. Do not run broad workspace, UI, mock-client,
Python SDK, unrelated driver, or general regression suites before this canary
unless one of those surfaces is itself the selected focus. `--launch-existing`
validates and launches the bound `cohesix-qemu-launch-artifacts.json`; it never
restages or silently rebuilds an immutable diagnostic artifact.

The convergence runner emits `cohesix-target-observation/v2`. In addition to
the UART and QEMU command records, it binds `operation_log` to the exact
authenticated `cohsh` transcript. QEMU evidence consumers require that
transcript to contain remote NineDoor readiness, successful authentication and
Queen attachment, and one successful `CAT`; a boot-only UART marker cannot
stand in for the operation.

The QEMU proof ladder is:

```text
source/config identity -> exact target build -> image validity -> target boot
-> root steady state -> changed service/Worker READY -> one real operation
-> changed failure/recovery path -> focused regression guard
-> broader integration regressions -> pressure/repeatability -> final acceptance
```

### Pi 4 convergence checkpoints and proof order

Use Pi early when QEMU cannot authoritatively model firmware/U-Boot, physical
MMIO/IRQ/timers, DMA/cache coherency, the driver-runtime ABI, physical device
ownership/networking/concurrency, or shared root capability construction that
may differ on hardware. The convergence runner does not discover or overwrite
an SD device and never fabricates physical evidence. Prepare and independently
preserve the exact readback and live boot record, then provide them explicitly:

```bash
scripts/ci/test_plan_converge.sh \
  --target pi4 \
  --focus pi4-driver \
  --pi4-target-evidence out/<run>/target-evidence.json \
  --pi4-readback-image out/<run>/readback.img \
  --pi4-identity-metadata out/<run>/readback.img.identity.json \
  --pi4-serial-log /absolute/path/to/current-nonempty-uart.log \
  --pi4-host <pi-address>
```

The first Pi diagnostic proves only: exact source/image identity; flash/readback
identity; one real boot; root and touched service/device readiness; one real
operation through the selected path; UART liveness across that operation; and
no unexpected fault in the bound boot. Missing hardware inputs produce
`BLOCKED`, never synthetic PASS. Full cold/warm repeatability, pressure, TCP
matrices, RF claims, benchmark, and hardware qualification remain later
acceptance activities.

Milestone 26e requires lightweight Pi checkpoints:

1. after the first complete MCS root boot is stable under QEMU;
2. after isolated critical services work under QEMU;
3. after Worker loading and fault recovery work;
4. before resource/capability/topology ABI assumptions are frozen; and
5. before Milestone 26e acceptance.

Each checkpoint is a new image/source-bound Pi observation, not permission to
reuse old hardware proof. The first four may use the lightweight convergence
lane. The fifth must be followed by the complete required Pi qualification and
acceptance evidence. Follow the canonical build -> flash -> readback -> boot ->
saved boot/profile policy -> device/network proof -> console/liveness proof ->
target-qualified Test Plan -> benchmark/repeatability ladder in
[HARDWARE_BRINGUP.md](HARDWARE_BRINGUP.md); this document does not redefine it.

### Rabbit-hole prevention rules

These rules are normative during target convergence:

1. No more than two speculative target-code edits may occur without rerunning
   the relevant QEMU or Pi diagnostic.
2. A target failure overrides host PASS results when diagnosing target
   behavior.
3. Do not add broad tests while the target remains red unless a new test
   distinguishes one specific observed target hypothesis.
4. Stop at the first failed proof layer. Do not debug TCP while boot, image
   identity, capability construction, service readiness, IRQ delivery, or an
   earlier layer remains unresolved.
5. Do not optimize or broadly refactor a failing path before its target failure
   mechanism is understood.
6. Every target fix must identify the observed target failure, hypothesis, code
   change, and target observation that proves or disproves the hypothesis.
7. Once the target fix is proved, preserve its smallest independently understood
   invariant with an appropriate regression guard or target evidence. A new host
   test is not mandatory for every target defect.
8. Unit tests are not authoritative evidence for live scheduling, capability
   installation, real IPC, IRQ delivery, DMA/cache correctness, or physical
   device behavior.

### Candidate collection, validation, and acceptance promotion

Target observations may be collected early as candidate convergence evidence
and validated for schema, source, image, profile, target, action-log, and UART
integrity. They are never promoted in place. Acceptance evidence is created
only by a new complete staged run after all required stages pass and current
source, image, target/profile/topology, pressure, repeatability, and hardware
identities still match. A stale convergence result, even a PASS, cannot become
accepted Milestone 26e evidence.


## Mandatory Acceptance Execution Contract

This contract is normative whenever a milestone, release, or claim-tier result
is being produced. It is unchanged by the development convergence lane.

1. Run `scripts/ci/test_plan_run.sh --list`, then use the staged runner with a
   dedicated state directory. The runner resumes digest-valid evidence by
   default:
   - `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/<run-id>`
   - `scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/<run-id>`
2. Use `--force` to replace an active stage result after a failure or input
   change. The old immutable attempt remains under `evidence/`; a failed,
   interrupted, or INCOMPLETE rerun cannot leave a reusable PASS marker.
3. Use `--stage <n> --iteration` only for focused debugging. Iterations have a
   separate evidence namespace and never write, remove, or refresh full-pass
   attestations.
4. Use `--reuse-common-from <state-dir>` only for source-, configuration-,
   toolchain-, catalog-, and artifact-identical common evidence. Target-specific
   actions are never relabelled across QEMU and Pi 4. Missing or legacy
   provenance fails closed.
5. Fix a failed stage before progressing. Skips write INCOMPLETE evidence and
   fail; a platform check may be `NA` only where the catalog explicitly permits
   it.
6. Keep `configs/test_plan_actions.toml`, this document, and the scripts aligned.
   `scripts/ci/check_test_plan.sh` must pass.
7. Before making a claim for a change set, select its conditional tiers from
   the catalog without losing paths that contain whitespace: `git diff
   --name-only -z <base> -- | python3 scripts/ci/test_plan_catalog.py recommend
   --stdin0 --format tiers`.
   Preserve the reported action IDs with the run evidence. An unmatched path
   selects every catalog action, and a conditional tier cannot be reported
   unless its named evidence action has also passed.
8. Keep the developer host responsive. The runner defaults build, libtest, and
   Rayon concurrency to half the detected logical CPUs (maximum six); on the
   10-core macOS development host this is five jobs. Playwright defaults to two
   workers. Set `TP_HOST_JOBS=<n>` or `TP_UI_WORKERS=<n>` to lower the cap.
   Oversubscription requires the explicit `TP_ALLOW_OVERSUBSCRIBE=1` opt-in.
   The Pi image builder consumes the same budget when invoked from a staged or
   conditional test-plan run.

The five staged entrypoints remain:

- Stage 01: `scripts/ci/test_plan_stage_01_integrity.sh`
- Stage 02: `scripts/ci/test_plan_stage_02_host_fast.sh`
- Stage 03: `scripts/ci/test_plan_stage_03_qemu_tcp_regression.sh`
- Stage 04: `scripts/ci/test_plan_stage_04_rest_multiplexer.sh`
- Stage 05: `scripts/ci/test_plan_stage_05_due_diligence.sh`

### Milestone 26e executable enforcement

Requirements must have an executable owner, not a second checklist document.
The stage scripts consume the catalog; the milestone runner calls the strict
evidence validators after all five stage attestations verify.

| Boundary | Executable enforcement |
| --- | --- |
| Deterministic contracts and feature coverage | Stage 01 executes the catalog's complete host suites and Python discovery, including compiler admission, ABI/layout, service/Worker/driver contracts, and the distinct `host.console-network-direct-genet` suite. `check_driver_test_coverage.py` checks the declared driver-suite mapping. |
| Exact target/profile and image | Stage 02 validates the selected seL4 profile and target build; Stage 03's `qemu_artifact.py` validators bind launch/image/source bytes or the supplied Pi transport identity. Physical readback remains a separate hardware gate. |
| Live protocol behavior | Stage 03 runs and validates the complete selected TCP matrix; Stage 04 runs the live REST batches and Python smoke on the bound target. Neither substitutes for fault, pressure, or hardware evidence. |
| Worker, critical-TCB and full-system outcomes | `test_plan_run.sh --m26e-evidence-kind component\|root\|system` invokes `worker_task_evidence.py` with explicit inputs. Its validators require the exact outcome matrices, generated topology, role integrations, raw-evidence hashes, and target/session bindings. Missing or partial input prevents that acceptance record. |
| Physical proof and performance | Conditional F uses `pi4_gate_proof.sh`, `pi4_trace_normalize.py`, image-identity and repeatability validators over real captures. Conditional B2 and the qualified Pi lane use the pressure collectors, `rest_perf_harness.py`, and target comparator. These are additional claim gates, not automatic work performed by an ordinary five-stage run. |
| Governance and release promotion | Stage 05 revalidates prior attestations and current due diligence. `worker_task_evidence.py promote-release` validates all six QEMU/Pi component/root/system records; `release_bundle.sh` and `release_qualify.py` separately validate the required projections and delivered artifacts. |

Ordinary Stage 01-05 PASS is not Milestone 26e PASS. Every applicable
conditional, component, root, system, Python, hardware, pressure/repeatability,
and release gate must also be complete. A script validates supplied physical
observations; it cannot manufacture a Pi boot, keyboard input, HDMI observation,
or provider result. Keep those collection procedures and pass criteria here
and in HARDWARE_BRINGUP, not in an auxiliary test inventory.

When a scoped requirement lacks a test, validator, or required physical
observation, record the enforcement gap under its BUILD_PLAN task and block
the affected acceptance claim until it is closed. Do not label a prose rule,
unexecuted action, diagnostic PASS, or historical candidate record as enforced
acceptance. Add a missing check to its owning suite/catalog or validator, with
a focused regression for the observed gap; do not duplicate the whole stage.

Milestone 26e host-integration inventory and evidence use the generated
[`host-integration-dependency/v1`](../configs/generated/host_integration_dependency.json)
graph. `scripts/ci/check_host_integration_inventory.py` checks exhaustive host
surface, six-scenario, and nine-playbook coverage. The bounded runner is:

```bash
scripts/ci/host_integration_run.sh \
  --matrix configs/host_integration_acceptance.toml \
  --matrix-only \
  --state-dir out/host-integration/m26e-matrix
```

Target-session lanes accept `qemu` or `pi4` only, reject stale or wrong-target
identity, and emit one `cohesix-worker-integration-evidence/v1` record per
dependency row. The mandatory target rows are exactly `worker-control`,
`gpu-receipt-path`, and `peft-receipt-path`; external provider observations
remain separate and cannot be promoted by receipt fixtures.

The target runner consumes target proof and row observations as separate,
caller-produced records; launching QEMU does not synthesize either record:

```bash
python3 scripts/worker_task_evidence.py emit-qemu-target-session \
  --repo-root . \
  --qemu-out out/cohesix \
  --resolved-manifest configs/generated/root_task_resolved.json \
  --topology configs/generated/root_task_topology.json \
  --out-dir out/<run>/session
```

The standalone emitter takes no digest arguments. It verifies the immutable
QEMU launch record and launch bytes, exact Worker and driver archive/manifests,
stable git-visible source bytes, Worker ABI sources, and generated-topology /
resolved-manifest parity before creating a new output directory. It refuses
aliases, drift, malformed archives, unignored in-repository output, and any
existing output directory. On PASS it atomically publishes exactly
`source-inventory.json`, `worker-abi-identity.json`,
`qemu-cyw43-coexistence.json`, and `target-session.json`; none is direct target
execution or acceptance evidence.

```bash
scripts/ci/host_integration_run.sh \
  --matrix configs/host_integration_acceptance.toml \
  --mode live \
  --target qemu \
  --target-session out/<run>/session/target-session.json \
  --observations out/<run>/host-integration-observations.json \
  --state-dir out/host-integration/<fresh-run>
```

`target-session.json` is an exact object containing `target` plus lowercase
SHA-256 fields `source_sha256`, `manifest_sha256`, `kernel_sha256`,
`root_image_sha256`, `driver_archive_sha256`, `driver_manifest_sha256`,
`cyw43_coexistence_record_sha256`, `worker_archive_sha256`,
`worker_image_manifest_sha256`, and `worker_abi_sha256`. The observations record uses
`cohesix-host-integration-observations/v1`, binds the exact dependency-graph
and resolved-manifest hashes, and supplies one mode, bounded outcome list, and
sorted raw-evidence list for every selected row. Capability material and
secrets are rejected; output state directories must be empty.

### Milestone 26e direct target acceptance evidence

Worker-component and root-TCB acceptance are separate from the normal staged
claim tiers. A QEMU launch, reachable gateway, stage marker, integration mock,
or packaged image never creates either record. The staged runner emits a 26e
record only after Stages 01-05 have independently passed and the caller supplies
the complete direct-observation inputs through `--m26e-evidence-kind`.

For clean live-QEMU acceptance builds, use a dedicated disposable checkout
with fresh `target/` and `out/` directories, then build through the canonical
GICv3 script. Keep retained evidence outside that checkout. Never delete the
working repository's `out/`, logs, or immutable attempts to obtain a clean run.
The build and artifact validators must reject stale ELF/archive/manifest input:

```bash
scripts/cohesix-build-run.sh \
  --clean \
  --cargo-target aarch64-unknown-none \
  --no-run
out/toolchain/arm-gnu-toolchain-15.2.rel1-darwin-arm64-aarch64-none-elf/bin/aarch64-none-elf-gdb \
  --version
```

The script rejects any machine/GIC override and builds QEMU evidence symbols
only for the selected `release-qemu,bootstrap-trace` profile. Keep the
unstripped ELFs under `target/aarch64-unknown-none/release/`, the Worker archive
and manifest under `out/cohesix/worker-images/`, and the canonical external
driver archive at
`out/cohesix/driver-runtimes/cohesix-driver-runtimes.cpio`. The driver archive
is byte-verified inside rootserver but is intentionally not duplicated in the
system CPIO. Rootserver also retains exactly one byte-identical Worker archive
and manifest for target loading; their system-CPIO copies remain the host and
release projection. A build fails if either embedded payload differs from the
validated source or target code treats typed BootInfo FDT bytes as a CPIO.

Capture each boot through macOS `script`. The ordinary preflight boot remains
running while the operator drives Worker turns from another terminal; terminal
service injection uses the separate fresh boots defined below:

```bash
RUN=out/m26e-qemu
mkdir -p "$RUN/preflight"
script -q "$RUN/preflight/uart.log" \
  scripts/cohesix-build-run.sh \
    --cargo-target aarch64-unknown-none \
    --raw-qemu \
    -- \
    -pidfile "$RUN/preflight/qemu.pid" \
    -gdb tcp:127.0.0.1:1234
```

Run `qemu-gdb` once for each role, preserving argument order. Each invocation
stays attached for three generations: the operator spawns the role for the
pre-READY fault, recreates it and submits an approved `kill` for the
during-IPC standard fault, then repeats that lifecycle call for MCS budget
exhaustion before creating a final READY instance. The instrumented
`cohesix_worker_qemu_evidence_call_dispatch` hook runs after validating the
received call and before its dispatch or reply. All three passive roles can
therefore exercise the real shutdown IPC path; Heartbeat needs no autonomous
publish turn. The runner reads the exact READY identity through the Python
SDK over direct TCP and publishes fresh GPU fixture inventory before each GPU
spawn. These direct sessions close before the gateway first attaches. The
separate seven-action matrix continues to use real v2 host tickets and receipts.

```bash
GDB=out/toolchain/arm-gnu-toolchain-15.2.rel1-darwin-arm64-aarch64-none-elf/bin/aarch64-none-elf-gdb
COMMON_GDB="--gdb $GDB --remote 127.0.0.1:1234 --target-session $RUN/session/target-session.json --generated-inventory configs/generated/root_task_topology.json --worker-image-manifest out/cohesix/worker-images/cohesix-worker-image-manifest.json"

python3 scripts/worker_task_evidence.py qemu-gdb $COMMON_GDB \
  --worker-elf worker-heartbeat=target/aarch64-unknown-none/release/worker-heart \
  --worker-elf worker-gpu=target/aarch64-unknown-none/release/worker-gpu \
  --worker-elf worker-lora=target/aarch64-unknown-none/release/worker-lora \
  --inject-role worker-heartbeat \
  --out "$RUN/preflight/gdb-worker-heartbeat.log"
python3 scripts/worker_task_evidence.py qemu-gdb $COMMON_GDB \
  --worker-elf worker-heartbeat=target/aarch64-unknown-none/release/worker-heart \
  --worker-elf worker-gpu=target/aarch64-unknown-none/release/worker-gpu \
  --worker-elf worker-lora=target/aarch64-unknown-none/release/worker-lora \
  --inject-role worker-gpu \
  --out "$RUN/preflight/gdb-worker-gpu.log"
python3 scripts/worker_task_evidence.py qemu-gdb $COMMON_GDB \
  --worker-elf worker-heartbeat=target/aarch64-unknown-none/release/worker-heart \
  --worker-elf worker-gpu=target/aarch64-unknown-none/release/worker-gpu \
  --worker-elf worker-lora=target/aarch64-unknown-none/release/worker-lora \
  --inject-role worker-lora \
  --out "$RUN/preflight/gdb-worker-lora.log"

COH_AUTH_TOKEN="$QUEEN_TOKEN" \
TEST_PLAN_CONVERGENCE_QEMU_OUT_DIR=out/cohesix \
python3 scripts/ci/test_plan_converge.py \
  --target qemu --focus ninedoor --launch-existing \
  --state-dir "$RUN/authenticated-ninedoor"

SERVICE_GDB="--gdb $GDB --remote 127.0.0.1:1234 --target-session $RUN/session/target-session.json --generated-inventory configs/generated/root_task_topology.json --qemu-out out/cohesix --auth-observation $RUN/authenticated-ninedoor/target-observation.json"
python3 scripts/worker_task_evidence.py qemu-service-gdb $SERVICE_GDB \
  --service ninedoor-service --mode during-call-standard \
  --service-elf target/aarch64-unknown-none/release/nine-door-runtime \
  --out "$RUN/ninedoor-during-call/service.gdb.log"
python3 scripts/worker_task_evidence.py qemu-service-gdb $SERVICE_GDB \
  --service ninedoor-service --mode between-calls-revoke \
  --service-elf target/aarch64-unknown-none/release/nine-door-runtime \
  --root-elf target/aarch64-unknown-none/release/root-task \
  --out "$RUN/ninedoor-between-calls/service.gdb.log"
python3 scripts/worker_task_evidence.py qemu-service-gdb $SERVICE_GDB \
  --service console-network --mode during-call-standard \
  --service-elf target/aarch64-unknown-none/release/console-network-runtime \
  --out "$RUN/console-standard-fault/service.gdb.log"
```

Each NineDoor command above attaches to its own fresh exact-artifact boot; its
matching `service.uart.log` is frozen after terminal teardown. The first is
triggered by one authenticated ordinary Secure9P Call. The between-Calls probe
resolves root-local evidence hooks by their exact defined, demangled Rust
symbols; those hooks remain deliberately non-exported and the collector must
not require a global control symbol. It counts two
successful root post-prepare returns and requests local revoke between them and
the next Call. Console-network Standard injection uses its own fresh
exact-artifact boot because its containment is terminal and has no same-boot
replacement. It is triggered by one authenticated control turn; the runner
neither waits for reconstruction nor attempts a second child handler after
teardown. Natural-postpone budget liveness is exercised under the retained
pressure boots rather than by the obsolete terminal timeout-spin injection. No
VM command or namespace fault-injection authority exists.

The four critical-duty observation hooks occur during startup, so collect them
on a separate halted boot of the exact same image/session. The collector first
continues to a fifth, post-SMP arm hook immediately before any restricted TCB
resumes, then replaces that breakpoint with the four duty breakpoints. This
post-secondary-core re-arm is required because accelerator hardware-debug
state can be reset while seL4 initializes secondary cores. It changes no guest
scheduling, budget, capability, or service behavior:

The five `release-qemu,bootstrap-trace` hooks carry distinct opaque identity tags so
release linking cannot fold separate duty addresses together. They perform no
I/O, scheduling, or authority change; the collector rejects a missing or
aliased address before it attaches GDB.

```bash
mkdir -p "$RUN/critical"
script -q "$RUN/critical/uart.log" \
  scripts/cohesix-build-run.sh \
    --cargo-target aarch64-unknown-none \
    --raw-qemu \
    -- \
    -pidfile "$RUN/critical/qemu.pid" \
    -gdb tcp:127.0.0.1:1234 \
    -S
python3 scripts/worker_task_evidence.py qemu-critical-gdb \
  --gdb "$GDB" --remote 127.0.0.1:1234 \
  --target-session "$RUN/session/target-session.json" \
  --generated-inventory configs/generated/root_task_topology.json \
  --root-elf target/aarch64-unknown-none/release/root-task \
  --out "$RUN/preflight/gdb-critical-duties.log"
```

After the same-boot integration records and the exact 7-by-3 receipt matrix are
present, derive the component needed by the gateway before pressure. The
collector treats cohsh `OK SPAWN`/`OK KILL` only as admission outcomes; READY,
artifact, receipt, and proof axes come from identity-bound UART/pressure records,
never from caller-supplied projection text.

```bash
python3 scripts/worker_task_evidence.py collect-qemu-preflight \
  --target-session "$RUN/session/target-session.json" \
  --generated-inventory configs/generated/root_task_topology.json \
  --qemu-out out/cohesix \
  --auth-observation "$RUN/authenticated-ninedoor/target-observation.json" \
  --uart "$RUN/preflight/uart.log" \
  --cohsh "$RUN/preflight/cohsh.log" \
  --gdb-log "$RUN/preflight/gdb-worker-heartbeat.log" \
  --gdb-log "$RUN/preflight/gdb-worker-gpu.log" \
  --gdb-log "$RUN/preflight/gdb-worker-lora.log" \
  --service-gdb-log "$RUN/ninedoor-during-call/service.gdb.log" \
  --service-gdb-log "$RUN/ninedoor-between-calls/service.gdb.log" \
  --service-gdb-log "$RUN/console-standard-fault/service.gdb.log" \
  --service-uart "$RUN/ninedoor-during-call/service.uart.log" \
  --service-uart "$RUN/ninedoor-between-calls/service.uart.log" \
  --service-uart "$RUN/console-standard-fault/service.uart.log" \
  --critical-gdb-log "$RUN/preflight/gdb-critical-duties.log" \
  --worker-archive out/cohesix/worker-images/cohesix-worker-images.cpio \
  --driver-archive out/cohesix/driver-runtimes/cohesix-driver-runtimes.cpio \
  --worker-image-manifest out/cohesix/worker-images/cohesix-worker-image-manifest.json \
  --worker-elf worker-heartbeat=target/aarch64-unknown-none/release/worker-heart \
  --worker-elf worker-gpu=target/aarch64-unknown-none/release/worker-gpu \
  --worker-elf worker-lora=target/aarch64-unknown-none/release/worker-lora \
  --service-elf ninedoor-service=target/aarch64-unknown-none/release/nine-door-runtime \
  --service-elf console-network=target/aarch64-unknown-none/release/console-network-runtime \
  --root-elf target/aarch64-unknown-none/release/root-task \
  --integration-dir "$RUN/integration" \
  --out-dir "$RUN/preflight-component"
```

After separate fresh medium- and high-pressure boots, freeze each boot-local
UART/GDB pair before writing its summary and run the final semantic collector:

```bash
python3 scripts/worker_task_evidence.py collect-qemu \
  --target-session "$RUN/session/target-session.json" \
  --generated-inventory configs/generated/root_task_topology.json \
  --qemu-out out/cohesix \
  --auth-observation "$RUN/authenticated-ninedoor/target-observation.json" \
  --preflight-uart "$RUN/preflight/uart.log" \
  --preflight-gdb-log "$RUN/preflight/gdb-worker-heartbeat.log" \
  --preflight-gdb-log "$RUN/preflight/gdb-worker-gpu.log" \
  --preflight-gdb-log "$RUN/preflight/gdb-worker-lora.log" \
  --preflight-service-gdb-log "$RUN/ninedoor-during-call/service.gdb.log" \
  --preflight-service-gdb-log "$RUN/ninedoor-between-calls/service.gdb.log" \
  --preflight-service-gdb-log "$RUN/console-standard-fault/service.gdb.log" \
  --preflight-service-uart "$RUN/ninedoor-during-call/service.uart.log" \
  --preflight-service-uart "$RUN/ninedoor-between-calls/service.uart.log" \
  --preflight-service-uart "$RUN/console-standard-fault/service.uart.log" \
  --preflight-critical-gdb-log "$RUN/preflight/gdb-critical-duties.log" \
  --uart "$RUN/medium/uart.log" --gdb-log "$RUN/medium/gdb.log" \
  --pressure "$RUN/medium/pressure.summary.json" \
  --uart "$RUN/high/uart.log" --gdb-log "$RUN/high/gdb.log" \
  --pressure "$RUN/high/pressure.summary.json" \
  --cohsh "$RUN/cohsh.log" \
  --worker-archive out/cohesix/worker-images/cohesix-worker-images.cpio \
  --driver-archive out/cohesix/driver-runtimes/cohesix-driver-runtimes.cpio \
  --worker-image-manifest out/cohesix/worker-images/cohesix-worker-image-manifest.json \
  --worker-elf worker-heartbeat=target/aarch64-unknown-none/release/worker-heart \
  --worker-elf worker-gpu=target/aarch64-unknown-none/release/worker-gpu \
  --worker-elf worker-lora=target/aarch64-unknown-none/release/worker-lora \
  --service-elf ninedoor-service=target/aarch64-unknown-none/release/nine-door-runtime \
  --service-elf console-network=target/aarch64-unknown-none/release/console-network-runtime \
  --root-elf target/aarch64-unknown-none/release/root-task \
  --integration-dir "$RUN/integration" \
  --run-dir "$RUN/test-plan" \
  --out-dir "$RUN/accepted"
```

The QEMU-only GPU bridge snapshot remains `source=fixture`, `mode=fixture`,
`profile=qemu`, `gate=bootstrap-trace`; it never becomes provider-live or
production evidence. That same admitted snapshot projects exactly one
read-only LoRA export job, `qemu-evidence-job`, containing only
`telemetry.cbor`, `base_model.ref`, and `policy.toml`. Both fixtures disappear
outside the explicit QEMU evidence gate. Publish it only through the existing
bridge path:

```bash
cargo run -p gpu-bridge-host --features rest -- \
  --mock \
  --publish \
  --rest-url "$COHESIX_GATEWAY_URL" \
  --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN"
```

The collector rejects missing,
reordered, changed, symlinked, target-mismatched, or secret-bearing inputs and
does not treat reachability or build markers as live proof.

The QEMU Worker-component invocation is:

```bash
scripts/ci/test_plan_run.sh \
  --target qemu \
  --state-dir out/test-plan/m26e-worker-qemu \
  --m26e-evidence-kind component \
  --m26e-target-session out/m26e-qemu/target-session.json \
  --m26e-generated-inventory configs/generated/root_task_topology.json \
  --m26e-observations out/m26e-qemu/worker-component-observations.json \
  --m26e-integration-dir out/host-integration/m26e-qemu/integration
```

`target-session.json` uses the exact hash object documented above. The
observations file uses `cohesix-worker-component-observations/v1` and contains
exactly `schema`, `target`, `target_session_sha256`, `workers`, `outcomes`,
`raw_evidence`, `verdict`, and `blockers`. Each of the three Worker rows is one
bounded live Heartbeat/GPU/LoRA exemplar and records its five-part identity,
state axes, image hash, READY/completion sequences, distinct endpoint/fault
badges, core, exact passive scheduling-context values `0/0`, and full per-slot
compiler-admission object inventory: TCB, CNode, VSpace, page table, ASID,
frame, endpoint, notification, standard/timeout fault cap, Reply, scheduling
context, CSpace slot, and untyped-byte totals. The emitter recomputes the
compiler topology digest and derives the maximum-role inventory from the
topology payload. It then requires each observed role's attach badge, fault
badge, core, passive scheduling context, per-slot object inventory, allowlisted
active executor donor, and generation-scoped Reply path to equal that generated
truth. A fresh passive Heartbeat may have completion sequence zero: it has
committed READY but has received no workload Call. The collector requires no
uncompleted control/lifecycle Call for that live identity; the separate
Heartbeat fault, shutdown, teardown and recreation proofs remain mandatory.
GPU and LoRA require a positive completion sequence and a confirmed receipt.
Separately, the topology digest seals all 256 Worker task rows and every
generated non-Worker row. `temporal_authority.tasks` may exceed the generic
128-item evidence-list bound only when its exact length, order, identifiers,
kinds, role classes, driver images, and fault-registry counts are derived from
that compiler topology; every other arbitrary list retains the generic bound.
A component PASS also requires the complete, sorted 26e event
matrix and the exact live `gpu-receipt-path`, `peft-receipt-path`, and
`worker-control` integration records. The emitter binds the observation,
generated-topology, and target-session input bytes into the resulting
raw-evidence graph and writes `worker-task-evidence.json` atomically only after
validation. It also preserves the exact three referenced integration-record
bytes under the output state directory's `integration/` subtree so downstream
recursive validation does not depend on an unrecorded external directory.

The QEMU root-TCB invocation is:

```bash
scripts/ci/test_plan_run.sh \
  --target qemu \
  --state-dir out/test-plan/m26e-root-tcb-qemu \
  --m26e-evidence-kind root \
  --m26e-target-session out/m26e-qemu/target-session.json \
  --m26e-worker out/test-plan/m26e-worker-qemu/worker-task-evidence.json \
  --m26e-generated-inventory configs/generated/root_task_topology.json \
  --m26e-observations out/m26e-qemu/root-tcb-observations.json
```

`coh-rtc` emits `configs/generated/root_task_topology.json` beside the selected
resolved manifest. Its `cohesix-root-tcb-generated-inventory/v1` envelope has
exactly `schema`, `profile`, `manifest_sha256`, `topology_sha256`, `topology`,
and `inventory`. `topology_sha256` is the SHA-256 of deterministic compact JSON
for the compiler-owned profile, root/driver topology, Worker runtime, temporal
authority, resource admission, NineDoor service, and console-network service.
The inventory is the admitted maximum: the exact fixed-object budget plus every
executable slot in the one compiler-validated maximum role mix. It is not a
kernel allocation or retype census. `scripts/check-generated.sh`
compares the canonical QEMU output byte-for-byte. The direct input uses
`cohesix-root-tcb-observations/v1` with the target-session digest and same
topology, `inventory_scope=admitted-maximum`, the UART-projected admitted
maximum, complete containment and operator-liveness
outcomes, raw-artifact descriptors, and verdict. A PASS is impossible when the
generated topology hash cannot be recomputed, its inventory cannot be derived,
generated and projected admitted-maximum inventories differ, or the accepted Worker record names
a different target session or topology. The resulting
`root-tcb-acceptance.json` binds all three input files by digest.
The strict release inventory packages this topology beside the resolved
manifest; `scripts/release_bundle.sh --check-manifest --pi4-stage-dir <path>` rejects either an
omission or an unexpected compiler-owned generated file rather than silently
shipping a partial as-built contract.
`ROOT_CRITICAL_OBJECTS scope=constructed-actual` separately records seven
constructed critical TCBs: the five root duties plus the two active Worker
executor lanes. Six are restricted children. Its active SC/Reply counts and
installed standard/timeout fault-cap counts form the bounded actual critical-
domain census and remain distinct from the complete generated fault-registry
capacity; neither is inferred from the admitted maximum.

Full-system evidence remains a verification-only layer over immutable accepted
component/root records. Its explicit `cohesix-mcs-smp-run-input/v1` observation
contains the target session, exact component/root digests, topology, four-core
admission rows, the complete timeout/fault/Reply/liveness/performance outcome
matrix, and raw evidence. It can be emitted after a full staged run with:

```bash
scripts/ci/test_plan_run.sh \
  --target qemu \
  --state-dir out/test-plan/m26e-mcs-smp-qemu \
  --m26e-evidence-kind system \
  --m26e-worker out/test-plan/m26e-worker-qemu/worker-task-evidence.json \
  --m26e-root out/test-plan/m26e-root-tcb-qemu/root-tcb-acceptance.json \
  --m26e-observations out/m26e-qemu/mcs-smp-system-input.json
```

Missing, empty, wrong-target, stale, non-live, hash-mismatched, partial, or
secret-bearing input fails before an acceptance record is published. Omitting
`--m26e-evidence-kind` runs the ordinary staged plan but emits no 26e acceptance
PASS. The same commands accept `--target pi4` only with independent fresh-Pi
target-session, observations, integration records, and raw artifacts; QEMU
files cannot be relabelled. Runtime release promotion still requires all six
validated QEMU/Pi component, root, and full-system records, so QEMU-first work
cannot produce Pi or Worker-runtime release acceptance.

The controlled Milestone 26e refresh of tracked `seL4/build_UBOOT` is complete:
it is a source-bound `pi4_production` SMP+MCS artifact set, and validation
requires its exact contract hash, generated configuration, 4-node/16-bit-root-
CNode profile, 54 MHz timer provenance, required artifacts, and complete
tracked tree. Static profile validation, deterministic host composition, and a
stage-only image build prove only their stated build and packaging contracts;
none is Pi boot, hardware-driver, network, performance, repeatability, or
acceptance evidence. Independent exact-image, fresh-target, pressure, and
hardware gates must still pass before any Pi acceptance command above may pass.

### Milestone 26e Python package and target projection

Build one target-neutral wheel, inspect its exact module and extras manifest,
and install it without dependency resolution into isolated CPython 3.11 and
3.13 environments:

```bash
python3 -m pip wheel --no-deps \
  --wheel-dir out/python-wheels tools/cohesix-py
scripts/ci/python_compat_run.sh \
  --wheel-smoke \
  --wheel-dir out/python-wheels \
  --package-manifest out/python-compat/m26e-python-package.json \
  --state-dir out/python-compat/m26e-wheel
```

The wheel must contain only target-neutral defaults. The package manifest
binds that wheel to independently compiler-generated QEMU and Pi 4
`cohesix-python-profile/v1` contracts. A successful install or mock Worker
observation remains host-model compatibility evidence, not target authority,
READY proof, provider completion, runtime release acceptance, or production
use-case acceptance.

After the direct QEMU role gate has emitted an accepted, live
`cohesix-worker-integration-evidence/v1` record for `worker-control`,
`gpu-receipt-path`, or `peft-receipt-path`, run the QEMU projection lane:

```bash
scripts/ci/python_compat_run.sh \
  --python-matrix 3.11,3.13 \
  --target qemu \
  --profile-contract configs/generated/cohesix_python_qemu_smp_production.json \
  --wheel-dir out/python-wheels \
  --package-manifest out/python-compat/m26e-python-package.json \
  --matrix configs/host_integration_acceptance.toml \
  --target-session out/<run>/worker-control.json \
  --state-dir out/python-compat/m26e-qemu
```

The result is the release-required `python-sdk-projection` row. It consumes the
direct role record and copies its exact target-session identities; it does not
replace that record or raise its proof class. The Pi 4 invocation uses the Pi
profile contract and an independently accepted fresh-Pi role record. Do not run
or report the Pi lane from QEMU evidence.

Stage 01 runs integrity first and then one broad host suite per distinct
feature configuration. Its common-hermetic attestation may be imported into a
second target state directory with `--reuse-common-from`. Stage 02 runs only
the selected provisioned-target profile and release checks. Stage 05 verifies
the immutable Stage 01-04 attestations and runs only unique release governance.
Direct `scripts/ci/due_diligence_gate.sh` execution remains exhaustive.

## Claim tiers and PASS terminology

Never report an unqualified “Test Plan PASS.” Report the exact claim tier(s):

| Claim tier | What a PASS proves | What it does not prove |
| --- | --- | --- |
| `common-hermetic` | Catalog integrity, generated contracts, formatting, lint, workspace/default tests, complete production-feature host suites, Python discovery, no-std Pi runtime compile, and risk ratchet. | Provisioned root-task target builds, QEMU boot, live transport, Pi hardware, performance, UI, federation, or bundles. |
| `qemu-integration` | `common-hermetic` plus the provisioned QEMU profile/release check, content-bound QEMU artifacts, and fresh Stage 03/04 boots with TCP and REST regression results. | Pi transport or hardware. |
| `pi4-transport` | `common-hermetic` plus the provisioned Pi profile/release check and TCP/REST results bound to one caller-supplied Pi target/boot/image evidence record. | Reflash/readback, RF, driver ownership, repeatability, benchmark, or hardware acceptance. |
| `pi4-hardware` | A separate machine-validated bundle containing image/readback identity, fresh serial proof, capture manifest, driver-task proof, and required repeatability report. | Performance unless the performance tier also passes. |
| `ui` | Deterministic replay-mode Playwright presentation and transcript coverage. | Control-plane authority or protocol correctness. |
| `performance` | Named no-retry/error-budget matrices with reviewable summaries. | General functional or hardware acceptance. |
| `federation` | Named three-hive relay, dedupe, WAL resume, failover, timeline, and scale evidence. | Unrelated release or hardware claims. |
| `release` | Unique advisory/governance checks; bundle validation is an additional release action when bundles are shipped. | Any target tier not explicitly included in the result. |

The normal five-stage QEMU run produces `common-hermetic`,
`qemu-integration`, and repository `release` governance evidence. The normal
five-stage Pi run produces `common-hermetic`, `pi4-transport`, and repository
`release` governance evidence. It must never be described as `pi4-hardware`
without the separate hardware bundle.

Conditional UI, performance, federation, Pi hardware, and bundle actions are
selected by their catalog `trigger_paths` or by the active milestone. An
unknown changed path selects the complete catalog conservatively.

## Immutable evidence and resume contract

Each stage attempt records:

- the Git HEAD plus every tracked and non-ignored untracked source file,
  including mode and submodule state;
- `Cargo.lock`, selected manifests/generated outputs, non-secret selectors,
  selected seL4 profile identity, toolchain versions, OS, target, and exact
  action-catalog digest;
- redacted argv, exit status, start/end timestamps, duration, and hashed logs
  for every action;
- required artifacts and their content hashes, including the exact QEMU image
  or caller-supplied Pi target evidence where applicable; and
- a terminal immutable stage manifest published atomically only after all
  assertions pass.

`stage_XX.attestation` is an atomic reference to the immutable manifest.
Compatibility `.done` files are not authority and are published only after the
attestation verifies. Missing/malformed provenance, changed inputs, tampered
logs/actions/artifacts, an iteration result, target mismatch, or a failed
attempt blocks resume. `target.env` is created once and cannot be overwritten
with a different target or start identity. A state directory has one writer:
the runner holds its lock across all selected stages, and a concurrent writer
must use a different state directory.

Secrets named like tokens, passwords, tickets, credentials, API keys, or
authorization values are redacted from command logs and structured evidence.
Pass secrets through inherited environment variables; do not interpolate them
into logged shell command strings.

## Target-qualified runner matrix

| Target | Stages | Required target-specific evidence |
| --- | --- | --- |
| `qemu` | 01-05 | Stage 03 builds one immutable artifact per unique manifest, content-binding all eight packaged host executables (`cas-tool`, `coh`, `cohsh`, `gpu-bridge-host`, `hive-gateway`, `host-sidecar-bridge`, `host-ticket-agent`, and `swarmui`), then uses a fresh boot for every regression group. Stage 04 reuses the validated default artifact but starts another fresh boot. Result manifests bind source, profile, manifest, image, scripts, boot identity, counts, and log hashes. |
| `pi4` | 01-05 | Stage 03 requires `COHSH_TCP_HOST` or `COHSH_HOST` plus `TP_PI4_TARGET_EVIDENCE_FILE`; Stage 04 requires an existing gateway URL and evidence binding that gateway to the same boot/image. These stages yield only `pi4-transport`. `TP_PI4_HARDWARE_EVIDENCE_FILE`, when required, must validate the stronger hardware bundle and is never synthesized by the runner. |

A Pi Stage 03 run refuses loopback unless `TP_PI4_ALLOW_LOOPBACK=1` records an
intentional tunnel. A Pi Stage 04 run without an existing gateway fails rather
than creating misleading local-QEMU evidence.

## Canonical action catalog

`configs/test_plan_actions.toml` is the sole staged command inventory. It owns
action IDs, exact commands, feature sets, stages, claim tiers, targets, trigger
paths, timeouts, expected evidence, and zero-test policy.
`scripts/ci/test_plan_catalog.py` validates semantic duplicates and forbids
zero-match-prone filtered library-test actions. The table below is generated
from that catalog.

<!-- test-plan-catalog:start -->
| Action | Stage | Claim tier | Scope / target | Command or proof |
| --- | ---: | --- | --- | --- |
| `integrity.cargo-metadata` | 1 | `common-hermetic` | common / qemu, pi4 | `cargo metadata --locked --no-deps` |
| `integrity.generated-contracts` | 1 | `common-hermetic` | common / qemu, pi4 | `scripts/check-generated.sh` |
| `host.format` | 1 | `common-hermetic` | common / qemu, pi4 | `cargo fmt --all -- --check` |
| `host.clippy` | 1 | `common-hermetic` | common / qemu, pi4 | `CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings` |
| `host.workspace-check` | 1 | `common-hermetic` | common / qemu, pi4 | `CARGO_INCREMENTAL=0 cargo check --workspace` |
| `host.workspace-tests` | 1 | `common-hermetic` | common / qemu, pi4 | `CARGO_INCREMENTAL=0 cargo test --workspace --exclude swarmui --exclude pi4-driver-runtime` |
| `host.swarmui-tests` | 1 | `common-hermetic` | common / qemu, pi4 | `CARGO_INCREMENTAL=0 cargo test -p swarmui` |
| `host.coh-mock-tests` | 1 | `common-hermetic` | common / qemu, pi4 | `cargo test -p coh --features mock` |
| `host.root-task-qemu-features` | 1 | `common-hermetic` | common / qemu, pi4 | `cargo test -p root-task --no-default-features --features driver-tests-qemu --lib -- --test-threads=1 --skip drivers::driver_task_net` |
| `host.root-task-pi4-features` | 1 | `common-hermetic` | common / qemu, pi4 | `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib -- --test-threads=1` |
| `host.root-task-net-console` | 1 | `common-hermetic` | common / qemu, pi4 | `cargo test -p root-task --no-default-features --features net-console --lib -- --test-threads=1` |
| `host.console-network-direct-genet` | 1 | `common-hermetic` | common / qemu, pi4 | `cargo test -p console-network-runtime --features direct-genet` |
| `host.pi4-runtime-tests` | 1 | `common-hermetic` | common / qemu, pi4 | `cargo test -p pi4-driver-runtime -- --test-threads=1` |
| `host.pi4-runtime-target-check` | 1 | `common-hermetic` | common / qemu, pi4 | `cargo check -p pi4-driver-runtime --target aarch64-unknown-none` |
| `host.cache-maintenance-tests` | 1 | `common-hermetic` | common / qemu, pi4 | `cargo test -p root-task --no-default-features --features cache-maintenance --test cache_maintenance` |
| `host.coh-doctor-smoke` | 1 | `common-hermetic` | common / qemu, pi4 | `cargo run -p coh --features mock -- doctor --mock` |
| `host.swarmui-dependency-policy` | 1 | `common-hermetic` | common / qemu, pi4 | `python3 scripts/ci/check_swarmui_dependencies.py` |
| `host.driver-coverage-contract` | 1 | `common-hermetic` | common / qemu, pi4 | `python3 scripts/ci/check_driver_test_coverage.py` |
| `host.python-tests` | 1 | `common-hermetic` | common / qemu, pi4 | `scripts/ci/python_test_gate.sh --tests` |
| `host.python-examples` | 1 | `common-hermetic` | common / qemu, pi4 | `scripts/ci/python_test_gate.sh --examples` |
| `host.rust-risk-bootstrap` | 1 | `common-hermetic` | common / qemu, pi4 | `python3 scripts/ci/test_rust_risk_gate.py` |
| `host.rust-risk-ratchet` | 1 | `common-hermetic` | common / qemu, pi4 | `env -u CARGO_HOME scripts/ci/rust_risk_gate.sh --baseline docs/audit/rust_risk_baseline.toml` |
| `target.qemu-profile` | 2 | `qemu-integration` | provisioned-target / qemu | `"${TEST_PLAN_ROOT}/out/toolchain/sel4-profile-venv/bin/python" scripts/sel4_profile.py validate --profile qemu_smp_production --build-dir "${TEST_PLAN_ROOT}/out/sel4/profile-v2/qemu-smp-production" --require-source --require-artifacts --for-runtime` |
| `target.root-task-qemu-release` | 2 | `qemu-integration` | provisioned-target / qemu | `scripts/ci/test_plan_target_root_check.sh --target qemu --sel4-build "${TEST_PLAN_ROOT}/out/sel4/profile-v2/qemu-smp-production" --profile qemu_smp_production --features release-qemu --timer-clock-hz 24000000` |
| `target.pi4-profile` | 2 | `pi4-transport` | provisioned-target / pi4 | `"${TEST_PLAN_ROOT}/.venv/bin/python" scripts/sel4_profile.py validate --repo-managed --profile pi4_production --build-dir "${TEST_PLAN_ROOT}/seL4/build_UBOOT" --require-artifacts --for-runtime` |
| `target.root-task-pi4-release` | 2 | `pi4-transport` | provisioned-target / pi4 | `scripts/ci/test_plan_target_root_check.sh --target pi4 --sel4-build "${TEST_PLAN_ROOT}/seL4/build_UBOOT" --profile pi4_production --features release-pi4 --timer-clock-hz 54000000` |
| `qemu.tcp-regression` | 3 | `qemu-integration` | target / qemu | `scripts/cohsh/run_regression_batch.sh` |
| `pi4.tcp-regression` | 3 | `pi4-transport` | target / pi4 | `scripts/cohsh/run_regression_batch.sh` |
| `qemu.rest-regression` | 4 | `qemu-integration` | target / qemu | `scripts/ci/test_plan_stage_04_rest_multiplexer.sh` |
| `pi4.rest-regression` | 4 | `pi4-transport` | target / pi4 | `scripts/ci/test_plan_stage_04_rest_multiplexer.sh` |
| `release.unique-governance` | 5 | `release` | target / qemu, pi4 | `scripts/ci/due_diligence_gate.sh` |
| `diagnostic.qemu-canary` | NON-CLAIMING diagnostic | `non-claiming` | conditional / qemu | `scripts/ci/test_plan_target_canary.sh --target qemu` |
| `diagnostic.pi4-canary` | NON-CLAIMING diagnostic | `non-claiming` | conditional / pi4 | `scripts/ci/test_plan_target_canary.sh --target pi4` |
| `diagnostic.guard-root-mcs` | NON-CLAIMING diagnostic | `non-claiming` | conditional / qemu | `cargo test -p root-task --no-default-features --test mcs_activation_order -- --test-threads=1` |
| `diagnostic.guard-worker` | NON-CLAIMING diagnostic | `non-claiming` | conditional / qemu | `cargo test -p root-task --no-default-features --features driver-tests-qemu --test worker_fault_lifecycle -- --test-threads=1` |
| `diagnostic.guard-ninedoor` | NON-CLAIMING diagnostic | `non-claiming` | conditional / qemu | `cargo test -p root-task --no-default-features --test ninedoor_service_isolation -- --test-threads=1` |
| `diagnostic.guard-console-network` | NON-CLAIMING diagnostic | `non-claiming` | conditional / qemu | `cargo test -p console-network-abi && cargo test -p console-network-runtime && cargo test -p console-network-runtime --features direct-genet && cargo test -p root-task --test console_network_service && cargo test -p root-task --test direct_genet_network_phasing && cargo test -p root-task --no-default-features --features driver-tests-qemu isolated_response_lane_pays_exactly_one_ordinary_debt_after_eight_units -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu isolated_help_capture_publishes_complete_body_then_one_terminal -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu isolated_fixed_synchronous_producers_cross_batch_depth_without_end -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu bounded_sync_capture_overflow_emits_only_typed_terminal_and_reconciles_metrics -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu bounded_sync_cache_snapshot_crosses_batch_depth_and_tombstones_on_quiet_cut -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu bounded_sync_response_is_retired_on_exact_identity_loss -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu pinned_network_line_cannot_dispatch_to_a_replacement_connection -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu physical_progress_is_bounded_while_heavy_producers_preserve_network_owner -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu blocked_physical_producer_retains_an_ordered_busy_terminal_and_prompt -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu hal::cache::tests -- --test-threads=1 && cargo test -p root-task --no-default-features --test isolated_virtio_network_phasing -- --test-threads=1 && .venv/bin/python -m pytest -q tests/test_console_network_runtime_packaging.py tests/test_qemu_tcp_response_matrix.py scripts/ci/test_run_regression_batch.py` |
| `diagnostic.guard-pi4-driver` | NON-CLAIMING diagnostic | `non-claiming` | conditional / pi4 | `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib -- --test-threads=1` |
| `diagnostic.guard-pi4-driver-contracts` | NON-CLAIMING diagnostic | `non-claiming` | conditional / pi4 | `cargo test -p root-task --no-default-features --test driver_task_mcs -- --test-threads=1 && cargo test -p coh-rtc --test pi4_profile && cargo test -p console-network-runtime --features direct-genet && cargo test -p root-task --test direct_genet_network_phasing` |
| `diagnostic.guard-live-transport` | NON-CLAIMING diagnostic | `non-claiming` | conditional / qemu, pi4 | `cargo test -p cohsh --no-default-features --features tcp` |
| `diagnostic.python-sdk` | NON-CLAIMING diagnostic | `non-claiming` | conditional / qemu, pi4 | `scripts/ci/python_test_gate.sh --sdk-tests` |
| `diagnostic.test-plan-tooling` | NON-CLAIMING diagnostic | `non-claiming` | conditional / qemu, pi4 | `python3 scripts/ci/test_test_plan_catalog.py && python3 scripts/ci/test_test_plan_converge.py` |
| `ui.swarmui-playwright` | conditional | `ui` | conditional / qemu, pi4 | `scripts/ci/swarmui_ui_gate.sh --run` |
| `performance.gateway-telemetry` | conditional | `performance` | conditional / qemu, pi4 | evidence-only: telemetry-summary-matrix, ops-csv, ramp-csv, ramp-svg |
| `federation.three-hive-relay` | conditional | `federation` | conditional / qemu, pi4 | evidence-only: federation-result-manifest, relay-counter-snapshots, evidence-timeline, scale-summary |
| `pi4.hardware-acceptance` | conditional | `pi4-hardware` | conditional / pi4 | evidence-only: pi4-image-readback-identity, pi4-gate-proof, pi4-capture-manifest, pi4-repeatability-report |
| `release.bundle-validation` | conditional | `release` | conditional / qemu, pi4 | `python3 scripts/release_qualify.py verify --macos-result "${TP_RELEASE_MACOS_RESULT:?}" --linux-result "${TP_RELEASE_LINUX_RESULT:?}" --pi4-result "${TP_RELEASE_PI4_RESULT:?}" --releases-dir "${TP_RELEASE_DIR:?}" --output "${TP_RELEASE_RESULT:?}"` |
<!-- test-plan-catalog:end -->

`performance.gateway-telemetry` may be selected alongside either target, but
its retained evidence is Conditional D's host-model gateway comparator; it is
not QEMU or Pi target-performance evidence.

## GitHub Actions gate mapping

`.github/workflows/ci.yml` is the sole repository-authored workflow and keeps
the stable required check `ci` directly; there is no aggregate fan-in job.

- Per-commit CI is deliberately a small health signal. On a clean macOS 26
  runner it checks locked Cargo resolution and generated contracts, checks the
  shipped host workspace, runs the default workspace tests once, compiles the
  QEMU and Pi root-task feature test binaries without executing their complete
  behavior matrices, and checks `pi4-driver-runtime` for
  `aarch64-unknown-none`.
- Formatting, strict lint/risk ratchets, complete production-feature behavior
  matrices, repository-wide Python tests, Playwright, examples, and target
  execution remain change-focused developer or canonical Test Plan work. They
  are not repeated on every push. A green `ci` job is not `common-hermetic`,
  QEMU, Pi, UI, performance, federation, or release evidence.
- `dependency-audit` runs only on the weekly schedule and explicit manual
  dispatch, using pinned `cargo-audit` and `cargo-deny` versions. Network-fetched
  advisory data therefore remains visible without making ordinary source
  changes depend on it.
- CI retains native GitHub job logs and no custom evidence uploads. Reusable
  provenance-bound evidence still comes only from the staged Test Plan.

## Preflight and guardrails
- `scripts/ci/test_plan_run.sh --list` (verify scripted stage inventory before execution)
- `scripts/ci/check_test_plan.sh`
- If IR or manifest changes: `cargo run -p coh-rtc` then `scripts/check-generated.sh`.
- Ensure `SEL4_BUILD_DIR` points at the validated production SMP kernel build
  (`$REPO/out/sel4/profile-v2/qemu-smp-production` by default). Preserved
  `seL4/build` or `seL4/SMP_build` trees may be selected explicitly for
  diagnostic comparison only and are claim-ineligible unless they pass a named
  profile contract.
- Default QEMU SMP topology is four single-threaded cores; set `COHESIX_QEMU_SMP=1` for single-core baselines or `COHESIX_QEMU_SMP_TOPO` for explicit topologies.
- seL4 16 QEMU artifact trees must be configured with
  `ElfloaderRootserversLast=ON`, scalar pre-MMU elfloader/libcpio code, and an
  embedded QEMU `virt,gic-version=3,virtualization=off` DTB whose PSCI method is
  `hvc`. The wrapper must select HVC `CPU_ON` only for an HVC DTB, retain the
  upstream SMC path for SMC-selected platforms, and reject a duplicate or
  missing PSCI SMP driver. Apple-Silicon/macOS uses HVF, `cortex-a57`, and
  `kernel-irqchip=off`; AArch64 Linux uses KVM, the `host` CPU, and the
  in-kernel GICv3. Generated `TIMER_CLOCK_HZ` and the console-network descriptor
  must equal the guest-visible virtual-counter frequency: 24,000,000 Hz for
  macOS `qemu_smp_production` and 31,250,000 Hz for Linux
  `qemu_smp_kvm_production`. TCG, `-icount`, or a mismatched timer frequency is
  diagnostic-only and cannot establish QEMU acceptance or performance
  evidence.
- Milestone 26d profile closure requires all five fresh
  `out/sel4/profile-v2/*` defaults to pass the fail-closed aggregate validator
  with source and artifacts required. QEMU build, release, regression, and
  publication entrypoints consume and revalidate `qemu_smp_production` by
  default. This makes it the canonical GICv3 build input; it is still not QEMU
  boot evidence. The repo-managed
  `seL4/build_UBOOT` exact-image CYW43 input remains separately coordinated
  and is not Pi hardware or release evidence. Exact-image composition treats
  that tracked tree as immutable input: the image wrapper validates relocated
  artifact digests, fingerprints the complete tree, reconstructs the tracked
  baseline elfloader byte-for-byte as a toolchain oracle, and relinks the new
  rootserver only in disposable output. Derived provenance binds the canonical
  stamp/tree, tool identities and oracle, and exact rootserver/CPIO/wrapper
  tuple; `--skip-build` may reuse only that provenance-bound assembly. No Pi
  seL4 source or build input may be selected below `out/`, and CMake or Ninja
  must never mutate `seL4/build_UBOOT`.
- macOS: FUSE mount coverage is optional unless the MacFUSE runtime is installed and approved (verify `/dev/macfuse0` exists, or `/dev/osxfuse0` on older OSXFUSE).
- On Linux, KVM requires `/dev/kvm`, `-cpu host`, and the in-kernel GICv3;
  `kernel-irqchip=off` is the macOS HVF envelope and is invalid for this KVM
  configuration. The launcher must agree with the generated GICv3 and
  31,250,000 Hz timer truth of `qemu_smp_kvm_production`.
- Before any QEMU TCP run, start tcpdump and confirm the log path (example: `logs/tcpdump-new-YYYYMMDD-HHMMSS.log`). Use the same path in TCP correlation checks.
  Observe the guest's `root-console.start.ok` serial marker before the first
  authenticated console request. Do not probe the forwarded console port with
  an unauthenticated connection: QEMU may defer that connection until the guest
  listens, consuming its sole console slot during a later test operation.
  For automated runs without host BPF privileges, set `COHESIX_QEMU_CAPTURE_DIR`
  to a private evidence directory and `COHESIX_QEMU_TCPDUMP` to a tcpdump executable
  that supports pcap input through `-r -`. Both canonical QEMU launchers start
  tcpdump before QEMU and retain the complete guest `net0` stream, decoded log,
  launch command and completion record in a fresh directory for each boot.
  Check every `result.json` has `complete=true`; an early decoder exit or
  truncated stream fails the launcher. These captures observe guest network
  traffic; use a host-interface capture when diagnosing host-side REST traffic.
  Keep the capture root outside a pressure runner's disposable `out/` tree.
- Headless Linux requires `xvfb-run` (`sudo apt-get install -y xvfb` if missing).
- Ensure `/updates` and `/host` are enabled for host tool tests:
  - `cas.enable = true` (and `ui_providers.updates.*` as needed)
  - `ecosystem.host.enable = true` with providers set
  - Re-run `coh-rtc` and `scripts/check-generated.sh` if toggled.
- Use a fresh run/log directory. Retain failed attempts and their exact inputs;
  do not clear shared log or evidence directories before a rerun.

The hosted `driver-tests-pi4` suite selects compiler-generated Pi tables from
`apps/root-task/tests/support/generated/pi4/`. This preserves the default QEMU
tables while exercising the Pi registry, bounds and admission configuration.
After changing the Pi manifest, regenerate this test profile with
`scripts/check-generated.sh --update-pi4-test-profile`; the ordinary generated
check verifies both profiles. These host tests do not constitute Pi boot or
hardware acceptance.

## Performance baselines (Authoritative)
- Performance evidence is only valid when it is **stored and reviewable**:
  - Preserve the canonical harness `*.summary.json` plus associated logs or
    target proof under `logs/bench/`, `out/bench/`, or a milestone-approved
    committed evidence path; and
  - Record its baseline/comparison/claim and artifact hashes in the owning
    BUILD_PLAN task or audit record. Follow `docs/BENCHMARKS.md` for methodology;
    do not duplicate the measurement history there or here.
- Do not use "last local run" as a baseline. If you need a new baseline,
  preserve and index its artifacts in the same change. Commit raw evidence only
  when the active milestone requires it; otherwise retain the immutable bundle.

## Staged and conditional procedures
For acceptance, run in order. Skips produce INCOMPLETE markers and the stage
will fail. During active target convergence, use the separate non-claiming
entry point above; its result cannot satisfy any procedure in this section.
- Scripted runner (recommended): `scripts/ci/test_plan_run.sh --state-dir out/test-plan/<run-id>`

### Automated Stage 01 — Reusable common-hermetic closure

Run `scripts/ci/test_plan_stage_01_integrity.sh`. The generated catalog table is
the sole Stage 01 command inventory; do not replay named tests as filters.
Stage 01 deliberately runs one complete harness for each distinct configuration:

- locked Cargo metadata and generated-contract/catalog integrity;
- workspace formatting, Clippy, check, and default tests, partitioned so
  SwarmUI and `pi4-driver-runtime` are not rerun by the workspace action;
- complete SwarmUI, `coh --features mock`, root-task QEMU, root-task Pi 4,
  minimal `net-console`, cache-maintenance, direct-GENET console runtime, and
  isolated Pi runtime suites;
- the `aarch64-unknown-none` Pi runtime compile, host dependency/driver policy,
  Python discovery and examples, and the Rust-risk bootstrap/ratchet.

Normal harness parallelism is retained. Serialization is limited to the
stateful root-task feature suites and isolated Pi runtime boundary recorded in
the catalog. `scripts/ci/check_driver_test_coverage.py` maps the documented
HAL/driver invariants to those broad suites and fails if the feature or target
closure drifts. Every catalogued test action enforces a non-zero inventory, so
a renamed or removed test cannot turn an empty filtered run green.

The Python actions use `scripts/ci/python_test_gate.sh`: a shared virtual
environment keyed by the canonical Python executable and hashed requirements,
with exact `pytest`, `pyserial`, and Python-package build-backend (`setuptools`)
pins. Repository, client, due-diligence, and runner contract tests execute in
one pytest process; four mock examples execute once in a separate smoke action.
A missing Python lane is INCOMPLETE, never PASS.

`scripts/check-generated.sh` already invokes `scripts/ci/check_test_plan.sh`,
so Stage 01 does not repeat that check. Fixture regeneration is intentionally
outside the normal pass and is allowed only when fixtures change:

- `COHESIX_WRITE_TRACE=1 cargo test -p cohsh --test trace`
- `COHESIX_WRITE_TRACE=1 cargo test -p swarmui --test trace`

The explicit NineDoor scale proof remains conditional rather than part of the
fast common closure:

- `cargo test -p nine-door --features scale-tests --test shard_scale sharded_attach_1k_scale_gate_exports_metrics -- --nocapture`

Stage 01 proves bounded host-model and feature behavior, including the
catalogued Secure9P, operator-liveness, driver-ring, CYW43/SDIO, GENET, USB,
HDMI, and scheduling invariants. It does not prove QEMU boot, live Pi hardware,
RF/DHCP, physical throughput, or image/readback identity. Pi trace normalization,
image identity, and repeatability remain post-capture hardware workflows under
Conditional F.

### Automated Stage 02 — Provisioned-target checks

Stage 02 runs only the catalogued checks for the selected target after a fresh
or imported Stage 01 common-hermetic attestation:

- QEMU profile validation against
  `out/sel4/profile-v2/qemu-smp-production`, followed by the
  `release-qemu` AArch64 root-task check. The check builds fresh Worker,
  NineDoor, console-network, and driver-runtime identities inside the Stage 02
  attempt and binds them to the root check under the selected 24 MHz profile.
  The console child selects `direct-virtio` independently of diagnostic tracing.
- Pi 4 profile validation against
  the immutable `seL4/build_UBOOT` `pi4_production` artifacts, followed by the
  `release-pi4`
  AArch64 root-task check. Its independently built component bindings use the
  selected 54 MHz header and the canonical Pi build's `direct-genet` feature
  and console-network/smoltcp optimization settings; this remains compile
  evidence, not Pi boot or hardware acceptance.

The remaining Pi-specific material in this section defines evidence semantics
for Conditional F. It is not additional Stage 02 execution and must not cause
the common host suites to be replayed.

#### Pi 4 post-capture and hardware evidence semantics

Stage 01 runs the identity/normalizer and HAL/runtime contract tests; it does
not create physical evidence. Conditional F validates actual owner,
DMA/cache/IRQ, serial, USB/HDMI, and CYW43/GENET observations with the image,
gate-proof, normalizer, and repeatability tools. Preserve their raw inputs and
failure verdicts. Missing, stale, duplicate, wrong-image, or cross-boot evidence
cannot be repaired by a passing compile or a historical transcript.

### Automated Stage 03 — QEMU or Pi transport regression
- `scripts/ci/test_plan_stage_03_qemu_tcp_regression.sh`
- Stage 03 sets resilient readiness defaults for clean hosts:
  `TP_STAGE3_READY_TIMEOUT=900`, `TP_STAGE3_PORT_TIMEOUT=60`, and
  `TP_STAGE3_AUTH_READY_TIMEOUT=120` (override as needed). The separate
  authentication-readiness bound applies to Pi/live transport; QEMU evidence
  workloads perform their own exact AUTH exchange after the UART marker.
- QEMU boots must emit exact `[mark] root-console.start.ok` before the first
  authenticated response-matrix or `.coh` workload. A listening TCP socket is
  not root-console readiness, and the runner must not consume a throwaway QEMU
  authentication connection before the evidence workload.
- `scripts/cohsh/run_regression_batch.sh` builds one immutable artifact for the
  default manifest and one for the gated manifest. Base, telemetry, and shard
  groups reuse the default artifact bytes; every group still receives a fresh
  QEMU boot.
- The gated manifest may change only its named audit, replay, policy, model,
  sidecar, UI, and client feature gates. It must preserve the selected base
  QEMU operational topology, including the root, Worker, temporal-authority,
  NineDoor, console-network, resource-admission, and timer contracts exactly.
  Audit/replay, model, and Modbus support must be enabled for their positive
  gated fixtures; repository-only CAS trust remains separate from production.
- The batch snapshots generated projections and restores them in an EXIT trap,
  including failure and interrupt paths. Each artifact and boot result has a
  machine-readable source/profile/manifest/image/action/log binding.
- QEMU close success is a same-connection protocol assertion. The fixed matrix
  and every `.coh` client must receive exact `OK QUIT` followed by target EOF;
  timeout, another post-terminal frame, or a client error fails the workload.
  The isolated console child owns close and relisten, so Stage 03 must not wait
  for the legacy root-stack UART string `audit tcp.conn.close`. Authentication
  by the next workload on that same boot proves listener restoration without a
  reconnect retry. Pi/live retains its lifecycle resume and per-script ledger.
- Pi 4 hardware bring-up uses the same official runner against an already-booted TCP console: `COHSH_BATCH_TARGET=pi4 COHSH_TCP_HOST=<pi4-ip> COHSH_TCP_PORT=31337 scripts/cohsh/run_regression_batch.sh`. Pi mode archives a full per-script ledger, runs lifecycle resume before/after groups and scripts, continues after failures by default, and writes a unique `out/regression-logs/pi4-full-<utc>/summary.log` unless `COHSH_LOG_ROOT` is set.
- Before the staged Pi 4 transport run, create its source/boot/image/endpoint
  binding and pass the result as `TEST_PLAN_TARGET_EVIDENCE_FILE`:
  ```sh
  source_digest="$(scripts/ci/qemu_artifact.py source-digest --repo-root .)"
  scripts/ci/qemu_artifact.py record-pi4-evidence \
    --output out/test-plan/<run-id>/pi4-target-evidence.json \
    --source-digest "${source_digest}" \
    --boot-id <fresh-boot-id> \
    --image-identity sha256:<staged-image-sha256> \
    --target-host <pi-host> \
    --gateway-url http://<gateway-host>:<port>
  ```
  Use `--gateway-target-host <gateway-host>` instead when the public URL is
  recorded separately. This caller-declared record prevents accidental target
  switching during Stages 03/04; it cannot independently detect a reboot or
  backend replacement and is transport evidence, not Pi hardware acceptance.
- Stage 03 archives per-script logs under the stage state dir (for example `out/test-plan/<run-id>/qemu-regression-logs/`).
- Manual runs of `scripts/cohsh/run_regression_batch.sh` default to `out/regression-logs/` unless `COHSH_LOG_ROOT` is set.
- Focused Stage 03 iteration may use `COHSH_BATCH_GROUPS=base`,
  `base-telemetry`, `base-shard`, or `gated` with `--iteration`. Without
  `--iteration`, any subset writes an INCOMPLETE record and cannot produce
  Stage 03 PASS evidence.
For focused manual diagnosis only (not a second mandatory Stage 03 pass), start
QEMU from the exact source artifact or bundle, then verify:
- Capture QEMU serial to `logs/qemu-console.log` (example: `./qemu/run.sh | tee logs/qemu-console.log`).
- `cohsh` (queen): `help`, `attach queen` (skip if you launched cohsh with `--role`),
  `log`, `tail /log/queen.log`, `ls /`, `cat /log/queen.log`,
  `test --mode quick`, `test --mode full`, `test --mode smp` (fresh boot),
  `spawn heartbeat ticks=100`, `ls /worker`, `kill worker-<id>`, `ping`,
  `tcp-diag`, `quit`
  - If policy gating is enabled (see `/policy/rules`), enqueue approvals before `spawn` and `kill`:
    - `echo {"id":"spawn-1","target":"/queen/ctl","decision":"approve"} > /actions/queue`
    - `echo {"id":"kill-1","target":"/queen/ctl","decision":"approve"} > /actions/queue`
- Capture cohsh output to `logs/cohsh-session.log` (example: `... | tee logs/cohsh-session.log`).
- Success criteria:
  - No unexpected `ERR` lines or reconnect loops; negative tests must match
    their exact expected error and prove its required side-effect boundary.
  - ACK/ERR/END ordering stable.

### Conditional A — TCP reliability smoke

Use this smoke for a changed TCP reliability path or its selected claim; it is
not an extra full regression pass after every Stage 03 run.

Run while QEMU is up:
- Repeat `tcp-diag` 5–10 times and record results (example: `... | tee logs/tcp-diag.log`).
- Run `pool bench path=/log/queen.log ops=500 batch=8 payload_bytes=64` and record throughput/latency (example: `... | tee logs/pool-bench.log`).
- Functional smoke acceptance:
  - `tcp-diag` has zero failures.
  - `pool bench` completes with non-zero operations. This is a liveness smoke,
    not a performance claim.
  - The `performance` tier is qualified by the lane that owns the claim:
    Conditional B2 for executable QEMU target pressure, a separate fresh-Pi
    target-performance path for Pi, and Conditional D for the exact packaged
    gateway's host-model large-telemetry comparator. Any regression claim also
    needs reviewable baseline artifacts indexed in `docs/BENCHMARKS.md`; never
    compare against unpublished local runs.
- Capture logs:
  - cohsh: `logs/cohsh-session.log`
  - QEMU serial: `logs/qemu-console.log`
  - tcpdump: recorded tcpdump log path
- Fail if any unexpected disconnects:
  - QEMU log: `rg -n "audit tcp\\.conn\\.close reason=error|audit tcp\\.send\\.partial|audit tcp\\.send\\.error|console\\.emit\\.failed" logs/qemu-console.log`
  - cohsh log: `rg -n "\\[cohsh\\]\\[tcp\\] connection lost" logs/cohsh-session.log`
  - tcpdump: `rg -n "Flags \\[R\\]" <tcpdump-log-path>`
- Acceptable disconnects: explicit `quit` or EOF; anything else is a defect.
- `audit tcp.flush.blocked` lines before any client connects are expected; do not treat them as failures.

### Conditional B — Host tools integration

Require exact packaged-tool/source identity and target-session correlation.
Historical staged PASS does not close missing host-tool, Python, provider, or
performance claims. The CAS boundary and rejection cases below remain required.

- QEMU log correlation (required):
  - Record a short note per tool in `logs/host-tool-runs.md` with start/stop time and tool name.
  - In the QEMU log, locate matching `audit tcp.conn.open`/`audit tcp.conn.close` lines for the same window.
  - Verify the session ends cleanly (`reason=quit`/`eof`) and no TCP errors are present in that window.
  - Use: `rg -n "audit tcp\\.conn\\.open|audit tcp\\.conn\\.close|audit tcp\\.send\\.partial|audit tcp\\.send\\.error|console\\.emit\\.failed" logs/qemu-console.log`
- `cohsh` (already covered in Section 3).
- Control grammar sanity (requires `control_plane.*` + `/proc` observability enabled):
  - `echo {"id":"sched-1","role":"worker-gpu","priority":2,"ticks":3,"budget_ms":120} > /queen/schedule/ctl`
  - `cat /proc/schedule/summary` and `cat /proc/schedule/queue`
  - `echo {"op":"grant","id":"lease-1","subject":"queen","resource":"gpu0","ttl_s":300,"priority":5} > /queen/lease/ctl`
  - `echo {"op":"preempt","id":"lease-1","reason":"timeout"} > /queen/lease/ctl`
  - `cat /proc/lease/summary`, `cat /proc/lease/active`, `cat /proc/lease/preemptions`
  - `echo {"op":"open","id":"export-1","ttl_s":900} > /queen/export/ctl`
  - `echo {"op":"close","id":"export-1","reason":"window-complete"} > /queen/export/ctl`
  - `echo {"op":"apply","id":"rev-2026-02-03","sha256":"<64-hex>"} > /policy/ctl`
  - `echo {"op":"rollback","id":"rev-2026-02-03"} > /policy/ctl`
- `coh` (TCP console; requires `configs/generated/coh_policy.toml`):
  - `./bin/coh gpu list --host 127.0.0.1 --port 31337`
  - `./bin/coh gpu lease --host 127.0.0.1 --port 31337 --gpu GPU-0 --mem-mb 4096 --streams 1 --ttl-s 60`
  - `./bin/coh run --host 127.0.0.1 --port 31337 --gpu GPU-0 -- echo ok`
  - `./bin/coh gpu status --host 127.0.0.1 --port 31337 --gpu GPU-0`
  - `./bin/coh telemetry pull --host 127.0.0.1 --port 31337 --out ./out/telemetry`
  - Live PEFT flow (requires live GPU bridge publish):
    - Preflight: `./bin/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen -c "ls /queen/export/lora_jobs"`
      - If `/queen/export/lora_jobs` is missing in dev-virt, **skip live PEFT** and rely on the mock PEFT tests above (this indicates no export job was seeded in the VM).
    - `./bin/coh --host 127.0.0.1 --port 31337 peft export --job job_0001 --out ./out/peft_export`
    - `./bin/coh --host 127.0.0.1 --port 31337 peft import --publish --model demo-model --from demo/peft_adapter --job job_0001 --export ./out/peft_export --registry ./out/peft_registry`
    - `./bin/coh --host 127.0.0.1 --port 31337 peft activate --model demo-model --registry ./out/peft_registry`
    - Verify in `cohsh` (after closing SwarmUI): `ls /gpu/models/available` and `cat /gpu/models/active`
  - Optional FUSE: `./bin/coh mount --host 127.0.0.1 --port 31337 --at /tmp/coh-mount` (requires a FUSE runtime; on macOS this means MacFUSE installed and approved, typically `/dev/macfuse0`).
- `swarmui` live (console + observability; do not attach cohsh simultaneously):
  - macOS: `./bin/swarmui`
  - headless Linux: `xvfb-run -a ./bin/swarmui`
  - Live telemetry (required for milestone-flagged UI changes):
    - Preconditions: Queen session reachable, workers emitting telemetry, and no parallel `cohsh` session attached.
    - In SwarmUI: set role `queen` + ticket, click **Connect**, then **Hive Start**.
    - Click a worker dot; confirm the detail panel updates within 1–2 poll intervals.
    - Click a telemetry overlay card; confirm the dot selection updates and the detail panel matches the card agent.
    - Confirm overlays show recent lines (no empty state) for at least one worker.
    - Record the result in `logs/host-tool-runs.md` with timestamps.
  - SwarmUI console exposes the core console verbs; CLI-only commands remain in `cohsh`.
- `swarmui` replay:
  - Source tree: `./bin/swarmui --replay-trace "$(pwd)/tests/fixtures/traces/trace_v0.trace"`
  - Release bundle: `./bin/swarmui --replay-trace "$(pwd)/traces/trace_v0.trace"`
  - Source tree: `./bin/swarmui --replay "$(pwd)/tests/fixtures/traces/trace_v0.hive.cbor"`
  - Release bundle: `./bin/swarmui --replay "$(pwd)/traces/trace_v0.hive.cbor"`
  - headless Linux: prefix with `xvfb-run -a`
- `cas-tool`:
  - Validate the selected generated host contract before using it. For the
    default profile, `limits.max_chunks` must be exactly `8`,
    `limits.max_payload_bytes` must be exactly `1024`, and `chunk_bytes` must be
    exactly `128`. A template that omits `limits` is legacy and falls back to
    the shared eight-chunk manifest-v1 maximum; if `limits` is present, neither
    a smaller nor a larger value is accepted. The optional `--chunk-bytes`
    argument must equal the selected template and cannot override it:
    ```bash
    jq -e '
      .chunk_bytes == 128 and
      .limits.max_chunks == 8 and
      .limits.max_payload_bytes == (.chunk_bytes * .limits.max_chunks)
    ' configs/generated/cas_manifest_template.json
    ```
  - Prepare two independent payloads. Preserve the full trace as replay truth
    and as the exact nine-chunk negative input. Pad the dedicated positive
    359-byte fixture at
    `sha256:7f97db91e95d67b8cdd7baa194c563e2a03f94045a4e5c3282b4bb8e7fc3fda1`
    as a whole to exactly eight unique chunks using the deterministic nonzero
    tail byte `((index * 73 + 19) % 251) + 1`; never truncate or relabel the
    trace.
    Use the source-tree invocation from the repository root or the release
    invocation from the release root, never a path guessed across the two
    packages:
    ```bash
    prepare_cas_payloads() {
      CAS_TRACE_SOURCE="$1" CAS_POSITIVE_SOURCE="$2" python3 - <<'PY'
    import hashlib
    import os
    from pathlib import Path

    chunk_bytes = 128
    max_chunks = 8
    capacity = chunk_bytes * max_chunks
    output = Path("out/cas")
    output.mkdir(parents=True, exist_ok=True)

    trace = Path(os.environ["CAS_TRACE_SOURCE"]).read_bytes()
    trace_padded = trace + b"\0" * ((-len(trace)) % chunk_bytes)
    assert len(trace) == 1073
    assert len(trace_padded) == 1152
    (output / "trace_v0.padded").write_bytes(trace_padded)

    positive = Path(os.environ["CAS_POSITIVE_SOURCE"]).read_bytes()
    assert len(positive) == 359
    assert hashlib.sha256(positive).hexdigest() == (
        "7f97db91e95d67b8cdd7baa194c563e2a03f94045a4e5c3282b4bb8e7fc3fda1"
    )
    if len(positive) > capacity:
        raise SystemExit("positive fixture exceeds manifest-v1 capacity")
    tail_len = capacity - len(positive)
    positive_padded = positive + bytes(
        ((index * 73 + 19) % 251) + 1 for index in range(tail_len)
    )
    assert len(positive_padded) == 1024
    positive_chunks = [
        positive_padded[offset : offset + chunk_bytes]
        for offset in range(0, capacity, chunk_bytes)
    ]
    assert len({hashlib.sha256(chunk).digest() for chunk in positive_chunks}) == 8
    (output / "max_chunks_v1.padded").write_bytes(positive_padded)
    PY
    }
    ```
  - Source-tree invocation, from the repository root:
    ```bash
    cargo build --locked --release -p cas-tool
    CAS_TOOL=(./target/release/cas-tool)
    CAS_FIXTURE_SIGNING_KEY=./resources/fixtures/cas_signing_key.hex
    test -x "${CAS_TOOL[0]}"
    test -f "$CAS_FIXTURE_SIGNING_KEY"
    shasum -a 256 "${CAS_TOOL[0]}"
    prepare_cas_payloads \
      tests/fixtures/traces/trace_v0.trace \
      tests/fixtures/cas/max_chunks_v1.txt
    ```
  - Release-bundle invocation, from the release root instead of the source
    invocation:
    ```bash
    CAS_TOOL=(./bin/cas-tool)
    unset CAS_FIXTURE_SIGNING_KEY
    test -x "${CAS_TOOL[0]}"
    shasum -a 256 "${CAS_TOOL[0]}"
    test "$(tr -d '\n' < cas/max_chunks_v1.txt.sha256)" = \
      "7f97db91e95d67b8cdd7baa194c563e2a03f94045a4e5c3282b4bb8e7fc3fda1"
    prepare_cas_payloads \
      traces/trace_v0.trace \
      cas/max_chunks_v1.txt
    ```
  - Negative pack preflight: require a fresh nonexistent
    `./out/cas/trace-v0-over-limit.bundle`, run `pack` on
    `./out/cas/trace_v0.padded`, require nonzero exit with exact causal text
    `CAS manifest capacity exceeded: payload_bytes=1152 chunk_bytes=128
    chunks=9 max_chunks=8 max_payload_bytes=1024`, and require the bundle path
    to remain absent. This is local proof: do not start or contact QEMU, Pi, or
    a gateway for the negative case:
    ```bash
    test ! -e ./out/cas/trace-v0-over-limit.bundle
    if "${CAS_TOOL[@]}" pack \
      --epoch 1 \
      --input ./out/cas/trace_v0.padded \
      --out-dir ./out/cas/trace-v0-over-limit.bundle \
      --chunk-bytes 128 \
      >./out/cas/trace-v0-over-limit.log 2>&1; then
      exit 1
    fi
    rg -F "CAS manifest capacity exceeded: payload_bytes=1152 chunk_bytes=128 chunks=9 max_chunks=8 max_payload_bytes=1024" \
      ./out/cas/trace-v0-over-limit.log
    test ! -e ./out/cas/trace-v0-over-limit.bundle
    ```
  - Exact-capacity fixture pack (source-tree gated-profile invocation only):
    ```bash
    test -n "${CAS_FIXTURE_SIGNING_KEY:?source-tree fixture key is required}"
    "${CAS_TOOL[@]}" pack \
      --epoch 1 \
      --input ./out/cas/max_chunks_v1.padded \
      --out-dir ./out/cas/max-chunks-fixture.bundle \
      --chunk-bytes 128 \
      --signing-key "$CAS_FIXTURE_SIGNING_KEY"
    ```
    This repository key is fixture-only. Upload that bundle only to the
    explicitly gated QEMU artifact whose selected verification key is the
    matching fixture key; never upload it to operational base QEMU or Pi.
  - Operational pack:
    ```bash
    test -n "${COH_CAS_SIGNING_KEY:?set external CAS signing-key path}"
    "${CAS_TOOL[@]}" pack \
      --epoch 1 \
      --input ./out/cas/max_chunks_v1.padded \
      --out-dir ./out/cas/max-chunks-operational.bundle \
      --chunk-bytes 128 \
      --signing-key "$COH_CAS_SIGNING_KEY"
    ```
    Its public key must match the selected profile's
    `cas.signing.verification_key_path`.
  - Direct upload of the bundle appropriate to the selected artifact:
    ```bash
    "${CAS_TOOL[@]}" upload \
      --bundle <exact-positive-bundle> \
      --host 127.0.0.1 \
      --port 31337 \
      --auth-token "$COH_AUTH_TOKEN" \
      --ticket "$QUEEN_TICKET"
    ```
    Upload must validate manifest chunk count before reading chunks or opening
    the socket. A foreign or legacy nine-chunk bundle must therefore fail
    locally with zero target connection.
  - Passing local preflight proves only the fixed manifest shape. The target's
    independent global store may still return typed `buffer-full` for an
    eight-chunk manifest when previously retained chunks or models consume
    capacity. Preserve that result; do not retry, increase capacity, or present
    the positive fixture as target success unless the exact target accepted it.
- `gpu-bridge-host`:
  - `./bin/gpu-bridge-host --mock --list`
  - Optional NVML: `./bin/gpu-bridge-host --list` (enabled by default on Linux builds; omit NVML with `--no-default-features`)
  - Live publish: `./bin/gpu-bridge-host --publish --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token "$COH_AUTH_TOKEN" --interval-ms 1000 --registry "$COH_GPU_REGISTRY"`
    - On macOS without a real compiled GPU backend, run `--mock --list` only. Fixture snapshots are rejected by the operational target and are not live evidence.
- `host-sidecar-bridge`:
  - `./bin/host-sidecar-bridge --mock --mount /host --provider systemd --provider k8s --provider docker --provider nvidia`
  - `./bin/host-sidecar-bridge --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token "$COH_AUTH_TOKEN" --watch` (requires `/host` enabled in `configs/root_task.toml`)
- `host-ticket-agent`:
  - `./bin/host-ticket-agent --mock --run-once`
  - `./bin/host-ticket-agent --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token "$COH_AUTH_TOKEN" --run-once`
  - REST mode (gateway required): `./bin/host-ticket-agent --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" --run-once`

### Conditional B1 — Control-ticket matrix (Milestone 25g)
All runs are required unless explicitly marked `NA` by platform constraints.
- Ticket namespace and bounds:
  - Append valid ticket JSONL to `/host/tickets/spec`; verify success.
  - Append malformed or over-bound lines; verify deterministic `ERR`.
- GPU/PEFT ticket flow:
  - Submit `gpu.lease.grant` and `gpu.lease.release` tickets (or `NA` if no GPU surface).
  - Submit `peft.import`, `peft.activate`, and `peft.rollback` tickets against a test registry.
  - Verify lifecycle receipts under `/host/tickets/status|deadletter`.
- systemd/docker remediation flow:
  - Submit `systemd.restart` and `docker.restart` tickets for allowlisted targets.
  - Verify receipts are deterministic and bounded.
  - Verify non-allowlisted actions are rejected before execution.
- K8s coexistence translation flow:
  - Use Python translation helpers (`K8sRbacIntent` -> `/host/tickets/spec`) to submit `k8s.cordon`, `k8s.drain`, and `k8s.lease.sync`.
  - Verify no scheduler-replacement semantics are introduced (coexistence only).
- Evidence/timeline replay flow:
  - `./bin/coh evidence pack --host 127.0.0.1 --port 31337 --out ./out/evidence/tickets --with-telemetry`
  - `./bin/coh evidence timeline --in ./out/evidence/tickets`
  - Verify `timeline.ndjson` contains host ticket correlation keys (`id + idempotency_key`).
- `hive-gateway` (REST gateway, Linux/systemd required for this section):
  - Install unit + env file (examples):
    - `sudo cp resources/systemd/hive-gateway.service /etc/systemd/system/`
    - `sudo tee /etc/cohesix/hive-gateway.env >/dev/null <<'EOF'`
    - `COH_TCP_HOST=127.0.0.1`
    - `COH_TCP_PORT=31337`
    - `COH_AUTH_TOKEN=changeme`
    - `COH_ROLE=queen`
    - `COH_TICKET=`
    - `HIVE_GATEWAY_BIND=127.0.0.1:8080`
    - `HIVE_GATEWAY_BROKER_CONTROL_RESPONSE_TIMEOUT_MS=120000`
    - `HIVE_GATEWAY_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS=120000`
    - `EOF`
  - `sudo systemctl daemon-reload`
  - `sudo systemctl enable --now hive-gateway`
  - Validate REST responds: `curl -sS http://127.0.0.1:8080/v1/meta/bounds | jq .`
  - Restart QEMU and confirm auto-reconnect:
    - stop QEMU, wait 5–10s, restart QEMU
    - `journalctl -u hive-gateway -n 200 --no-pager | rg -n "reconnect|connected|disconnected"`
    - Re-run: `curl -sS http://127.0.0.1:8080/v1/meta/bounds | jq .`
  - `sudo systemctl stop hive-gateway`
- Multiplexer regression (REST gateway, QEMU running; `hive-gateway` is the sole console client):
  - REST API smoke (manifest + namespace + log tail):
    - `curl -sS http://127.0.0.1:8080/v1/meta/bounds | jq .`
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/ls?path=/' | jq .`
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/lifecycle/state&max_bytes=64' | jq .`
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/tail?path=/log/queen.log&max_bytes=512&lines=64' | jq .`
    - Failure classification is part of the contract: a preserved target `ERR` or exact in-process host-model schedule/lease/export semantic-capacity refusal returns HTTP `200` with `GatewayResponse.status="ERR"`; capacity retains `ERR ECHO reason=quota detail=buffer-full path=<path> error=buffer full`. HTTP `429` means bounded broker queue backpressure, HTTP `503` means transport or session unavailability, and HTTP `504` means the broker accepted work but the backend response exceeded its response timeout.
  - REST `/proc` bounds (schedule + lease):
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/schedule/summary&max_bytes=128' | jq .`
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/schedule/queue&max_bytes=256' | jq .`
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/lease/summary&max_bytes=128' | jq .`
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/lease/active&max_bytes=256' | jq .`
  - Policy approval (only if `/policy/rules` exists):
    - `curl -sS -X POST http://127.0.0.1:8080/v1/fs/echo -H "Authorization: Bearer ${HIVE_GATEWAY_REQUEST_AUTH_TOKEN}" -H 'Content-Type: application/json' -d '{"path":"/actions/queue","line":"{\"id\":\"approve-rest-1\",\"target\":\"/queen/ctl\",\"decision\":\"approve\"}"}'`
  - REST spawn (heartbeat) through the gateway:
    - `curl -sS -X POST http://127.0.0.1:8080/v1/fs/echo -H "Authorization: Bearer ${HIVE_GATEWAY_REQUEST_AUTH_TOKEN}" -H 'Content-Type: application/json' -d '{"path":"/queen/ctl","line":"{\"spawn\":\"heartbeat\",\"ticks\":120,\"budget\":{\"ttl_s\":300,\"ops\":500}}"}'`
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/ls?path=/worker' | jq .`
  - Host publishers over REST:
    - `./bin/gpu-bridge-host --publish --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" --interval-ms 1000`
    - `./bin/host-sidecar-bridge --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" --watch --provider systemd --provider nvidia`
    - Validate: `curl -sS 'http://127.0.0.1:8080/v1/fs/ls?path=/gpu' | jq .` and `curl -sS 'http://127.0.0.1:8080/v1/fs/ls?path=/host' | jq .`
  - `coh` REST path coverage (queen role via gateway):
    - `./bin/coh gpu --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" list`
    - `./bin/coh gpu --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" lease --gpu GPU-0 --mem-mb 2048 --streams 1 --ttl-s 120`
    - `./bin/coh run --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" --gpu GPU-0 -- echo ok`
    - `./bin/coh telemetry --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" pull --out ./out/telemetry-rest`
    - `./bin/coh peft --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" export --job job_0001 --out ./out/peft_export_rest` (skip if no export job is seeded)
    - `./bin/coh peft --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" import --publish --model demo-model --from demo/peft_adapter --job job_0001 --export ./out/peft_export_rest --registry ./out/peft_registry_rest`
    - `./bin/coh peft --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" activate --model demo-model --registry ./out/peft_registry_rest`
    - `./bin/coh peft --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" rollback --registry ./out/peft_registry_rest`
  - REST mount exclusivity:
    - `./bin/coh mount --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" --at /tmp/coh-mount-rest`
    - In a second shell: `./bin/coh mount --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" --at /tmp/coh-mount-rest-2` → must fail with an exclusive lock error.
    - Read/write smoke (supported MIME types only):
      - `cat /tmp/coh-mount-rest/proc/lifecycle/state` (must be non-empty)
      - `head -n 5 /tmp/coh-mount-rest/log/queen.log` (must be non-empty)
      - `DEV=tp-mount-xfer-1; printf '{"new":"segment","mime":"text/plain"}\n' >> "/tmp/coh-mount-rest/queen/telemetry/${DEV}/ctl"`
      - `printf 'hello-from-test-plan ts_ms=%s\n' "$(date +%s000)" >> "/tmp/coh-mount-rest/queen/telemetry/${DEV}/seg/seg-000001"`
      - `cat "/tmp/coh-mount-rest/queen/telemetry/${DEV}/latest"` (expects `seg-000001`)
  - `cas-tool` REST upload:
    - Use the separately named exact-eight-chunk bundle prepared in
      Conditional B; never use a truncated prefix of `trace_v0.trace`.
    - Run:
      ```bash
      "${CAS_TOOL[@]}" upload \
        --bundle <exact-positive-bundle> \
        --rest-url http://127.0.0.1:8080 \
        --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN"
      ```
    - Require zero retry. A local over-limit refusal must occur before a REST
      request; a target `buffer-full` refusal for an eligible bundle remains a
      valid target-capacity failure, not a host-preflight failure.
  - `cohsh` REST CLI:
    - `./bin/cohsh --transport rest --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN"`
    - `attach queen`
    - `cat /proc/schedule/summary`
    - `cat /proc/lease/summary`
  - SwarmUI via gateway (REST transport enabled by default):
    - `SWARMUI_TRANSPORT=rest SWARMUI_REST_URL=http://127.0.0.1:8080 ./bin/swarmui`
    - Confirm Live Hive view renders telemetry and the console panel accepts standard verbs.
    - Open DevTools and run `window.__SWARMUI_HIVE_DEBUG.getMetrics()`; confirm renders advance and the UI stays responsive.
- Deterministic replay via cohsh (no QEMU needed):
  - Source tree: `./bin/cohsh --transport mock --replay-trace ./tests/fixtures/traces/trace_v0.trace`
  - Release bundle: `./bin/cohsh --transport mock --replay-trace ./traces/trace_v0.trace`

### UI Presentation Layer — SwarmUI (Playwright)

**Additive UI-only layer.** Playwright tests **DO NOT** assert control-plane correctness and **MUST NOT** introduce new verbs, protocols, or semantics. They validate presentation (rendering, wiring, and transcript parity) using deterministic replay fixtures.

#### 1) Scope
- Covers: SwarmUI launch, Spectrum shell control wiring, canvas presence, deterministic replay rendering, mint-ticket UI wiring, and embedded `>coh` console transcript output.
- Excludes: control-plane logic, NineDoor semantics, ticket validation correctness, and any non-UI behavior already covered by `.coh` scripts or regression pack.

#### 2) Modes
- **Replay mode (required, gating):** UI is driven from trace/snapshot fixtures and deterministic transcript outputs.
- **Live mode (optional, smoke only):** non-gating checks for basic launch and visibility; no protocol assertions.

#### 3) Test Categories
- Launch + smoke (UI loads and renders key panels).
- Replay visual regression (banner/shell screenshot baseline).
- Spectrum shell controls (buttons, text fields, pickers mount and remain wired to existing IDs/flows).
- Interactive `>coh` prompt (type commands, assert transcript lines).
- Mint ticket flow (UI-only assertion that the host-returned token is surfaced back into the session field).
- Live Hive UX (labels, role colors, and dot selection wiring).
- Native command argument parity (`detailAgent`, `snapshotKey`), selected detail
  refresh while the canvas is offscreen, and changing/absent telemetry when only
  an overlay is available.
- Structured Worker state (declaration, lifecycle, receipt, artifact, and
  proof render independently; absent axes render as unknown).
- Opaque Worker identity (a role-looking id prefix never supplies a role,
  READY state, receipt, artifact, or proof).
- Live Hive performance harness (bounded render cadence and backlog checks).
- Failure UI (auth error, disconnected state) as UI-only states.

#### 4) Determinism Rules
- Replay-first: all UI assertions are driven from replay fixtures.
- Version-1 model-only snapshots migrate without inferring structured READY or
  target proof from their legacy role/id strings.
- Avoid unbounded timing-based assertions; use explicit replay fixtures and the Live Hive metrics harness for bounded render/backlog checks.
- Transcript-based assertions only (match `OK`, `ERR`, `END` and static help lines).
- Shell assertions must preserve the existing SwarmUI IDs and Tauri invoke contract even when the underlying controls are Spectrum Web Components.

#### 5) CI Positioning
- Runs **after** `.coh` scripts and the regression pack.
- **Blocking for the `ui` claim:** replay-mode UI tests (snapshot + transcript parity + Live Hive UX + performance harness).
- **Warn-only:** live-mode smoke checks.

**Playwright commands (macOS ARM64):**
- `cd tools/swarmui-ui-tests`
- `npm ci`
- `npx playwright install webkit chromium`
- Source UI (default harness target): `npm test`
- Explicit source UI override: `SWARMUI_UI_ROOT=../../apps/swarmui/frontend npm test`
- Release bundle verification: `SWARMUI_RELEASE_DIR=../releases/<latest> npm test`
- Update snapshots only when UI changes are intended: `npm run test:update`

**Notes**
- The Playwright harness targets the **source SwarmUI frontend** by default so UI regressions are measured against the current shell, not a stale release bundle.
- Set `SWARMUI_RELEASE_DIR` to verify a packaged release bundle; set `SWARMUI_UI_ROOT` only when overriding the default source path.
- The harness injects a deterministic Tauri `invoke` mock for UI-only replay and transcript assertions; it does not exercise control-plane behavior.
- The current shell uses a vendored Spectrum Web Components layer for operator controls; Playwright interacts with the effective editable/button controls while keeping the canonical SwarmUI element IDs stable.
- The Live Hive performance harness reads `window.__SWARMUI_HIVE_DEBUG.getMetrics()`; `pending` must stay ≤ `swarmui.hive.pending_event_cap` and `renders` should advance without UI stalls under replay fixtures.
- Browser binaries are installed into the user Playwright cache (not committed).
- Snapshot coverage runs against the current browser matrix: `webkit-desktop` (baseline shell), `webkit-narrow` (responsive shell and scheduler), and `chromium-tablet` (interaction parity without snapshot gating).

### Automated Stage 04 — REST multiplexer regression

Stage 04 is self-contained for local QEMU. Without an external gateway URL it
starts the exact QEMU artifact and a local gateway with stage-local request
authentication and free loopback ports. `TP_STAGE4_GATEWAY_BIND` and
`TP_STAGE4_QEMU_TCP_PORT` override local placement. An existing gateway requires
its explicit request-auth environment and declared deadlines; Pi always uses
the external path bound to its supplied boot/image evidence. Python probes use
the same canonical Python 3.11+ selection as Stage 01.

- In a staged QEMU run,
  `scripts/ci/test_plan_stage_04_rest_multiplexer.sh` verifies and reuses the
  Stage 03 default artifact, then starts a fresh boot. Standalone Stage 04 may
  build its own canonical artifact. Set `COHESIX_GATEWAY_URL` or equivalent to
  target an existing gateway.
- `COHESIX_GATEWAY_URL=http://<gateway-host>:<port> HIVE_GATEWAY_REQUEST_AUTH_TOKEN=<token> scripts/cohsh/REST_regression_batch.sh`
- Pass the token through the inherited environment. The runner records a
  redacted command and must not place the token value in retained logs.
- Stage 04 composes one filesystem-operation response window from the gateway
  contract: `5,000 ms` bounded broker-queue admission plus
  `max(control_response_ms, telemetry_response_ms)` plus `5,000 ms` HTTP
  response-delivery grace. The canonical local gateway uses
  `120,000/120,000 ms`, so the canonical client window is `130,000 ms`.
  Metadata requests, name resolution, connection establishment, and response
  body transfer retain their separate short bounds. Because the HTTP library
  carries earlier send deadlines into response receipt, the filesystem agent
  applies the composed window to request send, request-body send, and
  response-header receipt; request and response byte bounds do not change.
- Local runs resolve
  `TP_STAGE4_GATEWAY_CONTROL_RESPONSE_TIMEOUT_MS` and
  `TP_STAGE4_GATEWAY_TELEMETRY_RESPONSE_TIMEOUT_MS`, falling back to the
  matching `HIVE_GATEWAY_BROKER_*_RESPONSE_TIMEOUT_MS` service variables, and
  pass the resolved values explicitly to `hive-gateway`. Existing-gateway runs
  must declare both values; Stage 04 does not infer external process
  configuration. `TP_STAGE4_REST_CLIENT_TIMEOUT_MS`, with
  `COHSH_REST_RESPONSE_TIMEOUT_MS` as its service fallback, may select a larger
  client window but fails before target work when it is smaller than the
  composition. The Stage 04 runner accepts each declared broker response
  deadline only in `5,000..=1,200,000 ms` and the client window only from the
  composed minimum through `1,210,000 ms`. Conflicting declarations fail rather
  than choosing one.
- The resolved client window applies identically to readiness, primary and
  pooled `cohsh` transports, concurrent core/parity batches, and the Python
  `RestBackend` smoke. This changes no batch concurrency, pool size, request,
  retry, script, or target timeout. The Stage 04 summary retains
  `gateway_timeout_declaration`,
  `gateway_broker_queue_wait_limit_ms`,
  `gateway_broker_control_response_timeout_ms`,
  `gateway_broker_telemetry_response_timeout_ms`,
  `rest_response_delivery_grace_ms`, and
  `cohsh_rest_response_timeout_ms`.
- Before it exports resolved endpoint, deadline, auth, or FUSE helper values,
  Stage 04 snapshots the exact inherited presence and value of its complete
  runner-owned environment set. It restores that snapshot before normal
  `tp_stage_complete` and before cleanup or failure delegates to
  `tp_stage_exit_trap`; an inherited value must be restored exactly and a
  runner-created value must be unset. Focused regression
  `test_stage4_restores_runner_owned_environment_before_final_context` covers
  both finalization paths and the existing timeout composition. Runner-local
  child configuration must not become an input-context change.
- Stage 04 runs two REST batches:
  - A concurrent "core" batch (boot, ingest, and root reachability): `scripts/cohsh/boot_v0.coh`, `scripts/cohsh/observe_watch.coh`, `scripts/cohsh/root_cut_basic.coh`. This is also the default selection for `REST_regression_batch.sh`. `session_pool.coh` remains a TCP check because REST batches only host ticket results. `host_absent.coh` remains under TCP because its entry-count assertion uses a console-specific ACK detail.
  - A strict "parity" batch (control-plane smoke): `scripts/cohsh/rest_control_plane_smoke.coh`.
    - Note: `scripts/cohsh/busy_backpressure.coh` and `scripts/cohsh/policy_gate.coh` remain covered by the TCP/QEMU regression matrix (Stage 03), where console-parser semantics are validated directly.
- Stage 04 also runs a Python REST smoke (`tools/cohesix-py` `RestBackend`) that performs `LS /` and reads `/proc/lifecycle/state` against the same gateway.
- Logs:
  - Scripted Stage 04 writes REST batch logs under the stage state dir (for example `out/test-plan/<run-id>/rest-regression-logs/`).
  - Manual runs of `scripts/cohsh/REST_regression_batch.sh` default to `out/regression-logs/<batch>/<script>.run*.log` unless `COHSH_LOG_ROOT` is set.
- Verify logs show no unexpected errors or disconnects.
- REST does not replace Stage 03 TCP coverage. Both stages are required for
  their applicable full target-integration claim.

### Conditional B2 — Milestone 26e QEMU executable-Worker REST pressure

Run the canonical `scripts/m26e_qemu_pressure.sh` command in
[BENCHMARKS.md](BENCHMARKS.md). Use a disposable checkout with
`--clean-root "$PWD"` when retaining development outputs. The macOS lane cleans repository `target/` and
`out/`, rebuilds the selected SMP+MCS seL4 profile, and uses
`scripts/cohesix-build-run.sh` for one canonical artifact build. The runner
then invokes the standalone exact-artifact session emitter once; it does not
synthesize source, ABI, CYW43, or target-session records inline. The runner
hash-binds frozen collector copies, then a dedicated `-S` critical-duty
observation and the separate medium/high four-core AArch64 `virt` boots use
`--launch-existing`, which verifies and launches the same locked elfloader,
kernel, rootserver, system CPIO, GICv3 topology, and build context without
rebuilding or repackaging. The complete staged QEMU plan runs only after those
QEMU transcripts and pressure reports are immutable. Final acceptance is not
emitted unless that plan passes, and its collector consumes the frozen target
session, topology, ELFs, archives, and image manifest rather than any shared
build output the staged plan may update. Retries remain disabled, control
errors remain strict, and in-flight work remains bounded.
The runner derives and revalidates the compiler-owned Queen console token from
the source and resolved manifests; an optional `COH_AUTH_TOKEN` must match it.
The separately supplied REST mutation bearer must be a fresh 64-character
lowercase hexadecimal value and must not appear anywhere in retained evidence.

For the AArch64 Linux KVM comparison, transfer the exact source and reviewed
patch, build the selected `qemu_smp_kvm_production` seL4 profile, and run the
equivalent documented workload. Its launch record binds the profile-qualified
guest to KVM, `-cpu host`, the native 31.25 MHz architectural counter, and the
in-kernel GICv3. Mac and Linux guest hashes are recorded separately; they are
comparable only when source/patch identity, topology, Worker population,
root/service bounds, and workload parameters match. The non-claiming target
canary still proves root, NineDoor READY, authentication, attachment, and one
real operation before pressure. Linux results are QEMU
performance/integration evidence, not macOS toolchain or final release
acceptance.

The iterative performance loop is log-only. After one separate correctness
baseline, run medium, then high, decode `/proc/schedule/qemu-flight` after each
load, and correlate activation gap, service quantum, drainage ratio, queue
high-water, exit reasons, host CPU/RSS, REST latency, and receipt completion.
Do not attach GDB to these benchmark boots. A later release-acceptance lane may
consume its independently frozen fault-containment transcript; it must not
alter or substitute for the pressure run.

Each full release-acceptance pressure boot has two evidence stages. Before
load, its independent fault collector and the existing
Queen/host-ticket paths must directly produce the complete Worker/service
fault, teardown, fresh-generation, GPU/LoRA receipt, operator-liveness, and
MCS observations. Direct `cohsh` fault injection finishes before the gateway
first attaches; after that attach, the gateway remains the sole console owner
for the rest of the boot. `collect-qemu-preflight` derives a same-boot component
from that immutable UART prefix, the role-specific fault transcripts, the exact
target-session/artifact graph, the separate same-artifact critical-duty GDB
transcript, and the three live integration rows. The critical-duty boot is an
explicit auxiliary fault transcript and is never relabelled as same-boot Worker
or pressure evidence. The gateway starts once with a fixed trust root, the
future same-boot component path, and the exact current target-session path. It
remains fail-closed while that component is absent or invalid, then promotes
the first fully validated PASS component exactly once; the accepted summary is
immutable for that gateway process. Pressure starts only after the shared
validator confirms the promoted current-session binding. The final
`collect-qemu` runs only after both pressure reports and their per-boot UART/fault
artifacts are immutable. A prior-boot component or the final component that the
current pressure run is helping produce cannot admit load.

For each summary:

- `report.population.mode` is `executable` and
  `report.population.maximum_live_tasks` equals the selected generated bound;
- requested, discovered, and structured READY populations are recorded
  separately and equal the selected generated population (256 for current
  QEMU); discovery uses
  only canonical `/shard/<label>/worker/<id>/telemetry` paths;
- backend class is `console-projection` and proof class is `qemu`, sourced from
  the shared-validator-backed gateway acceptance summary rather than gateway
  reachability or QEMU startup;
- top-level `target_session_sha256` matches the exact staged session bytes, and
  `report.executable_state.target_session` retains its manifest, root, Worker
  archive, image-manifest, and ABI hashes plus the generated topology hash;
- pre/post state binds the exact topology digest and aggregate 256 requested,
  discovered, and structured-READY population, while retaining one detailed
  live Heartbeat/GPU/LoRA exemplar with five-part identity, image hash,
  READY/control/receipt/completion sequences, core, passive executor/Reply
  identity, and the full generated per-slot admission object bundle (not an
  observed retype census), plus hash-bound
  `/proc/schedule/{summary,queue}` and `/proc/lease/{summary,active,preemptions}`
  snapshots;
- one bounded Heartbeat kill/recreate cycle proves terminal teardown and a
  larger supervisor generation; GPU and LoRA retain their identity while their
  receipt and completion sequences increase through real host-ticket-v2 work;
- exact per-run UART, GDB and authenticated Worker-log bytes match
  `fault_artifacts`, the marker index is
  complete, and the target transcript independently contains all role faults,
  all seven actions with confirmed/rejected Worker receipts and root-fenced
  stale results after retirement, exact teardown
  booleans, service containment, and the GICv3 target/session markers;
- host-integration observations decode complete authenticated Worker fragments
  and bind the original Worker-log bytes. Service teardown is validated from
  the three separate service-fault UART/GDB pairs by the preflight collector;
  it is not expected on the later receipt boot's UART. The unattended PTY
  capture owns an empty input channel so launcher EOF cannot inject terminal
  control bytes; raw UART bytes remain unchanged and strictly validated;
- a graceful shutdown's root completion report may follow synchronous teardown
  only once, with ABI status `5`, action `0`, and the same identity/sequence as
  the prior admitted shutdown Call. This reports the validated terminal result;
  any later READY, control, receipt, ordinary completion, or duplicate terminal
  report remains a post-revoke failure;
- the receipt matrix uses valid advertised GPU subjects and over-bound lease
  operation IDs for deterministic provider rejection. Its expired inputs use
  `expires_unix_ms=1` while their exact Worker stays READY to receive `Rejected`,
  as required by the host-ticket mapping in `ROLES_AND_SCHEDULING.md`. All 21
  success/failure/expiry cases must advance that exact Worker's receipt and
  completion sequence; a host terminal result alone cannot pass. Each operation
  retains the authenticated log before the next can evict its records.
  Seven additional cases hold the real agent's terminal result after admission
  validation, retire its pinned Worker, and start a fresh same-role generation
  before forwarding the unchanged result through the existing gateway. The root
  must retain `stale` for the old admission; the replacement stays READY with
  unchanged zero receipt/control/completion sequences. The optional collector
  input `--stale-ticket-observations` binds exact current-ticket records,
  result identity, ordered target teardown/READY, session hash and retained
  Worker-log prefix. Without that input the collector still requires the complete
  legacy marker matrix. Root fencing never claims a child `Stale` completion.
  Fault and lifecycle injections independently invalidate old-generation authority.
  Activation/rollback must publish the committed host registry and read the
  matching active-model snapshot through the ordinary bridge channel;
- `/gpu/bridge/status` and the bounded LoRA export job identify only the
  QEMU/bootstrap-trace fixture path. Missing, expired, production-labelled, or
  provider-live-labelled fixture input blocks the run;
- `report.workload.control_write_outcome` is `admitted`; no ACK, HTTP success,
  provider result, or control write is described as accepted or READY;
- both executable benchmark modes reject any failed GPU/LoRA Worker receipt
  and any supplied UART root-emergency fail-stop, independently of the
  aggregate error budget; successful telemetry cannot conceal lost execution;
- latency, throughput, all error classes, backpressure, operator liveness,
  timeout attribution, and post-run Worker/object state are retained. No
  synthetic id expansion, retry masking, or bounded-refusal reclassification
  is permitted.

Fail before load if the generated bound is lower than the requested population,
the structured READY count is insufficient, canonical shard placement is
invalid, the gateway/session/component hashes differ, the backend is not
`console-projection`, or any live fault/fixture artifact is absent or malformed.
A performance error-budget failure remains a faithfully retained QEMU pressure
result but cannot be used as a passing capacity or M26e acceptance claim.

### Current Pi performance contract

Declare the selected image, transport, workload, first-connection conditions,
boot classes/counts and thresholds before collection. The performance matrix,
lightweight convergence checkpoint, and full Conditional F repeatability gate
are different claims. Two passing GENET performance boots do not replace the
Wi-Fi cold/warm aggregate or any required hardware qualification. The
`pi4_wifi_repeatability.py` default remains ten cold plus ten warm passing boots;
retain the exact selected invocation and owning milestone requirement.
Historical one-off matrices are not permission to reduce it.

Do not warm a first-connection measurement with a readiness probe, ping,
`nettest`, or another TCP client. Cold-neighbour ICMP and first-connection raw
measurements have distinct starting-state requirements: schedule their proofs
explicitly and retain separate boot IDs where necessary. If an applicable
matrix requires incompatible ordering on one boot, resolve that contract before
collection rather than label a warmed run as first-connection evidence.

The current GENET performance contract, revised with operator authorization
on 2026-09-09, requires both complete-session throughput >=600 requests/s and
PING p95 <=5 ms over the canonical harness's unpaced
`--mode raw --raw-requests 1024` workload. Omit `--raw-request-rate` for this
gate. The workload retains one outstanding PING and measures sustained
request capacity; it does not claim concurrent target command execution or
Ethernet line rate. Retain every sample, maximum latency, first-connection
behavior, exact terminals and QUIT/EOF. Collect raw performance before active
diagnostics, and repeat on two boots of the same exact release image.

Controlled load at `--raw-request-rate 180` remains a separately reported
diagnostic, including achieved throughput and actual request starts. The
former 1.845-ms controlled-load limit is no longer an acceptance requirement.
Historical attempts retain their original workload and verdict; evaluating
retained measurements against this revised contract requires a separate,
dated assessment and cannot upgrade their target proof class. Preserve the
unchanged uncached medium/high REST workloads, cohsh scripts, zero failed
requests, no measured retry/reconnect/cache substitution, and bounded operator
liveness. A reproduced material defect still requires repair; meeting the
aggregate rate and p95 does not excuse protocol failure or unbounded stalls.

WiFi retains >=25.863 requests/s, p95 <=67.581 ms, and complete wall boot to
`status=ready` <=42 seconds, with its independent repeatability requirement.
Neither revised GENET targets nor RAM-transfer timing waives full boot-time,
exact-image, QEMU, or staged acceptance evidence. Follow
[BENCHMARKS.md](BENCHMARKS.md) for sample accounting and report metadata.

### Fresh Pi target-performance and QEMU-parity lane

Pi performance is independent of Conditional B2 and requires a fresh physical
boot of the exact image under test. Before the harness starts, the selected
GENET or Wi-Fi lifetime must pass its normal physical-device, timer, runtime/DMA,
DHCP, raw-TCP, authenticated-`cohsh`, operator-liveness, and boot-paired packet
evidence gates. The gateway must continuously project the same boot's exact
generated Worker population; the harness receives the canonical target-session
file, same-boot controlled packet capture, and live
`pi4-runtime-dma-proof.env` emitted by `scripts/pi4_gate_proof.sh
--require-driver-task-proof`. Stage-only
`out/pi4-sd/pi4-runtime-dma-proof.env` is an input to that live chain and is not
itself fresh-Pi proof.

On the fresh controlled Wi-Fi boot, `pi4_gate_proof.sh` must capture serial and
network bytes concurrently and emit its positive v2 CYW43 coexistence record.
Before a later Wi-Fi or GENET performance boot, finalize those bytes into a new
canonical immutable session bundle:

```bash
PI_CAPTURE_INTERFACE="${PI_CAPTURE_INTERFACE:?set the verified Pi-facing interface}"
PI_SERIAL_DEVICE="${PI_SERIAL_DEVICE:?set the sole Pi serial device}"
PI_WIFI_TARGET_IP="${PI_WIFI_TARGET_IP:?set the serial-reported Wi-Fi IPv4 address}"
COH_REST_URL="${COH_REST_URL:?set the exact already-running gateway base URL}"
PI_WIFI_EVIDENCE_DIR="${PI_WIFI_EVIDENCE_DIR:?set a private existing directory}"
PI_SESSION_DIR="${PI_SESSION_DIR:?set a new output directory below out/}"
PI_WIFI_SERIAL_LOG="$PI_WIFI_EVIDENCE_DIR/pi4-cyw43-serial.log"
PI_WIFI_NETWORK_CAPTURE="$PI_WIFI_EVIDENCE_DIR/pi4-cyw43-network.pcap"
PI_WIFI_RUNTIME_DMA_PROOF="$PI_WIFI_EVIDENCE_DIR/pi4-cyw43-runtime-proof.env"
PI_WIFI_CYW43_RECORD="$PI_WIFI_EVIDENCE_DIR/pi4-cyw43-coexistence.json"

test -d "$PI_WIFI_EVIDENCE_DIR"
test ! -e "$PI_WIFI_SERIAL_LOG"
test ! -e "$PI_WIFI_NETWORK_CAPTURE"
test ! -e "$PI_WIFI_RUNTIME_DMA_PROOF"
test ! -e "$PI_WIFI_CYW43_RECORD"

scripts/pi4_gate_proof.sh \
  --skip-build \
  --serial-device "$PI_SERIAL_DEVICE" \
  --log "$PI_WIFI_SERIAL_LOG" \
  --require-wifi-ready \
  --require-driver-task-proof \
  --network-interface "$PI_CAPTURE_INTERFACE" \
  --network-capture-out "$PI_WIFI_NETWORK_CAPTURE" \
  --gateway-status-url "$COH_REST_URL" \
  --gateway-target-host "$PI_WIFI_TARGET_IP" \
  --runtime-dma-proof-out "$PI_WIFI_RUNTIME_DMA_PROOF" \
  --cyw43-coexistence-record-out "$PI_WIFI_CYW43_RECORD"

.venv/bin/python scripts/worker_task_evidence.py emit-pi4-target-session \
  --repo-root "$PWD" \
  --runtime-proof "$PI_WIFI_RUNTIME_DMA_PROOF" \
  --cyw43-coexistence-record "$PI_WIFI_CYW43_RECORD" \
  --max-age-secs 21600 \
  --out-dir "$PI_SESSION_DIR"

PI_TARGET_SESSION="$PI_SESSION_DIR/target-session.json"
PI_CYW43_RECORD="$PI_SESSION_DIR/pi4-cyw43-coexistence.json"
```

Start the active gate before the freshly flashed exact image boots. Its output
files must be absent; offline pairing and normalize-only reuse are invalid.
`--skip-build` is permitted only because the retained clean stage proof must be
the exact flashed image. The finalizer must fail unless source is clean and
every staged image/topology, kernel/root/archive/manifest/ABI identity,
latest-boot serial marker, positive Wi-Fi outcome, and controlled
packet-capture binding agrees. It publishes
`target-session.json` and canonical sibling source, ABI, CYW43, runtime, serial,
and pcap bytes. GENET may use a later boot only when that boot has the same
staged image; its live runtime proof and controlled pcap remain separate
current-boot inputs. A build or stage proof cannot manufacture the positive
CYW43 record.
The gateway's `/v1/meta/status` must project normalized configured backend
`target_host="$PI_TARGET_IP"` and `target_port=31337` throughout the controlled
capture and benchmark; those fields bind endpoint identity but do not alone
prove target execution.

The complete structured READY census is a hard precondition for executable
pressure. If it is absent, a same-session REST `perf` status or telemetry run
may be retained only as transport/read-path diagnostic evidence with its exact
endpoint, backend/proof class, authentication/session continuity, run count,
latency, retries, and timeouts. It cannot substitute for Worker pressure,
mixed mutation, target capacity, Pi acceptance, or QEMU/Pi parity.

Run the exact HIGH workload documented under
[Qualified Pi executable pressure and QEMU parity](BENCHMARKS.md#qualified-pi-executable-pressure-and-qemu-parity):

```bash
test -f "$PI_TARGET_SESSION"
test -f "$PI_RUNTIME_DMA_PROOF"
test -f "$PI_NETWORK_CAPTURE"
test -f "$PI_CYW43_RECORD"

.venv/bin/python scripts/rest_perf_harness.py \
  --mode simulate --population-mode executable \
  --benchmark-target pi4 --benchmark-transport genet \
  --pi-runtime-dma-proof "$PI_RUNTIME_DMA_PROOF" \
  --pi-network-capture "$PI_NETWORK_CAPTURE" \
  --pi-cyw43-coexistence-record "$PI_CYW43_RECORD" \
  --benchmark-evidence-max-age-secs 21600 \
  --target-session "$PI_TARGET_SESSION" \
  --no-qemu --no-gateway --rest-url "$COH_REST_URL" \
  --tcp-host "$PI_TARGET_IP" --tcp-port 31337 \
  --workers-min 256 --workers-max 256 \
  --intensity-min 8 --intensity-max 8 \
  --duration-mins 2 --base-rps 4 --max-inflight 32 --seed 2608 \
  --no-transient-retries --strict-control-errors \
  --error-budget-rate 0.01 \
  --log-dir out/bench/pi4-genet --log-prefix m26e-pi4-genet-high
```

The pre/post report must bind the complete generated 256-Worker READY census,
exact topology digest, and one detailed Heartbeat/GPU/LoRA exemplar. Qualified
Pi provenance must bind the source inventory, retained Pi manifest, staged
image, root image, target session, live runtime/DMA proof, latest
same-boot serial/network bytes, retained exact-image CYW43 closure, workload,
and capture time. Re-read those inputs after load; reject any changed bytes,
new boot slice, session/build-graph drift, staleness, incomplete driver/network
gate, offline/operator-asserted capture pairing, or internally inconsistent
metric. Repeat with `--benchmark-transport wifi` only on a separately qualified
Wi-Fi boot of the same image and workload; for Wi-Fi, current runtime/capture
bytes must equal the canonical retained Wi-Fi siblings.

Compare the qualified QEMU HIGH and Pi GENET summaries with
`scripts/pi4_compare_driver_models.py --qemu-report ... --pi-report ...`, the
explicit `1.0` successful-throughput ratio, `21600`-second evidence-age bound,
and a predeclared same-harness physical GENET p95 ceiling. The parity verdict is
exactly successful-throughput ratio plus both identical explicit error budgets.
QEMU latency, comparative error counts, backpressure, and physical-latency
flags are reported but excluded from that verdict. Optional Wi-Fi thresholds
must describe the same REST workload; raw-TCP request-to-first-payload norms
cannot be relabelled as REST-summary norms. Retain the comparator JSON, its
QEMU/Pi/optional-Wi-Fi input SHA-256 values, thresholds, and command with the
benchmark evidence. A missing, stale, differently sourced, differently shaped,
tampered, or pre-existing output blocks this lane.

#### Separate conditional Pi Worker-component acceptance

The full Pi Worker-component collector below is not a prerequisite for the
performance lane above, and performance evidence cannot claim or replace its
acceptance result. Run it only after the authorized physical fault/integration
procedure has produced the complete same-boot three-role receipt, fault,
teardown, recreation, and integration matrix. The collector derives its
observations from gate-owned serial bytes, requires exactly the
`pi4-network-capture`, `pi4-runtime-dma-proof`, and `pi4-serial-boot` raw
evidence IDs, and fails closed instead of synthesizing a missing outcome.

```bash
PI_GENERATED_INVENTORY=out/pi4-sd/cohesix-root-task-topology.json
PI_INTEGRATION_DIR="${PI_INTEGRATION_DIR:?set the accepted Pi integration-record directory}"
PI_COMPONENT_DIR="${PI_COMPONENT_DIR:?set a new Pi Worker component output directory}"

test -f "$PI_TARGET_SESSION"
test -f "$PI_GENERATED_INVENTORY"
test -f "$PI_RUNTIME_DMA_PROOF"
test -f "$PI_NETWORK_CAPTURE"
test -d "$PI_INTEGRATION_DIR"
test ! -e "$PI_COMPONENT_DIR"

.venv/bin/python scripts/worker_task_evidence.py collect-pi4-component \
  --target-session "$PI_TARGET_SESSION" \
  --generated-inventory "$PI_GENERATED_INVENTORY" \
  --runtime-proof "$PI_RUNTIME_DMA_PROOF" \
  --network-capture "$PI_NETWORK_CAPTURE" \
  --transport genet \
  --integration-dir "$PI_INTEGRATION_DIR" \
  --max-age-secs 21600 \
  --out-dir "$PI_COMPONENT_DIR"
```

### Conditional C — SMP parity (Milestone 25+)
- Boot QEMU with a single core: `COHESIX_QEMU_SMP=1 scripts/cohesix-build-run.sh --transport tcp`
- Run `./cohsh --transport tcp --tcp-port 31337 --script scripts/cohsh/smp_parity.coh > out/smp_parity_1.txt`
- Reboot QEMU with multiple cores (match the SMP kernel build): `COHESIX_QEMU_SMP=4 scripts/cohesix-build-run.sh --transport tcp`
- Run `./cohsh --transport tcp --tcp-port 31337 --script scripts/cohsh/smp_parity.coh > out/smp_parity_4.txt`
- Compare transcripts: `diff -u out/smp_parity_1.txt out/smp_parity_4.txt` (must be byte-identical).

### Conditional D — Gateway large-telemetry reliability (Milestone 25f)
When the `performance` claim is selected, run each scenario with a fresh exact-version
Hive Gateway using its explicit in-process host-model backend. Pass
`--gateway-mock`, supply the exact packaged gateway with `--gateway-bin`, and
keep `--no-qemu`; the harness owns and tears down that gateway without probing
or authenticating target TCP. The
24-to-120 synthetic population belongs only to this `backend_class=host-model`
lane. A TCP console gateway reports `backend_class=console-projection` and must
fail before `/worker`, `/actions/queue`, or `/queen/ctl` access when paired with
`--population-mode host-model`; target-backed QEMU pressure instead uses
Conditional B2's exact generated executable population. QEMU cannot qualify
Pi, and neither this host-model lane nor Conditional B2 substitutes for fresh
Pi performance evidence.

These commands disable only the harness's transient operation retries:
`--no-retries` does not disable the gateway's bounded control-write retry
window, whose canonical value is 1200 ms. Preserve the gateway launch
configuration and retry counters with every report. Qualification requires
both the local and G5g evidence named by the active milestone.

The Worker telemetry read ceiling is explicitly `8192` bytes. This is the
existing complete structured Worker-state bound used by executable discovery;
it remains one fail-closed request and adds no retry or truncation. The earlier
implicit `256`-byte default predates `cohesix-worker-observation/v1` and cannot
admit its 381-byte host-model record. Results using `8192` are therefore a named
comparator-input revision and are not directly comparable to historical
`256`-byte Worker-tail results.

The schedule, lease, and export control mirrors retain the newest complete
JSONL records within their generated `ctl_max_bytes` bounds, matching the target
provider. Cumulative mirror bytes are not a lifetime write quota: when a valid
new record fits individually, the provider drops oldest complete mirror records
before appending it. The independent schedule queue, lease lists, and export
window limits remain semantic refusal boundaries and every refused operation
still counts against the error budget. The ramp must begin the configured
Worker/intensity maximum no later than the final ramp interval and hold it
through that interval; a report that never observes the configured endpoint is
non-qualifying.

- `.venv/bin/python scripts/rest_perf_harness.py --mode simulate --population-mode host-model --no-qemu --gateway-mock --gateway-bin "$HIVE_GATEWAY_BIN" --gateway-log out/bench/conditional-d/telemetry-1mb/gateway.log --gateway-broker-control-response-timeout-ms 120000 --gateway-broker-telemetry-response-timeout-ms 120000 --gateway-control-write-retry-window-ms 1200 --no-retries --strict-control-errors --tail-bytes 8192 --fast-ramp --scenario telemetry-1mb --error-budget-rate 0.01 --log-dir out/bench/conditional-d/telemetry-1mb --log-prefix telemetry-1mb`
- `.venv/bin/python scripts/rest_perf_harness.py --mode simulate --population-mode host-model --no-qemu --gateway-mock --gateway-bin "$HIVE_GATEWAY_BIN" --gateway-log out/bench/conditional-d/telemetry-10mb/gateway.log --gateway-broker-control-response-timeout-ms 120000 --gateway-broker-telemetry-response-timeout-ms 120000 --gateway-control-write-retry-window-ms 1200 --no-retries --strict-control-errors --tail-bytes 8192 --fast-ramp --scenario telemetry-10mb --error-budget-rate 0.01 --log-dir out/bench/conditional-d/telemetry-10mb --log-prefix telemetry-10mb`
- `.venv/bin/python scripts/rest_perf_harness.py --mode simulate --population-mode host-model --no-qemu --gateway-mock --gateway-bin "$HIVE_GATEWAY_BIN" --gateway-log out/bench/conditional-d/telemetry-100mb/gateway.log --gateway-broker-control-response-timeout-ms 120000 --gateway-broker-telemetry-response-timeout-ms 120000 --gateway-control-write-retry-window-ms 1200 --no-retries --strict-control-errors --tail-bytes 8192 --fast-ramp --scenario telemetry-100mb --error-budget-rate 0.01 --log-dir out/bench/conditional-d/telemetry-100mb --log-prefix telemetry-100mb`
- `.venv/bin/python scripts/rest_perf_harness.py --mode simulate --population-mode host-model --no-qemu --gateway-mock --gateway-bin "$HIVE_GATEWAY_BIN" --gateway-log out/bench/conditional-d/telemetry-1gb/gateway.log --gateway-broker-control-response-timeout-ms 120000 --gateway-broker-telemetry-response-timeout-ms 120000 --gateway-control-write-retry-window-ms 1200 --no-retries --strict-control-errors --tail-bytes 8192 --fast-ramp --scenario telemetry-1gb --error-budget-rate 0.01 --log-dir out/bench/conditional-d/telemetry-1gb --log-prefix telemetry-1gb`

The retained comparator leaves `seed=null` and requires
`strict_control_errors=true`, so every typed bounded refusal remains an error.
Changing either value, the gateway retry window,
or any timeout defines a different comparator and requires separately named
evidence. The harness applies the three explicit gateway values above to the
fresh process it owns.

Pass criteria:
- Every run exits `0`.
- Summary artifacts exist (`*.summary.json`, `*.ops.csv`, `*.ramp.csv`, `*.ramp.svg`).
- `error_budget_pass=true` and `error_rate <= 0.01` in each summary JSON.
- `no_retries=true`, `fast_ramp=true`, and `scenario` equals the requested preset in each summary JSON; `no_retries` describes harness operation attempts, not the external gateway.
- `report.workload.strict_control_errors=true` in every summary JSON; a typed
  control refusal must not be projected as success.
- `report.workload.tail_bytes=8192` in every summary JSON.
- Every exercised schedule, lease, or export semantic-capacity refusal is
  classified losslessly as `buffer-full` in the applicable reliability and
  retained-state fields and retains the canonical error text. Generic HTTP
  `503`, `other`, or `unclassified` attribution for that refusal is
  non-qualifying even when the aggregate error rate remains within budget.
- `report.population.backend_class=host-model`,
  `report.population.proof_class=host-model`,
  `report.capacity_boundary.worker_cap_limited=false`, and
  `report.capacity_boundary.configured_endpoint_observed=true`; a target-backed,
  unknown, or cap-limited population is non-qualifying even if its process exits
  zero.

Failure policy:
- Any scenario above the error budget is a release-blocking defect.
- Any host-model/backend mismatch fails before a benchmark marker or target
  mutation and must not be converted into a capacity result.
- Do not use retry flags or ad-hoc rerun wrappers to mask failures; tune/fix code and re-run the same matrix.
- Physical-target gateway deadlines belong to the independently named fresh-Pi
  target-performance lane; Conditional D always uses its harness-owned
  host-model gateway and supplies no Pi target evidence.

### Conditional E — Multi-hive federation relay (Milestone 25h)
When the `federation` claim is selected, run this matrix with three independent
hives (`hive-a`, `hive-b`, `hive-c`) and one `host-ticket-agent --relay` per
hive.

Required checks:
- Relay success path:
  - Append one federated spec line from `hive-a` to target `hive-b` via REST `/v1/fs/echo` (`source_hive`, `target_hive`, `relay_hop`, `relay_correlation_id` populated).
  - Verify target hive receives one request and one terminal receipt (no duplicates).
- Relay dedupe path:
  - Re-append the same spec line (`id`, `idempotency_key`, `source_hive`, `target_hive` unchanged).
  - Verify no duplicate side effects; relay counters show dedupe increment.
- Relay failure + WAL resume:
  - Stop target gateway temporarily; submit federated tickets from source.
  - Verify source relay queue/WAL grows deterministically and `relay_remote_write_failures` increments.
  - Restore target gateway; verify pending WAL entries drain exactly once.
- Failover pause/resume integration:
  - Run `python3 scripts/failover_watchdog.py --help` and execute watchdog with `--relay-pause-cmd` and `--relay-resume-cmd`.
  - Planned and unplanned cutover paths must pause relay before cutover and resume only after standby health checks pass.
- Evidence/timeline correlation:
  - `./bin/coh evidence pack --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" --out ./out/evidence/federation --with-telemetry`
  - `./bin/coh evidence timeline --in ./out/evidence/federation`
  - Verify timeline rows include federated fields (`source_hive`, `target_hive`, `relay_hop`) and stable correlation IDs.
- Multi-hive scale gate:
  - `python3 scripts/rest_perf_harness.py --mode simulate --multi-hive --hives 3 --workers-per-hive 1000 --no-retries --error-budget-rate 0.01`
  - Summary JSON must report `multi_hive=true`, `hives=3`, `workers_per_hive=1000`, and pass error budget.

Pass criteria:
- No split-brain writes: mutation authority remains single-writer per hive.
- Relay retries are deterministic and idempotent across restarts.
- No ACK/ERR/END grammar drift versus existing fixtures.
- Any failed mandatory federation check is release-blocking.

### Conditional F — Pi 4 hardware acceptance (Milestones 26a/26b)
Run this matrix in addition to the staged runner when Milestone 26a or 26b files change. Older checked-in M26B Wi-Fi/DHCP captures prove the retained compatibility baseline only; reopened 26a/26b closure additionally requires fresh USB/serial/HDMI responsiveness evidence under wired and Wi-Fi load plus the driver-task scheduling fields below.

- Require the exact Stage 01 common-hermetic attestation instead of rerunning
  compiler, generated-contract, DHCP, log-dump, CYW43, or GENET name filters.
  Its broad suites cover bounded DHCP policy, log streaming, suppressed
  benchmark traces, runtime-window-gated CYW43 TX/RX ordering, and GENET service
  budgets. Conditional F adds only image, boot, capture, repeatability, and
  live-hardware proof.
- Pi 4 image / U-Boot gate:
  - `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml`
  - `scripts/uboot/qemu-uboot-smoke.sh --net user`
  - Confirm U-Boot env control remains deterministic (`ipaddr`, `serverip`, `coh_net_mode`, `coh_net_interface`), generic persistent `uboot.env` import is disabled with `CONFIG_ENV_IS_NOWHERE`, `CONFIG_PREBOOT` stays on the serial/video console path, the staged Pi 4 boot script owns the first menu/input USB bootstrap, reloads `cohesix.env`, mirrors `coh_net_*` values into the staged padded `bcm2711-rpi-4-b.dtb`, and boots the seL4 elfloader through U-Boot `bootm` with that DTB. Host coverage must also prove that routine reflash requires the exact existing FAT32 `COHESIX` child of the explicit removable whole disk, fails before mutation while the macOS console is locked, holds an awake assertion through media mutation, performs no whole-disk erase/repartition or global label scan, preserves the private saved-policy copy, deletes obsolete payload files, and compares every staged regular file byte-for-byte before syncing and unmounting only that child. Whole-disk topology creation must require explicit `--initialize-disk` and must never be selected as a fallback. Interrupted mutation must print an explicit `--policy-recovery-file` retry path; recovery must reject a different non-empty policy, enforce the 384-byte bound, and consume the recovery file only after verified completion and unmount.
- QEMU compatibility gate:
  - `scripts/cohesix-build-run.sh --no-run --cargo-target aarch64-unknown-none`
  - Existing QEMU hostfwd defaults (`127.0.0.1:{31337,31338,31339}`) and ACK/ERR/END fixtures must remain unchanged.
  - QEMU virtio compatibility logs may first show `DRIVER_TASK_BOOT status=skipped reason=qemu-virtio-pre-net-resource-guard`; that preserves virtio TCP resources before network init.
  - Optional QEMU driver-task smoke uses `cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-qemu,qemu-driver-task-smoke` plus a deliberate QEMU boot such as `scripts/cohesix-build-run.sh --cargo-target aarch64-unknown-none --root-task-features kernel,serial-console,net-console,net-backend-virtio,cache-maintenance,qemu-driver-task-smoke --raw-qemu --tcp-port 31347`. The no-USB profile is the preferred local boot attempt while the full USB smoke image remains above the current elfloader placement ceiling. After virtio networking is ready, those logs must show a console-visible `DRIVER_TASK_BOOT_SMOKE phase=post-net-qemu status=summary configured=9 failed=0 live_tcb_count=9 vspace=isolated ipc_abi=shared-ring-command pointer_free_ipc=yes runtime_image_declared=7 runtime_transport_mapped=7 runtime_acceptance=7 runtime_declared_hot_paths=0x7f runtime_mapped_hot_paths=0x7f owner_state=not-proven` line. HAL-only per-contract boot lines may also include `runtime_image=<transport-mapped|none>`, `runtime_declared=<mask>`, `runtime_mapped=<mask>`, `runtime_acceptance=<yes|no>`, and `owner_state_reason=<reason>`, but the console-visible summary is the required QEMU proof surface. `cargo test -p sel4-sys --lib` must cover the host-stub invocation shape for AArch64 page-map/unmap and ASID-pool calls before any driver VSpace work is claimed. That proves QEMU live-TCB/cap/affinity, isolated VSpace mapping, runtime-image transport mapping, pointer-free transport readiness, and current isolated runtime acceptance eligibility only; it is not Pi 4 driver-task proof and must still leave full dedicated-driver-task hardware acceptance fail-closed until hardware roles and hot-path ownership are proved on Pi. If the run fails before root-task with `image load address overlaps with ELF-loader`, record that as a QEMU image-placement blocker, not a driver-task proof.
- Pi 4 runtime evidence gate:
  - Build-only/stage-only validation is useful but is not Pi 4 acceptance. A reopened 26a/26b hardware run must include a fresh serial capture from the reflashed image, not an older checked-in or operator-provided transcript.
  - The May 20 Wi-Fi capture is triage evidence only: it showed `WIFI_GATE=10` and DHCP bound, but `DRIVER_TASK_SUBSTRATE_READY=no`, `DRIVER_TASK_FAILED_COUNT=9`, and `live_tcb_count=0`. The next hardware run must show no `DRIVER_TASK_BOOT ... status=failed err=seL4_DeleteFirst`, `DRIVER_TASK_SUBSTRATE_READY=yes`, `DRIVER_TASK_FAILED_COUNT=0`, and live hot-path ownership before any dedicated-driver-task claim.
  - The minimum 26a wired/GENET closure command is:
    - `scripts/pi4_gate_proof.sh --log <fresh-pi4-serial.log> --require-usb-ready --require-wired-ready --require-driver-task-proof --require-input-responsive --expect DRIVER_TASK_ACTIVE_NET=genet --expect ROOT_PROMPT_SEEN=yes --expect SERIAL_CLEAN=yes --expect USB_BOOTLOADER_HANDOFF_SEEN=no --expect USB_COLD_BOOT_SEEN=yes`
  - The minimum 26b Wi-Fi closure command is:
    - `scripts/pi4_gate_proof.sh --log <fresh-pi4-serial.log> --require-ready --require-driver-task-proof --require-input-responsive --expect DRIVER_TASK_ACTIVE_NET=cyw43 --expect ROOT_PROMPT_SEEN=yes --expect SERIAL_CLEAN=yes --expect USB_BOOTLOADER_HANDOFF_SEEN=no --expect USB_COLD_BOOT_SEEN=yes`
  - Full-lifecycle CYW43/SDIO proof must combine the focused source tests with
    the boot-paired serial/pcap lifetime. Before Gate 8 and during steady
    traffic, every production DPC event must retain one exact event-sequence
    lease with no ordinary continuation grant; persistent op11 and urgent op7
    must likewise run from current durable local conditions to the first exact
    external wait,
    not recurrent scheduler edges. An exact op11 `Waiting` parent must create no
    root self-poll amplification while independent DPC/RX/sideband/deadline/
    terminal work remains live. Interleaved EVENT/DATA must cross one durable
    sideband batch and disjoint root ACK without an op11 terminal. Ordinary
    traffic must record `sdio_deadline_hints=0`, zero timer-created source
    probes, sequence defects, fallback-lane issues, or notification-count
    dependence. Each physical generation must show one release activation and
    no per-control activation cadence. Additional activation may occur only for
    activation-absent or mask-skewed repair or exact ACK debt bound to an
    already-submitted immutable activation frontier. Invalid, wrong-generation,
    poisoned, overrun, or lost-authority state must fail closed without repair.
    Pre-TX source work must bind one exact event and report
    zero lost-token/reactivation faults. Source/runtime proof must also cover the
    final SDIO command-ring sleep race: a fresh sequence-last one-way child
    re-enters intake without a second signal. Every accepted physical pair must report zero overruns and ACK
    failures; counters reset rather than accumulating across replacement pairs.
    Hardware counters and the paired pcap must agree with the accepted frame
    order and one terminal per immutable physical request.
  - Cold-neighbor reply-retention gate:
    - Before any host probe or TCP/`cohsh` connection on each accepted WiFi
      lifetime and the GENET control, send one ICMPv4 Echo Request while the
      Pi's peer entry is cold. Retain the boot-paired capture through ordinary
      ARP resolution.
    - Require the ordered pcap trace
      `Echo Request -> Pi ARP Request -> matching ARP Reply -> exactly one matching Echo Reply`,
      with the original identifier, sequence, and payload and with no second
      Echo Request, duplicate reply, stale-address reply, or unrelated
      cache-warming traffic. Report this semantic result separately from
      ARP-warmed latency. A missing first reply fails the lifetime even when
      Gate 8, DHCP, later pings, and TCP pass.
  - `--require-usb-ready`, `--require-wifi-ready`, and `--require-ready` are
    stricter than gate/blocker success. USB readiness requires current USB and
    PCIe descriptor/owner proof, Gate 10, command readiness, the exact one-deep
    interrupt-IN queue, and real linked-runtime HID/parser/display liveness.
    The reserved USB old-good ABI record is not published by the current
    runtime, so `USB_OLDGOOD_REPLAY=no` or `USB_OLDGOOD_MISSING` naming only the
    dormant receipt is not a blocker. Wi-Fi readiness still requires the
    isolated runtime old-good fields `WIFI_OLDGOOD_REPLAY=yes` and
    `WIFI_OLDGOOD_MISSING=none` for the selected full-ready path. Wi-Fi proof
    also requires
    `CYW43_BOOTSTRAP_SUPERVISOR_SEEN=yes`,
    `CYW43_BOOTSTRAP_SUPERVISOR_READY=yes`,
    `CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS=ready`,
    `CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER=none`,
    `CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT=1`,
    `CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES=0`, and
    `CYW43_BOOTSTRAP_SUPERVISOR_RECOVERIES=0`. Missing lifecycle telemetry
    fails closed; a boot `recovery`, `backoff`, `exhausted`, second `begin`, or
    attempt greater than one cannot qualify the boot. Its ordered old-good
    sequence is scoped to the sole supervisor `begin`, so separate boot or
    runtime-recovery episodes cannot be stitched into one pass. Wi-Fi proof
    additionally requires the retained Gate 7 history
    `WIFI_GATE7_COMPLETE=yes`, `WIFI_GATE7_SEEN=7a>7b>7c>7d>7e`,
    `WIFI_GATE7_LAST=7e`, and `WIFI_GATE7_MISSING=none`; the latest
    `WIFI_SUBGATE=7e` alone cannot hide a missing or reordered join,
    association, M1, M2/M3/M4/PTK/GTK, or secure-release step. Retained Gate 7
    and current Gate 8 rows are accepted from their production atomic commits
    and the separate retained old-good transaction below. New schema-v2
    `wifi diag` causal rows cannot replace them. Historical logs with the old
    bracketed verbose `wifi diag` transaction remain accepted only when nonzero
    matching begin/complete identity, Gate 7 identity, intervening Gate 8
    pair/generation, ordering, and terminal rules all hold; standalone, prior,
    malformed, scrubbed, clipped, or cross-identity rows fail closed. The
    separate old-good prefix comes
    only from a physical-console `smp` or `smp activity` request. It is one
    all-or-nothing 37-line batch: six compact current owner rows in the exact
    `(hot_path, contract, bus_link_seal)` order
    `(serial-console, serial, none)`,
    `(usb-keyboard, usb-local-seat, valid)`,
    `(hdmi-text, hdmi-text, none)`,
    `(pcie-root, pcie-root, none)`,
    `(cyw43-wifi, cyw43455, valid)`, and
    `(sdio-host, sdio-host, valid)`, immediately followed by 31 physically
    contiguous retained rows. Those rows are one BEGIN, three same-ID
    firmware/NVRAM/CLM hashes, the strict 26-step SDIO-engine-through-DHCP-bound
    legacy grammar, and one matching complete END. BEGIN requires
    `id=pair_epoch`, attempt 1, one nonzero pair/generation identity,
    `prefix_steps=26`, and the concrete
    artifact lengths, including normalized NVRAM upload length 1,744. The
    NVRAM hash nevertheless remains the SHA-256 of the immutable 2,074-byte
    source artifact. The association label is exactly one of `assoc`,
    `link-up`, `eapol-m1`, `eapol-m2`, or `eapol-m3`. Each row is at most 243
    bytes; emission reserves 32 further body rows for ordinary SMP output
    within the 69-row body bound.
    The latest malformed/incomplete reserved prefix quarantines older complete
    evidence, and a later Join, Gate 8 lifecycle, or recovery boundary revokes
    it. Cross-pair/generation tails fail. After END, the fresh tail order is
    netstats counters; a physically adjacent same-generation authenticated-TCP
    row; same-generation bound netstats; secure netstats; same-generation TCP
    ready; same-generation terminal nettest; then healthy DPC. The serial
    helper therefore requests this prefix before its fresh Wi-Fi tail. USB
    ready proof also requires `USB_LOCAL_SEAT_STATE=ready`,
    `USB_COMMAND_READY=yes`,
    `USB_FIRST_REPORT_READY=yes`, and `USB_BUSY_AFTER_READY=no` so parser
    admission cannot hide missing first-report or post-ready busy evidence. A
    decoded held-key/modifier report while the attach/recovery idle guard is
    closed must remain `FIRST_REPORT_PENDING`; recovery must revoke stale
    first-report, first-byte, parser, and HDMI command-ready latches until a
    fresh decoded all-zero release reopens them. Endpoint-health counters may
    advance during that interval but cannot substitute for readiness. Missing
    first-report or command-ready proof alone is USB service debt, not physical
    input; it may schedule one bounded `LocalSeat` turn but cannot retain the
    selected-network operator fence without a decoded or buffered byte or
    physical response. A
    Wi-Fi replay miss reports the first missing translated May/U-Boot/Linux
    behavior through `WIFI_OLDGOOD_MISSING`; Wi-Fi Gate 10 without replay
    remains triage evidence only. USB acceptance instead requires distinct
    endpoint, interrupt-IN, first-report, first-byte, and runtime-gate evidence,
    and the first report/byte must be isolated runtime HID sourced. Wi-Fi replay
    rejects failed readiness, failed join, generic EAPOL message tokens,
    firmware-supplicant shortcuts, and started-only nettest output.
  - The fixed 48-byte, pointer-free `DriverRuntimeUsbOldgoodReceipt` at
    shared-ring offset 192 remains an ABI/root-reader compatibility reservation.
    The isolated USB runtime does not stage or publish its former partial or
    terminal receipt state. This runtime-only ablation is a scoped Milestone 26b
    regression repair: fresh exact-image candidates with receipt instrumentation
    stopped immediately after otherwise successful phases 198, 316, and 412,
    while the earlier path reliably reached physical enumeration, the one-deep
    interrupt-IN queue, and command readiness. ABI tests must still prove the
    reserved record is fixed-layout, pointer-free, identity-bound, and
    commit-last; root tests must stable-read an unchanged zero record without
    granting it authority. No runtime publication-order test is required while
    the feature is dormant.
  - Each passive `usb status`, `usb dump-state`, and `usb diag` response must
    project exactly two adjacent rows before its ordinary detail:

    ```text
    USB_OLDGOOD_RETAINED v=1 task=<u32> token=0x<8hex> link_epoch=<u32> link_token=0x<8hex> epoch=<u32> seq=<u32> mask=0x<8hex> topology=0x<8hex> input_gen=<u32> commit=<u32> source=<linked-runtime-hid|none>
    USB_OLDGOOD_CURRENT contracts=usb-local-seat+pcie-root owners=<driver-owned|missing>+<driver-owned|missing> descriptors=<sealed|missing>+<sealed|missing> command_ready=<yes|no> proof_gate=<0|14> blocker=<none|receipt-missing|usb-owner-missing|pcie-owner-missing|usb-descriptor-missing|pcie-descriptor-missing|command-not-ready> root_pointer=no
    ```

    The current pairs are USB then PCIe. The dormant receipt is emitted as
    `v=1` with zero identity/body fields and `source=none`; its
    `receipt-missing`/`proof_gate=0` state is diagnostic and does not revoke
    otherwise current physical USB proof. Acceptance still requires both owners
    `driver-owned`, both descriptors `sealed`, `command_ready=yes`, Gate 10, the
    one-deep interrupt-IN queue, and current linked-runtime HID/parser/display
    liveness. A clipped or malformed current owner row fails closed.
    `usb enable-kbd` and `usb probe-kbd` are active and must not project either
    row.
  - Local-seat tests must preserve the known-working `2668c34f76ff`
    command/first-report path. The established attach phase performs PCIe
    descriptor/prep and owner registration, then USB descriptor replay and
    runtime initialization; it registers the USB owner once controller init is
    ready, before enumeration. There is no second owner/descriptor proof phase or
    deferred endpoint/byte cache after a linked completion. A valid input frame
    enters the existing parser-admission path once; a valid first-report
    completion follows the existing command-ready transition. HAL and gate
    tests independently remain acceptance-red without both current owners and
    descriptors, Gate 10, the exact one-deep queue, and real HID/parser/HDMI
    liveness.
  - `linked_usb_pending_enumeration_defers_retry_until_prompt` must retain the
    existing pre-prompt deferral only while the controller is attached,
    enumeration is pending, the keyboard is not ready, and the root prompt is
    absent. That deferral supplies no descriptor or owner proof authority.
  - Serial-helper prompt tests must accept a prompt split only across the
    physically contiguous tail of the prior guarded `ping` read and the next
    bounded read. The helper retains at most marker-length-minus-one bytes,
    rejects intervening asynchronous text as noncontiguous, and does not issue
    the next diagnostic before the fresh complete prompt.
  - Existing logs may be normalized for triage only:
    - `scripts/pi4_gate_proof.sh --normalize-only --log <existing-log> --allow-summary-only`
    - `--allow-summary-only` is not acceptance proof and must not be combined with any `--require-*` hardware acceptance flag.
  - `scripts/pi4_trace_normalize.py --boot-summary` is a fail-closed boot ledger, not an alternative proof path. A `pass` slice requires clean serial, prompt/root-console readiness, arch-counter timer proof, dedicated driver-task owner/DMA/counter proof, selected network proof, `NET_TCP_READY=yes` or `NETTEST_PROOF=yes`, USB cold-boot plus current descriptor/owner/queue and functional local-seat proof, USB burst proof, and HDMI/serial responsiveness. The dormant USB old-good receipt is not required. Console-only boots, DHCP-only wired boots, and Wi-Fi boots without `WIFI_OLDGOOD_REPLAY=yes` remain failed slices even when the prompt is usable.
  - When `cohsh` reaches the Pi over Wi-Fi/TCP, keep the raw serial log and the `cohsh` transcript together in the Pi 4 evidence directory. TCP `cohsh` output is not mirrored back into the UART log, so the normalizer may be run over a combined serial-plus-`cohsh` evidence file for the final `netstats`/`netstatus` assertions while retaining the raw serial log as the boot source of truth.
  - Capture boot evidence showing:
    - `manifest.hw.network.mode=<static|dhcp>`; Pi 4 manifest-default boots must show `dhcp`
    - `manifest.hw.network.interface=<wired|wifi|auto>`; Pi 4 manifest-default boots must show `auto`
    - `[net-policy] source=<manifest|dtb> ...` or `[net-policy] source=dtb rejected reason=<reason> ...`
    - explicit `wifi` boots may now emit `[net-console] pending-link backend=<driver> active=<iface> detail=wifi-associating ...` before later association / DHCP progress
    - when a bounded, successfully imported, coherent saved Cohesix network policy exists, `Cohesix boot menu` shows `Saved network settings loaded` and defaults to `Boot with saved settings`; an absent, empty, logo-only, oversized, malformed, or incoherent `cohesix.env` shows `Default network settings active` and defaults to `Boot with default settings`
    - host-side U-Boot template guards require the operator labels `Automatic (DHCP)` / `Manual (static IPv4)` and `Ethernet (wired)` / `Wi-Fi (wireless)`, a visible `Boot logo: On|Off` state, `0` Back/Cancel and `9` `Advanced: Open U-Boot shell` submenu navigation, back/discard reload of persisted policy, and repeated navigation through the iterative page dispatcher rather than recursive menu calls
    - host-side Wi-Fi guards require existing settings to be kept or changed without serial disclosure, invalid replacement input to preserve the old working credentials, credential entry to remain USB-keyboard/HDMI-only, and the display to warn that the network name and password are visible locally while hidden from serial output
    - host-side save/reset guards require a separate `Reset saved settings?` confirmation whose Enter-key default is `0` `Cancel`, `Boot once without saving` to remain distinct from `Save settings and restart`, and export, FAT write, post-write size, readback load, or private comparison failure never to report success or invoke restart; a successful save is byte-for-byte verified before restart, and confirmed reset redraws the default-settings state without requiring physical deletion of `cohesix.env`
    - Pi 4 acceptance still executes the staged `boot.scr.uimg` on the pinned U-Boot binary and records real menu traversal plus injected or observed media-failure behavior; source-template assertions alone are not runtime proof
    - for static boots sourced from the U-Boot wizard, `/chosen/cohesix,static-ipv4`, `/chosen/cohesix,static-prefix-len`, and optional `/chosen/cohesix,static-gateway` appear in the U-Boot handoff log
    - for DHCP boots, `[net-console] pending-dhcp ...` followed by `[dhcp] lease bound ...`; DHCP-bound evidence is address proof only, while acceptance still requires listener/command evidence (`netstatus ... tcp_ready=yes`, authenticated `cohsh`, or successful `nettest`).
    - USB cold-boot proof shows `USB_BOOTLOADER_HANDOFF_SEEN=no` and `USB_COLD_BOOT_SEEN=yes`; any U-Boot xHCI handoff, stop-seed, preserve-state, bootloader-authorized reset, or `run-uboot` label fails the Pi 4 USB gate.
    - USB keyboard proof reaches `USB_GATE=10` / `USB_BLOCKER=none` with `USB_COMMAND_READY=yes`, `USB_FIRST_REPORT_READY=yes`, `USB_LOCAL_SEAT_STATE=ready`, `USB_BUSY_AFTER_READY=no`, current USB and PCIe descriptor/owner proof, and the single interrupt-IN lane stably armed by a current `queue_valid=yes queued_reports=1` record; missing queue evidence is acceptance-red, zero is empty, and any larger active depth is an invariant failure, independent of the cumulative transfer-event count. An explicit `queue_valid=no` revokes the queue sample: current target output must render companion `queued_reports`, doorbell, preserved-event, `transfer_events`, and report-status fields as `unknown`. Historical logs may contain untyped bytes from an earlier enumeration result, and the normalizer must not export or classify them as HID queue counters. Current health uses the consecutive no-reply streak, not historical no-reply totals. The dormant USB old-good receipt is not hardware acceptance authority. The first HID report and first byte must be sourced from `linked-runtime-hid`; `usb status` must remain honest with `physical_input_proven=no` until that linked-runtime byte also reaches parser ingress. A linked first-byte latch or parser ingress reported only as `local-seat-queue-diagnostic`, local-seat queue text, or `source=first-byte` is diagnostic by itself and never sets the proof. A printable-key line such as `runtime keyboard first-printable-byte ...`, `physical_input_proven=yes`, visible HDMI echo, and a post-`usb diag` `USB_DIAG_LIVENESS_STATUS=pass` remain the default user-experience evidence. Sustained USB acceptance additionally requires `USB_POST_FIRST_BYTE_BLOCKER=none`, no `recovery-failed` report status, no post-first-byte queue collapse, and no growing no-reply or dropped-byte pressure during typing, arrow-history, and lock-key bursts. HDMI completion proof uses the current driver-task active request; an inactive historical submitted/completed counter gap remains telemetry and cannot fabricate a live outstanding turn. The passive status and immediately adjacent `hdmi: driver` row must jointly prove present counters, inactive authority, at least one completion, zero outstanding work, zero current no-reply streak, and no stale snapshot. `USB_EVENT_LOOP_RUNTIME_SKIPPED` may grow when those turns intentionally service input first and is not itself a blocker. Exact image `7a10b8fd6acc` is the recorded exception: no key was typed, `physical_input_proven=no` remained truthful, and the operator accepted repeated Gate 10/one-deep/command-ready/recovery-free/HDMI-complete sentinels plus exact restoration of the board-proven path. That exception is not parser authority and expires when the physical USB path changes.
    - if the attached keyboard exposes lock LEDs, Caps Lock, Num Lock, and Scroll Lock testing either proves the preallocated EP0 OUT DMA path (`xhci-control-out-prealloc` plus `pi4 keyboard led sync ready ...`) or cleanly logs `keyboard led sync unavailable ... action=disabled` without blocking input.
    - HDMI local-seat acceptance observes typed USB keyboard bytes echoing at
      parser ingress on the live prompt row, boot/progress messages refreshing
      at the documented 5-10 s cadence, and new output scrolling the isolated
      HDMI viewport like a serial terminal without full-screen blink. As soon
      as root-console and display-retry readiness hold, HDMI must keep the
      interactive `cohesix>` prompt withheld until USB command admission while
      showing `USB controller starting...` plus bounded stage feedback. A stage
      change appears immediately and an unchanged stage no more than once every
      two seconds. `USB console ready` reports the observed stage timings, but
      it is a passive EventPump record and may follow local-seat prompt release
      from the same command-readiness transition; the test must not require
      that passive record before the prompt. The physical HDMI ordering is
      nevertheless exact: the canonical `Cohesix console ready` rendering must
      be queued ahead of the HDMI `cohesix>` prompt. The canonical
      command-ready receipt must
      nevertheless appear exactly once on serial immediately before
      `[drivers] USB console ready`, while remaining exactly once in `queen.log`
      without a pre-cutover raw-UART copy. Prompt release itself still requires USB
      command readiness plus display health. Parser ingress and the final Ready
      banner remain false until their independent gates hold. On a
      pre-terminal or failed Wi-Fi episode, admitted USB characters must still
      update that visible input row. A partial line must schedule
      `Dispatch -> Display -> Serial` before any Network turn while retaining
      the exact CYW43 operator fence and parent, except that a pending reboot
      acknowledgement or physical response tail retains immediate Serial
      priority and leaves the echo queued. On a
      successful deferred Wi-Fi boot, Gate 8 commit remains progress only. The
      unique supervisor `ready` is the later current-generation DHCP Bound,
      nonempty-address, and TCP-listener-admission cut and releases the HDMI
      Wi-Fi `Ready to use` banner. USB command-ready proof remains an independent
      hardware acceptance gate, not a prerequisite for Wi-Fi Ready. `failed` or
      `permanent` retains the diagnostic root prompt but must never show Wi-Fi ready.
      Preflight may report diagnostics available but must not claim Wi-Fi or
      interactive-console readiness. The first attached viewport snapshot is
      one-shot, and asynchronous driver milestones arriving during a partial
      command must use the bounded row-preserving update and restore the exact
      prompt, typed bytes, backspace floor, and cursor. The canonical input row
      remains dirty until the matching generation receipt completes; an older
      completion cannot acknowledge newer input. Older FIFO output stays before
      the row and later FIFO output stays after it; reserved high-impact status,
      the closed command row, and its response retain their order under
      pressure. Readiness invalidation retracts the prompt and stale
      console-ready banner without losing the typed suffix, and a stale
      retraction receipt cannot acknowledge the row restored by fresh
      readiness. Held USB up/down arrows use a 300 ms initial
      and 50 ms repeat deadline from the virtual counter. Once a canonical
      viewport is materialized, each repeat advances the desired viewport by
      one row. Accumulated rendered-to-desired debt must emit the largest exact
      symmetric CSI `nS`/`nT` span that currently fits the bounded 512-byte
      HDMI frame. Each retained receipt must carry its monotonic mirrored-line
      generation and rebase the physical rendered anchor exactly once. History
      eviction, generation wrap, underflow/overflow, a missing row, or an
      invalid anchor must coalesce to one canonical redraw. Leaving a settled
      live tail must supersede pending unsent tail bytes with that canonical
      snapshot so CSI cannot overtake physical output; returning to the tail
      must not duplicate already rendered bytes. Rendering must retain only the
      union of old/new nonblank-column damage before row-origin rotation,
      preserve prior dirty cells, and clear only newly exposed rows;
      a full redraw is reserved for initial or recovery materialization and must
      use the framebuffer-derived safe-area row count even when the payload
      spans multiple bounded HDMI service turns. Each rendered row must use
      clear-to-end-of-line and the final chunk must use clear-to-end so
      framebuffer-derived wide modes cannot retain stale text on the right or
      below the viewport. Redraws must leave the cursor at the real end of the
      prompt/input text, not after padding spaces, and overflow recovery must
      not collapse into a stale or jumbled top-of-screen block. Arrow bytes
      must not enter the command parser or starve ordinary keyboard bytes.
      Linked HDMI submit misses, ring busy states, and queue backpressure must
      coalesce to one pending canonical redraw and supersede stale queued bytes
      rather than replaying raw payload tails; a capture with repeated
      `hdmi-text` no-reply growth, saturated `pending_bytes`, or
      jumbled/repeated screen content is not HDMI acceptance even if USB reaches
      Gate 10. Stage 01 driver coverage guards held-arrow timing and steady-poll
      emission, one-row HDMI scroll rendering, canonical input-row receipt and
      FIFO ordering, command-readiness invalidation/re-release, prompt and
      ready-banner readiness, startup-feedback cadence/timing, serial runtime
      ring RX/TX turns, and
      generation rebase, live-tail FIFO ordering, eviction/wrap recovery,
      long-line and full-buffer behavior, and Wi-Fi progress suppression during
      USB boot activity and after USB first-byte proof. These checks introduce
      no console command or USB/HDMI
      authority change. The reserved 48-byte USB old-good slot is the only ABI
      evidence extension described above; runtime publication is dormant.
      The Pi write-only damage-compositor regression must independently prove
      fixed row-ring scroll semantics, zero scanout reads during scroll,
      bounded old/new nonblank-column union damage for both directions without
      an ordinary full-surface redraw,
      genuinely resumable parse/plane/raster progress, exact command-identity
      retention, and no replay of an already-consumed prefix or completed
      multi-cell effect. A pure 4,096-byte parser-envelope case must require the
      expected bounded sequence of turns at no more than 1,280 operations each,
      while the production service path must enforce the unchanged 1,536-byte
      pointer-free frame maximum and must not advertise a 4,096-byte command;
      wide clears up to the 32,768-cell plane, repeated bottom-row scroll, and
      split escape sequences must preserve a retained cursor without duplicate
      row origin changes. Tabs must advance to the next eight-column stop by
      exactly 1 through 8 cells, including 8 when already aligned. Reject
      persistent/steady/unknown flags, auxiliary or frame metadata, and any
      zero, narrower, broader, or byte-insufficient grant before mutation;
      preserve exact 1,280-operation/4,096-byte/80-row bounds, form-feed clear
      completion, final XRGB8888 and RGB888 pixels, full-capacity row-ring wrap,
      and fail-closed rejection of incomplete, truncated, oversized, or
      misaligned framebuffer geometry. The compiled Pi profile must retain the
      exact 2,000 us HDMI budget, 1,800 us candidate WCET, 2,100 us derived
      response, 7,100 us derived GPU-executor response, and 7,400/9,000 us
      core-2 admitted/usable bound. The WCET and responses are static-admission
      inputs/results, not measured target timing. Those deterministic host
      checks, target compilation, staged image construction, or a completed
      HDMI command are not Pi boot, visible correctness, latency, refresh-rate,
      or acceptance evidence. A fresh exact-image Pi run must measure first
      takeover, printable echo, one-row and ten-row scroll, visible final state,
      outstanding/deferral debt, and serial plus USB liveness under display
      pressure before polished or world-class performance is claimed.
      Independently, early-progress tests must render the fixed child-owned
      two-line `Cohesix starting...` / `Initializing services...` tile only
      after full resource/geometry admission, validate both rows before the
      first store, keep its stores below the fixed glyph bound, perform zero
      stores for each rejected geometry, and prove the first ordinary frame's
      retained takeover clear replaces it. Empty-frame refresh tests use an
      injected counter to prove the two-second threshold, no early/repeated
      update, backwards/zero-clock rejection, coalesced missed periods,
      bounded decimal saturation, exact second-row RGB888/XRGB8888 stores,
      rejected-geometry zero stores, and permanent stop at first normal-frame
      admission. Fresh hardware must timestamp visible updates between the
      initial tile and serial cutover, including any scheduling/checkpoint
      overrun. The tile is not prompt, USB, network, or Pi acceptance evidence.
      The serial transport regression must prove that exactly four existing
      pages form two independent two-page/8,128-byte generation-bound SPSC
      rings. Prove only those CPU rings use identically cacheable,
      execute-never Normal-memory aliases in root and child, while DMA, MMIO,
      and every other driver payload retain their selected uncached mappings.
      Then prove payload-before-producer and consume-before-consumer
      publication, commit-paired cursor validation, wrap/full/empty ordering,
      cross-direction isolation, restart-generation fencing, poison/fail-closed
      invalid cursors, and both producer/consumer final rechecks. Fill the RX
      ring, retain the combined UART IRQ acknowledgement, drain it from root,
      and prove a badge-zero software continuation retires that same pending
      acknowledgement without a second UART owner or synthetic timer. Root
      tests must prove the cooperative EventPump observation path and must not
      model a direct IRQ wake into root. A fresh exact-image Pi run must then
      prove simultaneous RX
      and TX without loss, duplication, corruption, poison, stalled ACK, or
      command-latency regression; the unchanged baud/owner/budgets remain
      separate from CPU-side ring throughput.
      The SDIO/CYW43 regression must compare aligned prefix/`u64`-body/tail
      copies with bytewise truth for every source/destination alignment and
      direction, and reject zero-page, overflow, out-of-range, and
      discontinuous spans before mutation. It must prove the only endpoints
      are the existing shared command payload and private uncached DMA4 bounce,
      and that the exact pre-scrub discriminator plus owners, retries,
      deadlines, ordering, and pair-restart policy are unchanged. These tests
      are not SDIO, CYW43, association, or performance evidence; a fresh
      same-boot Pi serial/pcap pair must still prove the complete command engine
      and Wi-Fi acceptance ladder.
      A separate nonforeground CYW43-to-SDIO bus-link regression must exercise
      both runtime payload arenas, both transfer directions, and every 8-by-8
      actual virtual-address alignment pair. Compare the complete result with
      bytewise truth, preserve source/destination sentinels, and require the
      exact admitted word-copy count. Zero-length and exact-end ranges must
      succeed; runtime-arena seam crossing, owner overrun, integer overflow,
      invalid cursor/range, and reverse `len > frame.len` must reject before
      mutation. Foreground coverage must prove the sealed-parent trace and
      prepared-write/overlay path remains authoritative and that it increments
      none of the nonforeground physical-word counters. This CPU-copy
      regression grants no SDIO operation, owner, scheduling, association, or
      hardware-performance evidence.
      The Pi GENET profile regression must require exact IRQ 189, badge 1024,
      and default queue 16 while the QEMU profile retains exactly three
      driver-runtime IRQ entries, no GENET IRQ, and unchanged scheduling.
      Runtime tests must prove
      a maximum 16-frame/24,576-byte child DPC quantum, private-queue overflow
      preservation and wrap, bounded control/data fairness, exact-badge
      admission, durable masked continuation, device-store/unmask readback,
      final source/ring recheck, and no handler acknowledgement while accepted
      work remains. Direct-active coverage must prove an admitted owner/peer
      turn or the final condition-before-sleep cut may join an asserted owned raw
      source or advanced DMA index to the same sole-owner episode, then executes
      the existing mask/clear/bounded-drain/unmask/readback/recheck/rearm path.
      Badge zero alone, software-ring state, idle state, invalid direct identity,
      a timer, or an unadmitted call must not create or acknowledge work. Each
      successful physical-level join increments `dpc_level_adoptions` exactly
      once; a completed clean drain rearms once, while a full direct RX ring
      blocks for peer not-full rearm without self-poll. A separate regression
      must place a complete bounded frame
      behind an advanced durable RDMA producer without delivering an IRQ badge,
      then prove the admitted same-owner RX command queues and returns it within
      its existing operation/frame/byte budget without creating or
      acknowledging an unseen IRQ lifetime. The regression must also prove the
      additive packed-completion bit 30 decodes as zero for legacy results,
      remains zero for every rejected command budget and for an eager IRQ/DPC
      drain, becomes sticky only after a successful same-owner command drain,
      and projects as `runtime_cmd_drain_seen=0|1` on the existing bounded
      `netstats: genet_rxq` row. This passive route discriminator is diagnostic,
      not GENET traffic, performance, or acceptance evidence.
      TX completion coverage must prove an IRQ/DPC reclaim survives a later
      zero-reclaim poll or a budget-exhausted completion with no reclaim field,
      is consumed by exactly one eligible command completion, and is zero on
      the next eligible completion. Cumulative completions cannot exceed
      submissions and the 32-descriptor free/in-flight partition must remain
      exact. An impossible counter tuple is a telemetry defect, not permission
      to infer traffic or change the ring.
      Isolated-console coverage must separately prove that an exact retained
      authenticated response flush selects `poll_response_turn` under the same
      one-op/two-frame/fixed-byte charge as ordinary polling. QEMU direct-VirtIO
      must retain its strict lower rotor: `ObserveChild`, `StageOutput`,
      `Disconnect`, then `ServiceTick`. Direct GENET alone must select an
      exactly ready `StageOutput`, then an exactly ready `Disconnect`, and
      otherwise alternate exactly one `ObserveChild`/`ServiceTick` unit per
      Network visit. The selected Pi root/console relationship is cross-core:
      root-control on core 0 and console-network on core 2 at equal priority.
      Direct GENET and mediated WiFi must both commit durable state, perform
      Release, and send the exact one-hot Signal without child-SC pre-drain or
      `SchedContext_YieldTo`; topology or runtime drift fails closed. After an
      exact stage, direct GENET may retain the causal activation only while the
      sealed control still owes its exact child-consumption watermark. Tests must
      prove a final durable-state recheck, one Poll, Wait only after an empty
      Poll, and return to the ordinary operator/recovery-first rotor after the
      wake. A stable publication visible before that cut must return directly
      to the rotor.

      Cross-core productive progress requires exact generation, connection,
      stage, and publication identity. A stage-bearing continuation must bind
      the exact nonzero one-slot child-control sequence; a later sequential
      control in the same authenticated connection must not match it. Tests
      must prove the final durable-state recheck waits only while that exact
      control still owes its child watermark and no child publication is
      visible, and that a publication winning the condition-before-block race
      returns to outer recovery/operator-first arbitration without Yield.
      Every retained current-request quantum requires final Serial phase and
      rechecks response/queue identity, passive admission, physical operator
      input or response, display debt, recovery, fault, reboot, containment,
      quarantine, handoff, and local fault under the unchanged 64-quantum and
      root-SC bounds. Fused stage-and-drain and `ResponseDrained` retire the
      current request. Fresh-publication tests must prove that one ordinary
      recheck removes the old continuation, preserves clock/causal-wait count
      and the 64-quantum cap, charges an executed empty turn, stops without
      progress, and rejects lane drift or an absent frontier. Only fresh
      productive progress may create the next continuation. Tests must reject any post-response
      Network baton, cross-core empty hot tail, broad wait, or continuation for
      a future request; that request must publish its own command and enter the
      Serial-first rotor. A failed or backpressured stage, zero or stale
      sequence, failed Signal, second command, or identity/fence drift cannot
      mint continuation authority. Host timing or selector tests cannot
      establish Pi performance.
      Cold-bootstrap coverage must independently consume the existing
      child-to-root notification at most once after the selected bounded
      operator condition turn, then require a fresh exact terminal/fault parent
      read before Driver. An exact signal-bound finite one-way CYW43 command may use
      the same condition-before-block Poll/Wait cut only while its stable
      sequence-last terminal remains absent and its generated identity is
      unchanged. Persistent and steady CYW43 parents retain their exact
      transaction deadline classifier and cannot borrow this finite-child wait.
      An absent or stale hint, visible terminal, attached-stack work,
      sideband-only state, deadline, recovery, containment, quarantine, reboot,
      operator priority, or passive admission returns to ordinary arbitration
      without inferred child work. Only after all such arbitration is empty may
      the separate exact direct-GENET global-idle endpoint receive apply under the full
      timer/operator/fault fence below; it is not a transaction continuation.
      Commit-before-signal and sole attached EventPump ownership remain
      source-checked. A host predicate or QEMU pass cannot establish Gate 8
      timing.
      Isolated-console construction must also prove
      Nagle is disabled and the pinned stack's 10 ms ACK timer remains active.
      Real two-stack packet tests must prove that a prompt framed PONG/terminal
      response carries the receive ACK without a preceding standalone ACK,
      missing root output still emits its ACK at the deadline, and another
      small response transmits before its predecessor is acknowledged.
      While direct GENET awaits root control, source contracts must route a
      due child timer or peer-ready retained egress through both service-entry
      gates with a three-unit limit and without clearing the command latch.
      Pure predicates must reject background/empty work and blocked retained
      TX without a due timer. Real TCP tests must also expose the exact ACK
      deadline through `timer_service_due`, clearing it when the ACK is staged.
      These host contracts are not evidence of Pi latency.
      Generated-profile and ABI tests must prove that only the exact Pi
      `bcmgenet-v5` profile derives the direct GENET link: one CPU-only 32-page
      semantic range, zero physical/DMA/device-visible authority, page 0
      control, 15 RX slots, 16 TX slots, fixed reciprocal send-only caps, and
      no manifest-authored toggle. Root construction tests must prove the
      console TCB remains suspended while the old root path and GENET private
      state quiesce, exact `IDLE/QUIESCING` terminals retry without switching,
      only exact `PROGRESS/READY` activates the link and console TCB, malformed
      terminals pair-contain with no fallback, and QEMU emits no GENET link.
      Root must publish an atomic handoff-pending generation before DGHO,
      preserve only the unfaulted legacy drain in that phase, and defer every
      retry until the legacy coordinator plus root RX/TX frontiers are empty.
      Before READY, tests must prove the finite cutover masks the exact source,
      stops MAC RX with readback, retains QUIESCING across the generated 10 ms
      settle, clears only RDMA `DMA_EN` while retaining the default-ring enable
      and configuration, and requires DMA status bit 0 within the 5 ms timeout
      before freezing the producer. Hold
      private RX, TX reclaim, a pending direct cursor commit, and a retained
      handler lifetime independently and prove only the immutable, at-most-32
      descriptor frontier drains. The final fence must revalidate the stopped
      hardware and generation, clear retained raw sources, publish direct
      ownership while ingress remains stopped, then resume RDMA, MAC RX, and
      the exact source in order with readback. Between the final empty
      source/frontier recheck and direct-generation publication, it must issue
      exactly one unconditional ACK on the admitted IRQHandler so a
      queued-but-unobserved legacy notification cannot remain kernel-masked.
      The ACK must precede direct publication, RDMA resume, MAC RX resume, and
      source unmask; its failure must fault before READY with ingress still
      contained. It increments no IRQ-wake, packet, or cursor evidence and
      cannot recur as polling authority. Status timeout, frozen-producer or
      cursor movement, generation/token drift, ACK failure, or any
      stop/resume/unmask/readback failure must stop MAC RX/TX plus RDMA/TDMA,
      poison the link, signal the peer, and raise the standard fault without
      READY. No fence path may fabricate a notification badge, IRQ wake,
      packet, or cursor advance; a queued exact seL4 notification observed
      after READY is direct-epoch work for the same sole owner.
      Tests must cover a fault between child READY publication and root READY
      acceptance, a fence between pair-fault observation and publication, a
      retained legacy call across QUIESCING, full-ring peer rearm, blocked
      smoltcp ingress without self-poll, and direct-active IRQ unmask/handler-ACK
      failures reaching poison, peer signal, and the standard fault endpoint.
      Console-side validation failure must independently poison both of its
      owned cursor lines after raced peer progress, signal the peer exactly
      once, and then standard-fault. Direct-active service must admit at most
      one material TX or RX/finalization unit per guarded slice. Sixteen
      successive slices under continuous bidirectional pressure must yield the
      exact eight/eight share; an empty side donates its slice. Retained
      ambiguous TX/RX commit reconciliation consumes exactly one slice rather
      than preceding that bound. A malformed RX descriptor must be recycled in
      one slice while queued TX remains untouched until the next guarded slice.
      Containment must suspend GENET and delete both cross-child signal caps
      before unmapping/deleting all 32 external console mapping caps and before
      anchor revoke.
      Direct-GENET diagnostic coverage must prove the exact page-0 layout:
      control header `[0,64)`, four cursor records `[64,320)`, optional aligned
      320-byte diagnostic-v6 record `[320,640)`, record-relative sequence-last
      commit at offset 312, and still-reserved tail `[640,4096)`. ABI tests must
      prove record offset 12 round-trips the direct MCS packet-slice high-water,
      record offset 108 round-trips cumulative `dpc_level_adoptions`, and
      offsets 160, 168, and 176 round-trip nonzero raw notification receipts,
      exact-filter rejections, and the 32-bit badge union. Rejections may not
      exceed receipts, a nonzero badge union requires a receipt, every range is
      non-overlapping, maximum counter values encode/decode and render without
      truncation, and a missing,
      torn, stale, malformed, or wrong-generation record is unavailable rather
      than accepted. Root must stable-read the complete record around its commit
      and require the exact live nonzero direct generation. Ordinary packet turns
      must neither scan nor mutate the diagnostic or reserved tail.
      The 128-byte v6 extension at record offset 184 must preserve the first
      longest valid slice, reject invalid or backwards stage clocks and
      inconsistent direction/cursor/tuple fields. The three formerly reserved words
      must round-trip Signal entry/return and RX retirement completion; their
      presence and ordering must agree with the exact notification-due flag.
      Absent Signal timestamps must be zero. These samples remain within the
      original packet interval and cannot move or suppress a peer notification.
      Missing, reversed or contradictory samples are discarded observationally.
      Runtime header sampling must retain the validated RX frame length across
      commit reconciliation and occur before that same DMA slot is rearmed.
      Non-IPv4, fragmented or truncated headers must not fabricate a TCP tuple.
      Invalid observations must not change the scheduling decision or fault the
      device. The receipt measures its own slice, including descheduling and
      kernel time; it cannot identify wire arrival or consumed SC time.
      One active wired `netstats` request may issue exactly one idempotent,
      generation-bound DGHO replay. Tests must retain the stable pre-replay
      sample, label the replacement `phase=pre-idle-service`, and emit a complete
      available batch in this order: `genet_direct`, `genet_direct_flags`,
      `genet_direct_before`, `genet_direct_before_ring`, `genet_direct_irq`,
      `genet_direct_irq_source`, `genet_direct_notification`,
      `genet_direct_dpc`, `genet_direct_dma`, `genet_direct_ring`, then
      `genet_direct_peer`, `genet_direct_slice`, `genet_direct_slice_begin`,
      `genet_direct_slice_end`, `genet_direct_slice_packet`, then
      `genet_direct_slice_tcp`, then `genet_direct_slice_rx`. The last row retains
      exact `notify_due`, `signal_enter_ticks`, `signal_return_ticks` and
      `retired_ticks`; empty or absent stages use zero timestamps. Host trace
      fixtures must preserve legacy eleven/sixteen-row and current seventeen-row
      batches, including partial diagnostics without promoting network or
      acceptance evidence. The DPC row must retain the cumulative dense-window
      yield/fault reason mask and maximum measured packet-slice duration without
      granting scheduling authority. Runtime coverage must prove one direct
      notification can consume successive exact packet units through caller-local
      `Reenter` without generic command arbitration, while the elapsed guard and
      16-attempt cap are sampled between units. Peer wakes, IRQ ACKs, and
      bookkeeping-only transitions must report zero slice units; TX/RX issue,
      retained reconciliation, and malformed RX recycle must each charge the
      existing exact unit. Unresolved reconciliation must report no progress and
      cannot reset the one-stalled-retry guard. A final slice with exact empty
      rings and a rearmed source must close only the userspace episode before
      Block; a later reciprocal peer or IRQ wake must run without inheriting
      quiescent FRESH/GUARD/CAP/STALLED state. Continuously durable work must
      retain those guard/cap/stalled decisions, and an endpoint command's FRESH
      requirement must survive quiescent closure. Aligned volatile word-copy
      tests must retain identical
      frame bytes, descriptor/cursor ordering, and sequence-last commit while
      leaving bounded unaligned prefixes/tails bytewise. Runtime coverage must
      count each actual nonzero GENET receive result exactly once before badge filtering across the initial poll
      and every later combined wait, while proving the counters cannot service
      an IRQ/DPC/packet or alter scheduling. The replay is deliberately
      causal: waking GENET may allow its normal post-command idle path to drain
      already durable RX. Tests must reject a recurring poll, second replay,
      packet or IRQ authority, fallback, retry, recovery, or a claim that the
      before/after delta is a passive performance sample.
      A post-replay record is `fresh` only when a stable pre-replay sequence was
      available and the accepted sequence changed. READY with no stable
      pre-replay record is `ready-unverified`, even if a record is visible
      afterward; it cannot be promoted to fresh causal evidence.
      The `cohsh` TCP fixture must preserve all seventeen ordered rows. Legacy v4
      eleven-row and v5 sixteen-row captures remain parseable; partial v5/v6
      batches remain individual observational rows, without an asserted
      completeness verdict. The Pi trace
      normalizer must classify `genet_direct*` as wired-driver evidence before
      generic network parsing, prevent a component `active=yes` flag from
      overwriting canonical `NET_ACTIVE`, keep legacy traces with no optional
      rows parseable, and never label a truncated or incomplete batch as
      complete causal evidence. No row or batch supplies TCP, performance, or
      Pi-acceptance authority.
      Fresh same-boot wired evidence must prove DHCP/static policy, ARP, ICMP,
      raw TCP, authenticated `cohsh`, focused `.coh` scripts, queue/IRQ health,
      loss, latency, and throughput before any Pi GENET or overall performance
      claim. Pi 4
      manifest-default boots must
      use
      `hw.local_seat.enabled=true`, `hw.local_seat.required=true`, and matching
      `usb-kbd0`/`hdmi0` `hw.devices[] required=true` declarations so missing
      declared devices fail visibly. Runtime backend attach failures may
      degrade with `required=yes action=serial-shell`; that keeps the UART root
      shell reachable but does not satisfy HDMI/USB acceptance.
  - `netstats` must report:
    - `mode=<off|static|dhcp> policy=<wired|wifi|auto> active=<iface> standby=<iface|none> addr_src=<source> ip=<ipv4> gateway=<ipv4> dhcp=<phase>`; the normalizer exposes the selected state as `NET_ACTIVE`, `NET_ADDR_SRC`, and `NET_DHCP`, and separately exposes command/listener proof as `NET_TCP_READY` and `NETTEST_PROOF`. Component-local booleans such as `netstats: cyw43_priority_lease ... active=yes|no` cannot overwrite the selected interface.
    - exactly one complete `nettest: generation=<connection> run_generation=<run> enabled=<bool> running=<bool> verdict=<none|running|pass|peer-assisted-pass|fail> tx_ok=<bool|na> udp_echo_ok=<bool|na> tcp_ok=<bool|na> console_ok=<bool|na> peer_assisted_ok=<bool|na>` status line. `OK NETTEST detail=started run_generation=<run>` admits one immutable run; only a terminal line for the same positive run generation is proof. An internal-only asynchronous log, an incomplete or truncated line, or a prior connection/run-generation verdict is not terminal proof; backend and target strings remain on the separate `nettargets:` line.
    - canonical interactive Pi diagnostics must load a strictly valid clean `pi4-image-identity.json` before opening serial and observe its exact complete build marker before the fresh root prompt. A generic marker prefix or marker from another image is not source/image provenance. This marker binds only the root image; the complete boot partition and physical media retain their separate hash/readback obligations.
    - when the compiler-declared console-network child owns TCP/IP, `nettest` must not inherit the root adapter's default `unsupported` result. Its existing 15-second generation remains peer-assisted and must observe its state on every selected console poll even when ordinary network activity is already true. A run that starts without a live peer binds the first later authenticated connection to that connection's fresh zero byte-counter epoch; it must latch same-connection command bytes read, response bytes written and exactly drained, listener readiness, and later RX/TCP progress. A physical backend additionally requires a NIC TX completion observed while that exact connection remains current; direct VirtIO uses the exact child drain and requires no synthetic root NIC completion. Once the bound identity disappears, neither a replacement connection nor later unrelated NIC activity may provide a missing fact, though already established connection facts remain available for truthful terminal reporting. Historical traffic cannot satisfy a new run. The unchanged terminal schema reports `udp_echo_ok=false` when only peer-assisted proof is present. Native ICMP echo response is a separate reachability check and cannot be relabelled as the UDP self-test.
    - the controlled live gate and serial helper must preflight canonical `cohsh`, manifest, credential, and workload inputs before acquiring the UART. After one positive admission they must select only the exact command-bound DHCP lease for the required physical lane, validate the corresponding host route, run one authenticated peer for the canonical observation window, always reap it, and still accept only the generation-matched target terminal. Ambiguous or stale status, wrong-lane or invalid addressing, route/input drift, peer failure, or incomplete terminal evidence fails closed; peer exit alone is never acceptance.
    - isolated child liveness triage must retain the bounded `netstats: isolated_progress`, `netstats: isolated_units`, and `netstats: isolated_state` rows. They distinguish selected child observation/output/disconnect/ingress/tick/egress/diagnostic turns, material-progress time, command/output queue depth, pending egress, response-drain state, and ingress backpressure/drop. Additive direct-GENET fields are `pcont=<candidates>/<admitted>/<rejected> peff_us=<n> preason=0x<n>`, `output_ok=<n>`, and `ycalls=<n> ycredit_us=<n> yinvalid=0x<n>`. `peff_us` is observational and never admission authority. `preason` uses fence/cap/clock/policy/counter/arithmetic bits `0x01` through `0x20`, retains `0x40` as a schema-reserved retired bit, and uses `0x80` for token rejection; `yinvalid` uses pre-drain/counter-or-frequency/syscall-result/overflow bits `0x01` through `0x08`. The fields are zero outside the exact Pi direct-GENET path. Pi `netstats` causal triage additionally retains the six bounded fast-path rows `cyw43_publication`, `cyw43_publication_cut`, `cyw43_productive_window`, `genet_compact`, `genet_compose`, and `genet_defer`. `cyw43_productive_window` counts exact same-lifetime, authenticated generation/connection/accepted-command window opens and closes; its retired transient-empty `idle_admitted` field remains schema-stable and zero under event-backed continuation. It grants no refill, retry, readiness, or device authority. Every aggregate compact Deferred increments exactly one `genet_defer` counter, the reason-counter sum equals `genet_compact deferred`, and `compose_open` represents typed `NotSealed`; typed `NoPending` is interpreted jointly with exact stage-ready evidence or the `output_missing` defer and never as authority by itself. When isolated timing is available it adds exactly seven `netstats: isolated_seam schema=v2` rows with the names, decimal totals and bounded hexadecimal age histograms defined in USERLAND_AND_CLI. Regress the distinct command creation/publication/root timestamps, every histogram boundary, invalid clocks, maximum-width rendering, explicit histogram clipping, and complete serial/TCP NETSTATS terminal retention. The old v1 command-publication age included child queueing; do not infer root reaction time from it. `stage-control-observe` combines child consumption and root observation; OutputDrained additionally waits for TCP ACK retirement. Every physical-Pi seam endpoint must use the shared absolute `CNTVCT_EL0` epoch and generated `TIMER_CLOCK_HZ`; a root-elapsed/child-absolute pair is invalid instrumentation, not target latency. All lifetime `mcs_quantum*`/`mcs_yield*`/dispatch/pending/budget values are emitted by explicit `smp mcs` on Pi release profiles in seventeen rows, with version-2 combined exit/state, Yield cause and budget records. Three global idle rows (unchanged header and two version-2 rows of eight u64 fence counters) remain in SMP, giving twenty total MCS diagnostic rows. The eight latest-session rows remain in `netstats`. The complete selected-Pi WiFi SMP body is exactly 64 rows including every CPU row and the end marker; both physical and synchronous TCP delivery retain their protocol terminal. Latest-session Yield cause counts use the six fixed trigger positions defined in USERLAND_AND_CLI, reset only on a different nonzero identity, exclude invalid clocks and saturate without truncating the row. Both batches must remain bounded, count invalid/backwards time separately, and omit QEMU-release accounting and output. These counters diagnose where progress stopped; they are not TCP, performance, or acceptance evidence by themselves.
    - `tx_submit=<count> tx_complete=<count> tx_free=<count> tx_in_flight=<count> tx_double_submit=<count> tx_zero_len_attempt=<count> arp_rx=<count> arp_tx=<count>`; on CYW43, `tx_complete` is the root release count from exact joined Function-2 terminals. `tx_submit > tx_complete` means an outstanding root TX owner, not a missing firmware-credit acknowledgement.
    - `wifi_assoc=<0|1> wifi_link=<0|1> eapol_rx=<count> eapol_start=<count> eapol_secure=<0|1>`
    - driver-task scheduling evidence for the active hardware path in reopened 26a/26b acceptance captures: contract name, service class, isolation mode, poll/service count, budget exhaustion/yield count, RX/TX queue depth, drop count, manifest-selected affinity core, observed service latency, and timer backend proof. The normalizer exposes this as `TIMER_BACKEND`, `TIMER_CLOCK_HZ`, `TIMER_EL0_COUNTER`, `DUMMY_TIMER_SEEN`, `DRIVER_TASK_CONTRACTS`, `DRIVER_TASK_DEDICATED`, `DRIVER_TASK_COMPATIBILITY`, `DRIVER_TASK_DEDICATED_READY`, `DRIVER_TASK_SERIAL_DEDICATED`, `DRIVER_TASK_USB_DEDICATED`, `DRIVER_TASK_DISPLAY_DEDICATED`, `DRIVER_TASK_NET_DEDICATED`, `DRIVER_TASK_SDIO_DEDICATED`, `DRIVER_TASK_PCIE_DEDICATED`, `DRIVER_TASK_SUBSTRATE_READY`, `DRIVER_TASK_FAILED_COUNT`, `DRIVER_TASK_CAPSET_PROOF`, `DRIVER_TASK_FAULT_PROOF`, `DRIVER_TASK_REVOKE_PROOF`, `DRIVER_TASK_SCHED_PROOF`, `DRIVER_TASK_AFFINITY_PROOF`, `DRIVER_TASK_AFFINITY_CONFIGURED`, `DRIVER_TASK_AFFINITY_APPLIED`, `DRIVER_TASK_AFFINITY_MANIFEST_PROOF`, `DRIVER_TASK_AFFINITY_MANIFEST_MATCHES`, `DRIVER_TASK_AFFINITY_MANIFEST_MISSING`, `DRIVER_TASK_AFFINITY_MANIFEST_MISMATCHES`, `DRIVER_TASK_VSPACE_PROOF`, `DRIVER_TASK_POINTER_FREE_IPC_PROOF`, `DRIVER_TASK_OWNER_STATE_PROOF`, `DRIVER_TASK_DMA_PROOFS`, `DRIVER_TASK_DMA_BLOCKER`, `PI4_RUNTIME_DMA_PROOF`, `PI4_RUNTIME_DMA_PROOF_REASON`, `PI4_RUNTIME_DMA_COUNTER_PROOF`, `DRIVER_TASK_ACTIVE_NET`, `DRIVER_TASK_BUDGET_OVERRUNS`, `DRIVER_TASK_LATENCY_PROOFS`, `DRIVER_TASK_RING_CALL_BEGIN`, `DRIVER_TASK_RING_CALL_RETURN`, `DRIVER_TASK_RING_CALL_OUTSTANDING`, `DRIVER_TASK_RING_CALL_TIMEOUT`, `DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT`, `DRIVER_TASK_BOOTSTRAP_DEFERRED`, `DRIVER_TASK_RESOURCE_INIT`, `DRIVER_TASK_RESOURCE_BLOCKER`, and `DRIVER_TASK_RESOURCE_CURRENT_BLOCKER`. `DRIVER_TASK_OWNER_STATE_PROOF=yes` must be backed by per-hot-path owner-state descriptor lines for serial, USB, HDMI, PCIe, and the selected network owner set (`cyw43-wifi` plus `sdio-host` when `DRIVER_TASK_ACTIVE_NET=cyw43`, or `genet-nic` when `DRIVER_TASK_ACTIVE_NET=genet`). Pi 4 performance evidence must report `TIMER_BACKEND=arch-counter`, `TIMER_CLOCK_HZ=54000000`, `TIMER_EL0_COUNTER=vct`, `DUMMY_TIMER_SEEN=no`, `DRIVER_TASK_DMA_BLOCKER=none`, and `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`; otherwise latency proof is red even if driver-task owner-state proof is present. `DRIVER_TASK_RESOURCE_BLOCKER` is the first lost resource proof in the capture; `DRIVER_TASK_RESOURCE_CURRENT_BLOCKER` is the latest non-ready resource-init blocker. The source `DRIVER_TASK_RESOURCE_INIT` line carries the current isolated runtime owner/action, active request, `expected_request_valid` / `expected_aux0_valid`, expected aux/request values when present, same-request flag, and child progress marker needed to diagnose the live turn. Any positive `DRIVER_TASK_RING_CALL_OUTSTANDING`, `DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT`, `DRIVER_TASK_BOOTSTRAP_DEFERRED`, or non-`none` resource blocker is an isolated runtime no-reply/deferred-proof frontier; raw `DRIVER_TASK_RING_CALL_TIMEOUT` counts remain diagnostic when a later return closes the same request. Contract-only root-task compatibility evidence, resource-init breadcrumbs, and declared `max_service_us` budgets are diagnostic and must not be counted as dedicated driver-task closure or latency proof.
    - routine WiFi, GENET, and HDMI call begin/return chatter may be absent in steady, nonblocking, prompt-slice, and retained modes. Initialization, descriptor non-acceptance, fault, budget-exhaustion, and non-quiet timeout evidence remains required; absence of a routine call row cannot be treated as progress or failure proof.
    - a Wi-Fi pair-recovery diagnostic may retain four pre-scrub rows named `scheduler_sdio_fault`, `scheduler_sdio_status`, `scheduler_sdio_dma`, and `scheduler_sdio_regs`. `captured=yes` is valid only for two stable reads of an exact Fault completion with version-3 116-byte payload, aligned in-ring cursor, contained or owner-poisoned flag, matching magic/version, and matching terminal result. `captured=no` is unavailable evidence, and all-zero rendered words in that case are not register values. These passive rows can distinguish SDHCI inhibit/status from DMA4 `CS.ERROR`, control-block, or debug state, but cannot satisfy owner, DPC, association, traffic, performance, or acceptance gates.
    - driver-task counter evidence for performance triage is separate from owner-state proof. Activity-gated `DRIVER_TASK_COUNTER` lines are normalized as `DRIVER_TASK_COUNTER_SNAPSHOTS`, `DRIVER_TASK_COUNTER_INVALID`, `DRIVER_TASK_COUNTER_BUSY`, `DRIVER_TASK_COUNTER_SAME_REQUEST`, `DRIVER_TASK_COUNTER_TIMEOUTS`, `DRIVER_TASK_COUNTER_KEEP_ACTIVE`, `DRIVER_TASK_COUNTER_ABORTS`, `DRIVER_TASK_COUNTER_STAGED_BYTES`, `DRIVER_TASK_COUNTER_CACHE_OPS`, `DRIVER_TASK_COUNTER_CACHE_BYTES`, `DRIVER_TASK_COUNTER_RX_FRAMES`, `DRIVER_TASK_COUNTER_TX_FRAMES`, `DRIVER_TASK_COUNTER_RX_BYTES`, and `DRIVER_TASK_COUNTER_TX_BYTES`. `DRIVER_TASK_COUNTER_SNAPSHOTS` counts distinct `(contract, hot_path)` owners and the totals aggregate only each owner's latest cumulative snapshot; repeated diagnostic commands therefore cannot inflate activity. `DRIVER_TASK_COUNTER_INVALID` still counts every observed empty, truncated, non-root-ring, or otherwise malformed line, including an invalid sample superseded by a later valid one. Reopened 26b performance evidence must keep `DRIVER_TASK_COUNTER_INVALID=0`, and selected Wi-Fi counter qualification requires current CYW43 plus SDIO owner snapshots rather than unrelated USB, HDMI, or PCIe activity.
    - the canonical Pi connectivity diagnostic sequence must bracket its workload with two complete `smp activity` batches. The first may report `sample=first run_again=yes`; the second must report a positive `window_ms` counter-delta view and cannot report `sample=first` or `status=stale`. This routes serial, USB, HDMI, and selected-network pressure but is not a benchmark or performance-acceptance result.
    - CYW43 runtime RX proof must include bounded glom/data service counters: Function 2 runtime block reads, glom descriptor/subframe counts, queued/dropped frames, budget yields, and runtime RX oversize recoveries. Control-plane reply reads and runtime data/glom reads must be reported separately when diagnosing Wi-Fi latency.
    - responsiveness evidence under network load: `SERIAL_RESPONSIVE_PROOF=yes`, `USB_BURST_PROOF=yes`, `USB_BURST_DROPS=0`, `USB_POST_FIRST_BYTE_BLOCKER=none`, and `HDMI_RESPONSIVE_PROOF=yes`.
    - wired 26a closure must show `NET_ACTIVE=wired`; Wi-Fi 26b closure still requires `active=wifi`, `addr_src=dhcp-lease`, `dhcp=bound`, `eapol_secure=1`, non-zero TX/RX packet counters, and `WIFI_OLDGOOD_REPLAY=yes` / `WIFI_OLDGOOD_MISSING=none`. The Wi-Fi replay contract requires SDIO and CYW43 owner-state, isolated SDIO engine readiness, transport/firmware/Function 2 readiness, matched Linux-shaped control setup, primary join request, association plus link-up proof, explicit host-EAPOL M1/M2/M3/M4, PTK/GTK install, secure release, DHCP, nettest, and final netstats. Readiness and Function 2 proof must be positive, the primary join result must be success, nettest must report pass/success, and a condensed `join complete ... m1=yes m2=yes m3=yes m4=yes` line alone is not replay proof.
    - `netstatus: ip=<ipv4> gateway=<ipv4> src=<source> dhcp=<phase> tcp_ready=<yes|no>`
  - `nettest` refusal detail must preserve the reason when the run cannot start:
    - `detail=dhcp-pending`
    - `detail=wifi-associating`, `detail=wifi-host-eapol-pending`, `detail=wifi-host-eapol-required`, `detail=wifi-association-failed`, or `detail=wifi-link-down`
    - `detail=not-ready:<root-ep|ipc-buffer|cspace-window|bootstrap-commit>`
    - `detail=policy-disabled` or `detail=selftest-disabled` when the profile/runtime disables self-test
  - explicit `wifi` now supports both `static` and `dhcp` through the HAL-backed CYW43455 path; `auto` remains DHCP-only and single-active-interface. On the physical driver-task profile, bounded credentials select CYW43 and selected-CYW43 attach/join/runtime failure is fatal driver evidence rather than wired fallback; QEMU/host compatibility profiles may retain absent-device fallback coverage. Final 26b compatibility evidence still requires Pi 4 hardware captures proving join + DHCP and documenting which fallback profile, if any, was exercised.

### Conditional G — Release bundle validation (macOS, Linux and Pi4)

This is mandatory when shipping the three `Cohesix-1.0.0-beta` archives. It runs
after the applicable five-stage plan and all M26e hardware, pressure,
repeatability, Worker/full-system and review gates. Stage 05 alone does not run
this conditional action. Follow [HOST_TOOLS release factory](HOST_TOOLS.md#release-factory)
to build/test on Mac and the selected Linux ARM64 builder and assemble candidates
under `releases/`. The catalogued `release.bundle-validation` action is an
executable final verifier and requires all three installation result records.

Extract each archive into a fresh directory outside the source checkout and
outside `releases/`. Keep the original tarball beside its extracted folder.
Do not reuse repository binaries, a prior release extraction, or a rebuilt guest.
Prepare Python/pytest and the existing Playwright dependencies on each host;
Linux also requires `xvfb-run`. Configure `COH_AUTH_TOKEN` or
`COHSH_AUTH_TOKEN` securely in the environment; it is never recorded in argv.

On Mac, and then independently on Linux using the `-linux` paths:

```bash
python3 scripts/release_qualify.py host \
  --bundle <clean-extraction>/Cohesix-1.0.0-beta-MacOS \
  --archive <clean-extraction>/Cohesix-1.0.0-beta-MacOS.tar.gz \
  --port 31337 --output <fresh-evidence>/macos/result.json
```

The command checks the exact manifest and archive, executes all eight native
binaries, checks replay, installs the packaged wheel into an isolated temporary
venv, runs `python/cohesix-py/tests`, boots the packaged `qemu/run.sh`, runs its
packaged authenticated TCP EXPECT script, and runs the existing SwarmUI replay
presentation suite against the extraction. The selected TCP port and its next
two ports must be free. The Linux image uses the native KVM timer/profile;
Mac uses HVF. The final record remains installation smoke evidence, not a new
M26e performance or full-system claim.

For the first Pi4 SD-image release, qualify the actual distributed `.img`:

1. Extract `Cohesix-1.0.0-beta-Pi4.tar.gz` into its own clean directory. Verify
   its manifest and image SHA-256. Identify a removable whole SD card whose byte
   capacity is at least the metadata's `minimum_target_bytes`. Follow the
   packaged QUICKSTART and HARDWARE_BRINGUP device-identification rules to write
   the raw image. This is an explicitly selected whole-card installation, not
   the routine stage-to-existing-FAT development reflash.
2. Before booting or changing the card's FAT contents, read back the exact image
   prefix. Supply the same explicit whole-disk device (macOS raw device or Linux
   block device) used for the write. Elevate this read-only command if needed:

   ```bash
   python3 scripts/release_qualify.py media \
     --bundle <clean-extraction>/Cohesix-1.0.0-beta-Pi4 \
     --archive <clean-extraction>/Cohesix-1.0.0-beta-Pi4.tar.gz \
     --device <explicit-whole-disk-device> \
     --output <fresh-evidence>/media/result.json
   ```

   The verifier reads exactly the distributed image length, rejects a smaller
   card or mismatched bytes, and retains the device and image identity. Extra
   card capacity remains outside the image and unallocated. Layout validation
   requires one FAT32 MBR partition at LBA 2048 and the exact compiler payload.
3. Eject and freshly boot that card. Use the single serial owner to retain a
   new log containing exactly this one boot. Exercise initial network setup and
   confirm authenticated networking. This verifies the first-install path;
   do not silently import a development card's saved `cohesix.env`. Select wired
   or Wi-Fi according to the release claim; the full M26e every-boot acceptance
   requirements remain independently applicable.
4. After observing successful provisioning, run the packaged-client TCP check:

   ```bash
   python3 scripts/release_qualify.py pi4 \
     --bundle <clean-extraction>/Cohesix-1.0.0-beta-Pi4 \
     --archive <clean-extraction>/Cohesix-1.0.0-beta-Pi4.tar.gz \
     --media-result <fresh-evidence>/media/result.json \
     --serial-log <fresh-single-boot-serial.log> \
     --host-bundle <clean-extraction>/Cohesix-1.0.0-beta-MacOS \
     --host <pi-ip> --provisioning-verified \
     --output <fresh-evidence>/pi4/result.json
   ```

   `--provisioning-verified` records the operator's observed initial-configuration
   check. The tool independently checks the media/archive binding, sealed build
   marker, one root-console boot and the packaged authenticated TCP operation.
   It does not turn this smoke into complete Pi hardware/performance acceptance.

Copy each result directory with all its logs back to the release host. Then run
the executable catalog action with a fresh final output directory:

```bash
python3 scripts/release_qualify.py verify \
  --macos-result <fresh-evidence>/macos/result.json \
  --linux-result <fresh-evidence>/linux/result.json \
  --pi4-result <fresh-evidence>/pi4/result.json \
  --releases-dir releases --output <fresh-evidence>/final/result.json
```

The catalog exposes the same inputs as `TP_RELEASE_MACOS_RESULT`,
`TP_RELEASE_LINUX_RESULT`, `TP_RELEASE_PI4_RESULT`, `TP_RELEASE_DIR` and
`TP_RELEASE_RESULT`. Missing/failed checks, changed logs or archives, an old
version, or different source commits fail closed. Retain the final result and
the complete evidence directories with the release delivery record. Only after
this gate and the independently required M26e acceptance/reviewer gates pass may
the candidate archives be published.

### Automated Stage 05 — Release governance and attestation
- `scripts/ci/test_plan_stage_05_due_diligence.sh`
- In staged mode, `scripts/ci/due_diligence_gate.sh` verifies the
  source-bound Stage 01/02 attestations and Stage 03/04 target result manifests.
  It does not rerun formatting, Clippy, workspace check/tests, generated
  contracts, the risk bootstrap, or regression scripts.
- Reused Stage 03 evidence must pass `qemu_artifact.py verify-aggregate` for
  the exact target, claim tier, source digest, catalog action digest, and all
  four regression groups; non-empty logs or pass counts alone are insufficient.
- Stage 05 uniquely runs required audit-asset checks, `cargo audit`,
  `cargo deny check advisories`, findings/exception lifecycle validation, and
  the hardcoded-secret scan. It records the audit-tool versions and all
  governance logs in an immutable Stage 05 artifact root.
- Stage 05 is deliberately refreshed even when Stages 01-04 resume. Advisory
  data and governance state are time-sensitive, so an older valid Stage 05
  attestation never suppresses the current audit.
- Direct standalone `scripts/ci/due_diligence_gate.sh` remains exhaustive and
  executes every mandatory baseline plus the regression batch unless
  provenance-bound reuse is supplied explicitly.
- The due-diligence gate fails on the first failed or incomplete check by
  default. Use `--collect-all` or `DD_COLLECT_ALL=1` only when a diagnostic run
  should continue to accumulate all failures.
- Do not progress beyond this stage until all prior attestations verify and the
  due-diligence gate is green.

Verifier context must bind controls only to stages they govern: `DD_*` selectors
belong to Stage 05, while applicable `TP_*`, source, toolchain, and target
selectors remain bound throughout. The evidence-runner tests enforce this
selector isolation; old successful finalization is not evidence for new inputs.

### Milestone 26e production-surface and fallback-retirement gate

The compiler-owned source is `configs/implementation_surfaces.toml`; the only
accepted generated inventory is
`configs/generated/implementation_surface_inventory.json` with schema
`cohesix-implementation-surface-inventory/v1`. Run:

```bash
cargo test -p coh-rtc implementation_surface
python3 scripts/ci/check_implementation_surfaces.py \
  --inventory configs/generated/implementation_surface_inventory.json
cargo test -p root-task --tests production_fallbacks
cargo test -p gpu-bridge-host
scripts/release_bundle.sh --check-manifest --pi4-stage-dir out/pi4-sd \
  --macos-artifact <accepted-mac-artifact.json> --macos-result <accepted-mac-base.json>
scripts/check-generated.sh
```

Generation fails on missing, duplicate, or stale package/target/feature/public
surface rows and on any production-reachable fixture class. The source/drift
guard independently resolves Cargo metadata, selected entrypoints and feature
closures, tracked current claims, compiled spin/no-op bodies, operational
fallbacks, and the exact release artifact set. WorkerBus is the sole legal
`model_only` role. A fixture, host model, diagnostic, contract, deferred,
retired, or not-enabled row cannot satisfy target, release, attestation,
integration, or use-case evidence.

`scripts/release_bundle.sh --check-manifest --pi4-stage-dir <path>` validates the
inventory-selected version, source-bound accepted host/guest hashes and native
GICv3/timer profile, passing TCP evidence, and the compiler-owned exact Pi 4 SD staging
set. The Pi gate rejects missing, extra, linked, stale, non-current, or
primary/fallback-divergent files and re-verifies the sealed image identity. It
also validates every individually listed document, script, Python artifact, UI
asset, trace/transcript fixture, support file, and versioned migration. Bundle
creation compares each host bundle against `release.expected_bundle_files` and
the peer Pi4 bundle against `release.expected_pi4_bundle_files`; each emits
`MANIFEST.sha256`. Recursive wildcard copies, ignored files, missing files, and
unexpected files fail. The Pi4 builder creates a compact MBR/FAT32 raw image
from the exact stage, mounts it read-only, and rechecks every embedded
regular-file hash before release. A Linux bundle
additionally requires exact source-bound provenance for every remote-built ELF
and creates the final tarball on the argument-selected ARM64 builder before
downloading and byte-validating it. Neither SD packaging nor remote archiving is
Pi, QEMU, Worker, or full-system target acceptance.

GPU bridge tests cover real-or-empty live registry behavior, no first-model
activation, placeholder-secret rejection before connection, exact manifest and
CAS identities, base/adapter compatibility, source epoch/sequence monotonicity,
activation receipt validation, stale snapshot rejection, and TTL withdrawal to
`unavailable`. The target begins with no GPU, model, lease, temperature, node,
unit, or provider fixture state. QEMU acceptance must use the selected GICv3
MCS closure and fresh build output; this gate is not Pi hardware evidence and
does not modify or reclassify CYW43 behavior.

### Milestone 26e executable-slot and critical-TCB admission gate

Compiler admission must prove the generated maximum role mix, exact object and
untyped bounds, per-core schedulability, unique capabilities, registry sealing,
and constructor rollback. Target evidence must separately prove critical-duty
startup, real Worker READY and work, timeout/fault routing, generation-scoped
Reply recovery, reclamation, and operator liveness.
Stage 01/02 own the compiler and constructor contracts. The explicit component,
root, and system validators in `worker_task_evidence.py` enforce the live
outcome matrices; ordinary stage markers alone do not close this gate.

### Milestone 26e NineDoor service-isolation gate

Require the exact generated ABI/image binding and live passive bootstrap,
ordinary authenticated namespace operations, and both during-Call fault and
between-Calls revoke containment on separate fresh exact-artifact boots.
A readiness marker alone cannot prove donated-SC/Reply return or teardown.
Stage 01 owns the ABI, runtime, boundary, and packaging tests. The QEMU service
collector validates both fault/revoke transcripts; root acceptance requires
the matching containment outcomes and donated-time return or revocation.

### Milestone 26e console-network service-isolation gate

Require generated ABI/image/transport parity, exact frame and terminal ordering,
bounded response ownership and fairness, real TCP close/relisten, negative
framing and pool recovery, service containment, and uncached target pressure.
Stage 03 and Stage 04 remain distinct mandatory TCP and REST gates.
Stage 01 owns the default and direct-GENET runtime contracts; Stages 02-04 own
exact builds and TCP/REST results. Root/system acceptance and the pressure
collectors enforce the separate containment, liveness, and load outcomes.

### Milestone 26e linked-driver MCS and coexistence gate

Require exact selected-profile resource attributes, pointer-free ABI and
sequence-last publication, sole physical ownership, MCS fault/Reply policy,
coexistence, bounded driver recovery, and independent serial/local-seat liveness.
QEMU and host contracts cannot supply Pi IRQ, DMA/cache, Wi-Fi, GENET, USB, or
HDMI acceptance.
Stage 01/02 own the driver/ABI contracts and target compilation. Conditional F
and the explicit Pi component/system validators own physical and coexistence
evidence; stage-only runtime/DMA metadata cannot satisfy their live gates.

## Trace replay limits
<!-- coh-rtc:trace-policy:start -->
### Trace replay limits (generated)
- `trace.format.version`: `1`
- `trace.hash`: `sha256`
- `trace.max_bytes`: `1048576`
- `trace.max_frame_bytes`: `8192`
- `trace.max_ack_bytes`: `2304`

_Generated by coh-rtc (sha256: `fa11c64fe53b859365c45c8e33e565d428029a87529be00cd158fd6336b6484e`)._
<!-- coh-rtc:trace-policy:end -->

## Manifest fingerprints
- `configs/root_task.toml` — `sha256:a9c7718df5d929230230c74ecdffa82dec23b16d9d268f952b36e9c8093c2bd7`
- `configs/generated/root_task_resolved.json` — `sha256:2e8a6e3aab7aa80832ec92c064970de16b02a98899a1823bb92f1f04875e19be`
- `configs/root_task_pi4_uboot_aarch64.toml` — `sha256:a67be421617a22b8aca017835bfc5798ca99037ebffaabaa97b9ab0295b2333e`
- Pi `pi4_production` transient resolved binding — `sha256:a48a867083142b652cc97294a754284e8033bc7c7bca6b4a31d6d73f9b80653a`

## Transcript fixture hashes
- `tests/fixtures/transcripts/boot_v0/serial.txt` — `sha256:2ea58218a937f0c702fd67dac83aa838a8c49b9d1fba1e0165dfa93a44ab3c6d`
- `tests/fixtures/transcripts/boot_v0/core.txt` — `sha256:2ea58218a937f0c702fd67dac83aa838a8c49b9d1fba1e0165dfa93a44ab3c6d`
- `tests/fixtures/transcripts/boot_v0/tcp.txt` — `sha256:2ea58218a937f0c702fd67dac83aa838a8c49b9d1fba1e0165dfa93a44ab3c6d`
- `tests/fixtures/transcripts/abuse/serial.txt` — `sha256:8b674462606ff7d0d324d7678d8d3700583611296f83e32af1a041790e84b6c8`
- `tests/fixtures/transcripts/abuse/core.txt` — `sha256:8b674462606ff7d0d324d7678d8d3700583611296f83e32af1a041790e84b6c8`
- `tests/fixtures/transcripts/abuse/tcp.txt` — `sha256:8b674462606ff7d0d324d7678d8d3700583611296f83e32af1a041790e84b6c8`
- `tests/fixtures/transcripts/converge_v0/serial.txt` — `sha256:dafd88f7d7e984454e12815ccffd203f98c446d0eb1e8a364d79805aa69de017`
- `tests/fixtures/transcripts/converge_v0/core.txt` — `sha256:dafd88f7d7e984454e12815ccffd203f98c446d0eb1e8a364d79805aa69de017`
- `tests/fixtures/transcripts/converge_v0/tcp.txt` — `sha256:dafd88f7d7e984454e12815ccffd203f98c446d0eb1e8a364d79805aa69de017`
- `tests/fixtures/transcripts/converge_v0/cohsh.txt` — `sha256:dafd88f7d7e984454e12815ccffd203f98c446d0eb1e8a364d79805aa69de017`
- `tests/fixtures/transcripts/converge_v0/coh.txt` — `sha256:96b57611f848ef6f9691678df8b20f261dffd47db449cd63459f12f166c0f4a7`
- `tests/fixtures/transcripts/converge_v0/swarmui.txt` — `sha256:367fe0ef871277d7e3606a6747946304f1dff2217c360190f2fc8dd115f015fa`
- `tests/fixtures/transcripts/converge_v0/coh-status.txt` — `sha256:b026211888edf50538f61b66c79dc6ae1eaf59cc33b8dd3506e57ae60b3606c4`
- `tests/fixtures/transcripts/control_plane_v0/cohsh.txt` — `sha256:f43434e6b3071753596e919021e573cb7f6a9831123769dd7cefb5b0c115c1ef`
- `tests/fixtures/transcripts/run_demo_v0/cohsh.txt` — `sha256:d429aa09972892adaeabed60ef2a36e4fe366eb9e730a8467a85f27870957040`
- `tests/fixtures/transcripts/peft_roundtrip_v0/cohsh.txt` — `sha256:ba07819ad952f6f03c4b2d583e5c5deb3459a07b7d61eb6052b47cf9536d7c2c`
- `tests/fixtures/transcripts/trace_v0/cohsh.txt` — `sha256:56b97a2d8486ed783d7cb93d38ea67811d93df6efcc24d7ed97265a4df1b1c4f`
- `tests/fixtures/transcripts/trace_v0/swarmui.txt` — `sha256:56b97a2d8486ed783d7cb93d38ea67811d93df6efcc24d7ed97265a4df1b1c4f`
- `tests/fixtures/transcripts/trace_v0/coh-status.txt` — `sha256:a002a369390cc197714ac291ba08531966af658ed797c569f1ece4bab9b1820b`

## Trace fixture hashes

- `tests/fixtures/traces/trace_v0.trace` — `sha256:f5cd6eb44c1b4a51f5e1516dad9a7ec1f76fae148169744c9e8e3809f9b6c30b`
- `tests/fixtures/traces/trace_v0.hive.cbor` — `sha256:977113ebcfad69272cbb15ddc57e7ce1ccd1df87baa6568704253cacc55e8e2d`

## Guard
- `scripts/ci/check_test_plan.sh` verifies hashes, required scripted-stage references, and command alignment (`python3`, workspace/tests gates); `scripts/check-generated.sh` invokes it.
