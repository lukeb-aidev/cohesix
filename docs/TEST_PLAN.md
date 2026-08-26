<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document Cohesix test fixtures, hashes, and convergence guardrails. -->
<!-- Author: Lukas Bower -->

# Test Plan

## Development order: target first, acceptance complete

During active target development, obtain real QEMU or Pi 4 evidence as early as
safely possible. Use host/unit tests to freeze target-discovered invariants and
prevent known regressions. Use the complete staged pipeline only when producing
acceptance evidence. This is a sequencing correction, not a reduction in test
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
| `pi4-driver` | pi4 | one exact-image Pi boot, touched service/device liveness, one live operation, UART liveness, and no unexpected target fault | `pi4_diagnostic / configs/root_task_pi4_uboot_aarch64.toml` |
| `worker` | qemu | canonical QEMU boot, real Worker READY, and one bounded startup/teardown/restart recovery operation | `qemu_smp_production / configs/root_task.toml` |
| `ninedoor` | qemu | canonical QEMU boot, isolated NineDoor READY, and one real 9P operation | `qemu_smp_production / configs/root_task.toml` |
| `console-network` | qemu | canonical QEMU boot, isolated console READY v3, and the fixed one-socket HELP/NETSTATS/SMP/CACHELOG matrix | `qemu_smp_production / configs/root_task.toml` |
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
10. the smallest focused host regression guard that freezes the discovered
    invariant.

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
7. Once the target fix is proved, add the smallest appropriate regression/unit
   guard that freezes the discovered invariant.
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
  --target-session out/<run>/target-session.json \
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

For the live QEMU lane, first remove both Cargo and packaged output so an old
ELF, archive, or manifest cannot enter the session, then build through the
canonical GICv3 script:

```bash
rm -rf -- target out
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
pre-READY fault, recreates it and submits one ordinary role work item for the
during-IPC standard fault, recreates it and submits one more work item for MCS
budget exhaustion, then recreates a final READY instance. GPU and LoRA work
uses bounded v2 tickets and `host-ticket-agent --run-once`; Heartbeat uses its
ordinary publish turn. These disposable work items are not the claimed source
of the later seven-action receipt matrix.

```bash
GDB=out/toolchain/arm-gnu-toolchain-15.2.rel1-darwin-arm64-aarch64-none-elf/bin/aarch64-none-elf-gdb
COMMON_GDB="--gdb $GDB --remote 127.0.0.1:1234 --target-session $RUN/target-session.json --generated-inventory configs/generated/root_task_topology.json --worker-image-manifest out/cohesix/worker-images/cohesix-worker-image-manifest.json"

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
  --target-session "$RUN/target-session.json" \
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
`raw_evidence`, `verdict`, and `blockers`. Each of the three Worker rows records
its five-part identity, state axes, image hash, READY/completion sequences,
distinct endpoint/fault badges, core, active scheduling-context budget/period,
and full per-slot compiler-admission object inventory: TCB, CNode, VSpace, page table, ASID,
frame, endpoint, notification, standard/timeout fault cap, Reply, scheduling
context, CSpace slot, and untyped-byte totals. The emitter recomputes the
compiler topology digest and derives the maximum-role inventory from the
topology payload. It then requires each observed role's attach badge, fault
badge, core, scheduling context, and per-slot object inventory to equal that
generated truth. A component PASS also requires the complete, sorted 26e event
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
manifest; `scripts/release_bundle.sh --check-manifest` rejects either an
omission or an unexpected compiler-owned generated file rather than silently
shipping a partial as-built contract.
`ROOT_CRITICAL_OBJECTS scope=constructed-actual` separately records the five
constructed duties, four restricted child TCBs, active SC/Reply counts, and
installed standard/timeout fault-cap plus registry counts; it is the bounded
actual critical-domain census and is never inferred from the admitted maximum.

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
it is a source-bound `pi4_diagnostic` SMP+MCS artifact set, and validation
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
| `target.pi4-profile` | 2 | `pi4-transport` | provisioned-target / pi4 | `"${TEST_PLAN_ROOT}/.venv/bin/python" scripts/sel4_profile.py validate --repo-managed --profile pi4_diagnostic --build-dir "${TEST_PLAN_ROOT}/seL4/build_UBOOT" --require-artifacts --for-runtime` |
| `target.root-task-pi4-release` | 2 | `pi4-transport` | provisioned-target / pi4 | `scripts/ci/test_plan_target_root_check.sh --target pi4 --sel4-build "${TEST_PLAN_ROOT}/seL4/build_UBOOT" --profile pi4_diagnostic --features release-pi4 --timer-clock-hz 54000000` |
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
| `diagnostic.guard-console-network` | NON-CLAIMING diagnostic | `non-claiming` | conditional / qemu | `cargo test -p console-network-abi && cargo test -p console-network-runtime && cargo test -p root-task --test console_network_service && cargo test -p root-task --no-default-features --features driver-tests-qemu isolated_response_lane_pays_exactly_one_ordinary_debt_after_eight_units -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu isolated_help_capture_publishes_complete_body_then_one_terminal -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu isolated_fixed_synchronous_producers_cross_batch_depth_without_end -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu bounded_sync_capture_overflow_emits_only_typed_terminal_and_reconciles_metrics -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu bounded_sync_cache_snapshot_crosses_batch_depth_and_tombstones_on_quiet_cut -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu bounded_sync_response_is_retired_on_exact_identity_loss -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu pinned_network_line_cannot_dispatch_to_a_replacement_connection -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu physical_progress_is_bounded_while_heavy_producers_preserve_network_owner -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu blocked_physical_producer_retains_an_ordered_busy_terminal_and_prompt -- --test-threads=1 && cargo test -p root-task --no-default-features --features driver-tests-qemu hal::cache::tests -- --test-threads=1 && cargo test -p root-task --no-default-features --test isolated_virtio_network_phasing -- --test-threads=1 && .venv/bin/python -m pytest -q tests/test_console_network_runtime_packaging.py tests/test_qemu_tcp_response_matrix.py scripts/ci/test_run_regression_batch.py` |
| `diagnostic.guard-pi4-driver` | NON-CLAIMING diagnostic | `non-claiming` | conditional / pi4 | `cargo test -p root-task --no-default-features --features driver-tests-pi4 --test driver_task_mcs -- --test-threads=1` |
| `diagnostic.guard-live-transport` | NON-CLAIMING diagnostic | `non-claiming` | conditional / qemu, pi4 | `cargo test -p cohsh --no-default-features --features tcp` |
| `diagnostic.python-sdk` | NON-CLAIMING diagnostic | `non-claiming` | conditional / qemu, pi4 | `scripts/ci/python_test_gate.sh --sdk-tests` |
| `diagnostic.test-plan-tooling` | NON-CLAIMING diagnostic | `non-claiming` | conditional / qemu, pi4 | `python3 scripts/ci/test_test_plan_catalog.py && python3 scripts/ci/test_test_plan_converge.py` |
| `ui.swarmui-playwright` | conditional | `ui` | conditional / qemu, pi4 | `scripts/ci/swarmui_ui_gate.sh --run` |
| `performance.gateway-telemetry` | conditional | `performance` | conditional / qemu, pi4 | evidence-only: telemetry-summary-matrix, ops-csv, ramp-csv, ramp-svg |
| `federation.three-hive-relay` | conditional | `federation` | conditional / qemu, pi4 | evidence-only: federation-result-manifest, relay-counter-snapshots, evidence-timeline, scale-summary |
| `pi4.hardware-acceptance` | conditional | `pi4-hardware` | conditional / pi4 | evidence-only: pi4-image-readback-identity, pi4-gate-proof, pi4-capture-manifest, pi4-repeatability-report |
| `release.bundle-validation` | conditional | `release` | conditional / qemu, pi4 | evidence-only: macos-bundle-result, ubuntu-bundle-result |
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

## Purpose
Validate the full Cohesix stack end-to-end: generated artifacts, QEMU boot, TCP console reliability and performance, deterministic replay, and every shipped host tool.

## Goals
- Pre-existing features continue to work; new features are validated against documented behaviour.
- QEMU boots the VM and exposes Secure9P/TCP console without protocol drift.
- TCP console remains reliable under load (no unexpected disconnects/resets/partial writes).
- Performance baselines are captured with reviewable `*.summary.json` artifacts
  and interpreted in `docs/BENCHMARKS.md` for any changes affecting
  throughput/latency. Raw artifacts normally live under `logs/bench/` or
  `out/bench/`; commit them under `docs/bench/` only when the active milestone
  explicitly requires checked-in evidence.
- Host tools behave correctly: `coh`, `cohsh`, `swarmui`, `cas-tool`, `gpu-bridge-host`, `host-sidecar-bridge`.
- Deterministic replay passes for cohsh and SwarmUI (trace + hive snapshot).
- Fixtures and manifests remain hash-consistent.

## Scope
- Source tree validation (macOS 26 ARM64 host).
- Release bundle validation on macOS 26 and Ubuntu 24 aarch64.
- Milestone-agnostic: run sections appropriate to the change set.

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
  in-kernel GICv3. Both host envelopes require generated `TIMER_CLOCK_HZ` and
  the console-network descriptor to equal the guest-visible 24,000,000 Hz
  virtual-counter frequency. TCG, `-icount`, or any other timer frequency is
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
  configuration. The launcher must still agree with the generated GICv3 and
  24,000,000 Hz timer truth.
- Before any QEMU TCP run, start tcpdump and confirm the log path (example: `logs/tcpdump-new-YYYYMMDD-HHMMSS.log`). Use the same path in TCP correlation checks.
- Headless Linux requires `xvfb-run` (`sudo apt-get install -y xvfb` if missing).
- Ensure `/updates` and `/host` are enabled for host tool tests:
  - `cas.enable = true` (and `ui_providers.updates.*` as needed)
  - `ecosystem.host.enable = true` with providers set
  - Re-run `coh-rtc` and `scripts/check-generated.sh` if toggled.
- Clear old logs if needed: `rm -rf out/regression-logs logs`.

## Performance baselines (Authoritative)
- Performance evidence is only valid when it is **stored and reviewable**:
  - Preserve the canonical harness `*.summary.json` plus associated logs or
    target proof under `logs/bench/`, `out/bench/`, or a milestone-approved
    committed evidence path; and
  - Index/interpret the result in `docs/BENCHMARKS.md` when it becomes a
    baseline, comparison point, or release claim.
- Do not use "last local run" as a baseline. If you need a new baseline,
  preserve the artifact path and update `docs/BENCHMARKS.md` in the same change.

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
  minimal `net-console`, cache-maintenance, and isolated Pi runtime suites;
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
- Pi 4 profile validation against
  the immutable `seL4/build_UBOOT` `pi4_diagnostic` artifacts, followed by the
  `release-pi4`
  AArch64 root-task check. Its independently built component bindings use the
  selected 54 MHz header; this remains compile evidence, not Pi boot or
  hardware acceptance.

The remaining Pi-specific material in this section defines evidence semantics
for Conditional F. It is not additional Stage 02 execution and must not cause
the common host suites to be replayed.

#### Pi 4 post-capture and hardware evidence semantics

CYW43 repeatability is a separate post-capture aggregate gate. Run
`scripts/pi4_wifi_repeatability.py` with the staged source image, an independently
read-back image file at a distinct path, their expected SHA-256, the exact
embedded `[BUILD]` line, the required identity-v2 sidecar plus its independently
preserved SHA-256, the expected clean Git commit and canonical build ID, a
capture-manifest-v2 ledger, and every cold/warm serial log. The default threshold
is 10 passing Wi-Fi cold boots and 10 passing Wi-Fi warm boots. Every counted
slice must carry its own exact sealed marker, show the persistent bootstrap
supervisor reaching `ready` with the complete current production suffix, have
`NET_ACTIVE=wifi`, and satisfy `boot_evidence_blockers`; failed slices cannot be
offset by additional passes. Duplicate log paths or byte-identical captures
are rejected across and within both classes. The v2 gate requires both images
to be distinct paths and open files with identical raw SHA-256, correct legacy
U-Boot structure and CRCs, exactly one canonical marker, and the same
domain-separated normalized `image-id`. That ID covers the complete image after
zeroing only its fixed self-reference and the two independently checked CRC
fields. Output may not alias any evidence input. The capture manifest must bind
each distinct raw serial boot slice to one unique run ID and one distinct,
nonempty, independently hashed pcap, plus its recorded boot class, sealed image
ID, clean commit, canonical build ID, and capture epoch. It must also carry the
trusted identity-sidecar SHA-256 supplied on the command line. Cold versus warm
is an operator-recorded reset classification, so the serial logs and boot-paired
pcaps must retain that per-run collection ledger. Unit coverage is
`python3 -m pytest -q tests/test_pi4_image_identity.py tests/test_pi4_wifi_repeatability.py tests/test_pi4_trace_normalize.py`.
Synthetic tests prove the scorer and identity binding only; a `PASS` report
requires real supplied captures and still does not replace their boot-paired
pcaps.

For host-EAPOL Wi-Fi captures, byte-level isolated runtime RX evidence must win
over downstream prompt symptoms. `CYW43_DRIVER_TASK_HOST_EAPOL_STATUS` records
with `last_rx_idle_detail=0x570a`, `0x570b`, `0x5709`, `0x570c`, `0x570d`, or
`rx_firstread_decode_miss>0` normalize to `cyw43-data-rx-firstread-empty`,
`cyw43-data-rx-firstread-invalid-sdpcm`,
`cyw43-data-rx-firstread-failed`,
`cyw43-data-rx-firstread-remainder-failed`,
`cyw43-data-rx-firstread-remainder-too-large`, or
`cyw43-data-rx-sdpcm-decode-miss` respectively, even when later `wifi diag`,
`nettest`, or `netstats` lines report the coarse `host-eapol-required`
net-disabled cause. The same status records expose the association-gated linked
receive window through `event_rx`, `control_rx`, `associated`, `link_up`,
`assoc_event`, `assoc_poll`, `post_assoc_polls`, and
`control_rx_firstread_*`; empty first-read records also expose `rxsrc_*` and
`control_rxsrc_*` fields decoded from the packed runtime result so tests and
captures can distinguish missing firmware RX source, masked CCCR/SDIO
interrupts, absent SDHCI card-int, and Function 2 readiness. Those fields
diagnose missing association/control events but do not override a more precise
nonzero `rx_firstread_*` data-RX blocker.
`CYW43_DRIVER_TASK_HOST_EAPOL_RXTRACE` detail lines must be self-contained:
the standalone line carries the RX trace flags, detail, request length, CMD53
shape, transfer result, queue pressure, IRQ-preserve decision, and the compact
VCNT-backed timing chain (`trace_seq`, `start_ticks_lo`,
`pre_sample_delta_ticks`, `transfer_delta_ticks`,
`post_sample_delta_ticks`). `scripts/pi4_trace_normalize.py --gate-summary`
promotes those fields to `WIFI_RXTRACE_*` and `WIFI_RX_IRQ_PRESERVE_*` so a
failed Pi boot can distinguish first-read transfer shape, runtime queue
pressure, interrupt preserve/clear policy, and RX timing without depending on a
large status line staying untruncated. Runtime RX IRQ preservation is valid only
for concrete queued work such as a cached SDPCM next-frame length or a nonzero
Function 1 RFRAME count; the old preserve reason code `5` is normalized as
`deprecated-source-asserted` because a pre-ACK reread of `I_HMB_FRAME_IND` can
be stale source evidence, not fresh RX admission proof. Repeated pending status
records with no semantic progress may be suppressed; the next full
`CYW43_DRIVER_TASK_HOST_EAPOL_STATUS` row reports `suppressed_status=<count>`.
Terminal `required`/`secure` rows and edge rows for events, RX observation,
EAPOL progress, admission refresh/rescue, and post-secure handling must remain
unsuppressed.
For isolated CYW43 control failures, `CYW43_DRIVER_TASK_COMMAND_NO_REPLY` records
with `reason=cyw43-runtime-command-no-reply` must normalize to Gate 7
`control-plane-reply-idle-loop`. If the cached child progress marker matches the
active request sequence and CYW43 aux tag, the marker phase is preserved as
`WIFI_EXACT` / `WIFI_PHASE`; otherwise the exact reason remains the generic
runtime no-reply. Split-control `poll-complete` and nonmatching reply samples are
context only until the same attempt emits a terminal
`CYW43_DRIVER_TASK_CONTROL_SPLIT` / `CYW43_DRIVER_TASK_COMMAND_FAULT` pair.
Valid nonmatching CDC replies may also be preserved for a later exact `(cmd,id)`
match; when that happens the later attempt must emit `cached-matched-reply` and
`cached-response-ready` split-control evidence, and must still validate CDC
status and response length through the normal matched-reply path. These cached
reply records are proof of correlation repair only, not a longer reply deadline
or a relaxed host-EAPOL `wsec_key` gate.

Reopened Milestones 26a/26b also require HAL driver-task contract coverage before hardware claims: `hal::driver_task` must validate the serial, USB/local-seat, HDMI, GENET, CYW43, SDIO host, PCIe root, RTL8139, and virtio-net contracts. Historical M26B completion evidence remains a compatibility baseline, not reopened acceptance proof. Reopened Pi 4 captures must include compact `DRIVER_TASK_*`, `SCHED_CONTRACT`, `BUDGET_OVERRUN`, observed per-driver latency, `SERIAL_ECHO`, `USB_BURST`, and `HDMI_RESPONSIVE` evidence; `scripts/pi4_trace_normalize.py --gate-summary` exposes those as machine-checkable hardware proof fields.

The Pi 4 release cold-boot UART is intentionally a decision-bearing
projection: identity, selected policy, live owner/counter/acceptance rows, and
failures remain physical, while repeated/static contract detail is retained in
the bounded Queen log. A convergence boot may route the first failed invariant
from that projection, but the projection alone is not reopened acceptance.
Before `--require-driver-task-proof` is claimed, the same boot must export the
retained `SCHED_CONTRACT` and other required `DRIVER_TASK_*` rows into its
evidence bundle; evidence must not be joined across boots. The physical
transcript must also contain `BOOT_TIMING stage=root-console-ready
elapsed_us=<nonzero> source=cntvct-el0`; host file timestamps are not target
latency proof.

Dedicated-driver-task closure is stricter than contract declaration: `DRIVER_TASK_DEDICATED` must cover the required active roles, `DRIVER_TASK_COMPATIBILITY` must be `0`, `DRIVER_TASK_DEDICATED_READY=yes` must be present, `DRIVER_TASK_FAILED_COUNT=0` must be present, serial, USB/local-seat, display, selected network, selected-role SDIO (`DRIVER_TASK_SDIO_DEDICATED=yes`) for Wi-Fi, and PCIe role booleans must all be `yes`, and substrate/capset/fault/revoke/scheduling/per-driver-affinity/VSpace plus pointer-free IPC, owner-state proof, sealed runtime descriptor proof, and active-network identity fields must all be `yes` when `scripts/pi4_gate_proof.sh --require-driver-task-proof` is used. Physical Pi bootstrap is limited to the selected generated isolated runtime hardware contracts; RTL8139 and virtio-net remain QEMU compatibility contract coverage only. Owner-state proof requires one `DRIVER_TASK_OWNER_STATE ... hot_path=<exact> owner_state=driver-owned descriptor=present descriptor_version=8 descriptor_seal=valid artifact_hash=nonzero root_pointer=no` line for each current acceptance hot path: `serial-console`, `usb-keyboard`, `hdmi-text`, `pcie-root`, and the selected network path (`genet-nic` for wired or `cyw43-wifi` plus `sdio-host` for Wi-Fi). The canonical sealed descriptor fragment is `DRIVER_TASK_OWNER_STATE ... descriptor=present descriptor_version=8 descriptor_seal=valid`. Split clients must carry `bus_link_seal=valid` for USB-to-PCIe or CYW43-to-SDIO while non-split roles report `bus_link_seal=none`. Aggregate owner-state text, inferred hot paths, inactive-network hot paths, truthy aliases such as `owner_state=yes`, historical descriptor versions, or pre-seal `descriptor=present root_pointer=no` logs without descriptor-seal fields must fail current closure.
Pi serial migration proof additionally requires
`DRIVER_TASK_IRQ_TOPOLOGY contract=serial irq=125 badge=126 handler_slot=4 notification_slot=3 trigger=level status=bound proof_effect=irq-rx-ready`
and `DRIVER_TASK_NOTIFICATION_BOUND contract=serial ... source=generated-serial-irq-topology`
before serial runtime init. A serial `poll-fallback`, `status=failed`,
`DRIVER_TASK_NOTIFICATION_BIND_DEFERRED contract=serial`, descriptor without
the exact IRQ, or physical input accepted only after slow character pacing is
acceptance-red. Focused tests must prove queue capacity is checked before
`MU_IO`, a full queue defers ACK without dropping the pending hardware byte,
later root consumption drains and acknowledges the same source, warm takeover
does not clear either root-configured FIFO and drains handoff RX before IRQ
enable/ACK, and an unrelated notification badge cannot service serial RX.

For `scripts/pi4_gate_proof.sh --require-driver-task-proof`, SDIO dedication is
mandatory for Wi-Fi and full-ready closure, but a wired-only
`--require-wired-ready` capture closes the selected network path with GENET and
must not be failed solely because `DRIVER_TASK_SDIO_DEDICATED=no`.

CYW43/SDIO host tests must prove the shared owner command page remains SPSC:
root submission and staging are admitted before handoff, handoff is rejected
while the root slot is active or its completion is undrained, successful
handoff deletes/zeros root's SDIO endpoint authority, all later root SDIO
submission and staging fail before copying bytes, and delegation cannot return
to root. The handoff test must then prove that a delegated CYW43-to-SDIO
multi-phase command progresses without an endpoint cap through the exact
acknowledged shared grant and the generated send-only owner notification; the
notification alone cannot spend authority, and granting or reacquiring root
endpoint authority fails the test. Live Wi-Fi proof must contain the successful
one-way handoff marker before the first CYW43 transport/firmware command. HAL
tests must
reproduce the generated 21-bit monotonic device-untyped ordering: admitting
`0xfe007000` before `0xfe00b000` preserves both pages, while mailbox-first
allocation makes the lower page unavailable. Wi-Fi selection must admit
exactly one early page; wired and disabled selections must admit none. The
retained entry must be exclusive and root-unmapped. Ordinary root mapping,
ordinary child mapping, an incompatible cached entry, alias creation, and
second consumption must fail. Exclusive SDIO mapping must remove the admission
record exactly once. Coverage must also prove that the already root-preseeded
mailbox frame remains available for a child capability copy after fresh
device-untyped coverage is consumed; no second retype is permitted. Runtime
staging coverage must traverse all twelve published root aliases: the first
eight cover the exact 32-KiB owner aperture and the final four cover the RX
batch payload plus disjoint ACK region. Clearing a partial transport must also
zero cached
progress magic, sequence, phase, and auxiliary identity before a replacement
generation can observe it. Production AArch64 coverage must configure the
EventPump in place, borrow it through both ordinary and deferred console loops,
retain the CYW43 supervisor in place while beginning its boot episode exactly
once, and retain material emitted headroom within the 256-KiB root stack; a
source-level linker-size check alone is insufficient.
Runtime
tests must drive the retained Linux-ordered
GET_GPIO_CONFIG/polarity, output-low, power-off, 2 ms wait, power-up, 10 ms
wait, release-high, startup-clock, 10 ms wait, and finalize phases one turn at
a time. Pending turns publish no completion or owner notification, wait phases
must not repeat physical writes, firmware GPIO success requires the returned
zero GPIO token, and only the pair transaction's `ReplaySdioEngine` may enter
this cursor or publish a physical-lifetime terminal. Each GET_CONFIG, SET_CONFIG,
ASSERT_LOW, and RELEASE_HIGH firmware-property operation must post exactly
once, retain the DMA request page across later reply-poll turns, use a
virtual-counter deadline, and publish distinct begin/done progress plus an
operation-specific terminal detail. Property requests must carry a zero
request/response-size word. GET_CONFIG, SET_CONFIG, and SET_STATE acceptance
must match Linux: global transaction success plus the firmware-overwritten zero
GPIO token. Per-tag returned-length, tag, and end-marker fields are not consumer
failure predicates because the Raspberry Pi property ABI permits extended
response lengths and Linux does not expose those fields here. Tests must reject
bad global status and nonzero GPIO tokens and preserve distinct protocol reason
bits for status, token, retained-cursor identity, and mailbox phase.
Root must extend same-request retention
only while one of those exact begin phases is current; mismatched sequence,
aux, contract, mode, done phase, or unrelated progress retains the ordinary
SDIO bound. Tests must prove timeout retention cannot permit a new request to
replace the firmware-owned page.

The live regression input for this closure is
`/Users/lukasbower/pi4-serial-20260719-180716.log`, boot-paired with
`/Users/lukasbower/tcpdump-wifi-20260719-180713.pcap` and
`/Users/lukasbower/tcpdump-usb-eth-20260719-180713.pcap`, marker
`df7196c7bc56`, image id
`2fb39b8be336200d73082e0b00d265900da50041d24af31d28a7120d5264357d`.
It completed SDIO engine initialization, began CYW43 engine initialization,
then cycled pair/context replay on attempt 1 for more than seven million outer
turns before any transport command; the paired captures contain no Pi EAPOL,
DHCP, ARP, IP, or TCP traffic. The causal regression test is therefore the
stranded delegated `Pending` command after root's SDIO endpoint deletion plus
the context-replay-success/recurrent-fault restart cycle, not an association or
TCP rewrite.

The July 10 W01 serial
`/Users/lukasbower/pi4-serial-20260710-123050-m26d-authoritative-W01-pyserial.log`
and pcap `/Users/lukasbower/tcpdump-wifi-20260710-112826.pcap` from
`918a58c09-dirty` remain a compatibility oracle only. They completed all ten
Wi-Fi gates, host EAPOL, PTK/GTK, DHCP, raw TCP, authenticated
`boot_v0.coh`/`smp_parity.coh`, and `tcp_accepts=4 tcp_auth=4`. Host tests must
preserve those upper-path invariants, but this historical pass cannot satisfy a
current-image gate and cannot authorize timing-dependent loops,
same-generation replay, root-owned SDIO, or a legacy fallback.

The CYW43 software and cadence closure gate is authorized by Milestone 26d
tasks `m26d-cyw43-hardware-free-closure` and
`m26d-benchmark-revalidation-and-tuning`, with active defect authority from
Reopened Milestone 26b tasks
`m26b-wifi-sdio-notification-dpc-closure` and
`m26b-net-control-priority`. It exercises the host-testable production
transaction data and state transitions (`begin_turn`, frontier reservation,
retained submit, completion miss, continuation grant, immutable
ticket/completion validation, completion commit, and cached replay). The host
ring adapter executes the production sequence-last command publication, stable
owner intake, sequence-last completion publication, and stable client read,
stages the reciprocal owner descriptor, and obtains the completion from the
real descriptor/controller service path rather than fabricating a direct
result.
Physical mapped addresses, cache-maintenance effects, seL4 notification
send/receive, and target transaction entry/exit remain target-compile checked
and require Pi proof. Under the ordinary EventPump, each outer turn opens at
most one monotonic CYW43 parent-operation permit; a rejected second parent
attempt must leave the retained ticket, deadline, payload fingerprint,
generation, and cursor unchanged. Once admitted, persistent op11, the separate
finite urgent-op7 lease, and a post-release DPC event lease advance inside the
linked runtimes whenever the current durable condition is locally runnable,
including equal deterministic private state, and block at the first exact
external wait. Every helper remains bounded and each immutable hardware request may
issue at most once, but no scheduler turn, yield, or notification is a required
edge between semantic phases.

Ordered Gate 8 coverage must exercise one production diagnostic snapshot with
these exact subgates: `8a-pair-generation`, `8b-control-program`,
`8c-join-terminal`, `8d-association-link`, `8e-bssid-refresh`,
`8f-eapol-keys`, `8g-post-key-maintenance`, and `8h-data-admission`. Tests must
prove all of the following:

- 8a and 8b are derived from one current linked pair/control epoch; 8c through
  8h are derived from one current logical connection generation.
- Snapshot evaluation is passive, immutable, and performs no HAL, SDIO,
  runtime, retry, completion, or owner mutation. All eight records are formatted
  from that single value and admitted with the immediately following
  identity-bound `CYW43_GATE8_COMMIT` record as one all-or-nothing retained
  transaction. The commit is nonterminal and opens only the exact-generation
  data consumer.
- Gate 8 commit requires the same stable pair epoch and logical generation on
  two consecutive ordinary control turns. Both observations must have no
  current-generation pending host-EAPOL event or queued pre-secure EAPOL RX
  frame, no host-EAPOL prompt, session work, deferred-reauthentication, or BSSID
  work, no maintenance or logical control owner, no prompt-poll or
  terminal-drain cursor, no retained HAL driver-task request, and no
  recovery/rejoin. The linked SDIO DPC ring must have producer equal to
  consumer, current-pair flags exactly `OWNER_ACTIVE`, zero current-pair
  overruns, and the same
  nonzero epoch, producer watermark, and per-pair IRQ-ACK-failure count on both
  observations. `ack_failures` is attempt history, not current fault authority:
  after an exact ACK retry succeeds and pending/fault flags clear, a stable
  nonzero value does not by itself block Gate 8 or healthy work. Final hardware
  acceptance still requires zero. Pair replacement resets these ring counters;
  cross-pair history remains in root first-cause recovery records. New counter
  movement is not admissible. Any owner activity, DPC publication,
  counter movement, DPC epoch change, or logical/pair generation change resets
  the candidate. The producer revalidates the exact
  pair/generation/DPC/history receipt and commits that snapshot before
  `CYW43_GATE8_COMMIT`, then rechecks it after consumer-token publication.
  Tests must reject pending, flagged, overrun, torn, zero-epoch,
  producer-advanced, epoch-advanced, and counter-advanced DPC snapshots while
  allowing stable per-pair ACK-attempt history after exact recovery and normal
  DPC activity after accepted commit.
  First-cause deferred-recovery and terminal-drain diagnostics must survive a
  rejected exact receipt, consumer publication, or commit output preflight and
  clear only after the complete retained commit transaction linearizes.
  Partial, reordered, duplicate, mixed-generation, generation-regressing,
  cross-recovery, and changed-before-commit snapshots fail closed.
- The unique terminal
  `CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=ready ...` must occur only after
  the committed generation remains the active CYW43/Wi-Fi interface, DHCP is
  bound with a nonempty address, and the TCP console listener is actually
  admitted. It must not depend on a prior TCP accept/auth session. Tests must
  prove Gate 8 commit cannot release the final HDMI ready banner or admit host
  test commands, while supervisor Ready does both. Later restored service uses
  `CYW43_RUNTIME_RECOVERY status=ready generation=<n> ...` and can neither
  replace nor duplicate bootstrap Ready.
- Transport attachment publishes `stabilizing`. Initial Gate 8 publication in
  the sole `attempt=1` outer boot episode uses one absolute
  `now + 90,000 ms` deadline. Gate 8 is passive: a logical subgate failure
  remains inside its bounded gate-local policy and cannot request pair repair.
  The initial pair is the only pre-service physical lifetime; typed faults must
  drain/fence exact ownership and cannot start pair 2. Gate 8 commit does not
  stop or renew the deadline. Missing exact-generation DHCP/listener service at
  expiry must terminate with `blocker=service-readiness-deadline`. Deadline
  exhaustion must retain the complete eight-line snapshot and adjacent
  `CYW43_GATE8_TERMINAL ... action=quarantine`, emit terminal
  `status=permanent`, and quarantine attached Wi-Fi while serial, local-seat,
  HDMI diagnostics, authentication, and reboot remain live. Only a separately
  typed runtime/SDIO fault after exact service readiness may invoke one
  consumed-once runtime pair repair. Duplicate Ready for the same generation
  must not replenish that budget; restored new-generation service may re-arm
  one later episode only after the distinct runtime Ready publication. Gate 10
  remains downstream acceptance evidence and cannot grant recovery authority.
  Tests must prove output backpressure permits only hardware-free operator-output
  turns; schema/route/capacity preflight mutates no output and invokes no
  terminal decision; the final typed-fault probe may preserve a stronger
  physical causal terminal before the decision cut but may not publish
  `status=recovery`, start pair 2, or repair the pre-service lifetime; a clear
  probe commits the explicit decision cut immediately before atomic retention;
  and no child/network poll, automatic whole-bootstrap backoff, reset, second
  `begin`, or attempt 2 is admitted after that cut.
  Gate-local
  association, DHCP, and protocol retries remain bounded inside their owning
  gates.
- An ordinary firmware `AUTH` timeout remains telemetry and cannot set the
  terminal association latch. Unsuccessful `SET_SSID`, link-down/no-network,
  deauthentication, and disassociation must make 8c pending with
  `blocker=association-retry-pending`; the single association supervisor drains
  any accepted child action, suspends authentication, applies bounded backoff,
  and begins the next logical generation/Join on the same linked pair without a
  pair-recovery request.
- Exact current-generation BSSID-refresh and required post-key-maintenance
  failures must remain logical: 8e and 8g report Pending with
  `bssid-refresh-retry-pending` and
  `post-key-maintenance-retry-pending`, respectively. The same association
  supervisor must suspend authentication, enter bounded backoff, and start a
  fresh logical generation on the unchanged pair epoch. Tests must prove both
  causes clear in the new generation and neither sets the pair-restart signal.
- A fresh non-stable or different-generation observation retracts Gate 8 commit
  to `stabilizing` when it represents a material pair, generation,
  control-program, carrier, security, handoff, recovery, rejoin, or
  post-publication loss-invariant failure. The old snapshot becomes
  non-authorizing and a later commit requires a complete new snapshot. A
  delayed HDMI Wi-Fi/console-Ready claim, including bytes already handed to
  the local-seat queue, is superseded by a canonical Stabilizing redraw while
  one independent open root prompt/input row is retained and repainted.
  Conversely, bounded
  same-pair/current-generation post-secure key maintenance must keep the commit
  and the exact data consumer published while 8a through 8f, secure carrier,
  control program, handoff/publication token, and loss-free data-plane
  invariants remain valid. Tests must prove GTK/EAPOL maintenance does not
  create another Gate 8 deadline or interrupt DHCP/TCP, while generation,
  carrier, publication-token, root-drop, and runtime-overflow failures still
  retract.
- Gate 8h stays pending until one idempotent root data-handoff commit is
  published for the current logical generation. Tests must prove that
  generation start alone cannot publish it; every attached non-recovery turn
  continues the sole ordinary NetStack association/control lane while the
  CYW43 NetDevice blocks queued-data delivery, Device-originated fresh data
  polling, DHCP start, fresh ARP staging, and fresh smoltcp TX. The one
  pre-poll physical RX ingress may continue so EVENT/CONTROL frames reach
  their existing policy owners, but it must queue ordinary data. An exact
  assigned NetData continuation may complete before commit only if its
  returned ordinary data frame is retained rather than delivered. After that
  turn, the helper must use the freshly
  observed logical connection generation, not the bootstrap pair generation,
  and must revalidate current pair, association/link, BSSID,
  protected-key/open-network, maintenance, logical-owner, and recovery state
  without performing HAL, SDIO, CYW43, retry, or completion work. The commit
  must reject and separately account only stale-generation tokens, preserve
  valid current-generation backlog for the following consumer turn, snapshot
  sticky cumulative root-drop and runtime-overflow counters, and
  release-publish the baseline generation token before publishing the matching
  consumer commit token last. The producer must capture a new Gate 8 diagnostic
  after commit. Repeating the helper in the same generation is a no-op and must
  not purge a later frame.
  A newly latched recovery must then follow the strict
  `Network -> yield -> Operator -> yield -> Driver` phase trace. Coverage must
  prove the Driver phase repeats the may-begin guard and performs no child
  operation when the preceding operator turn accepted `reboot` or lost linked
  serial admission.
- Gate 8h passes with bounded non-full root RX, pending data TX/ARP, runtime
  backlog, or one exact assigned current-generation NetData request. The
  pre-commit publication-quiescence check must nevertheless wait for that exact
  request and its terminal-drain/HAL ownership to finish; after commit, one exact
  current-generation NetData continuation remains legal and cannot alone
  retract stable proof. A missing/stale commit or baseline token and a lossless
  full root RX queue are
  pending, with exact blocker `data-handoff-commit-pending` for the former and
  bounded drain priority for the latter. Pre-commit queue pressure must not
  become `root-rx-drop-since-generation`. After commit, Gate 8h fails for a
  stale prompt generation, retained request-less NetData pre-poll while
  priority root work exists, an exact-generation root drop, or a monotonic
  runtime-overflow increase since the committed baseline. Root-drop telemetry
  must saturate rather than wrap, while the exact-generation loss latch remains
  fail-closed. Recovery/generation advance invalidates the token; the next
  consumer-active commit captures a new baseline without clearing cumulative
  boot or stale-purge telemetry. Pending tokens must capture their enqueue
  generation, stale tokens must be rejected before current-generation
  consumption, and the handoff must preserve valid current-generation backlog.
- Bounded data-TX/RX ordering tests must prove one generation-scoped aggregate
  with total capacity 16 feeds exactly one active op7 owner. It uses urgent
  control and bulk FIFO classes: ARP, EAPOL, DHCP, TCP SYN/FIN/RST, and
  payload-free TCP control must precede other payload-bearing TCP and ordinary
  data, while fragmented IPv4 remains bulk, a paired-RX response is
  independently urgent, and FIFO order is preserved within each class.
  Ordinary `Device::transmit` reservations must stop at 15 aggregate slots and
  leave the final permit for the TxToken paired with `Device::receive`;
  consuming a valid reservation must not fail merely because an older owner
  remains active, must enqueue without immediate promotion, and dropping an
  unused token must release only that local permit. EventPump must be the sole
  production TX coordinator: `Device::receive` and reservation failure must
  perform zero TX service. A retained exact foreign HAL descriptor, including
  NetData op8, must remain unchanged while the queued op7 stays requestless and
  the TX hook spends no service budget, deadline, or recovery. After that owner
  reaches its terminal, the coordinator must advance one active op7 or promote
  and advance one eligible FIFO head before a continuously replenished
  copied-RX queue. Copied RX may return first,
  with zero physical operations and before pending ARP staging, only when no op7
  is legally runnable because a retained foreign owner holds the lane. With all
  16 aggregate slots occupied, the dedicated pre-smoltcp EventPump hook must
  prove that promotion removes one eligible
  head and restores paired capacity before exactly one physical advance. A
  terminal must not promote a successor; that frame remains queued for a later
  coordinator turn. Promotion starts the one op7 lifetime without a root-owned
  credit mirror. The runtime alone compares `sdpcm_seq` with `tx_max`; a closed
  window must retain the exact promoted op7 in `WAIT_CREDIT`, and DPC may update
  that window without changing parent identity. A joined Function-2 terminal
  must release root and allow a later successor with no intervening RX credit
  acknowledgement. Generation/reset must purge queued never-issued frames locally, while
  issued/ambiguous active ownership remains poison-and-recovery work. All
  classing, reservation, promotion, service, and telemetry behavior must be
  absent from GENET.
- Root and child tests must bind their queue capacity and the child bounded RX
  drain budget to
  `pi4_driver_abi::DRIVER_RUNTIME_CYW43_RX_QUEUE_CAP=50`. They must reject
  divergent private capacities, prove the root can preserve one complete child
  backlog, and keep queue saturation subject to the Gate 8h rules above.
- `wifi dump-state` formatting coverage must preserve the
  untruncated passive `wifi: data_handoff` records: generation/commit/baseline
  tokens, baseline generation, and `queue=<used>/50`; one stable root RX-queue
  snapshot; one stable runtime RX-batch snapshot; an RX notification-hint
  record that explicitly carries no authority or history and includes the
  bounded SDIO deadline-hint count; current/baseline root-drop and
  runtime-overflow counters; total/last-token/last-count stale-purge state;
  boot-first loss state; and current-handoff post-commit first-loss state. It
  must also preserve
  boot-cumulative association service-turn/Join-start counters and the latest
  complete non-recovery Gate 8 frontier so sticky recovery cannot replace the
  causal subgate with only a generic pair-failure state. The passive
  current-episode, boot-cumulative transition-count, and boot-cumulative fault
  records must remain separate and untruncated at maximum widths. The
  normalizer's `WIFI_PRIORITY_EPISODE_COUNTS_SCOPE` and
  `WIFI_PRIORITY_EPISODE_FAULTS_SCOPE` must remain independently `none` when
  only the other record is present. Coverage must distinguish
  `pair-placeholder`, `owner-context`, and `exact-owner`, preserve the immutable
  valid `scope=first-pre-fence` scheduler tuple latched sequence-last at the HAL
  outer-lease-poison/sticky-restart seam through refinement, reject
  `scope=unavailable` as causal proof, preserve the first-writer recovery source
  as `WIFI_DEFERRED_RECOVERY_SCHEDULER_CAUSE`, and distinguish
  the passive generation-matched runtime call site as
  `WIFI_DEFERRED_RECOVERY_RUNTIME_SOURCE_LINE`. A queue-poison test must prove
  the value is nonzero, survives pair scrub in the first-pre-fence snapshot,
  and cannot create a wake, grant, scheduler phase, or recovery predicate;
  non-queue causes retain zero. Coverage must also distinguish
  the root command sequence from the doorbell-issued fact. The retained summary
  must preserve the exact bounded grammar
  `wifi: deferred_recovery retained=yes refinement=<...>
  logical_terminal_observed=<...> cause=<...> subphase=<...> gate=<...>
  current=<...> live_generation=<...>`. A retained Gate 8
  or Gate 10 receipt must outrank a later post-scrub missing-clock snapshot;
  Gates 4 through 7 still require current clock evidence. The passive
  maintenance snapshot must render as adjacent state and action records,
  preserve generation/current/pending, all four masks, next stage, exact action
  generation/request/issued/turn fields at their maximum widths, and never
  truncate either record. A positive loss record must retain sampled
  generation, commit state where applicable, reason,
  queue length, channel, EtherType, and priority and end with
  `attribution=current-epoch-sample`. Tests and normalizers must not reinterpret
  that sampled generation as producer, runtime, SDIO, or physical-owner proof.
  `wifi_post_dhcp_rx` coverage must increment exactly once at each actual
  smoltcp delivery boundary and must prove that trace-only `rx-preserve` and
  `rx-deliver` observations do not double-count one frame.
- `wifi diag` coverage must enforce schema v2's maximum eight body lines and
  2,048 body bytes, an untruncated matching begin/frontier/transport/complete
  identity, terminal status and ACK/prompt liveness, and zero physical device
  operations. The first failing gate must precede downstream state. The causal
  episode must bind physical epoch, logical generation, immutable parent, SDIO
  child, terminal/exit, and pending mask; the latest child-timing row must keep
  publication, intake, issue, terminal, and final-consumer acceptance distinct;
  grant and wake fields remain evidence rather than authority. The current
  causal-progress row must render the passive SDIO command/completion tuple or
  explicit unavailability. First-recovery coverage must retain the exact
  delegated tuple before pair scrub only after two identical record-pair reads,
  survive later transport cleanup, and prove that an unstable pair fails
  closed. Snapshot collection must perform no wake, retry, consume, or owner
  transition. The current producer's SDIO ring field must be either `ring=u`
  or the exact fixed-width lowercase result-bearing command/completion tuple.
  Schema-v2 must keep both the early row with no ring suffix and the exact
  historical seven-field tuple parseable as old evidence, while neither may
  refine containment. Only the current result-bearing form may refine after
  nonzero publication and episode sequences, the immutable parent sequence,
  nonzero matching physical epoch, matching child sequence, concrete
  transport-parent fault, Fault completion, and stable episode all correlate
  inside the unsigned 32-bit identity domain. A present short, uppercase,
  extended, zero-identity, or overflowing tuple/episode fails normalization.
  A multi-record snapshot must say so and cannot replace Gate 7/8, DPC, DHCP, nettest, TCP, or
  authenticated-`cohsh` acceptance. Historical verbose `wifi diag` fixtures
  remain parseable, while new verbose proof belongs to `wifi dump-state`.
- A current-generation association creates the post-association BSSID
  obligation independently of the EAPOL-Start timer, but only the secure-keys
  boundary may issue it; tests must prove M1/M2 cannot open BSSID maintenance
  ahead of M3. The policy lane remains runnable until an exact BSSID
  success/failure terminal, blocks fresh NetData, and still permits an
  already-assigned op8 owner to reach its exact terminal.
- Gate 9 address/DHCP and Gate 10 nettest/TCP/authenticated-`cohsh` acceptance
  bind to the same logical generation as Gate 8. Recovery, generation advance,
  or readiness retraction invalidates downstream proof instead of allowing
  evidence stitching. DHCP tests must prove each start has a fresh
  generation-bound nonzero XID and that delayed Offer/ACK packets from the
  prior generation or prior same-generation transaction are rejected.

Normalizer tests must expose `WIFI_GATE8_COMPLETE`, `WIFI_GATE8_SEEN`,
`WIFI_GATE8_LAST`, `WIFI_GATE8_MISSING`, `WIFI_GATE8_STATUS`,
`WIFI_GATE8_GENERATION`, and `WIFI_GATE8_BLOCKER`. They must cover snapshot
prefixes, cross-recovery fragments, repeated/replayed records, U32 rollover
versus reset/regression, atomic 8a-through-8h adjacency to exact
`CYW43_GATE8_COMMIT`, later same-generation bootstrap Ready, malformed or mixed
commit identity, duplicate bootstrap Ready, distinct
`CYW43_RUNTIME_RECOVERY`, `ready -> stabilizing -> runtime-ready` reproof, and
rejection of Gate 9/10 evidence from any other generation. Legacy no-commit
logs may remain readable but cannot satisfy the current production schema.

SDIO request-lifecycle coverage must drive production owner seams for card
CMD5, generic CMD52, and data CMD53 descriptors. It must prove that every
request receives a separate 10-millisecond entry-inhibit fence and a fresh
10-second watchdog armed only after that fence, including a response at the
legal request edge after a long pre-issue wait. Data and short-busy requests
must write `TIMEOUT_CONTROL=0x0e`. Before every COMMAND, the owner must W1C and
read back the request-owned command/data/error status bits while excluding
`CARD_INT`; a retained nonzero readback may retry only that W1C within the
entry deadline, and expiry must report typed pre-issue stage 8 with zero command
issues. After COMMAND, a delayed-response adversary must withhold the current
response register value until the request owner W1C-acknowledges its immutable
`RESPONSE` edge. The owner must then read the current response exactly once,
preserve a coalesced `CARD_INT`, complete the CMD52 from that response, and
issue no second command. Stage 8 containment must poison that SDIO owner
generation, and every subsequent ordinary descriptor must reject with zero
command issues until the canonical root pair transaction scrubs the owner and
`ReplaySdioEngine` establishes a replacement physical lifetime. The owner may
reserve two transfer attempts,
but a second issue is legal only after an entry-inhibit failure proves the first
command was never written; command, response, data, busy, or later failures are
issued-unknown and perform no same-generation replay. Each failed attempt gets
its own 220-millisecond containment deadline. Containment may advance at most
24 immediate deterministic owner-local phases in one admitted turn and must
stop when a hardware/time wait remains false. `HOST_CONFIG` may similarly
advance at most 18 phases, stopping only at either clock-stability poll. Tests
must prove one sample per real wait, unchanged condition-before-deadline order,
no private poll loop, no second issue, and fail-closed invalid cursor/bound
behavior. If containment itself fails before another issued-request failure is
recorded, stage 9 must retain the immutable first subphase plus pre-recovery
controller/DMA snapshot; later reset writes cannot overwrite it. Shared-ABI
tests must derive the
exact 20.56-second CYW43 child bound from those maxima plus the 100-millisecond
handoff margin and prove root's initial 30.56-second child lease cannot expire at
the child boundary. Multi-child parent coverage must prove that only a fresh
same-request `OWNER_REPLY` edge renews a 30.56-second child lease, that a
repeated reply cannot renew twice, stale/wrong-sequence progress cannot arm a
renewal, and the shared 1,024-action trace bound ends renewal deterministically.
Idle and external-DMA requests must retain the exact named persistent
`INT_ENABLE=0x02ff000b` policy without a per-request rewrite. Active retained
PIO must remove DMA-only `DMA_END` and `ADMA_ERROR`, add only the
direction-correct `SPACE_AVAIL`/`DATA_AVAIL` source, and preserve that source
across an interleaved CARD_INT policy rewrite without adding it to
`SIGNAL_ENABLE`. `CARD_INT` is added only while that asynchronous source is
armed. The terminal PIO snapshot must restore ordinary interrupt policy before
publishing completion in the same bounded terminal quantum. Terminal
classification must still recognize every bit in broad `INT_STATUS` error mask
`0xffff8000`.

External-DMA lifecycle coverage must prove the exact Linux order within one
bounded preissue/issue owner quantum with `PREISSUE_STEP_BOUND=16`: SDHCI
block-gap inspect/repair/verify, DMA-authority/idle snapshot, full immutable
control-block staging, status clear, timeout, block size, block count, argument,
transfer mode, exactly one COMMAND, and then exactly one BCM2835 DMA `ACTIVE`
write.
The status-clear step must include the same request-owned readback fence.
If owned W1C bits survive that readback, the cursor must retain
`ProgramVerifyStatusClear`, issue no COMMAND, and yield for one
deadline-bounded retry. No other deterministic setup register may force a
phase-per-outer-turn schedule; an optional pre-TX DPC fence may finish the old
child proven-not-issued, and that child can never retry, issue, or replay. After
canonical DPC consumes the exact committed source, the same immutable op11
parent may publish one fresh F2 child under its unchanged absolute deadline;
the logical request still has exactly one physical issue.
That quantum must remain within the shared 256-operation contract and perform
no post-issue completion snapshot. Stale pre-command
`SPACE_AVAILABLE` cannot satisfy the fresh response or switch an immutable
more-than-two-block request into PIO.
Command/controller and R5 errors after COMMAND must show one DMA start, one
descriptor-level containment, and no replay. A coalesced `RESPONSE|DATA_END`
must survive the join; delayed DREQ must still complete; and the response/R5,
`DATA_END`, and terminal `CONBLK_AD == 0` evidence may arrive in any order. The
final BCM2835 control block must set Linux's `INT_EN`, intermediate blocks must
not, and every case must prove `SDHCI_TRNS_DMA` remains clear. A posted-start adversary must hold
control-block and ACTIVE writes invisible until the full store-completion fence
and same-channel readback, then prove the join cannot accept the initial zero
`CONBLK_AD`. Completion must require the terminal `CS.INT` edge and acknowledge
it exactly once with `INT | ACTIVE`, including immediate-terminal and delayed-
DREQ schedules. Failure coverage must separately retain telemetry capture, DMA
inspect/abort/poll/stop/reset/verify, host acknowledgement/reset, clock restore,
final inhibit, and final snapshot phases; no containment phase may replay the
issued command.

Retained-PIO lifecycle coverage must prove immutable engine selection from the
normalized host block count: byte mode and one- or two-block CMD53 select PIO,
while three or more blocks select external DMA. All cases must pass through the
same production descriptor owner and retained request cursor; no ABI selector,
second command lane, post-admission engine switch, or compatibility fallback is
permitted. PIO must succeed without DMA authority and must perform no DMA MMIO,
control-block staging, activation, or containment. After the fresh response and
R5 are validated, only the direction-correct ready interrupt paired with the
matching `PRESENT_STATE` source may authorize FIFO ownership. A stale or early
ready edge without present-state ownership must move zero bytes and require a
fresh edge, the opposite-direction ready source must remain untouched, and an
R5 error coalesced with a ready edge must produce zero `SDHCI_BUFFER` accesses.
Each fresh direction-owned ready edge may move exactly one complete normalized
host block, 1-512 bytes and at most `ceil(block_bytes / 4) <= 128` FIFO
accesses. The quantum must not cross into the next block, which requires
another fresh edge. Completion must join exact payload movement, authoritative
`DATA_END`, response, and host quiescence in every arrival order, and the
terminal snapshot must restore ordinary interrupt policy before publishing
completion. Missing ready ownership, short payload, missing `DATA_END`,
timeout, or controller error must enter common bounded host containment, poison
the generation, and perform no engine switch or same-generation replay.

SDIO service-dispatch coverage must initialize the production runtime, submit
every former non-descriptor raw/aux command shape, and prove each is rejected
before any SDHCI or DMA access. Only a structurally valid
`DriverRuntimeSdioCommandDescriptor` may reach owner service after init. DPC
activation coverage must prove the owner performs zero Function 1
`RFRAMEBCLO`/`RFRAMEBCHI` CMD52 reads: with host `CARD_INT` latched it publishes
that retained event, and without a latched bit it advances a retained masked
rearm. State, health, `INT_ENABLE`, `SIGNAL_ENABLE`, ring/disposition, IRQ
acknowledgement, and signal phases must remain separate; the level route cannot
rearm early. Subsequent dongle/FIFO source reads must occur only through the
retained CYW43 DPC, one admitted operation per later turn. SDIO descriptor
opcodes 8 (`GENERATION_RESET`) and 10 (`GENERATION_COMMIT`) are retired and
reserved: coverage must prove both are invalid as ordinary descriptors yet
produce their stable site-typed rejection before SDHCI, DMA, mailbox,
power-sequence, ring mutation, or retained-owner cursor activity. It must also
prove poison rejects otherwise valid ordinary descriptors until the canonical
pair-entry scrub. CYW43 network op8 `RX_POLL` and op10 `CONTROL_POLL` remain
separate active operations and are not covered by this retirement.

Terminal-fault coverage must decode the immutable 116-byte version-3 frame and
preserve its existing 68-byte prefix. Before containment changes either engine,
the extension must capture SDHCI argument, transfer-mode/command,
timeout/block-gap, interrupt-enable, signal-enable, and host-control-2 plus
BCM2835 DMA `TI`, source, destination, length, stride, and debug. Tests must
reject a truncated or version-mismatched frame and prove root renders the same
terminal registers rather than post-reset state.

Initial shared-core coverage must prove the exact owner-first phases:
register SDIO service, replay the SDIO descriptor, register CYW43 service,
replay the CYW43 descriptor, prove the SDIO prerequisites, hand the mailbox to
SDIO, and enter the canonical pair transaction directly. No preliminary SDIO
or CYW43 engine command may issue. The transaction must preserve all 22
SDIO-before-CYW43 actions, replay the SDIO engine exactly once, replay CYW43,
and hand off the producer ring. Only after the restart completes may root
register the SDIO owner, begin context replay, program firmware/control, prove
control-plane readiness, and lower SDIO and CYW43 in two separate outer turns.
Descriptor or engine replay must not lower either child early, and each final
lowering must consume a separate outer turn. Bootstrap and recovery must remain
owner-first, per-action lanes at priority `255` and must never acquire the
steady Network priority lease.

Physical-Pi selection coverage must prove every valid Wi-Fi selection,
explicit Wi-Fi or `Auto` with credentials, routes to the persistent post-prompt
supervisor with or without a local seat and never calls the pre-root network
constructor. Wired selection and `Auto` without credentials must not enter that
supervisor. Generic root network engine-init coverage must prove GENET emits its
network-init command and every CYW43 contract is rejected; CYW43 engine replay
may occur only as `ReplayCyw43Engine` inside the canonical pair transaction.

Cross-layer physical-lifetime coverage must drive the production cold
supervisor through every retained yield and prove exactly one owner-published
`begun_epoch`, followed by either the matching `completed_epoch` or matching
`failed_epoch`. It must prove pair-ring reset preserves only the exact 16-byte
`DriverRuntimeSdioPhysicalLifetimeRecord` and zeroes every command, completion,
DPC, grant, continuation, fault-telemetry, and pair-generation-counter byte outside
it. Restart must immediately publish `failed_epoch = begun_epoch` for an active
lifetime before clearing volatile state, including when its runtime cursor is
already missing. A later replacement lifetime must advance the epoch without
erasing the record's previous failed epoch, and no root or CYW43 client may
write the record. The passive bounded Gate 1 owner-lifetime line must carry
these fields without truncation:

```text
wifi: gate 1 owner_lifetime lifetime_begun=<u32> lifetime_completed=<u32> lifetime_failed=<u32> lifetime_active=<yes|no|unknown> source=sdio-owner
```

The following causal Gate 1 status line must also remain untruncated through
its `dependency`, `source`, and `next` fields.

The exact completed epoch established during `ReplaySdioEngine` and returned
by the completed pair transaction must bind the supervisor before context
replay. Gate 1 must require a valid inactive record whose nonzero
`begun_epoch == completed_epoch`, whose `failed_epoch != begun_epoch`, and
whose begun epoch equals that supervisor-bound expected epoch. Gate 8 mark,
Gate 8 publication commit, operational Gate 8 continuity, and Gate 10
acceptance must each reject when that owner epoch is absent, active, failed, or
changed. The passive `Cyw43ServiceWorkSnapshot`, sequence-last queue record,
and batch record must include the same epoch with connection generation and
pair epoch; changing only the physical epoch must invalidate cached queue/batch
state and any pending operator fence without admitting Wi-Fi work under the
replacement identity. A notification observation must not survive that cut or
substitute for the three-part identity. Missing, active, failed, or changed
physical-lifetime state must reject the records without a source probe, and a
replacement lifetime may publish only from its own Gate 8 data-consumer
boundary. The identity grants permission but must never
set a durable-work reason, schedule a fresh op8, or change TX admission.

Cold-lifetime tests must prove a separate typed provenance lifecycle. Only
successful completion of a pair transaction owned as `ColdBootstrap` may
record the exact initial physical-lifetime provenance; a pre-handoff recovery
that happens to produce the same numeric pair/lifetime epoch or replay state
must not. Every recovery request, pair-transaction failure, unfinished cursor
drop, and replay terminal must clear it. A recovery request arriving during the
cold cursor must remain visible and supersede a later successful completion
rather than being cleared or minting authority. The provenance must never
select a publication fast path: every cold, recovery, and steady parent uses
the same retained phase machine.

Pair-restart `ReplaySdioEngine` and `ReplayCyw43Engine` non-ready completions
must survive the failure fence byte-for-byte and populate the corresponding
retained engine-init diagnostic. A generic `engine-replay-failed` without the
exact completion is a test failure.

Once both are lowered, non-CYW43 retained runtimes keep their request-bound
outer-turn sequence: prepare one immutable sequence-zero record; boost the bus
owner when required; boost the primary child; commit the nonzero sequence;
publish one authority edge; poll once per later turn; latch completion; restore
the primary before the bus owner; then release the lease. Selected steady WiFi
has one narrower exception: an actionable `Network` quantum reserves the pair,
boosts SDIO then CYW43 exactly once, and admits only exact root-to-CYW43 parents
from that current pair generation. Those parents reuse the quantum lease but
retain separate immutable request, command fingerprint, issue, grant,
notification, and completion identities. Every outer EventPump turn still
admits at most one root CYW43 parent operation. The linked runtimes may then
advance an already-admitted persistent or finite event lifetime through
consecutive bounded helpers whenever the current durable condition is locally
runnable. Equal deterministic private state continues; the first exact external
child, credit, queue, or peer wait blocks immediately, even when newly observed.
Each immutable physical request remains single-issue.

Closing the quantum must fence fresh pair work. An exact already-`Prepared` or
already-`Issued` parent may drain alone. Tests must prove that an ABI-invisible
sequence-zero `Prepared` parent keeps an open quantum actionable and that a
closing parent does not depend on one Network turn per physical-operator
rotation. Every root turn must admit at most one parent operation and revalidate
the same request plus a monotonic `Prepared`-to-`Issued` state. The 25-ms
virtual-counter cap must fence only fresh-parent admission and must not
interrupt that exact parent. A pending physical or buffered-console response
may yield only at a stable external wait. The immutable 192-operation budget is
a HAL-operation bound, not a scheduler-turn or notification count; any resumed
slice must retain the same fenced parent. Restore order is CYW43 then SDIO after
its exact terminal.
Tests must reject a fresh or switched parent during close, issue-state
regression, wrong-generation reuse, torn phase/reservation state, invalid
active-parent state, and partial acquisition or restore. A clean partial
acquisition must roll back the exact reservations; any state that cannot be
rolled back completely must poison the lease and request pair recovery.
Pair-epoch advance, quarantine, and reboot must either complete the same exact
close/drain or enter pair recovery; none may silently clear or alias ownership.
GENET must remain outside this state machine, retain its ordinary
single-`Network`-turn rotation, and emit no WiFi priority-lease telemetry.

For every root-to-CYW43 generation, including zero, non-op11 retained foreground
commands outside finite op7, including non-op11 bootstrap and recovery
commands, keep their exact endpoint/grant authority and ABI-invisible `Stage`,
`CommitRing`, `PublishGrant`, `NotifyRing`, and `PollRing` cadence. The finite
urgent op7 steady-service lease retains its distinct identity, admission, and
budget, while its current durable runnable condition continues locally to its
first exact external wait. Tests must retain current grant acknowledgement,
replacement, re-signal, identity, exhaustion, and endpoint-fallback rejection
coverage for the ordinary foreground path.

Every exact op11, including logical generation zero and bootstrap/recovery
lifecycle use, uses the narrower persistent contract. Tests must prove that HAL
rejects a caller-supplied marker and derives
`DRIVER_RUNTIME_COMMAND_FLAG_PERSISTENT_TRANSACTION` only from the fully valid
staged op11 descriptor, complete payload, full budget, request, and generation.
The exact shared budget is 192 operations, 64 frames, and 65,536 bytes; mutate
each field independently and require rejection before child issue. Test the
logical connection generation and physical bus-link epoch as distinct domains:
both logical zero and nonzero parents bind once to the current private physical
epoch, parent `aux1` remains logical, and every retained transaction and SDIO
child carries the physical epoch. A changed logical parent or stale physical
epoch must reject without publishing a child; comparing either domain directly
with the other is a regression.
The sequence-zero `Stage` remains invisible; for exact persistent op11, the
same admitted producer call writes and cleans the full command body/payload,
executes the barrier, commits the nonzero sequence last, records `Issued`, and
emits exactly one reserved-root-badge notification. Other retained transaction
classes retain their declared phase-separated Stage contract. One HAL admission
with zero root continuation-grant publications
must advance a real Join from `PreTxDpcProbe` through `PreTxDrain` to
`WaitCredit` and onward without grant 19, replacement grant,
second notification, or endpoint send. Every completion miss retains `Issued`;
only the exact terminal or typed fault containment closes the parent. Missing,
coalesced, and repeated notification hints must produce identical terminal
state, and a terminal committed immediately before the consumer blocks must be
observed by its final condition recheck without another hint. Root's
non-renewable 30-second CNTVCT containment deadline starts at sequence-last
issue: a stable terminal at expiry must win, while a double stable miss must
request the existing pair recovery with zero added grant, signal, replay, or
runtime wake.

Semantic-retirement coverage must exercise the production root/HAL path for a
healthy logical op11 terminal, an interleaved EVENT/DATA physical terminal,
recovery becoming sticky before canonical consumption, and an omitted
outer-turn finalizer. Construction-level coverage separately requires a
dropped uncommitted raw-retirement token to fail closed before semantic
publication; receipt-level coverage compare-validates every exact identity
field before release.
Before finalization, tests require the copied or routed semantic body to be
visible, the active HAL request to be replaced by its exact
request/fingerprint/`aux1`/pair-epoch receipt, and the logical owner to remain
live. A normal finalizer clears the matching logical owner, receipt, and
pair-terminal-drain authority without introducing another recovery; an interleaved finalizer clears
only physical authority. Every dropped, orphaned, duplicate, or mismatched case
must preserve the fence, admit no successor operation, and request the sole
pair recovery where the corresponding production state is reachable. The
paired diagnostic counterfactual requires an exact observed Gate 8 terminal
retained across pair scrub to preserve the Gates 1-7 causal prefix. It reports
canonical retirement only while the same exact runnable parent is live; after
finalization it reports pair recovery while preserving the historical prefix.
An unobserved hint or lower-gate terminal must not gain that authority.

Scheduler coverage must prove that repeated canonical cut/owner and combined
service-level questions in one active outer policy turn each derive one
immutable routing snapshot, while the next turn derives fresh snapshots and
outside-turn diagnostics never reuse them. A real sleeping op7 must remain one
cached hint when its child commits asynchronously, then expose the terminal on
the next turn without a successor issue. Separately, publish DPC and copied
root-RX levels after an idle routing snapshot in the same outer turn and
require the narrow condition-before-sleep cut to retain Network while the
full-classifier derivation count remains unchanged. Generic work must not acquire
same-lifetime time-cap authority, and an issued waiting parent must mask
identity-only policy levels plus retained TCP flush, console, and ICMP demand.
Fresh DPC, copied root RX, and the exact sequence-last terminal must remain
runnable beside that sleeping parent. These caches are performance assertions only: the
production consumer, HAL admission, and explicit condition-before-sleep
boundary retain independent exact durable rechecks, and a cached hint cannot
issue, consume, retire, recover, or make notification history authoritative.

`PreTxDpcProbe` must classify the durable generation condition without exposing
housekeeping as a bus transaction. A valid current-physical-generation,
owner-active, empty/unmasked ring continues locally with no `DPC_ACTIVATE`
child. A committed front event
binds one attempt-scoped exact sequence token, including sequence zero, widens
the fairness watermark once, and waits for canonical DPC consumption. Only
activation-absent or mask-skewed state, plus exact ACK debt bound to an
already-submitted immutable activation frontier, uses the retained
activation-repair transaction. Invalid, wrong-generation, poisoned, overrun,
or lost-authority state fails closed and quarantines the generation. A later
event cannot widen the bound token or starve TX, and a vanished/replaced token
without durable consumed proof quarantines the generation.

Interleaved EVENT/DATA before the exact BCDC reply must not produce an op11
terminal or a replacement parent sequence. CYW43 must publish one through eight
frames through a stable sequence-last `DriverRuntimeCyw43RxBatchRecord` while
preserving the op11 descriptor, payload, BCDC id, deadlines, and child frontier.
Root must first reserve capacity for the complete batch. If capacity is
insufficient, it must copy nothing, leave the child commit visible, and withhold
the ACK until one later all-or-nothing copy. Root then validates, copies,
post-copy re-reads, and delivers the batch once before publishing a
cache-line-disjoint `DriverRuntimeCyw43RxBatchAck` by clearing its
commit, writing exact generation/parent/queue-commit/count, cleaning/barriering,
and committing `queue_commit_sequence` last. CYW43 may advance or reuse the
batch only after two stable ACK samples match every field. Torn, stale,
wrong-generation, wrong-parent, wrong-queue-commit, wrong-count, duplicate, and
missing ACKs must retain both batch and parent and perform zero SDIO operations.

Root scheduling coverage must stable-read the exact issued op11 parent
condition. `Waiting` masks only that parent's descriptor, logical-owner, and HAL
lease self-demand, so repeated root completion polls cannot become its service
clock. The mask must leave every other reason unchanged; specifically,
independent committed DPC/RX, exact terminal-drain work, sideband-batch
consumption/ACK, and an expired exact deadline-arm fault hint must remain
schedulable. `TerminalVisible` must unmask the exact terminal consumer, while
`DeadlineFault` and `NotExact` must follow existing typed containment. The same
mask must not apply to an op7 parent or to an ordinary foreground command.

After the one-way owner handoff deletes root's SDIO endpoint, every CMD52,
CMD53, or `DPC_ACTIVATE` child derived from that op11 must carry
`FLAG_PERSISTENT_TRANSACTION` paired with the parent marker. Tests must reject a
partial pair, unrelated primitive, changed command/body, stale generation,
replay, mixed finite steady-service marker, or wrong parent binding before I/O.
The real shared ring and owner cursor must commit the complete child command
sequence last, optionally signal badge 256 once, and retain the exact child to
terminal with zero delegated grants. Every service helper remains bounded and
each immutable hardware request may issue at most once; a current durable local
condition may continue without a scheduler handoff. The owner re-reads the
matching completion, joined IRQ state, CARD_INT/DPC work, child frontier, and
sequence-last one-way command ring immediately before `Wait`; a fresh child
re-enters intake without a new notification, while an unchanged ring and
unchanged external work condition may block. Tests must prove the PIO CMD53 path reaches one terminal with
IRQ158/host state and zero DMA use, and prove the external-DMA CMD53 path joins
IRQ158/IRQ116 in both arrival orders into exactly one terminal. The completion
body/cache clean/barrier must precede sequence-last commit and any signal.
Signal-count coverage must prove that an intake-sealed pre-cursor crossing,
active `DPC_ACTIVATE`, and PIO/CMD terminal `CARD_INT` join each emit zero peer
hints before child-terminal commit and exactly one afterward. DMA4 IRQ116 must
join that body without signalling CYW43 directly. A failed completion commit
must emit zero hints. The complementary idle-owner rearm crossing, where no
one-way child owns a later terminal, must still publish its durable event and
emit exactly one immediate liveness hint.

Every active autonomous SDIO phase that can remain blocked must publish
`DriverRuntimeSdioDeadlineArm` body before its final request-sequence commit at
the condition-before-sleep boundary. The body contains the current nonzero
physical-lifetime epoch, immutable request sequence, and that phase's exact
64-bit counter expiry. Coverage must include pre-issue inhibit/status-clear,
issued polling, containment/reset polls, and host-clock polls; containment clock
settle must publish the earlier settle/overall expiry. A blockable phase without
the required counter must fail closed rather than sleep. Phase progress must
refresh or clear the arm, and terminal/reset must clear it before publication.
Normal IRQ- and durable-level-driven requests must record zero fault hints. A stable unchanged
expired arm may produce at most one reserved-root hint; CYW43 must stable-read
the same epoch/request/commit/expiry plus terminal and restart state before
forwarding its existing badge-256 hint, and SDIO must recheck terminal before
deadline before containing the exact request. Tests must cover torn, zero,
stale-epoch, wrong-request, cleared, repeated, restarted, and terminal-racing
arms. None may create a new request, grant, replay, poll, source probe, direct
root-to-SDIO operation, or second hint.

Non-op11 delegated foreground commands retain their acknowledged shared-grant
tests, including exact identity, commit-before-signal, irrevocable ACK,
replacement only after consumption, and zero I/O on mismatch. From the first
post-release event, production DPC instead binds the exact event sequence and
physical generation to the finite steady-service child marker before or after
Gate 8. Mutable `steady_data_plane_live` state must never select the ordinary
continuation-grant mode; the immutable DPC cursor and committed ring remain
authoritative between exact children until the event terminal, while the
semantic snapshot is diagnostic only. Immutable
root/delegated commands must reject endpoint fallback even when mutable gate
state is absent. All physical deadlines remain fault-containment limits for the
exact active autonomous phase or ambiguous request, not traffic-progress
mechanisms. In particular,
an op11 post-TX `WaitReply` with unchanged durable CARD_INT/DPC/RX/credit and
child-terminal state must block without a forced source probe, ordinary-traffic
deadline hint, synthetic DPC, or manufactured completion.
Focused runtime coverage must include
`persistent_control_reuses_healthy_dpc_lifetime_without_a_child`,
`control_pre_tx_binds_one_event_then_advances_past_reassertion`,
`control_pre_tx_missing_bound_event_faults_without_reactivation`, and
`post_release_dpc_child_uses_exact_event_lease_before_gate8`. They prove the
healthy zero-activation-child path, exact one-event binding including sequence
zero, later-event fairness, fail-closed lost-token handling, and a pre-Gate-8
event using the exact DPC lease with zero ordinary continuation grants.
An event's source/frame-length hint must be copied into the canonical DPC cursor only on
the first admission of that exact sequence. Later completion-poll,
legacy ordinary-mode test, or durable-condition recheck turns must not
reapply it: doing so can resurrect `I_HMB_FRAME_IND` after a completed F2 read
and create an endless same-frame drain. A different sequence while that
canonical cursor is active must poison the generation rather than merge event
identities. A later sequence queued behind the exact pre-TX token remains
pending and does not widen that parent's watermark. Production-chain coverage
must drive the real reciprocal `DPC_ACTIVATE` owner only for release
establishment, activation-absent or mask-skewed state, or exact ACK debt bound
to an already-submitted immutable activation frontier, then drive the healthy
DPC event ring into a committed queue level and then one immutable op8 batch parent; it
must not fabricate queue-empty op8/op10 probes or a synthetic terminal stream.
Invalid, wrong-generation, poisoned, overrun, or lost-authority state must fail
closed and quarantine the generation without `DPC_ACTIVATE` repair.
The producer clears `commit_sequence`, writes and cleans the complete 24-byte
queue body, executes the barrier, and commits a new nonzero sequence last at
local-ring offset 192. It then builds the real 128-byte batch record at shared
offset 36864 with one through eight entries pointing to the fixed 1,536-byte
payload slots beginning at 36992. Layout version 3 must retain the complete
128-byte size, keep the eight 8-byte entries unchanged, publish eight parallel
`u32` DPC-admission low-CNTVCT words as `source_cntvct_lo: [u32; 8]` at bytes
88-119, publish `first_data_stage_deltas_q11` at bytes 120-123, commit the
repeated parent sequence at byte 124 last, and only then signal root. The low
`u16` must be the Q11 floor from the first populated `CHANNEL_DATA` entry's
source to its successful sequence-last private-queue commit; the high `u16`
must be that queue commit to the final precommit evidence-word sample. Values
`0x0000..=0xfffe` are valid, including raw zero; `0xffff` is
saturated/UNKNOWN. An EVENT before DATA must not take ownership of the word,
and the runtime must write zero when no DATA entry exists. The passive word is
excluded from body validity and authority identity. Every unused entry/source
pair must remain zero. Every prior or otherwise wrong-version, torn, or
source-only-changed authority sample must fail closed. Two otherwise exact
stable samples that differ only in the packed timing word must remain
behaviorally accepted, return `0xffff/0xffff` timing (UNKNOWN), and request no
recovery. One parent terminal with detail `0x5803` and
`result=count` publishes that batch. Root must double-sample the queue and batch
headers, validate generation, queue commit, parent, count, remaining, entry
bounds, slot bounds, and source stability, copy every frame, and reject any
authority-bearing header change on post-copy revalidation. A post-copy change
confined to `first_data_stage_deltas_q11` must preserve the payload and exact
ACK while degrading the timing halves to `0xffff/0xffff` (UNKNOWN). Runtime
queue tests must prove source and post-commit admission provenance follow
logical removal/reordering, that
admission is stamped only after successful queue-state commit and before wake,
and that the freed physical slot is cleared. Q11 tests must cover floor, raw
zero, modulo-low-word wrap, exact `0xfffe`, saturation, placeholder-body then
evidence-word then sequence-last commit ordering, and a production value. It
must prove an association event and a data frame are
delivered in order, stale work cannot mutate a replacement generation, and
malformed, torn, or issued-unknown completion state poisons without replay. A
zero-status `SOURCE_PENDING` event is consumed and rearmed through the ordinary
DPC lane with zero Function-2 reads. A real `I_HMB_FRAME_IND` or validated
retained frame condition remains mandatory for the fixed first read. A blind
ring advance, overwritten slot, stale generation, mismatched sequence, or
recovery state must fail closed. Missing, coalesced, and repeated notifications
must not change admission or require another edge.
An injected DPC child fault must retain its primitive detail, result, frame,
event sequence, action, and I/O phase through the later prompt quarantine.
Exactly one fresh child ticket is allowed only for a telemetry-bound
`CONTAINED` entry-inhibit fault that proves no command issue; the second such
failure and every command-or-later, owner-poisoned, malformed, timed-out, or
issued-unknown cut must fence the pair without advancing the event.
Accepted-normal-child continuation coverage, jointly required by
`m26b-wifi-sdio-notification-dpc-closure` and
`m26b-net-control-priority`, must prove the correction compositionally. The
production case drives one real matching normal exact `SteadyLease` terminal
through acceptance, proves the old global child is released, publishes exactly
one successor in the same admitted CYW43 call, and returns at that successor's
external wait without physically issuing it. In the same focused coverage, a
separate constructed production-private state proves cached `FifoWindow` is the
sole bounded deterministic `AdvanceState` transform allowed between two exact
children and that it routes to one successor submission. This composition must
not be reported as an artificially forced physical path through `FifoWindow`.
The old terminal mailbox must be stable-read into its immutable accepted CYW43
trace entry before coherent successor staging reuses that sequential mailbox.
Counterexamples prove no same-call successor for a contained-preissue `FAULT`
retry, malformed or mismatched terminal, issued-unknown state,
`OrdinaryContinuation`, `RxQuantumBoundary`, `RxQueueWait`, `RecoverySettle`,
`CompleteEvent`, `DeferredOwner`, or another external wait; no invocation may
loop into a second successor. The tests also prove unchanged event identity and
physical generation, one global/physical child at a time, later SDIO-only issue,
and zero new EventPump admission, poll, signal, priority, recovery, or GENET
behavior.
These explicitly scoped descriptors cover bootstrap/control/host-EAPOL and
Join-specific protocol fences; they must not authorize fresh steady NetData
work. A post-release DPC producer level retains only its exact event-sequence
lease. A stable
nonempty committed queue record may admit one immutable op8 batch parent with
one through eight entries. Notifications only prompt that stable read, and no
steady deadline may add source-inspection authority or create a rescue op8.

One admitted `DPC_ACTIVATE` authority must execute the Linux-ordered mask,
host-status/ring inspect, durable publish/coalesce, exact IRQ ACK, signal, and
rearm policy as a single bounded owner quantum with
`SDIO_DPC_ACTIVATE_STEP_BOUND=32`. Only failure of the frozen exact IRQ
acknowledgement may return `Pending` and cross an outer-turn boundary. That
retry may only acknowledge the same epoch; it must not reread source state,
republish the durable event, or replay device work. Exhausting
`SDIO_DPC_IRQ_ACK_ATTEMPTS=3` must produce one terminal fault, leave the
single CARD_INT event durable, keep the source masked and exact ACK pending,
disable DPC activation, and perform no reread or republication. The persistent
CYW43 DPC must inspect and drain the dongle/FIFO source, then recheck the
durable condition before sleeping and rearming the sole SDIO owner.

Active-DPC queue-delivery coverage must stage both an active DPC cursor and an
issued child, then publish already-completed private frames through the real
sequence-last queue and batch records. One intake-sealed, current-generation
op8 parent may commit one through eight frames while performing zero Function
1, Function 2, reciprocal-SDIO, or DPC-cursor actions and preserving the active
cursor and child identity byte-for-byte. The parent produces one terminal,
never one terminal per frame. Root copying that batch consumes no physical
owner operation and does not disturb the active DPC cursor.

Repeat the same memory-only publication with one exact persistent op11 instead
of op8. It must produce no op11 terminal, preserve the complete control cursor,
deliver the batch once, and advance only after root commits the exact disjoint
ACK last. The later exact CONTROL reply alone closes op11.

The focused case must drive the production state, not a fabricated completion:
clear the queue commit before mutation, write the body, clean/barrier, commit a
new nonzero sequence last; write all batch entries/payloads, clean/barrier, and
commit the matching parent sequence last. Root must double-sample queue state,
validate generation/queue-commit/parent/count/remaining/entry bounds, copy all
frames, and reject delivery if the header changes on its post-copy re-read.
Empty, torn, stale, mismatched, unsealed, out-of-bounds, physical-capable,
unrelated-control-exchange, recovery, issued-unknown, restart, and quarantine
cases fail closed. Exact active-op11 sideband accepts only its immutable parent
identity and matching root ACK. The former one-frame active-DPC exception and
pre-baseline rollback shortcut are retired.

Issued-unknown arbitration must completion-reap the immutable child before
applying the pair-restart hold. A late exact completion is terminal ownership
proof only: quarantine its result and payload, release the exact child, emit
one exact retained terminal fault for the old parent, and then permit the
fenced cold pair restart. No same-generation child replay, late payload
application, or second parent terminal is allowed. Every `Waiting` reap turn
must retain the exact selected ordinary grant or persistent parent/child
identity, reject endpoint command authority, and allow only that same reaper or
old parent to resume.

Backplane-attach coverage must drive the production retained cursor through
ALP request, every ALP read, FORCE_ALP, the 65-microsecond settle, the Pi
pull-up clear, LOW/MID/HIGH window programming, the first ChipCommon read, and
completion. Each child submission, completion-poll turn, explicit later grant
turn, exact-grant owner quantum, retained deadline observation, and pull-up-clear
operation must consume its own outer EventPump turn.
The terminal child poll and terminal deadline observation must return before a
following action can issue. Tests must hold ALP unavailable for more than 1,024
exact reads under a still-live virtual-counter deadline, then make it available,
and prove that cursor checkpoints—not foreground-trace growth—carry the last
`CHIPCLKCSR` value, poll count, and deadline. The request must be issued once,
the action trace for each checkpoint must remain bounded, and reaching trace
capacity must never substitute for the one-second timeout.

Firmware-preparation coverage must separately drive the production op-9 cursor
through passive cores, KSO, CARDCTRL, PMUCONTROL, Function 2 disable,
`CHIPCLKCSR=0`, `ALP_AVAIL_REQ`, ALP polling, and SoCRAM preparation. Tests must
prove ARMCR4 reset is asserted, configured, then cleared before a final
`IOCTRL=CPUHALT|CLK` fence, while D11 remains reset asserted. The ARMCR4 and D11
LOW/MID/HIGH window bytes, IOCTRL/RESETCTRL writes, flush reads, retained
settles, KSO read/conditional write, CARDCTRL read/write, PMU window/read/write,
and Function 2 disable read/write must each consume distinct outer turns,
including when the real controller seam completes immediately. Tests must
prove the zero write precedes the single ALP request on initial attach, every
child operation and deadline observation consumes a distinct outer turn, and
unavailable reads are separated by retained five-millisecond virtual-counter
settles under the absolute one-second deadline. Production-timing tests must
prove that one second and five milliseconds derive from the permitted counter
and permit about 200 physical reads, with terminal timeout on a later zero-I/O
deadline turn carrying exact ALP detail and last `CHIPCLKCSR`. A separate
structural checkpoint test may deliberately install an extended synthetic
counter deadline, hold ALP unavailable for more than 1,024 reads, and then make
it ready; that test proves the cursor advances without `0x5310`, trace growth,
or a second request, but is not production wall-clock proof. Function 1 enable
must likewise retain separate `IOEx` read, one-shot `IOEx.F1` write, `IORx`
poll, and deadline turns; an extended structural test must exceed 1,024
unavailable reads without trace growth or a second write, while production
remains bounded by the one-second elapsed deadline. Stale owner or
issued-unknown completion must perform zero same-generation I/O.
Stale request/generation, a failed `0x5337` SD-only write, issued-unknown
ownership, and a second same-generation preparation must all perform zero
replay. Exercise the exact clock-zero, request, and read descriptors through
the production parent-command plus staged-owner-descriptor/controller seam and
prove one controller issue per descriptor without a fabricated completion.
The same real reciprocal seam must prove PMUCONTROL uses one incrementing,
four-byte Function 1 CMD53 read followed by one incrementing, four-byte
Function 1 CMD53 write at the backplane-word address `0x8600`. Both operations
must be sealed as retained PIO by their normalized one-block geometry, validate
R5 before FIFO access, and move the complete four-byte host block in one
post-issue ready-edge owner quantum. The read and write remain separate
immutable child requests. The write payload must be the little-endian read
value with only `RES_RELOAD` added. Failure injected at either immutable child
operation must
terminate with `0x5333` or `0x5334`, perform no same-generation replay, and
never select a bytewise CMD52 shape or another engine.

After SoCRAM preparation, the production cursor must invalidate cached
firmware-transfer authority and re-prove the live contract before the first
bulk CMD53. Real reciprocal-ring/controller coverage must exercise, one outer
turn each, Function 1 block-size low/high reads, CCCR capability read with
`CAP_SMB` and low-speed `4BLS` validation, four-bit interface readback,
SHS+EHS speed readback, ALP availability/readback, RAM-window LOW/MID/HIGH
writes, and matching LOW/MID/HIGH reads. A later zero-I/O contract-commit turn
may publish block-mode, width, speed, ALP, window, and upload-prepared authority
only when every sampled value belongs to the unchanged request and generation.
A table-driven failure cut at every phase, including the local commit, must be
terminal, invalidate every derived fact, mutate no card shadow after a failed
write, and perform no second controller issue when the same command is polled
again. No fabricated direct completion may satisfy this coverage.

Firmware-release coverage must drive the production release cursor through the
production parent-command plus staged-owner-descriptor/controller seam. Prove
that every immutable child request issues at most once and that the persistent
parent advances through changed durable child state without requiring another
EventPump turn, notification, or yield between semantic phases. Preserve exact
Linux ordering from stale interrupt clear to DPC activation. Exercise 51 retained ARMCR4
clear/settle/read cycles, the exact
200-attempt no-counter HT fallback, final-success Function 2 readiness on poll
3,000, and final-success firmware readiness on poll 1,000 without trace or
deadline-table poisoning. Separately use an explicitly extended synthetic
counter deadline for more than 1,024 HT reads to stress checkpoint capacity;
that structural case is not the production one-second timing contract.
`IOEx.F2` must be written exactly once; the 20-millisecond SD-only
fence, HT settles, and all deadline observations must be zero-I/O turns.
Mutating the request sequence, generation, or reset vector, resubmitting after
an issued-unknown action, or repeating a terminal failure must perform no
child I/O. `firmware_execution_started` and `firmware_released` must remain
false until the exact RESETCTRL-clear and DPC-activation completions,
respectively.
Production-chain coverage must additionally drive normal control and
EAPOL-Start TX through the ordinary retained cadence and one cached-window F2
CMD53 child. It must separately intake-seal valid EAPOL-Key M2, M4, and
group-key responses and drive each through the existing paired finite-op7
marker with one current-generation frame, four owner operations, and 1,536
bytes. Drive a genuine cache miss through
exact LOW/MID/HIGH CMD52 writes followed by F2 with no per-packet IORx child,
drive the 20-command post-F2 release sequence to the generation's one retained
DPC activation and prove later controls reuse it, then
drive one pre-Gate-8 DPC event under its exact event-sequence steady lease
through owner-backed status, F2 read, empty-confirmation, and post-status work
before committed queue/batch publication. That event must use zero recurrent
continuation grants and retain the same lease between exact children. Drive eight real
RX frames through one immutable op8 parent, one sequence-last batch commit, and
one detail-`0x5803` terminal with `result=8`; copying all eight frames must issue
zero SDIO-owner operations. A full 256-frame software backlog uses 32 full batch
parents rather than 256 per-frame polls, grants, terminals, or signals.
Repeat the batch with an exact active op11, require zero op11 terminal until its
matching CONTROL reply, and require one cache-line-disjoint root ACK after the
post-copy header recheck before CYW43 may publish another batch.
Pre-issue terminal, post-issue unknown, torn queue/batch state, stale generation,
action-fingerprint, timeout, and continuation-grant cuts must not issue a second
child or mutate the replacement generation. Exercise the
shared op11 outcome classifier through the real association, PTK/GTK, and
SCB/filter/BSSID maintenance consumers: pre-TX `NOT_READY` and decoded firmware
replies are terminal, while every encoded post-TX reply timeout must suppress
Gate 7a/cursor advancement, publish the immutable ambiguity ticket, and enter
the exact pair restart with no same-generation replay. Association coverage
must additionally prove that an exact HAL-issued Join at
`CONTROL_TX_BEGIN` remains event-unarmed, stale sequence or route progress
cannot arm it, and only its exact post-Function-2 progress can do so. Inject an
EVENT after the initial pre-TX drain while the cursor is waiting for credit;
that event must commit and be root-consumed through the sideband batch/ACK with
zero Function 2 writes and zero op11 terminals before the single Join write is
admitted. At the later final SDIO pre-issue boundary, assert host
`CARD_INT` for a Join-marked Function 2 child and prove a typed not-issued
terminal, zero controller/DMA/FIFO work, unchanged operation-11 parent and
absolute counter deadline, no SDPCM advance or pair recovery, exact event
publication and attempt-scoped token binding, canonical DPC consumption with no
activation child, and exactly one fresh later Function 2 child with one total
physical issue after source clear. A fresh defer must reset the prior token; a
vanished or replaced token faults without reactivation. The same asserted source on an unmarked Function 2
descriptor must preserve its bounded foreground-fairness lane.

Card-init tests must prove CMD7 uses the R1b short-busy response and distinguish
the pre-command entry-inhibit wait from a post-command busy timeout. Only the
former may retry; the latter must be classified issued-unknown and leave through
pair recovery without same-generation replay. After CMD7, production-chain
coverage must drive the retained card-lane cursor through CCCR revision and
capability reads, SPEED read/write/readback, host clock while one-bit, CCCR IF
read/modify/write/readback, and final host-width programming in that exact
order. Every child or host operation consumes a separate outer EventPump turn.
The fixed Pi lane must reject CCCR revisions below 1.20, missing `CAP_SMB`, a
low-speed card without `4BLS`, a card without SHS, a changed request or
generation, and an enumerated-card flag without matching completed lane proof.
Every rejection clears derived width/speed/multi-block authority, poisons the
generation, performs no FBR or firmware operation, and cannot resume or replay
in that physical lifetime. Recovery coverage must run the canonical root pair
transaction through its sole `ReplaySdioEngine` lifetime and then drive fresh
retained card adoption end to end. It must prove no discarded card fact or
descriptor owner crosses the scrub and that adoption belongs only to the
completed replacement lifetime. Zero request or generation identity must fail
before the first CCCR read.

The first read must validate the request's writable bits while allowing only
the asynchronous availability bits to differ. Boundary tests must prove the
absolute one-second deadline, including that its terminal observation spends a
turn and cannot issue another CMD52. Per-command progress must identify ALP
request/poll, FORCE_ALP/settle, `BACKPLANE_PULLUP_CLEAR`, ChipCommon read, and
each LOW/MID/HIGH window CMD52 so a generic backplane phase cannot misclassify
the stalled command.

Before the first linked SDIO operation, HAL pinctrl coverage must prove
GPIO34-GPIO39 ALT3, CLK pull-none, and CMD/DAT0-DAT3 pull-up with BCM2711
register-native value `1`. Pure readback tests must accept the exact selected
fields while preserving unrelated bits and reject every single selected-field
function or pull mismatch. Target bootstrap must fail closed if that stable
readback was not published, and passive `wifi dump-state` must render both complete
GPFSEL3/GPPUPPDN2 words and the expected masked values.

Gate 4 coverage must exercise the ABI, SDIO owner, and passive root reader as
one proof chain. ABI tests must keep the 44-byte
`DriverRuntimeSdioClockSnapshot` disjoint from the CYW43 parent descriptor and
SDPCM TX aperture, reject torn/invalid samples, and validate the
HOST_CONFIG-only CCCR readback fields. The retained owner test must publish a
current completed-lifetime snapshot and prove a 50,000,000 Hz request becomes
41,666,666 Hz from the 250,000,000 Hz source and divisor `6`, with final
internal-stable/card-enable, CCCR `EHS`, host/card 4-bit, and generated
54,000,000 Hz `CNTVCT_EL0` timer evidence. Root ring tests must decode the same
record without a write or child turn. `wifi dump-state` tests must prove a current
snapshot can pass Gate 4 and that a missing or stale snapshot fails Gate 4 with
explicit `unavailable` fields; `clock=0Hz width=unknown` is forbidden for the
linked-runtime path. A separate owner test must prove snapshot-publication
failure cannot convert a successful physical `HOST_CONFIG` into a fault. These
tests must leave the GENET path unchanged.

The production pull-up-clear turn must perform exactly one child-runtime SDIO
operation: Function 1 CMD52, `SBSDIO_FUNC1_SDIOPULLUP=0`. It must return before
the first ChipCommon action and advance only after the exact completion. Each
fresh attach in one completed physical lifetime must contain exactly one such
descriptor. A stale owner must be rejected without I/O, and a failed or
issued-unknown clear must poison that lifetime and prevent replay. The
reciprocal descriptor-ring/controller test must also prove that the SDIO owner
claims the exact clear once before issue and rejects a duplicate without
controller I/O. A later clear is legal only after the canonical pair
transaction scrubs both runtimes and `ReplaySdioEngine` establishes a
replacement lifetime; there is no pending-generation reprobe allowlist. A
production-chain test must carry the action
from the CYW43 transport cursor through `service_command_turn`, the reciprocal
descriptor ring, and the real SDIO controller seam, including terminal poison
with no second controller issue. Legacy `BACKPLANE_PULLUP_SKIPPED` and
`BACKPLANE_PULLUP_FAULT_CONTAINED` parser tests may preserve old-capture
decodability but cannot satisfy current-image acceptance.

Pair-restart coverage must preserve the 22 logical
SDIO-before-CYW43 actions while treating each bootstrap-priority,
register-programming, resume, descriptor, and engine-replay substep as a
separately admitted operation. The two steady-priority lowers occur outside
that canonical restart only after control-plane readiness and each consume a
separate later supervisor turn. Coverage must also include firmware and NVRAM
chunks, core release, operation-11 control exchange,
control/data/any-frame work, generation, association, and host-EAPOL recovery,
data TX, ARP/GARP output, and the ordered pre-Join drain snapshot. The finite
pre-Join polling cap remains separate from steady RX batching. Tests must prove
the Join-only final pre-issue source fence closes the interval after that
snapshot without extending the policy to generic control/data descriptors.
The old typed not-issued child must never issue, retry, or replay. It preserves
the same logical parent, binds the exact event through the sole canonical DPC
lane without another activation, and permits that parent to publish one distinct
fresh child for one later source-clear issue under the unchanged deadline.
Failure-cut tests must reject stale completions, forbid same-generation replay after
issued-unknown ownership, and resume or fail deterministically at every
retained action. EventPump/NetStack tests must prove Wi-Fi urgency is retained
across later turns by committed state rather than private pre-root, EAPOL,
tail-ingest, TCP-flush, hot-dispatch, or smoltcp device bursts. An observed
notification only prompts a stable queue/batch read and is discarded after
that turn; it never becomes a consumed edge cursor or durable work level. The
same committed batch must drain with the notification delivered, lost,
coalesced, or repeated. Coverage must exercise every reason-mask class,
torn/stale queue and batch records, physical-lifetime changes, pair and
connection generation changes, quarantine, reboot, and selected-NIC change.
Coverage must also prove that a stable committed poisoned queue whose
generation matches the stable active SDIO DPC owner and whose two restart
contexts exist latches the one existing pair-recovery supervisor without a
signal, grant, or fallback lane. Unpoisoned, torn/unavailable,
generation-mismatched, owner-inactive, and context-incomplete samples must not
latch recovery; aggregate DPC client-sample staleness with a healthy ring must
remain diagnostic only. Repeated reads of the accepted poison must be
idempotent, and pair scrub must clear the old queue record before a replacement
owner becomes active.
A hard turn cap, fresh-parent time cap, actual physical response, or buffered
physical input must retain unfinished Wi-Fi work behind a fence and prove
`Serial`, optional `LocalSeat`, and `Dispatch` each receive their bounded turn
before Network re-admission. A complete command belonging to the exact active
authenticated CYW43 connection must instead end the current Network quantum and
use the next separate hardware-free `Dispatch` turn when no prior response
cursor is active or completed on that turn and no physical input/response is
pending. Dispatch must still consume newly arrived serial or local-seat input
first. Passive USB service debt may be deferred only through that command and
its existing bounded response cursor; after any exact retained parent receives
its current already-admitted turn, cursor completion must force one ordinary
USB/operator rotation before a second buffered command. Unauthenticated,
wrong-connection, GENET, and physical-input cases must retain the full fence.
The 25-ms time cap must not interrupt an exact
already-`Prepared` or already-`Issued` parent; physical/dispatch pressure and
the hard ordinary-EventPump turn cap may yield root admission only with the same
identity retained. A consumed terminal that sequence-publishes one immutable
request-less same-generation successor must also survive the fresh-parent cap
after stable before/after generation, pair, and physical-lifetime checks. Tests
must cover M4-to-PTK, active-parent-to-causal and causal-to-active handoffs,
requestless child-progress data TX, urgent paired-RX/control/authenticated-console
TX, wrong-generation rejection, and prove the continuation grants neither a
second operation nor generic host-EAPOL authority. Generic bulk TX remains
fresh-parent work. That fairness cap is not the persistent parent's
192-operation budget and cannot become a runtime progress clock. A
queued USB report containing actual input and a buffered complete network
command must not be bypassed.
GENET must neither sample nor retain the CYW43 queue/batch snapshot and must
leave its own operator-fence state untouched. CYW43 diagnostics must report
stable queue commit/depth, batch parent/count/remaining, and hint observations
without pending-wake, hit, clear, or recheck counters. Tests must also stage a child-invisible
sequence-zero
NetData request at the Gate 8 handoff, prove the next outer turn decodes it
through HAL's immutable retained identity and advances it beyond `Inactive`,
then prove host-EAPOL receives the next fresh prompt-poll turn without a pair
recovery latch.

Physical-input fence coverage must distinguish actual buffered input or a
physical response from USB service debt. Persistent first-report or
command-ready debt with no decoded or buffered byte may request exactly one
`LocalSeat` rotation, then must permit `Dispatch` and re-admit the same
sequence-zero `Inactive` CYW43 parent. A queued partial local-seat command must
still retain the fence. Recovery and terminal USB failure remain device-local
and must not mutate the CYW43 cursor.

Parent-replay coverage must table every CYW43 operation against transfer
stages 1 through 7. Only stage-1 `0x5103` on the seven single-action parents may
retry in-generation. Stage 7 is admitted only for the Join-marked Function 2
child and is a proven not-issued DPC deferral, not recovery or replay of an
issued action. That old child never issues again; after exact canonical DPC
consumption, the unchanged parent may publish one new F2 child with one total
physical issue. `TRANSPORT_INIT`, `FIRMWARE_PREP`, `RELEASE`, and every other
`CONTROL_EXCHANGE` failure must publish exactly one reciprocal parent request
and then fence pair recovery. Real-ring adversarial cases must cover maintenance op11,
all four prompt-poll owners, association and WSEC payload drift, and an ETH_TX
child cursor whose parent request was released before a carrier-generation
change. They must preserve the original descriptor, digest, ticket, and owner
generation and prove stable `submitted_turns` after the fault. The AArch64
control-preinit case must carry `bus:txglomalign=8` through the real reciprocal
controller ring and prove that either `BADARG` or `UNSUPPORTED` produces one
terminal action, no value-4 submission, and deterministic pair recovery.
Host-EAPOL timing tests must prove the retained PAE multicast refresh keeps
`allmulti=0` and `promisc=0` beyond every former rescue threshold.
Association-supervisor tests must hold an exact host-EAPOL prompt poll across
the absolute join timeout and terminal-event edge, prove the ordinary
host-EAPOL lane drains it through the real ring service without replacement or
pair recovery, advances the same persistent op11 from changed durable state
with every immutable child single-issue, and allow authentication
suspension/backoff only after that retained action is gone. An unchanged
external wait must block locally rather than require successive root turns.
They must prove an ordinary AUTH timeout is telemetry, while unsuccessful
SET_SSID, link-down/no-network, deauthentication, and disassociation remain
same-pair logical retry inputs and make Gate 8 report
`association-retry-pending` without synthesizing a physical recovery signal.
Prepared work with no accepted HAL request must be cancelled at the absolute
deadline without submitting a new action. A real fault or issued-unknown
prompt poll must retain the existing generation-poisoning proof. Other
optional-control tests must reject
transport-fault phase advancement while
retaining only their Linux-supported semantic `UNSUPPORTED`/`BADARG`
continuations and visible transport telemetry. A semantic rejection at an
allowed optional phase must not replace the first retained causal fault or
emit a false SDIO-owner snapshot; a transport fault at that same phase must
still become the latest visible transport terminal. Firmware-replay fault
coverage must retain the exact current-generation ticket, descriptor, payload
digest, completion sequence, detail, and result before the sole pair-recovery
transition clears the pending action.

The boot-supervisor lifecycle unit test must admit exactly one outer episode as
`attempt=1`, reject every attempt-2-or-later record, reject a second `begin`,
and prove no automatic whole-bootstrap backoff or reset can rearm it even at
`u64::MAX`. The initial physical pair is admitted exactly once; the pre-service
repair limit is zero. A typed runtime/SDIO or issued-unknown physical fault must
drain, fence, and poison exact ownership as bounded terminal cleanup, then
terminate/quarantine rather than publish `status=recovery`, start pair 2, or
renew the original deadline. A pre-issue
lease conflict that performed no child action and changed no scheduler state
must clear locally. Gate-local association, DHCP, and protocol retries remain
independently bounded and must not mutate the boot-episode identity.
Separate lifecycle coverage must hold every logical Gate 8 failure until the
original 90-second deadline, and must apply that same deadline after Gate 8
commit while DHCP/listener readiness is absent. It must retain
`CYW43_GATE8_TERMINAL`, publish one `status=permanent`, and quarantine without
entering `status=recovery`.

Production failure-cut coverage must show one queued `status=failed` record
after a retryable terminal failure and after the HAL guard is released, followed
by ordinary EventPump turns with no automatic CYW43/SDIO operation. When a
network stack was already attached, those turns must leave its poll/flush count
at zero behind an explicit quarantine, dispatch no buffered TCP command, and
end any network-origin session/stream/cursor authority locally while timer,
serial, local-seat, HDMI, diagnostic, fresh authentication, and reboot service
continues. Adversarial coverage must seed that retained stack with apparently
healthy CYW43 DHCP/EAPOL/TCP counters, then install a newer terminal Gate 2 or
Gate 6 fault and prove quarantine performs no status read, rejects the stale
frontier, renders the terminal gate as `fail`, renders later gates as `blocked`,
and keeps direct proof below the failure even if older observations reached a
later gate. Paced serial and
local-seat diagnostic, authentication, and reboot commands must still dispatch
or return a typed unavailable/fenced result. The same coverage is required for
an attached non-retryable bootstrap or runtime-recovery failure and a
completion lacking ready-generation proof: exactly one permanent terminal
status, explicit network quarantine, no later supervisor driver turn, and
ordinary operator liveness. High-impact
`preflight`, `begin`, `recovery`, `stabilizing`, `ready`, `failed`, and
`permanent` transitions must retain an HDMI rendering in their original order.
After quarantine, the ordinary linked-runtime phase test must also commit a
CYW43 RX queue/batch, optionally signal its hint, add HDMI work, and prove that
neither the state nor NIC is serviced while one bounded `Display` turn remains
reachable before `Serial`.
Serial and qlog must contain the exact machine record byte-for-byte; each HDMI
line must begin `[drivers] WiFi` and contain no
`CYW43_BOOTSTRAP_SUPERVISOR`. Coverage must delay display long enough to fill
the ordinary FIFO, add a terminal transition at saturation, prove that the
FIFO plus terminal reserve does not overwrite start/progress transitions, lose
the readiness release, or affect serial/qlog, and drain at most one rendering
per later `Display` turn. A second boot `begin`, any `backoff`, attempt greater
than one, any pre-service recovery, a recovery that renews the Gate 8 deadline,
repeated terminal record, same-turn display
submission, swallowed command, automatic post-failure pair repair, or
quarantined network poll fails the gate.

Production supervisor-schema coverage must use the exact compact suffix
`recovery=full ... telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes`, where
`full` is configured fail-closed policy, `qlog` is `/log/queen.log`, and `hdmi`
means a semantic mirror rather than byte-identical formatting. It must prove
every `preflight`, `begin`, `recovery`, `stabilizing`, `ready`, `failed`, and
generic `permanent` raw record and typed display rendering fit losslessly in
their separate fixed 256-byte queues at maximum integer widths. Attempt-zero
preflight cannot consume the episode; every later record is `attempt=1`. The
parser must reject `backoff`, `exhausted`, a second `begin`, and any attempt
greater than one. It permits at most one recovery in the active episode with
zero backoff and no deadline renewal, and only after the initial service Ready.
`failed` requires `backoff_ms=0` and the
exact no-next-attempt sentinel. It also permits
`attempt=1 status=permanent` as the sole pre-`begin` record when fallible
construction or immutable configuration/artifact validation fails. A
maximum-length terminal record must survive
saturated background breadcrumbs without evicting a response tail or prompt.
Only the exact-generation Gate 8 commit plus bound DHCP address and admitted
TCP listener may open one distinct steady-state runtime-recovery episode with
one consumed-once pair repair. Duplicate Ready for that generation must not
replenish it; restored service must emit the separate runtime Ready record
before one later episode can be armed. That lifecycle cannot reset the boot
result, and Gate 10 remains independent downstream acceptance.

Local-seat retained-service coverage must classify `Pending`, `Complete`, and
`Failed` through the production HAL wrapper. Every normal `Pending` phase must
leave the immutable USB command, readiness flags, no-reply counters, and
recovery state unchanged; a pre-issue terminal `Failed` must clear the active
command and fail closed exactly once, while issued-unknown retains its poisoned
identity without replay. Tests must also prove that sustained Pending traffic
cannot manufacture the pressure signal that suppresses HDMI, or the physical-
input signal that retains the CYW43 operator fence. Missing first-report or
command-ready proof with an empty local-seat queue must earn one bounded service
turn without becoming input; decoded or buffered input must retain its existing
precedence. Adversarial lease faults before and after the issue boundary must
prove that USB, serial, and HDMI never request CYW43/SDIO pair recovery:
pre-issue requests fail locally, while issued-unknown requests retain their
immutable identity in a poisoned slot.
Equivalent CYW43 and SDIO faults must still request deterministic pair recovery.

Descriptor replay, engine init, prerequisite admission, context replay, and
post-secure retained maintenance must have absolute virtual-counter deadlines.
Engine active-state validation covers sequence, opcode, flags, both arguments,
both auxiliaries, budget, and frame descriptor. A completion permanently
withheld after publication must expire into generation poisoning without a new
same-generation request. Sparse linked-serial stage telemetry may be queued
only after the HAL guard is released and is emitted on transitions plus
power-of-two repeats; it must not add a second child operation to that turn.
Tests must distinguish the authoritative bounded `/log/queen.log` append from
the all-or-nothing, best-effort linked-serial mirror: saturation may reject the
UART enqueue without a raw fallback, and a rejected sparse record must remain
eligible for a later same-stage attempt. Once the linked runtime owns serial,
every root diagnostic helper, including explicitly raw variants, must append to
`/log/queen.log` rather than touch the physical UART or current TCB.

Recovery coverage must include a fault before initial firmware admission. Once
the ordered pair restart completes and context-replay ownership is acquired, a
supervisor without a retained bundle must consume one later outer turn to
reacquire and validate the HAL bundle, validate its reset vector, normalize
NVRAM, publish the retained recovery context, and only then continue with
firmware replay. A forced bundle-admission failure must release context replay
as unsuccessful and produce the exact typed failure without firmware work, an
empty replacement context, or a HAL bypass.

Association, host-EAPOL, post-secure maintenance, data-TX, and pair-signal
fault injection must use the real reciprocal ring/controller integration point.
Each source must publish at most one immutable generation-bound recovery record
and return after its current outer turn, including when its own session guard is
held. The record distinguishes the current recovery generation from the owner
generation of an unresolved action. Tests must prove no recovery callback
relocks that guard, the retained supervisor is the only recovery-poison
mutator, a recovery generation advances exactly once, duplicate current records
coalesce without replay, a stale retained record cannot mask a later current
fault, and a stale completion cannot fence, clear, restart, or otherwise mutate
a replacement generation. A reciprocal-ring issued-owner-unknown key action
must bind its exact descriptor, payload digest, ticket, and owner generation
even when normal association policy advanced the logical epoch after initial
bootstrap. An association-generation mismatch and a pair-restart signal while
an op11 join cursor is retained must publish the current recovery generation
with that cursor's original owner generation and exact immutable fingerprint;
neither path may discard the cursor behind an empty pair-signal ticket.

The supervisor-lifetime test must keep the same retained owner after initial
network attachment and show that a later association, EAPOL, data, pair
restart, or context-replay signal fences ordinary network work before recovery
advances. Production-state serial tests must prove cutover from a matching
linked-runtime `Idle`, `Progress`, or `FrameReady` service completion without
requiring an input byte, while preserving accepted nonempty `FrameReady` as the
distinct RX-byte proof. The supervisor must remain blocked and report the
serial route as blocked when that service proof is absent. Tests must prove a
transient first miss retains the supervisor, keeps the ordinary root-UART
EventPump live, and retries after the 250 ms virtual-counter interval rather
than abandoning Wi-Fi for the boot. After cutover, supervisor status,
failure, result, and raw diagnostic records must enqueue or append to
`/log/queen.log` without a raw/current-TCB UART write; tests must show the
linked flush waits for the following operator turn and does not share a turn
with a CYW43 operation. Ordinary linked EventPump coverage must prove the
`Serial -> LocalSeat -> Dispatch -> Network -> Display` phase classes and the
bounded CYW43 network weighting. Selected-CYW43 admission coverage must commit
one empty-to-nonempty queue state from `Display`, `LocalSeat`, and `Dispatch`,
with the notification respectively delivered, suppressed, and coalesced. The
prompt must not skip or rewrite the already-scheduled `LocalSeat`, `Dispatch`,
or pending `Display` phase. At the next safe boundary, a stable nonempty queue
read admits `Network` when no physical-input or recovery owner is pending. The
independent durable HDMI queue must likewise admit one bounded `Display` phase
after `Dispatch` when CYW43 is idle, quarantined, or has no retained rotation
token. After a completed CYW43 physical-operator rotation it receives exactly
one bounded turn before the same durable Network identity resumes; after any
Display turn, a one-shot Network entitlement prevents a persistent redraw or
no-reply level from starving CYW43 discovery or GENET. Outside that bounded
post-rotation exception it must not overtake actionable durable Network work or
a physical response tail. Root-prompt coverage must combine visible pre-terminal prompt,
USB parser-ingress false, final-Ready false, and the explanatory USB-starting
line in one state rather than inferring authority from separate fixtures. The
admitted turn must still use the sole existing CYW43 owner. Repeating or
consuming the notification cannot re-arm admission or skip another phase. Real
serial/local-seat input and USB recovery retain their operator precedence.
First-report or command-ready service debt with no decoded or buffered input
receives one `LocalSeat` turn but must not be classified as real input or
retain the post-Dispatch fence; quarantine and reboot invalidate cached CYW43
state without a NIC turn. The same committed CYW43 state and optional hint
under GENET must leave its ordinary phase result and CYW43 quantum counters
unchanged.

Ordinary post-prompt EventPump coverage must also prove that the same
linked-serial cutover advances for every physical-network selection, including
GENET, rather than depending on WiFi supervision. A failed attach must preserve
the emergency root console without starving USB, display, or selected-network
service. Runtime serial tests must coalesce the notification edge, leave the
mini-UART RX level asserted, and prove that a later bounded service turn still
drains the byte. The pre- and post-service samples must share one byte grant;
after exhaustion only line status may be read and IRQ acknowledgement must stay
pending.

The central `network_contract_service_admissible` check must fence both direct
EventPump service entries, ordinary `poll_runtime` and pre-root
`poll_pre_root_network`. Coverage must prove the check runs before Network
service and again immediately before either CYW43 polling or retained TCP
flushing. Missing, active, failed, or replaced physical epochs and
recovery-active snapshots must invalidate cached CYW43 queue/batch state and
admit neither operation; the same cases must leave GENET service unchanged.

`Serial` may perform one TX-first reciprocal-ring turn; `LocalSeat` then polls
one retained USB keyboard turn so fresh physical input is buffered before the
network quantum. USB service debt may request that turn but cannot itself count
as input. `Dispatch` may consume one serial, buffered local-seat, or buffered
network command without polling the NIC or flushing TCP. `Network` may
admit exactly one ordinary NIC service or one retained GENET response flush and
must leave any received command buffered for a later `Dispatch` turn. One
CYW43 admission may let the linked runtimes follow an already-authorized
persistent op11, urgent op7, or DPC event lifetime through changed durable state
to bounded quiescence; it cannot admit a second root parent. NIC admission, TCP
flushing, and command dispatch must occupy distinct outer turns. A dispatched GENET command must
schedule zero same-turn flushes and retain a cursor owned by its active
connection. Each later
`Network` phase consumes exactly one flush attempt, bounded to eight phases
normally or sixteen while the display reports backlog pressure. A second
buffered command stays behind the first cursor, and a changed or absent active
connection rejects stale cursor work. A data-ready CYW43 connection must not
create the GENET cursor. A response flush, exact socket/parser work,
runtime/root RX backlog, current valid pending or masked SDIO DPC event, or
retained CYW43 NetData/TX continuation may retain `Network` subject to the
compiler-declared CYW43 operation/frame/byte budgets. The 192-operation parent
bound counts admitted HAL operations, not root polls, scheduler turns, or
notification hints. A separate 25-ms seL4 virtual-counter cap fences admission
of a fresh physical parent.

An exact issued persistent op11 whose stable HAL condition is `Waiting` must
not retain `Network` merely because its runtime descriptor, logical owner, or
HAL lease exists. That mask is exact-parent self-demand suppression only:
every other reason remains unchanged, committed DPC/RX and exact terminal-drain
work retain their ordinary scheduling authority, and `TerminalVisible` admits
the exact terminal consumer. Tests must prove that op7 and ordinary foreground
owners never enter this op11-only mask.

Gate 8 data-consumer publication must bind the queue and batch records to the
current connection generation, pair epoch, and completed physical-lifetime
epoch. Queue-state coverage must prove commit-clear-before-body mutation,
body-clean/barrier-before-sequence-last commit, nonzero monotonic commit, and a
stable double-sample. A changed generation, pair, or physical epoch rejects the
record even when a notification was observed.

Batch coverage must begin with one immutable op8 parent and one stable nonempty
queue record. It writes one through eight fixed payload slots and entries,
cleans/barriers, writes `committed_parent_sequence` last, and publishes exactly
one detail-`0x5803` terminal with `result=count`. Root must validate the parent,
generation, queue commit, count, remaining depth, and entry bounds, copy all
frames, and reject the entire batch if the post-copy header differs. The same
batch must deliver once with its notification delivered, suppressed,
coalesced, or repeated. No wake-hit, pending, clear, recheck, or per-frame
terminal state may influence the batch identity, content, or result. A
successful root-wake poll may set only the transient EventPump admission latch
for one safe Network turn, and passive poll/hit counters may record that
observation; both still require independently schedulable durable state.

Sideband coverage must then reuse that batch layout under one still-active
persistent op11. It must prove EVENT/DATA before the exact BCDC reply commits a
batch without publishing any op11 terminal or new parent sequence. Root copies
and delivers the frames once, then writes the separate 64-byte-aligned ACK body,
cleans/barriers only that line, and commits its queue sequence last. CYW43 must
not advance the op11 cursor or overwrite the batch until the exact stable ACK is
visible. Producer and consumer writes must remain cache-line disjoint. Missing,
torn, stale, wrong-parent, wrong-generation, wrong-queue-commit, wrong-count,
duplicate, and notification-independent ACK schedules preserve the batch and
op11. The later exact CONTROL reply alone terminalizes that parent.

From the first post-release event, the exact event-sequence DPC lease must
complete joined interrupt work, drain bounded RX, update SDPCM credits, admit
an already-ready urgent TX, and recheck completion/queue/event state immediately
before blocking. The current durable runnable condition must continue locally
across exact children, including equal deterministic private state; the first
exact external condition may block immediately even when newly observed. A nonempty
committed queue retains the next batch parent; exact committed empty state ends RX service.
Bootstrap, split-control/host-EAPOL, control pre-TX, and Join-only fences remain
separately bounded and must not become periodic steady NetData polling. The
sealed M2/M4/group-key finite parent is an explicit host-EAPOL bound;
EAPOL-Start and other control remain ordinary.

Every non-RX durable-work reason—data TX, ARP TX, runtime descriptor, root RX,
control reply, logical owner, terminal drain, host EAPOL, HAL lease, prompt,
maintenance, recovery, and their ordinary combination—must fail fresh-batch
admission without creating a notification or physical source transaction.
Those reasons still retain EventPump service for their own bounded work. An
accepted data TX must not create receive demand or fence another credited TX.
Conversely, an already-retained immutable op7 or op8 parent must continue
through its exact terminal without displacement. Deadline expiry must enter
typed exact-owner containment and produce zero rescue polls, source probes, or
synthetic RX completions.
Once a valid current-generation frame has been accepted into the FIFO, its
payload, digest, ticket, and generation must remain immutable, and acceptance
must not promote it outside the EventPump coordinator. A real retained NetData
op8 plus a queued DHCP-sized op7 must preserve the op8 request and identity,
spend no TX budget, create no op7 request or recovery, complete op8 first, and
only then admit op7. A committed copied/DPC/runtime RX level must run before a
fresh or requestless op7, while an already-started op7 remains exact through
its terminal and cannot be displaced by later RX. Once promoted, the runtime
must retain that exact op7 in `WAIT_CREDIT` without root replay, drop, or
recovery until DPC commits an admissible window. A separate no-RX
counterfactual must prove the joined Function-2 terminal releases the root
owner and permits a later queued successor without a credit-acknowledgement
poll. The active op7 must retain
its payload, digest, ticket, request, and generation through nonterminal
HAL/runtime turns until a typed `Submitted` terminal or its retained
virtual-counter deadline. No fixed turn count may abandon it after promotion.
Corruption, lost ownership, generation replacement, deadline expiry after
promotion, and typed fatal terminals must remain fail-closed.
Connection-generation, pair, physical-lifetime, recovery, quarantine, reboot,
and selected-NIC changes must clear the lifetime cursor. GENET must never read,
install, clear, or report this state.

Authentication without pending work must not extend the quantum. Tests must
prove that an independent 25-ms virtual-counter clock, not operation count,
performs one bounded `Serial -> LocalSeat -> Dispatch` checkpoint and leaves at
most one `Display` turn pending after elapsed expiry. The checkpoint must
perform zero NIC operations, preserve the exact CYW43 parent/lease and
quantum-composition identity, complete the three physical-console phases, reset
only its own cadence clock after `Dispatch`, and resume that same quantum. More
than three cheap Network operations inside 25 ms must perform no checkpoint. A
fresh committed empty-to-nonempty queue transition must remain visible without
rewriting a scheduled LocalSeat, Dispatch, or Display phase; a notification may
prompt the later check but cannot latch admission. Tests must advance time
between outer turns and
prove that a partial local-seat line with pending HDMI echo takes
`Dispatch -> Display -> Serial`, retains the exact operator fence/parent, and
admits no intervening Network turn. With the same pending echo, an owned reboot
acknowledgement or physical response tail must instead route Dispatch directly
to Serial and leave the echo pending. Tests must also
prove that a quantum already at its deadline and without an exact retained
parent returns to `Serial` with zero additional NIC/SDIO operations. An idle
selected interface must not acquire the pair priority lease. EventPump must
evaluate the exact side-effect-free association-owner predicate and open the
outer pair lease before NetStack polls or allocates the first Join, including
when the rendered status remains stale. The first actionable selected-WiFi
turn must reserve and boost SDIO then CYW43 once;
later exact current-generation parents in the same quantum must add no
scheduler writes. Every quantum exit path must latch the fresh-work close
fence. An exact active parent, including an ABI-invisible
sequence-zero `Prepared` parent, must prevent an open lease from closing
between stages only after HAL proves a root-continuation operation, a nonzero
immutable fingerprint, matching request and logical generation, current pair
epoch, open priority
reservations, and no pair restart or context replay. This identity must remain
stable across the EventPump's before/after snapshot. Once its exact wait
receipt proves the child is blocked, EventPump must perform no CYW43/SDIO poll
while retaining the lease `Open`; a durable IRQ/DPC/RX/terminal condition must
resume that same episode. If close has already
fenced it, successive admitted `Network` turns may advance only that same root
parent while rechecking request identity and monotonic issue state after every
turn. Inside an admitted persistent or finite event lifetime, bounded linked
runtime helpers may continue while durable semantic state changes and each
immutable hardware request may issue at most once. Elapsed 25-ms time must not
interrupt that parent. An operator response or the hard ordinary-EventPump
turn cap may yield to `Serial` and `LocalSeat`, but the next Network slice must
resume the same parent; when elapsed and hard bounds coincide, telemetry must
classify the yield as `turn_cap`, not `time_cap`. Request substitution,
`Issued`-to-`Prepared` regression, or disappearance without a typed terminal
must request pair recovery. After the exact parent terminates, restore order
must be CYW43 then SDIO before the EventPump exits the slice.
Fresh generic NetData pre-poll admission at both the outer stack wrapper and
inner budgeted service must also reject any retained host-EAPOL TX/key owner.
Coverage must stage a request-less post-secure M4 op7,
prove fresh op8 admission is closed without changing the WiFi data-ready
label, prove an already-assigned exact NetData continuation remains
non-revocable, and prove fresh admission reopens only after the M4 terminal.
The successful exact M4 Function-2 terminal must rearm only its retained
post-secure M3 tuple for a later AP-driven retransmission; a submit fault must
not rearm it, and neither case may create a proactive replay or poller.
This bounded service is available before TCP authentication so raw DPC and
retained owner work cannot be starved while establishing a connection. Every
turn must still admit no more than one CYW43 physical operation, and either cap
must release to `Serial` and `LocalSeat`. A complete buffered TCP command and a
pending physical response must also exit immediately. The exact authenticated
command may choose the next hardware-free `Dispatch` turn before passive USB
service debt, but an active response cursor blocks the next command and its
completion restores the ordinary USB/operator rotation. Tests must prove idle,
stale-epoch, poisoned, overrun, acknowledgement-failed, and inconsistent CYW43
DPC work plus GENET do not enter the quantum. GENET must retain its ordinary
single-Network-turn rotation and all CYW43 quantum counters must remain zero.
At `Network` entry, quarantine and an already owned physical response must skip
NIC inspection and polling, open no CYW43 quantum, consume no CYW43 turn, and
return directly to `Serial`. The sole exception must be the exact
network-origin reboot acknowledgement drain; after that required NIC service
turn, or when a physical response becomes pending during an admitted operation,
the next phase must be `Serial` rather than `Display`. `netstats` must expose
quantum count, turns, maximum duration, `operator_yields`, and exit reasons;
`operator_yields` counts bounded physical-console checkpoints
(`Serial -> LocalSeat -> Dispatch -> pending Display`) and may be nonzero only
for selected CYW43. The reported checkpoint cadence is 25 ms and no
network-operation count is a second trigger. Selected WiFi must additionally
emit:

```text
netstats: cyw43_quantum runs=<n> turns=<n> max_turns=<n> max_elapsed_us=<n> operator_yields=<n> checkpoint_ms=25
netstats: proof_policy m26d_net_first=no physical_input_yield=enabled
netstats: cyw43_priority_lease state=<inactive|acquiring|open|closing|restoring|poisoned> pair_epoch=<n> mask=0x<mask> active=<yes|no> close_pending=<yes|no>
netstats: cyw43_priority_lease_counts opens=<n> closes=<n> restores=<n> recovery_revocations=<n> amortized_requests=<n> failures=<n>
```

`wifi dump-state` must report stable RX queue generation,
depth/capacity, flags and commit sequence; batch parent, generation,
queue-commit, count, remaining and final committed parent; and the passive
constant `rx_hint observed=no authority=none history=none` plus the fault-only
`sdio_deadline_hints` count. They must separately report the root-hint route as
`authority=none condition=durable-service-state` and cumulative root-wake
poll/hit counters. The condition label is a route/recheck contract, not a
causal attribution for the most recent hint. Those counters are passive
observations only: tests must
prove that the transient admission latch is consumed after one safe Network
turn, cannot admit an operation without durable work, and leaves the
generation/pair/physical-lifetime-bound durable-resume identity live while
work remains. Tests must also prove stable
double-sampling, one terminal per batch, post-copy header revalidation,
continued service from remaining committed depth, identical behavior with a
missing/coalesced/repeated notification, identity-cut rejection, and GENET
non-interaction. Legacy `rx_watch`, `deadline_probes`, wake-hit, clear, and
recheck fields on the data-handoff record may remain only in old-capture parser
fixtures and must not be treated as reachable driver state; they are distinct
from the live passive root-hint counters.

The focused acceptance tests
`cyw43_sdio_network_priority_lease_amortizes_scheduler_transitions`,
`cyw43_sdio_network_priority_lease_closing_drains_exact_parent_and_blocks_fresh_pair_work`,
`cyw43_sdio_network_priority_lease_partial_failure_is_rolled_back_or_poisoned`,
`retained_priority_lease_identity_rejects_request_fingerprint_and_generation_aliases`,
`eventpump_join_opens_one_network_episode_from_inactive_outer_lease`,
`eventpump_join_issue_and_terminal_share_one_open_network_episode`,
`missing_clock_caps_untyped_gate_but_not_exact_retained_receipt`,
`pair_restart_completion_mints_only_owned_cold_epoch_provenance`,
`recovery_request_during_cold_cursor_survives_and_supersedes_completion`,
`cold_epoch_provenance_is_revoked_by_failure_and_unfinished_drop`,
`cyw43_pending_request_before_context_replay_routes_recovery_without_waiting`,
`root_retained_grant_is_exact_monotonic_and_rearmed_from_consumed_truth`,
`root_grant_ack_between_poll_and_notify_advances_without_signal`,
`late_root_completion_between_poll_and_publish_suppresses_replacement`,
`cyw43_persistent_transaction_is_derived_only_from_exact_staged_op11`,
`persistent_op11_commits_then_signals_once_and_poll_miss_creates_no_edge`,
`sdio_persistent_transaction_marker_is_scoped_to_one_linked_primitive`,
`cyw43_parent_admission_requires_exact_source_for_steady_ack_pending`,
`critical_eapol_f2_terminal_survives_concurrent_dpc_ack_pending`,
`steady_parent_ack_before_event_retains_op7_to_terminal_without_root_grant`,
`malformed_tagged_steady_parent_and_child_never_enter_root_grant_lane`,
`dpc_routes_durable_work_without_starving_commands`,
`dpc_durable_event_and_cursor_ignore_deferred_hint_history`,
`dpc_idle_prewait_reenters_for_source_committed_before_cursor_creation`,
`dpc_exact_child_waits_for_terminal_without_poll_or_grant_hint`,
`control_pre_tx_reuses_only_a_quiescent_generation_long_dpc_lifetime`,
`control_pre_tx_binds_one_event_then_advances_past_reassertion`,
`control_pre_tx_missing_bound_event_faults_without_reactivation`,
`persistent_control_reuses_healthy_dpc_lifetime_without_a_child`,
`persistent_control_marked_lifecycle_reaches_reply_without_grant_or_hint_history`,
`idle_prewait_reenters_only_for_a_fresh_one_way_sdio_child`,
`production_masked_control_uses_exact_owner_activation_before_tx`,
`production_join_final_fence_runs_canonical_dpc_then_issues_exactly_once`,
`dpc_cursor_routes_by_exact_owner_state`,
`production_dpc_normal_exact_completion_publishes_one_successor_before_return`,
`production_dpc_normal_exact_continuation_preserves_rxbound_queue_and_settle_stops`,
`sdio_external_dma_joins_irq158_and_irq116_once`,
`cyw43_rx_queue_state_commit_is_sequence_last_and_stable`,
`cyw43_rx_batch_parent_commits_eight_frames_once`,
`cyw43_rx_batch_survives_missing_coalesced_and_repeated_hints`,
`cyw43_rx_batch_rejects_torn_stale_or_mutated_state`,
`cyw43_rx_deadline_faults_without_rescue_poll`,
`production_owner_notification_is_hint_for_one_exact_granted_quantum`,
`cyw43_idle_receive_lifetime_fails_closed_on_pair_or_physical_change`,
`cyw43_non_rx_work_reasons_cannot_manufacture_fresh_batch`,
`cyw43_data_tx_terminal_admits_successor_without_inbound_credit_ack`,
`cyw43_eth_tx_no_credit_reports_observed_window_snapshot`,
`cyw43_receive_delivers_copied_rx_without_advancing_queued_paired_tx`,
`cyw43_copied_rx_is_delivered_before_pending_data_tx_progress`,
`cyw43_event_tx_hook_drains_copied_rx_before_fresh_tx`,
`cyw43_event_tx_hook_drains_durable_runtime_rx_before_fresh_tx`,
`cyw43_event_tx_hook_finishes_exact_tx_before_later_runtime_rx`,
`cyw43_event_tx_hook_defers_queued_tx_behind_exact_netdata_owner`,
`cyw43_event_tx_hook_does_not_stage_arp_into_the_paired_rx_slot`,
`cyw43_event_tx_hook_preserves_paired_rx_slot_ahead_of_arp`,
`cyw43_event_tx_hook_frees_full_fifo_for_pending_rx`,
`cyw43_eth_tx_terminal_reports_admission_window_without_new_rx`,
`steady_network_dpc_condition_preserves_ack_before_sequence_publication_order`,
`sdio_dpc_snapshot_exposes_front_only_after_sequence_last_producer_commit`,
`cyw43_rx_queue_signal_is_only_a_non_authoritative_hint`,
`cyw43_open_network_parent_requires_complete_current_identity`,
`linked_cyw43_operator_checkpoint_preserves_quantum_composition_state`,
`linked_cyw43_operator_probe_completes_serial_command_under_durable_pressure`,
`linked_cyw43_persistent_usb_service_debt_gets_one_operator_rotation`,
`linked_cyw43_hard_turn_cap_wins_simultaneous_time_deadline`,
`linked_cyw43_closing_lease_drains_exact_parent_contiguously`,
`linked_cyw43_unleased_boundary_adopts_only_one_exact_parent`, and
`post_secure_host_eapol_tx_blocks_fresh_net_data_pre_poll` must pass.
The eight generation-bus cases collectively prove healthy zero-child activation
reuse, exact sequence-zero-capable one-event binding, lost-token quarantine
without reactivation, hintless retained-parent progress, the final one-way
command-ring pre-wait recheck, typed mask-skew repair, and one physical
Function-2 issue after canonical DPC.
Together they must prove the exact policy line
`m26d_net_first=no physical_input_yield=enabled` and exactly four scheduler
writes for a clean quantum
regardless of how many exact parents it covers (`SDIO boost`, `CYW43 boost`,
`CYW43 restore`, `SDIO restore`), close-time fresh-work rejection and
exact-parent drain, clean rollback versus poisoned recovery, current-generation
binding, and GENET non-applicability. The WiFi `netstats` fixture must preserve
both complete records at maximum counter widths and a quiescent clean sample
must report `state=inactive active=no close_pending=no` and `failures=0` with
`opens=closes=restores`; after steady traffic, `amortized_requests` must be
nonzero. A recovery revocation is acceptable only with matching contemporaneous
typed pair-recovery evidence. The GENET fixture must omit this WiFi-only
line and keep all CYW43 quantum counters zero.

The 2026-07-31 exact `0b15321d0c12` hardware baseline for this performance
gate is deliberately non-passing despite 7/7 first-lifetime startup. One
power-off boot and warm R01-R06 all used attempt 1 and pair epoch 1, passed
Gate 8a-8h, and bound DHCP without pair recovery. Its clean 506-request WiFi
TCP stream had about 380 ms request-to-first-response p50, 468 ms p95, and only
about 2.42 exchanges/s despite no retransmission, reset, sequence disorder,
reconnect, or zero window. Across the six ICMP-probed lifetimes—the power-off
boot and R01-R05—the first echo was received and caused the Pi to ARP-resolve
the host about 126-179 ms later. smoltcp 0.13.1 had already constructed its
automatic stateless Echo Reply, but Ethernet dispatch returned
unresolved-neighbor and discarded it. This proves the first CYW43 wire ingress
was delivered and isolates a common NetStack reply-lifetime defect; it is
neither RF loss nor acceptable cold-neighbor behavior. Current acceptance must
retain that original reply through neighbor resolution. These figures still
locate the separate material persistent-flow defect in CYW43
linked-runtime/HAL cadence and do not prove either source repair.
R06 sent TCP before ping: the Pi retained the SYN across cold neighbor
resolution, ARP-requested the host after 130.616 ms, returned one SYN/ACK after
322.861 ms, and closed cleanly without a SYN retry. Preserve that cold sample
separately and evaluate the driver-performance threshold on ARP-warmed traffic.

The instrumented slow-path comparison is part of that non-passing baseline:
roughly 140,000-168,000 outer Network turns ran over about 130 seconds, or
roughly 1,100-1,300 turns/s, while only about 10,000-11,000
`cyw43_quantum` runs completed. f4 R02 recorded 153,576 Network turns for 1,936
covered root parents, while root/runtime RX queues reported zero drops or
overruns. Its paired pcap returned host ACKs for Pi data in roughly 0.1-0.2 ms
but left some later Pi segments off air for roughly 0.37-0.40 s. This rules out
RF/TCP recovery, smoltcp queue loss, and a simple shortage of raw EventPump
turn opportunities as the primary delay. Historical source audit of that
rejected cadence falsifies ordinary request setup as the missing f4 repair: f4 already used
`PREISSUE_STEP_BOUND=16` for one deterministic CMD52/CMD53
preflight/register/COMMAND owner quantum, apart from the request-owned
status-clear retry. Coverage must preserve that existing invariant without
crediting it as a new fix. The phase-separated f4 scheduler is a historical
single-lifetime oracle, not the current cadence architecture. The current
proof keeps one immutable op8 parent alive end to end while its linked
Function-2 children cross the bounded SDIO owner. A normal first-read-sized
receive uses about five physical CYW43-to-SDIO children, or six with a
remainder. The shared 32-KiB aperture means a hot DPC episode performs zero
Function-1 window writes; a cold episode adds exactly one LOW/MID/HIGH
sequence. Coverage must prove those hot/cold counts, exact child identity across
ordinary foreground grants, persistent markers, DPC event leases, and terminal
completions, and no replay after an issued-unknown cut. Production DPC must use
the same exact event-sequence lease from the first post-release event before
Gate 8 through steady traffic; mutable data-plane readiness cannot select a
second continuation-grant path.
SDIO IRQ158 and DMA channel-4 IRQ116 must join in the same physical owner and
yield exactly one terminal in either arrival order. The CYW43 producer must
clear the queue or batch commit before body mutation, write and clean the
complete body, execute the publication barrier, write the new nonzero commit
sequence last, and only then signal its consumer. The consumer must treat that
notification as a coalescing hint, stable-read the durable condition, and make
progress even when the hint is missing, coalesced, or repeated. Coverage must
also drive the same batch as persistent-op11 sideband, commit the disjoint
root-owned ACK line last after post-copy validation, and prove the CONTROL reply
alone terminalizes op11. Coverage must
also prove one hardware wake services the active transfer, a bounded RX batch,
SDPCM credit updates, and an already-ready urgent TX before the final
condition-before-sleep recheck. The 32-step `DPC_ACTIVATE`, v11
cause/frame-turn counters, and generation-scoped TX phase counters remain
passive and bounded; none authorizes progress or reconstructs history.
The DPC overrun and ACK-attempt counters are per-pair telemetry. Current flags
and exact pending state authorize service; a recovered historical ACK attempt
cannot disable healthy work, while accepted hardware evidence still requires
zero errors. The phase-authoritative SDIO deadline arm must clear on ordinary
terminal with zero fault hints; only a stable expired exact arm may cause the one-shot
root-to-CYW43-to-SDIO containment hint.
Immutable descriptor checks must reject endpoint fallback after gate-state
loss, and repeated issued-unknown reap waits must preserve the exact grant and
parent identity. The historical phase count is non-authoritative; current tests
target the persistent durable-condition transaction. TX coverage must prove
the aggregate-capacity-16 urgent/bulk priority classes, EventPump-only
coordination, queue-only TxToken consumption, exact foreign-owner preservation,
active-op7 terminal priority, committed RX before fresh/requestless TX, copied
  RX before ARP when paired-response capacity exists, no same-turn successor
  promotion, runtime-window retention of the exact op7, and joined-terminal
  root release.

The exact `25f406d9cc26` image (image id
`92d8326196f954c5f56b45b092cc2b17ae7cf5ffe9bfff7bbc6df806c1030884`,
SHA-256
`6c2fcbb266e4158f94ef6436b8fc37830118111ce53b2239a066318448cd19a1`)
fails this gate. The power-off boot and warm R01-R05 all passed Gate 8a-8d on
attempt 1 and pair 1, then failed Gate 8e with
`host-eapol-prerequisite-required`; 0/5 warm boots reached usable service.
R01-R04 first recorded PTK-stage deauthentication reason 2 at generations 1,
2, 1, and 3, while R05 recorded no first terminal receipt before the common
generation-6 failure. DPC counters remained balanced and loss-free, but the
boot-paired pcap contained only Pi-source LLC/XID broadcasts and no Pi EAPOL,
ARP, IPv4, DHCP, ICMP, or TCP. `.coh`, TCP latency, and REST pressure were
therefore correctly withheld. Any replacement image must first restore this
boot/EAPOL gate before performance qualification.

The later exact `b91b31f9a2b471d37ceeb66469e3fc10609e4df2` hardware group
is also non-passing. The power-off boot and R01 each exchanged 40 Discovers /
39 Offers without a Request; R02-R05 each completed one DORA, yielding only
4/5 bootstrap. R05 then dropped 21/21 host Echo Requests after the rejected
design's consumable DPC/root scheduling edges stopped advancing. This is
hardware evidence for the retained-TX/queued-Offer ordering repair and, more
importantly, against treating a wake edge or a timed probe as durable work.
The current repair is the committed queue-state plus one-parent batch
transaction: the persistent child rechecks that condition before sleep and no
32-ms rescue exists. Wi-Fi `.coh` and pressure remain withheld for that image.
GENET on the same image passed the
cold-neighbor request/ARP/reply
sequence, `.coh`, and a one-minute pressure control with a clean TCP flow and
1.216 ms p50 / 1.538 ms p95 request-to-first-Pi-payload latency. The new Wi-Fi
source remains pending rebuild, flash, and hardware proof.

The source-shape test must preserve Linux `brcmfmac`'s relevant performance
invariants without importing its private workqueue or host lock. Normal SDIO
mode has interrupts enabled and polling disabled; the ISR marks durable pending
work, the ordered owner performs an RX-first bounded drain, and TX completion
does not mandate a physical receive-source read. Preserve `BRCMF_RXBOUND=50`,
`BRCMF_TXBOUND=20`, `BRCMF_TXMINMAX=1` while RX remains pending, a 2,048-entry
TX queue, 32-KiB aggregation, and block-mode CMD53. Cohesix must prove its
translation through the existing 50-frame RX bound, credits/glom, 32-KiB shared
aperture, 512-byte Function 2 blocks, multi-block CMD53, external DMA, stable
queue level, and one committed root batch. Its scheduler form remains one
bounded EventPump root-parent admission with physical-console checkpoints,
ordinary foreground grants, persistent op11 and urgent-op7 parents, an exact
event-sequence DPC lease, one HAL owner, and one linked SDIO runtime issuer.
The current durable runnable condition continues inside the admitted runtime
lifetime, including equal deterministic private state, to the first exact
external wait; no test may pass by
adding a timer poller, rescue inspection, private physical drain loop, or a
second transaction lane. Notifications may prompt a durable-state check but
carry no authority, work count, or history. At quiescence the final stable
condition recheck must observe no work and block without requiring a later
edge.

The persistent-control composition must first prove the normal path: one
post-release activation remains live for the physical generation, a healthy
empty/unmasked ring admits ordinary control with zero `DPC_ACTIVATE` children,
and one exact committed pre-TX event is consumed without admitting an unbounded
later-event stream. A separate activation-repair fixture must include the exact
inactive/mask-skewed generation-1 Join boundary observed on hardware: its
marker-paired `DPC_ACTIVATE` arrives
with activation false, state/ring `CARD_INT` mask state skewed, and an exact
IRQ158 epoch pending after the child frontier was submitted. That child must
remain generation/link/ring/poison checked, retain parent admission solely from
its immutable active frontier, ACK exactly that epoch, commit cleared owner
health, publish its terminal without one SDHCI command or continuation grant,
and let the same parent reach its fenced Function-2 TX. Removing the submitted
frontier or changing its operation must reject the parent. Under the identical
precondition, ordinary persistent CMD52/CMD53 children must remain rejected;
wrong epoch, poison, overrun, and marker mutation must also fail closed. A
real IRQ arriving immediately before or after durable owner-command publication
must retain its exact ACK epoch through private cursor admission, including a
zero host-status sample; only the same owner quantum may consume it. An idle
unclaimed zero-status badge must still be acknowledged without creating work. A
failed dedicated final rearm must preserve the typed first owner fault rather
than surface only as a later persistent-marker rejection.

Common NetStack coverage must use one fixed-capacity raw IPv4/ICMP socket as
the sole Echo Reply owner. From its two-frame RX side, the service admits only
checksum-valid Echo Requests from a unicast source addressed to the exact
assigned local IPv4 address and constructs at most one reply per NetStack
service turn. Its one-frame TX side
must remain queued while smoltcp reports the neighbor missing, allow ordinary
ARP retry policy to run, and emit exactly one reply after resolution.
Identifier, sequence, and payload must match exactly. Duplicate, malformed,
nonlocal, wrong-peer, saturated, expired, and stale-generation/reset work must
not emit a reply. WiFi generation, DHCP address, and explicit stack-reset
transitions purge both queues. The host queue model and Pi-feature production
tests must exercise this driver-neutral policy; neither may pass by warming ARP
before the cold test, adding another packet issuer, manufacturing CYW43 source
work, or changing GENET scheduling.

Run the focused source gates serially:

```bash
cargo test -p root-task --lib --no-default-features --features net-console icmp_echo -- --test-threads=1
cargo test -p root-task --lib --no-default-features --features driver-tests-pi4 retained_icmp_echo_due_extends_only_the_cyw43_network_lane -- --test-threads=1
cargo test -p root-task --lib --no-default-features --features driver-tests-pi4 socket_capacity_covers_full_profile_with_outbound_probe -- --test-threads=1
cargo test -p root-task --lib --no-default-features --features driver-tests-pi4 icmp_echo_reservation_failure_releases_earlier_leases -- --test-threads=1
```

Default future exact-image acceptance must pair one power-off plus five
first-pair warm WiFi boots with one GENET control and then run the same
sequential request and no-retry pressure workloads. Before ping, TCP, `cohsh`,
or benchmark traffic warms the Pi's host-neighbor entry on each lifetime, send
exactly one ICMPv4 Echo Request. The boot-paired pcap must show that request,
the Pi's ARP request, the matching host ARP reply, and exactly one Echo Reply
with the original identifier, sequence, and payload, without a second host Echo
Request or duplicate Pi reply. Failure of that semantic cold-neighbor gate
fails the lifetime. Record its elapsed time separately and use only subsequent
ARP-warmed ping, SYN, and request-to-first-payload samples for cadence
comparison. The earlier tenfold floor of WiFi request-to-first-payload p95 at
most 40 ms and at least 29 sequential requests/s is a non-blocking optimization
target; the aggressive low-overhead target remains p95 at most 10 ms and at
least 100 requests/s. Exact image `7a10b8fd6acc` closed Milestone 26b through the
operator-approved usable/reliable matrix in the Build Plan. That image-specific
record did not run the pre-TCP ICMP probe, explicitly excluded the RF-affected
W04 sample, and cannot be reused as a generic waiver for a future changed image.
At true idle the committed queue must be stably
empty, no batch parent may be active, and elapsed time alone must produce zero
Function-2 reads, op8 parents, priority leases, live notification-derived
admission latch, or GENET work. Historical passive root-wake poll/hit counters
may remain nonzero but cannot schedule work. A deadline may terminate only an
already-active exact request whose
physical completion failed to arrive; it must never create a source
inspection, infer RX demand, or rescue traffic. A visible queue level must
produce one immutable op8 parent whose single `0x5803` terminal carries up to
eight real frames, and a remaining nonzero level must be serviced again without
requiring a fresh notification. Missing, coalesced, and repeated hints must
produce the same frame order and terminal count.
If op11 is active, the visible queue may instead produce the same batch as
nonterminal sideband; its exact disjoint root ACK must release the batch without
changing parent identity. Ordinary acceptance also requires
`sdio_deadline_hints=0`; a nonzero value is fault-containment evidence and fails
the normal-latency sample even if the request eventually completes.
Accepted samples must additionally show no unresolved warmed-traffic loss,
sequence gap, reset, established zero window, or reconnect, and the pressure
run must have zero timeout masking. Bounded recovered retransmissions remain
visible evidence rather than being hidden. GENET must pass
the same common cold-neighbor semantic gate while latency, throughput, and
scheduler counters remain within its control contract. Exact image
`7a10b8fd6acc` supplies the accepted Milestone 26b hardware result; a future
changed image must establish its own evidence rather than inheriting this one.

CYW43 device tests must also prove that a retained TX blocks another physical
issue, while a runtime-closed SDPCM window retains the already-promoted exact
op7 in `WAIT_CREDIT` without blocking bounded root-queue reservation or
memory-only copied-RX delivery. They must preserve the sole active owner,
urgent-before-bulk selection and FIFO-within-class order, reserve paired
response capacity before dequeueing RX, and produce zero fabricated TX drops.
TxToken consumption must remain queue-only. An exact retained op8 must defer
op7 promotion without losing either identity or charging TX budget; once the
lane is free, the coordinator must advance an active op7 to terminal, then
drain one committed copied/DPC/runtime RX level before promoting a fresh or
requestless op7. Copied RX must return before pending ARP while paired-response
capacity remains available.
Full-capacity backpressure may promote and advance only one eligible head
and must never issue a second operation or promote a successor in the same outer
turn. Runtime `WAIT_CREDIT` must preserve the same immutable parent, and a newer
DPC window may resume only that exact op7. A terminal-release counterfactual
must then admit a successor without any root credit-proof event.

The socket pack must cover the maximum enabled profile: one raw ICMP responder,
active and standby console acceptors, DHCP, two UDP self-test sockets, two TCP
self-test sockets, and the optional outbound probe. All application close
origins enter one `Draining`/`PeerCloseWait`/`Closing` state machine. Clean
`QUIT` must drain
`OK QUIT`, wait up to one second for the peer FIN, and close from `CloseWait`
before standby promotion. If the active socket remains `Established` at grace
expiry, the policy must force abort and promote that terminal result; it must
never initiate a local FIN from `Established`. Tests must cover this timeout
policy and actual smoltcp FIN/ACK ordering, as well as clean quit, peer EOF,
authentication failure/timeout, receive error, inactivity timeout, early
standby FIN/RST recycling, promotion only after the old socket and session
authority are clear, and pair abort on network-generation or stack reset. A
standby acceptor may be armed only after that transition, may retain at most one
unauthenticated peer for the 21-second virtual-counter handoff deadline, and
must never run the console parser or authentication server concurrently with
the active socket. Fresh Pi proof must include immediate sequential `.coh`
connections plus a probe-then-authenticated connection on both selected WiFi
and GENET; a delayed successful reconnect does not satisfy this gate.
`Display` may perform at most one retained HDMI attach or pending-frame turn.
Every retained phase must return before its successor, and CYW43 quantum
telemetry must remain zero on GENET.
During
bootstrap/recovery, only proved linked-runtime serial IRQ preservation, bounded
software-queue polling, and flushing plus already-buffered local-seat bytes are
permitted; generic/current-TCB UART
fallback, USB backend polling, HDMI frame submission, and network polling must
remain absent in that same turn. Root-owned echo state may be updated, then one
later isolated Display turn submits it. Accepting reboot must fence all later
physical and network
command intake and may discard only nonessential `BackgroundLine` records whose
authoritative copies already exist in `/log/queen.log`; command output and
protocol tails remain retained. Reset admission requires an empty pending-output
backlog, an idle physical-response barrier, and an exact linked-serial drain
outcome of `Complete`. That outcome must include an explicit runtime sample of
the UART transmitter-idle bit after the ACK bytes drain; FIFO acceptance alone
is insufficient. A busy sample completes only that immutable probe and retains
RX fairness before a fresh idle ticket. A `Pending` drain remains retained until
the three-second virtual-counter deadline; `Failed` poison or deadline expiry
must record a fail-closed reason in `/log/queen.log` and must not invoke reset.
Poison cleanup that empties a queue is not successful ACK delivery. The outer
turn that first proves wire idle must record it and return; a later outer turn
dispatches only platform reset, with no serial, driver, network, local-seat, or
display service. Reboot ACK and reset dispatch must complete before another
Wi-Fi operation is admitted.

Linked-serial adversarial coverage must exercise the production reciprocal
ring rather than only a fabricated completion: publish a staged TX action,
leave it pending for one outer turn, have the registered child controller
publish the delayed completion, and consume it on the next turn with the same
immutable command and ticket. It must also prove partial-prefix FIFO handling,
no replay after an impossible completion, a 128-byte TX action bound, RX service
between completed TX chunks, RX/TX ticket fencing, and fail-closed drain
classification after TX poisoning. EventPump saturation coverage must fill the
ordinary backlog, prove three records remain reserved for response tails,
reject rather than truncate an over-bound line, retain the current stream cursor
and pending `END`, retain the prompt as a `ResponseTailPrompt` backlog record,
and prevent a later command from overtaking the current response. Saturation and
reboot-preemption tests must prove a response-priority enqueue or accepted reboot
can drop only a `BackgroundLine`, never command output or a retained tail.
Reboot coverage must feed
`reboot` followed by another command, observe the complete linked-runtime ACK
before the one reset request, and prove the later command was never dispatched.
RX coverage must reserve the heapless SPSC sentinel slot and prove the child
never returns more bytes than the root-issued command grant.

Local-seat coverage must prove the retained USB attach and keyboard cursors
issue one immutable linked-runtime request or poll one exact completion per
outer turn, including one outer turn for each explicit `usb probe-kbd` attempt.
It must prove probe policy restoration, immutable command fingerprints, and a
complete compact probe result/continuation/contract/verdict/`OK` response below
the 2,048-byte serial bound. A live completed slice is required for
`probe_result=attached`; cached keyboard readiness with `Pending` must remain
`keyboard-unavailable continuation=pending`. Ordinary retained keyboard polls
must fail closed at their finite protocol-attempt bound, invalidate stale
command readiness, and record no-reply recovery rather than accumulating one
permanently active submitted/completed gap. Target-shaped coverage must distinguish a terminal
slice from a bounded command-owned continuation, advance that continuation by
one operation per later `LocalSeat` turn, and restore the prior polling policy.
The real linked-serial path must also prove that the passive compact `usb diag`
performs no USB poll, emits Gates 1 through 10, preserves `OK USB` and the
prompt within the three-record protocol-tail reserve, retires the physical
response fence, arms a passive post-command liveness baseline, and then accepts
fresh commands from both serial and buffered USB input. A later `usb status`
may report that sentinel as passed only from positive linked-runtime HID,
parser-accepted, parser-drained, and echoed deltas with zero new drops. Cached
Gate 10 and unchanged cumulative byte counters remain startup evidence only.
The normalizer must classify an active outstanding USB request with retained
no-progress resumes as `usb-retained-request-no-terminal`, including legacy
status lines that omit the explicit active fields. The default hardware helpers must not issue
`usb probe-kbd`; active probe coverage remains an explicit opt-in. Saturation
coverage must prove ordinary response-body lines cannot consume those tail
slots even while response ordering is active. USB status coverage must also
prove that `runtime_skipped` is input-first scheduling telemetry rather than a
post-first-byte fault.
Controller-init coverage must prove `USB_RESET_DONE` remains in the existing
bounded extended controller-reset timeout class through the following run-stage
setup, rather than reverting to the generic three-resume allowance. It must not
add a timeout class, retry, wake, owner, or unbounded lifetime. Command-readiness
coverage must retain the canonical `[local-seat] usb keyboard command-ready
action=enable-command-input ...` receipt exactly once in `queen.log`, keep its
verbose counter detail log-only, and prove EventPump projects the canonical
receipt exactly once on serial immediately before `[drivers] USB console ready`.
With no current accepted linked-runtime HID byte, the runtime and normalizer
must report `usb-physical-input-unproven`; Gate 10 or `first_byte=no` must never
seed first-byte evidence or a `usb-post-first-byte-*` blocker.
Display coverage must prove an attach miss is retained and that attach and frame
submission cannot share an outer turn. Passive status must distinguish queued
display bytes from a completed isolated-driver receipt and must not claim HDMI
ready while a turn is outstanding, no-reply debt is active, or the snapshot is
stale after retry exhaustion. SMP rate coverage must attribute keyboard polls,
bytes, drops, and no-replies only to the manifest-selected USB core and HDMI
bytes/mirror drops only to the manifest-selected HDMI core; the combined
local-seat snapshot must not duplicate both devices' rates on both cores. The current synchronous PCIe HAL
prerequisite runs before EventPump construction as local bookkeeping and
authority setup only; tests and target traces must show that a missing proof
leaves the retained USB cursor blocked rather than bypassing HAL or constructing
a root-owned steady USB backend.

Host tests must prove the fixed-layout pointer-free command/completion records remain primitive-only and bounded, including primitive aux fields for service-turn arguments, nonzero-progress/frame-ready-only hot-path credit, owner-state descriptor rejection when the matching runtime spec is not acceptance-eligible, owner-state acceptance requiring the explicit owner hot-path mask plus acceptance-eligible runtime images, the separate root-context diagnostic versus pointer-free selector registration classes, the common `DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE` bit forced onto transitional root-context ring commands, and the one-way command flag used by send-only bootstrap/background turns so isolated runtimes do not call `Reply` without a reply cap. Runtime-init records must carry primitive MMIO/DMA/shared physical page metadata, fixed virtual bases, semantic resource ranges for large apertures and large buffer arenas, bus-address policy, optional IRQ descriptors, optional bus-link descriptors, and framebuffer metadata without root pointers.

The physical Pi profile requires isolated child VSpaces for driver bootstrap and
loads isolated `pi4-driver-*` runtime image payloads only from the raw
driver-runtime CPIO embedded by `scripts/pi4-image-build.sh`. The generated
`sdio-host` contract has three MMIO pages, ten low DMA pages (mailbox,
control-block arena, and eight CMD53 bounce pages), and 32 shared pages. The
CYW43-SDIO bus link exposes exactly one 32-KiB shared backplane aperture.
The staged U-Boot CPIO remains audit/packaging evidence, never a physical
runtime fallback. The build strips the root-task ELF injected into the derived
seL4 archive and proves exact newc membership independently; the 4 MiB rootfs
guard remains on the QEMU/release system payload CPIO rather than that elfloader
archive. All seven generated runtime specs remain acceptance-eligible and keep
their 320-page code aperture. The current common linked-runtime ELF spans 257
pages, leaving 63 pages of bounded growth margin. Current Pi proof requires
`DRIVER_TASK_EARLY_MMIO_ADMISSION selection=Wifi pages=1 owner=hal-unmapped-child-cap status=ready`
before mailbox preseed and before the SDIO runtime entry/boot records; wired and
disabled selection requires the same record with `pages=0`. Packaging or early
admission alone is not owner-state proof. Host coverage must prove the complete
generated table and its exact budgets (`usb` 128 DMA/32 shared, `hdmi` 0 DMA/16
shared plus framebuffer, `genet` 64 DMA/32 shared, `cyw43` 0 DMA/64 shared,
`sdio` 3 MMIO/10 DMA/32 shared, `pcie` 16 shared, `serial` 4 shared), then
compile the separate runtime package for host and `aarch64-unknown-none`.

USB runtime coverage must prove that the one-deep first interrupt-IN transfer
arms a five-second virtual-counter liveness deadline only after its successful
doorbell and binds that deadline to the exact slot, endpoint, report slot, TRB,
and transfer generation. HID setup must encode a one-second idle interval in
four-millisecond units, strictly inside that deadline, so an unchanged keyboard
can complete the existing interrupt-IN request without a key transition.
Polling the same identity must not refresh the deadline. Before expiry the
result remains first-report pending; exact expiry
with no transfer event, valid report, preserved event, or pending doorbell must
fail the stalled attach closed with `FULL_QUEUE_NO_EVENT`, clear the endpoint
identity, and retain one terminal recovery failure. Coverage must separately
prove that any completed first transfer clears the watch and that a rearmed
one-deep successor remains healthy through post-first idle well beyond the
former deadline. The focused cases are
`usb_keyboard_first_transfer_deadline_reenumerates_same_one_deep_no_event_identity`
and `usb_keyboard_post_first_idle_successor_never_false_expires`; the former's
acceptance assertion is timeout-to-fail-closed behavior, not proof that a new
enumeration completed.

SDIO runtime tests must prove the sole owner seals its engine from normalized
host-block geometry: at most two blocks use retained PIO and more than two use
external DMA. Every issued request remains in one immutable cursor. Preserving
the existing f4 invariant, one preissue/issue owner quantum must batch the
finite Linux-ordered setup and exactly one issue within
`PREISSUE_STEP_BOUND=16` and the shared 256-operation contract. A failed
request-owned status-clear readback may yield with zero issues for a later
deadline-bounded verification; no other setup retry is admitted. Every later
external DMA continuation performs at most one retained snapshot, while every
fresh PIO-ready edge may move one complete normalized host block of at most 512
bytes and 128 FIFO accesses without crossing into a later block. Common
completion requires response/R5, exact payload movement, authoritative
`DATA_END`, and host quiescence. PIO tests must prove direction-correct
ready/present-state ownership, zero DMA accesses, block-granular progress, and
that every owner quantum remains within 256 modeled HAL operations.
External-DMA tests must prove control blocks split only at admitted
physical-page boundaries, COMMAND is followed by exactly one DMA activation,
and lone PIO-ready bits remain outside the request's W1C ownership. SDHCI IRQ158
and DMA channel-4 IRQ116 must be bound to the same SDIO physical owner. Tests
must present those IRQs in both orders, simultaneously, repeatedly, and with
either one withheld; no case may publish a terminal until response/R5, exact
payload movement, authoritative `DATA_END`, `CONBLK_AD == 0`, this request's
`CS.INT`, and host quiescence all join for the immutable request identity.
Exactly one terminal may then commit. Request-local `DMA_END` cannot substitute
for the DMA4 terminal, and an early DMA4 terminal cannot substitute for SDHCI
completion. Timeout or selected-engine failure must perform bounded containment
without engine switching, notification rescue, or post-issue replay; malformed
external-DMA resources must fail before command issue without falling back to
PIO or root-owned service.
The idle-owner adversary must publish a fresh sequence-last one-way command
after the earlier empty intake sample and before receive. SDIO's final stable
command-ring re-read must re-enter arbitration with zero physical I/O and no
second notification; unchanged or call-shaped intake may still block.
Firmware tests must prove one retained 32-KiB aperture drains as Linux
MMC-shaped `511 * 64` external-DMA plus `1 * 64` retained-PIO CMD53 children,
while the true final aperture uses the maximum full-block request plus one
bounded four-byte-padded PIO byte tail. The full-aperture proof must begin with
the production `FIRMWARE_CHUNK` parent, publish and consume the sequence-last
reciprocal ring plus acknowledged grants, and drive the real retained
controller; manually fabricated child descriptors or completions do not count.
Function 2 TX coverage must prove IORx readiness remains an exact
release/recovery boundary, a current cached backplane window emits only one F2
CMD53 child, and a mismatched cache emits exact LOW/MID/HIGH/F2 children on
separate outer turns without IORx. Timeout never toggles IOEx, changes the
sealed engine, or selects another lane in place. Control
exchange coverage must retain Linux's separate absolute 2.5-second TX and
reply windows. It must hold an exact Function 2 child through terminal
completion without abandoning or reissuing it, apply that completion before
the timeout decision, and prove the original deadline is unchanged. A child
that returns the typed pre-TX DPC defer is proven not issued and can never be
retried or replayed; canonical DPC consumes its exact event, then the unchanged
parent may publish one distinct fresh Function 2 child with one total physical
issue. A child
that finishes after the TX deadline must produce a post-TX fault and ordered
pair fence, never replayable `NOT_READY`; the later reply window starts only
after an on-time exact TX completion and is not inflated by any child wait.
The focused adversarial cases include
`cyw43_sdio_requests_use_linux_watchdog_and_derived_child_bound`,
`sdio_owner_clamps_drifted_descriptor_to_linux_request_watchdog`,
`sdio_linux_inhibit_fence_does_not_consume_issued_request_watchdog`,
`sdio_runtime_rejects_all_legacy_nondescriptor_commands`,
`sdio_retired_generation_operations_typed_reject_before_mmio`,
`sdio_poison_rejects_ordinary_descriptors_until_canonical_pair_scrub`,
`canonical_pair_entry_scrub_clears_owner_poison_without_starting_another_lifetime`,
`control_pre_tx_reuses_only_a_quiescent_generation_long_dpc_lifetime`,
`control_pre_tx_binds_one_event_then_advances_past_reassertion`,
`control_pre_tx_missing_bound_event_faults_without_reactivation`,
`persistent_control_reuses_healthy_dpc_lifetime_without_a_child`,
`persistent_control_marked_lifecycle_reaches_reply_without_grant_or_hint_history`,
`cyw43_parent_admission_requires_exact_source_for_steady_ack_pending`,
`critical_eapol_f2_terminal_survives_concurrent_dpc_ack_pending`,
`steady_parent_ack_before_event_retains_op7_to_terminal_without_root_grant`,
`malformed_tagged_steady_parent_and_child_never_enter_root_grant_lane`,
`dpc_durable_event_and_cursor_ignore_deferred_hint_history`,
`dpc_idle_prewait_reenters_for_source_committed_before_cursor_creation`,
`dpc_exact_child_waits_for_terminal_without_poll_or_grant_hint`,
`idle_prewait_reenters_only_for_a_fresh_one_way_sdio_child`,
`production_masked_control_uses_exact_owner_activation_before_tx`,
`production_join_final_fence_runs_canonical_dpc_then_issues_exactly_once`,
`sdio_physical_lifetime_completes_once_for_one_low_high_cycle`,
`sdio_pair_restart_immediately_fails_the_active_power_lifetime`,
`sdio_pair_restart_uses_the_durable_active_epoch_when_the_cursor_is_lost`,
`sdio_physical_lifetime_failure_survives_runtime_restart_and_later_success`,
`ring_scrub_preserves_only_the_sdio_physical_lifetime_record`,
`runtime_engine_init_command_has_no_cyw43_root_init_lane`,
`genet_engine_init_uses_only_the_genet_net_init_command`,
`gate8_and_gate10_reject_missing_active_failed_or_replaced_physical_lifetimes`,
`cyw43_service_snapshot_rejects_invalid_physical_lifetime_metadata`,
`wifi_gate_one_requires_one_completed_supervisor_bound_physical_lifetime`,
`linked_cyw43_physical_lifetime_change_invalidates_resume_and_operator_fence`,
`cyw43_gate8_ready_publication_is_a_separate_retractable_consumer_fence`,
`cyw43_idle_receive_lifetime_fails_closed_on_pair_or_physical_change`,
`cyw43_committed_queue_level_schedules_one_batch_parent_while_idle_stays_quiet`,
`cyw43_data_tx_terminal_admits_successor_without_inbound_credit_ack`,
`cyw43_rx_hint_carries_no_authority_history_or_work_count`,
`cyw43_rx_condition_before_sleep_needs_no_second_hint`,
`linked_cyw43_persistent_usb_service_debt_gets_one_operator_rotation`,
`linked_cyw43_rx_admission_honors_input_and_clears_on_guards`,
`cyw43_durable_work_requires_one_completed_non_recovery_physical_lifetime`,
`linked_cyw43_epoch_zero_work_never_admits_an_ordinary_network_turn`,
`cyw43_lifetime_fence_covers_direct_runtime_entries_without_affecting_genet`,
`pi4_wifi_supervisor_defers_explicit_wifi_with_or_without_local_seat`,
`pi4_wifi_supervisor_defers_auto_wifi_only_with_credentials`,
`pi4_wifi_supervisor_does_not_defer_wired_net_console`,
`pi4_wifi_supervisor_keeps_wired_immediate_with_stale_wifi_credentials`,
`sdio_fault_telemetry_v3_distinguishes_dma_terminal_states`,
`firmware_prep_live_contract_crosses_real_controller_once_per_outer_turn`,
`firmware_prep_live_contract_failure_cuts_are_terminal_and_never_replayed`,
`sdio_init_rejects_dma_alias_and_high_memory_before_card_service`,
`sdio_external_dma_join_waits_for_later_dma_before_read_publication`,
`sdio_external_dma_join_waits_for_later_sdhci_before_read_publication`,
`sdio_external_dma_join_accepts_irq158_irq116_in_either_order`,
`sdio_external_dma_join_coalesces_repeated_irqs_to_one_terminal`,
`sdio_external_dma_join_rejects_missing_dma_completion`,
`sdio_dma_abort_failure_still_attempts_sdhci_reset`,
`sdio_data_requests_without_dma_authority_fail_before_command_for_both_directions`,
`sdio_dma_error_telemetry_is_immutable_across_containment_reset`,
`sdio_stale_dma_generation_is_contained_before_fixed_memory_reuse`,
`sdio_descriptor_rejects_unrepresentable_cmd53_byte_mode_before_issue`,
`sdio_wifi_power_sequence_advances_one_bounded_action_per_turn`,
`sdio_engine_init_turn_withholds_completion_until_pwrseq_terminal`,
`sdio_retained_external_dma_paces_32_blocks_across_finite_fifo_turns`,
`sdio_retained_bootstrap_cmd52_and_card_commands_issue_once_without_private_polls`,
`sdio_preissue_status_clear_requires_verified_readback_before_one_issue`,
`sdio_retained_host_config_runs_recovery_and_set_ios_across_outer_turns`,
`sdio_retained_dpc_activation_completes_one_bounded_owner_quantum`,
`sdio_real_irq_before_request_deadline_is_consumed_by_one_owner_quantum`,
`sdio_retained_dpc_condition_coalesces_event_present_at_owner_entry`,
`sdio_retained_card_irq_publishes_before_retrying_only_the_exact_ack`,
`sdio_retained_card_irq_ack_exhaustion_fails_closed_without_republication`,
`sdio_older_ack_completion_cannot_clear_a_newer_irq_epoch`,
`sdio_pending_irq_ack_retry_is_not_acked_twice_before_rearm`,
`sdio_retained_dpc_condition_coalesces_existing_events_without_second_ack`,
`sdio_retained_dpc_ack_epoch_is_consumed_before_no_source_rearm`,
`sdio_retained_dpc_bad_ring_acknowledges_exact_epoch_before_failing_closed`,
`firmware_parent_reciprocal_ring_drives_retained_sdio_owner_as_511_plus_one`,
`cyw43_linked_f2_tx_uses_cached_window_without_per_packet_iorx`,
`cyw43_linked_control_and_eapol_tx_use_one_cached_window_f2_issue`,
`control_and_eapol_tx_cross_reciprocal_ring_and_retained_sdio_owner`,
`control_tx_cold_window_crosses_exact_three_writes_then_f2`,
`release_post_f2_crosses_exact_linux_order_to_real_dpc_activation`,
`production_dpc_event_drains_real_owner_rx_before_foreground_poll`,
`production_control_and_rx_polls_consume_only_dpc_owned_queue`,
`dpc_cursor_serializes_physical_commands_but_allows_exact_queue_delivery`,
`active_dpc_child_allows_only_intake_sealed_current_queue_delivery`,
`cyw43_rx_idle_trace_v11_layout_is_additive`,
`cyw43_rx_idle_trace_v11_appends_dpc_causes_without_v10_prefix_drift`,
`cyw43_dpc_cause_and_frame_turn_accounting_is_episode_exact`,
`firmware_terminal_and_issued_unknown_cuts_never_reissue_a_child`,
`stale_foreground_completion_cannot_mutate_replacement_generation`,
`mutated_action_fingerprint_poisoning_never_replays_issued_child`,
`production_committed_queue_state_reaps_to_one_retained_batch_parent_terminal`,
`consumed_foreground_grant_rebases_exact_child_inactivity_fence`,
`consumed_dpc_grant_rebases_exact_child_inactivity_fence`,
`issued_unknown_timeout_retains_one_child_without_same_generation_replay`,
`dpc_pair_restart_arbitration_publishes_only_released_child_terminal_cause`,
`corrupted_continuation_fingerprint_fences_real_owner_without_second_quantum`,
and `cyw43_foreground_baseline_requires_release_published_snapshot`.
The eight generation-bus cases collectively prove healthy zero-child activation
reuse, exact sequence-zero-capable one-event binding, lost-token quarantine
without reactivation, hintless retained-parent progress, the final one-way
command-ring pre-wait recheck, typed mask-skew repair, and one physical
Function-2 issue after canonical DPC.

QEMU packaging must pass `scripts/cohesix-build-run.sh --no-run --cargo-target aarch64-unknown-none` and the 4 MiB rootfs guard with no `cohesix/bin/root-task` entry. The build embeds a boot-minimized rootserver in the staged elfloader and retains the unchanged target ELF as `out/cohesix/staging/rootserver` for diagnostics and external QEMU loading; these boot artifacts are outside the payload CPIO. The CPIO inventory manifest must match its component paths, and all seven `cohesix/bin/pi4-driver-*` payloads must be byte-identical to their target artifacts. Removing or stripping a runtime image, forging the manifest, or bypassing `scripts/ci/size_guard.sh` fails this gate.
The AArch64 runtime ELF audit must additionally show the full fixed CYW43
foreground transaction (1,024 action entries plus 128 KiB replay payload) in an
`SHT_NOBITS` section covered by writable `PT_LOAD` memory. The full baseline
slot must be explicitly invalid `MaybeUninit` loader-zeroed storage, become
readable only after exact parent admission release-publishes validity, and
restore the live state byte-for-byte on every continuation; it must not create
a second file-backed state image. The loader-zero contract,
`p_filesz <= p_memsz`, generated 320-page aperture, exact packaged runtime
bytes, and 4 MiB rootfs guard must all pass together; reducing trace capacity,
aliasing runtime artifacts, or post-link stripping is not an acceptable size
fix.

Milestone 26c Pi runtime/DMA proof states are machine-checkable and must not be inferred from adjacent evidence. `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml --venv .venv` consumes the immutable `seL4/build_UBOOT` artifacts, retains the exact selected Pi resolved manifest as `out/pi4-sd/cohesix-root-task-resolved.json` before cleanup restores canonical QEMU generated outputs, and writes `out/pi4-sd/pi4-runtime-dma-proof.env` with `PI4_RUNTIME_DMA_PROOF=target-build`, `PI4_RUNTIME_DMA_PROFILE=bounded-no-iommu`, the retained manifest path and hash, runtime CPIO hash, runtime uImage hash, staged image hash, and the hash of `pi4-image-identity.json`; the retained manifest hash must still match after canonical cleanup. The exact pre-root-task Pi driver-runtime CPIO is bound by `cohesix-pi4-sel4-image-provenance/v4` and copied byte-for-byte into the SD stage. A `--skip-build` pass must validate that archive and must not repackage whichever Pi- or QEMU-feature child most recently occupied the shared Cargo target directory. This proves repository artifact identity, packaging, and exact legacy-image identity only. Under Milestone 26e, the controlled Pi refresh validates independently as a repo-managed `pi4_diagnostic` seL4 16.0.0 `bcm2711` SMP+MCS artifact set with its completed build-input stamp, `KernelRootCNodeSizeBits=16`, `KernelArmExportVCNTUser=ON`, physical counter/timer-control exports off, `TIMER_CLOCK_HZ=54000000`, and no retained one-domain `KernelDomainSchedule` cache entry. The 16-bit root CNode admits the complete compiler-bounded 256-Worker population, linked-runtime images, isolated framebuffer mapping, and post-construction reserve; a 13- or 14-bit external Pi tree is stale and cannot satisfy image or hardware proof. The static `seL4/build_UBOOT` PASS proves only the canonical diagnostic artifact contract and cannot substitute for release proof, staged/read-back image identity, boot, Wi-Fi, TCP/`cohsh`, or benchmark lanes. The image wrapper must validate one complete relink tool family against the tracked baseline oracle and must never invoke CMake or Ninja in the immutable tree. `scripts/pi4_trace_normalize.py --gate-summary` emits `DRIVER_TASK_DMA_PROOFS`, `DRIVER_TASK_DMA_BLOCKER`, `DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF`, `DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_BLOCKER`, and `PI4_RUNTIME_DMA_PROOF=absent`, `diagnostic`, `qemu-or-stale-log`, or `fresh-pi` from serial evidence. `scripts/pi4_gate_proof.sh --require-driver-task-proof --runtime-dma-proof-out out/test-plan/<run-id>/pi4-runtime-dma-proof.env` writes the live proof bundle only after normalization passes. Only `fresh-pi` counts as live hardware runtime/DMA proof, and it requires driver-task dedicated readiness, cap/fault/revoke/scheduling/affinity proof, isolated VSpace, pointer-free IPC, per-hot-path `DRIVER_TASK_OWNER_STATE ... descriptor=present root_pointer=no`, sealed descriptor version/hash/identity proof for every active hot path, sealed bus-link proof for USB and CYW43 split clients, per-hot-path `DRIVER_TASK_DMA_PROOF` with bounded no-IOMMU profile and cache/bus-address policy, aggregate `DRIVER_TASK_DMA_BLOCKER=none`, no compatibility service roles, no unresolved ring timeouts/deferred bootstrap, no resource blockers, a fresh Pi cold-boot marker, and a live prompt. Raw `DRIVER_TASK_RING_CALL_TIMEOUT` events remain diagnostic, but `DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT` must be `0` after later return proof closes any bounded keep-active turn. It also emits `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified` only when `TIMER_BACKEND=arch-counter`, `TIMER_CLOCK_HZ=54000000`, `TIMER_EL0_COUNTER=vct`, `DUMMY_TIMER_SEEN=no`, every observed `DRIVER_TASK_COUNTER` line is valid, and the latest activity-bearing snapshot exists for every selected network owner. A selected CYW43 path therefore requires both `contract=cyw43455 hot_path=cyw43-wifi` and `contract=sdio-host hot_path=sdio-host`; repeated cumulative snapshots cannot substitute another driver's activity or be added together.

The isolated runtime engines contain production service turns for serial
mini-UART init/RX/TX, HDMI framebuffer rendering, PCIe MMIO turns,
direct-root-port xHCI boot-keyboard polling, GENET MDIO/MAC/RX/TX rings, CYW43
shared-control SDPCM command records, and SDIO fixed-layout
CMD52/CMD53/POLL_IRQ service turns. Physical Pi root starts each isolated
runtime with `TCB.WriteRegisters(resume=1)` at a shell-safe bootstrap priority
while preserving the contract MCP; roles outside the deferred Wi-Fi pair keep
their existing profile-specific priority transition. Deferred CYW43 and SDIO
remain at bootstrap priority `255` through prompt publication, owner-first
descriptor and engine replay, firmware/control-context replay, and
control-plane readiness; the supervisor proves SDIO first, then CYW43, and only
then lowers SDIO and CYW43 in two separate outer turns.

A linked serial runtime cuts over after its descriptor/init/owner-state path
plus a valid service completion; RX-byte proof remains separate. Root may emit
`DRIVER_TASK_BOOTSTRAP_DEFERRED ... reason=root-shell-before-first-service-proof`
so an unproved child cannot starve the root shell. Captures may show SDIO host,
PCIe root, GENET, and CYW43 records with that exact marker before the prompt;
those markers are acceptable only as shell-preserving fail-closed evidence, and
the retained descriptor must later replay with
`DRIVER_TASK_RUNTIME_INIT_DEFERRED ... status=resumed owner=linked-runtime proof_effect=deferred-proof-retry-enabled`
before matching call/return service proof can close SDIO, PCIe, or network
acceptance.

Wi-Fi descriptor replay is prompt-safe: when physical Pi pointer-free IPC proof
and linked serial service proof are present, root emits
`[net-console] deferred resume scheduled reason=driver-startup-before-root-prompt action=publish-prompt-then-supervise`,
publishes the serial `cohesix>` prompt, and then starts the persistent
SDIO/CYW43 supervisor. HDMI reports startup and concise Wi-Fi milestones during
this episode. Atomic 8a-through-8h plus adjacent
`CYW43_GATE8_COMMIT` is the nonterminal driver frontier that opens DHCP.
Supervisor `ready` is the later actual ready-to-use cut and requires
current-generation DHCP bound with a nonempty address plus the bound/admitted
TCP console listener. It intentionally does not wait for an accepted or
authenticated TCP client. USB/local-seat readiness remains an independent
hardware acceptance gate rather than a prerequisite for Wi-Fi supervisor
Ready.

Bootstrap has exactly one outer episode, always `attempt=1`. Its raw lifecycle
is `status=begin|recovery|stabilizing|ready|failed|permanent`;
`attempt=0 status=preflight` remains the linked-serial admission state. A failed
or permanent episode retains the already-published diagnostic HDMI root prompt
and may release further diagnostic lines, but must never render the Wi-Fi
`Ready to use` banner. The episode admits no
automatic whole-bootstrap backoff, reset, second `begin`, or attempt 2. Once
both linked-runtime restart contexts exist, the initial physical pair remains
the only pre-service lifetime. A typed runtime/SDIO or issued-unknown physical
fault drains and fences its exact owner but cannot emit pair 2. Gate-local
association, DHCP, and protocol retries remain independently bounded. Gate 8
itself never requests pair repair; logical failure or committed service that
still lacks DHCP/listener readiness waits to the original absolute deadline,
then
emits `CYW43_GATE8_TERMINAL ... action=quarantine` and terminal
`status=permanent`. A terminal failure quarantines network service and returns
to the ordinary EventPump so
diagnostics, authentication, reboot, serial, local-seat, and HDMI remain
responsive while Wi-Fi stays acceptance-red. Only exact service readiness
authorizes one later independent steady-state runtime-recovery episode with one
consumed-once pair repair. Duplicate Ready cannot replenish it; restored
new-generation service emits `CYW43_RUNTIME_RECOVERY status=ready` before one
later episode can be re-armed. That lifecycle cannot emit or reset a boot
attempt, and Gate 10 remains downstream acceptance rather than recovery
authority.
Fallible supervisor construction or immutable configuration/artifact validation
may emit `attempt=1 status=permanent` before `begin`; it is the sole terminal
boot result and returns to the same bounded diagnostic operator mode.

A no-reply runtime must emit `DRIVER_TASK_RING_CALL_TIMEOUT` and
`DRIVER_TASK_RUNTIME_INIT_DEFERRED ... status=pending owner=linked-runtime action=serial-shell proof_effect=acceptance-red-until-replayed`;
retained gate-local progress may continue only on later ordinary EventPump
turns within the existing operation and deadline. It cannot start another outer
episode. If pointer-free proof is absent, root must emit
`[net-console] deferred resume skipped reason=driver-task-net-runtime-unproved action=serial-diagnostics-only`,
preserve serial diagnostics, and leave Wi-Fi acceptance red until reboot starts
a new boot.

Physical Pi local-seat captures must show HDMI as an independent display sink
without making it a pre-prompt serial dependency: the framebuffer hint must be
available before driver-task bootstrap, and after EventPump construction each
`hdmi-text` descriptor/attach step and pending-frame service must consume one
retained `Display` outer turn. Attach and frame submission cannot share a turn,
and a successful attach schedules its canonical viewport snapshot only once.
High-impact supervisor records may retain an HDMI mirror only after the Wi-Fi
HAL guard is released; the mirror must wait for a later `Display` turn and
restore any open prompt plus typed row with a bounded carriage-return/erase-line
update rather than closing it or restarting a full viewport redraw. A no-reply
HDMI render path is display-red evidence and must emit
`DRIVER_TASK_RING_CALL_TIMEOUT`, but it must not prevent `cohesix>` from
becoming responsive on UART. No HAL-mapped framebuffer diagnostic mirror is
allowed; HDMI output must come from the isolated `hdmi-text` runtime only.

Before linked-serial cutover, raw UART remains the boot log; after cutover, the
serial child is the sole physical UART owner, root diagnostics are authoritative
in `/log/queen.log`, and operator output may reach UART only through the linked
reciprocal ring. Before EventPump construction, the current synchronous PCIe
HAL prerequisite runs only as local bookkeeping and authority setup for the USB
cursor. It cannot construct a root-owned steady USB backend or authorize
combined later work, and missing proof leaves USB attach blocked at the PCIe
prerequisite. After the serial `cohesix>` prompt, local-seat must defer if the
USB runtime already has an active command, emit one
`prompt-settle attach deferred reason=usb-runtime-active` summary, and retry
after the normal quiet window; PCIe descriptor replay, USB init, enumeration,
and keyboard report service then advance as retained one-action `LocalSeat`
outer turns.

While a Wi-Fi HAL scope is held, local-seat service is buffered-only: it must
not poll USB, submit HDMI, or start network work. It may update the bounded
root-owned echo/scrollback state for already-buffered bytes; the resulting
frame must be submitted only by one later retained Display turn. Tests must
prove a printable byte remains visible during Wi-Fi startup or failure without
composing USB, HDMI, and CYW43 hardware operations. HID discovery must then
advance only through the explicit keyboard-enumeration aux while ordinary
background polls report the current frontier without re-entering enumeration.
No-reply background USB polls must produce `DRIVER_TASK_RING_CALL_TIMEOUT` plus
`[local-seat] isolated USB runtime keyboard poll suspended contract=usb-local-seat source=linked-runtime reason=driver-task-no-reply action=serial-shell`
rather than repeated blocking `usb-local-seat` calls. `usb probe-kbd` must
retain its bounded keyboard-enumeration cursor and must not replay the whole
isolated local-seat attach/init chain; each attempt consumes one later
`LocalSeat` outer turn and is permitted only while the child USB enumeration
marker advances, stopping at the finite cap, keyboard readiness, or no new
marker.

Root-console startup must emit UART-visible `[mark] root-console.start.begin`
and publish the ready line plus command list after bounded non-Wi-Fi driver
startup settles or fails closed. On the isolated QEMU VirtIO path, source emits
that ready line and command list directly under the bootstrap SC, then queues
`[mark] root-console.start.ok`; the nonempty FIFO retains the initial
`cohesix>` prompt behind the marker.
Neither record drains until a steady Operator after the one-time activation
yield, so absence of the marker on the wire is not evidence that source has not
crossed the console lifecycle boundary. Physical Pi retains its existing
direct/linked startup route. Host-EAPOL, association, DHCP, and retained
gate-local progress cannot hold the serial shell hostage. Once `cohesix>` is
published and USB polling is armed, serial UART and USB keyboard input must both
feed the shared parser concurrently after USB proof succeeds; during a Wi-Fi
HAL turn local-seat dispatch remains buffered and command-fenced even though
HDMI has not yet claimed interactive readiness.

Steady physical Pi root submits serial/network service turns through bounded
ring calls; HDMI submits are limited to high-impact progress lines, while init,
deferred-resume, timeout, and proof turns retain bounded diagnostics and may
enqueue linked-UART breadcrumbs. USB keyboard auto-poll uses bounded
nonblocking sends until the runtime proves it can reply without risking serial.
The isolated runtime `_start` entry must preserve root's task key, install the
mapped driver-local IPC buffer before receiving commands, skip `Reply` for
commands marked with the one-way flag, and emit replies only for call-delivered
commands. Hardware captures should show `DRIVER_TASK_RING_CALL_BEGIN` and the
matching `DRIVER_TASK_RING_CALL_RETURN` for init/deferred-resume/proof turns;
routine steady console data turns may be suppressed to keep interactive serial
latency bounded. Any `DRIVER_TASK_RING_CALL_TIMEOUT` or positive
`DRIVER_TASK_BOOTSTRAP_DEFERRED` keeps driver-task acceptance red until later
service proof closes it.

A role boolean is credited only from a line proving both `live_tcb=yes` and
`hot_path=dedicated`; static contract isolation, callback-pointer live-TCB
service turns, shared-root ring service turns, runtime-image declarations,
runtime-region mapping, runtime-image smoke loops, runtime-init descriptor
commands, and any ring command marked root-context or init-descriptor
non-acceptance are diagnostic until the driver state boundary is owned by an
isolated ring-backed task, VSpace proof is `yes`, pointer-free IPC is `yes`, and
`owner_state=driver-owned` is present. Pre-root bootstrap turns, including the
serial bootstrap reply proof, must not sample timer registers. Later ring
latency telemetry may sample the EL0 virtual counter only when the profile
enables `timers-arch-counter`; dummy-timer Pi captures must suppress latency
proof rather than reading CNT registers.

The HDMI supervisor sequence has a fixed episode-sized ordinary FIFO plus one
bounded terminal reserve, so a delayed display cannot lose the only readiness
release.
The raw record uses `local_seat=enabled|disabled` for manifest configuration;
USB command readiness remains a separate proof. A successful HDMI attach must
schedule its one canonical snapshot before queued incremental startup text can
drain. Pre-terminal USB input may update the parser preview but cannot open an
unprompted HDMI row. A row-preserving driver update must clear the restored row
to its end, and a closed command must retain its newline before response text.

For supervisor lifecycle evidence, `telemetry_sinks=serial+qlog+hdmi` names the
configured routing targets, not delivery proof. Serial and qlog preserve the
raw `CYW43_BOOTSTRAP_SUPERVISOR` schema; when HDMI is present and its bounded
FIFO or bounded terminal reserve admits the transition, it preserves only the
concise typed `[drivers] WiFi ...` rendering on a later `Display` turn.
During an active Wi-Fi attempt, the first physical display frontier must be
exactly `[drivers] WiFi starting one CYW43/SDIO physical lifetime` and must not
remain unchanged for more than five virtual-time seconds: its bounded heartbeat
adds `(still working)`. Tests must reject the retired baseline-normalization
wording because it concealed the distinction between physical-lifetime
construction and later pair/control progress. Fill the routine serial bootstrap
backlog in the focused EventPump test and prove that it cannot starve this
display lane. An authenticated serial response tail may preempt display only
until its bounded ACK/ERR/END plus prompt sequence completes.

CYW43/SDIO runtime foreground coverage must prove the correct authority path,
not only software state transitions. Non-CYW43 root-to-runtime commands use an
immutable one-way endpoint `NBSend` carrying the retained sequence; the child
admits one foreground quantum only if that send rendezvouses while it is
waiting and the complete no-reply ring record still matches the retained
intake. Tests must prove that a dropped send is not queued, multiple sends
cannot accumulate authority, and repeated doorbells never republish, mutate,
or replay the command.

Every root-to-CYW43 generation, including zero, retains one selected authority
contract. Non-op11 retained commands outside finite op7, including non-op11
bootstrap and recovery commands, keep their stable acknowledged exact shared-grant cadence.
EAPOL-Start and ordinary control must remain on that cadence. A separate
admission table must accept only completely intake-sealed current-generation
EAPOL-Key M2, M4, and group-key responses for the existing paired finite-op7
contract; malformed, truncated, ordinary Ethernet, EAPOL-Start, wrong-message,
untagged, stale-generation, and mismatched descriptor/payload cases must fail
before issue. Each accepted parent must bind both CYW43 and SDIO request
priority, enforce exactly one frame and the four-operation/1,536-byte budget,
commit and notify once, publish no grant or later notification after issue, and
keep root blocked until the durable matching Function-2 terminal is visible.
The data/EAPOL child must not inherit persistent control's pre-TX fence. A
concurrent source-bearing event remains independently durable; an
`ACK_PENDING`-before-front publication window must retain the same parent
without admitting DPC issue or any ordinary root grant, and the later
sequence-last front event must become serviceable without another authority
edge.
The finite-EAPOL counterfactual must drive the real PIO SDIO owner with a
production-reachable masked `CARD_INT` plus exact IRQ158 ACK debt. With the
finite steady child unfenced it issues exactly once, reaches
`COMPLETION_PROGRESS`, clears only the exact ACK debt, and leaves the committed
DPC event durable. Temporarily restoring the persistent-control pre-TX fence
must instead produce the typed pre-issue fault with zero command issues. A
second final-prewait counterfactual must commit the exact child terminal, begin
a later masked producer window before the consumer sleeps, and prove the
retained parent still yields for that terminal; an admitted-only parent
predicate must fail that test. Root counterfactuals must likewise prove that
fresh TX cannot outrun copied or durable runtime RX, an already-active exact TX
remains non-preemptible, a full aggregate restores paired capacity by promoting
one eligible head without manufacturing op8 credit discovery, and the HAL
front-event snapshot exposes only the sequence-last committed event.

The episode ledger introduced by driver-runtime ABI v7 and retained unchanged
by ABI v8 must prove the exact 49,344 offset, 128-byte/cache-line-isolated size,
final-word publication commit,
staged/torn/wrong-version/wrong-sequence rejection, body clean/barrier before
commit, and zero notification-count change. Foreground and DPC accumulators
must coexist without identity corruption; foreground clear,
`Cyw43RuntimeState` restoration, and the 4-KiB pair-ring scrub must preserve the
last publication. Production-path tests must cover queue-only op8 without a
child, active and terminal PIO/DMA child contracts through the real owner path,
DPC RX-queue wait and same-episode resume, exact committed-batch parent
attribution, typed quarantine/fault exits, and a late terminal or changed
durable condition suppressing a false block. The prewait checkpoint must occur
before the final durable-condition and exact-child rechecks; an interlock that
changes either condition across passive publication must re-enter the owner.
Authoritative completion signalling must precede diagnostic work, and the first
typed lane fault plus its child must dominate either later peer exit order. Root
must double-read the record, reject torn state, and format one bounded
`CYW43_BUS_EPISODE` line that fits `DEFAULT_LINE_CAPACITY` at maximum field
values and survives the linked-runtime serial transport without truncation.
The trace normalizer must accept only the anchored compact grammar and expose it
as passive diagnostic structure; the record must not change any Pi gate or DPC
acceptance result. The trace normalizer and Pi gate fixtures must require
descriptor version 8; descriptor version 7 and older remain parseable
historical text but cannot satisfy current sealed-owner acceptance.
Expiry may select exact recovery only and must not create a source probe, poll,
replay, or fallback. Ordinary post-Gate-8 TCP
must retain its O(1) finite-op7 classification and service path without running
the EAPOL-Key parser.
Every exact op11 instead uses one HAL-derived persistent marker regardless of
logical generation or lifecycle. ABI coverage must prove the receipt's exact
offset 40, exact 24-byte size, distinct magic, logical generation zero, body
clean/barrier and commit-last publication, producer-monotonic nonzero epoch and
overflow failure, stable root double-read, and rejection of torn, stale,
wrong-request, wrong-fingerprint, wrong-generation, wrong-epoch, and
wrong-commit state. Focused EventPump/root-admission models must cover both an already-open pair lease
and an initially inactive outer lease without calling a test pre-open helper.
Without a test pre-open helper, production EventPump must observe the exact
association claim, open the outer pair lease, and only then let NetStack
allocate and issue Join. Assert one lease open, one amortized request, zero
request-bound boost/grant/resignal actions, sequence-last `CommitRing`,
`Issued`, and exactly one signal-last notification. The already-open case must
reuse its pair reservations with no duplicate boost. Both then retain `Open`
across the exact persistent wait receipt without polling, and resume the same
parent for terminal consumption and deterministic lease close/restore with no
`PublishGrant`, grant 19, replacement grant, later `NotifyRing`, same-call
`PollRing`, second notification, or unrelated owner. An intake-sealed
current-generation EAPOL-Key M2, M4, or group-key finite-op7 request-bound
terminal must remain canonically exact through its bounded CYW43/SDIO restore
turns. The physical inactive-outer negative path must also drive the real typed
Join entry and prove zero active-slot, request-sequence, priority-mask, grant,
doorbell, signal, and ring mutation. Generic cold/replay op11 with both peers
in `Bootstrap` must retain mask zero, while the intake-sealed finite op7 class
must still acquire its exact request-bound CYW43-plus-SDIO mask. A valid
unacknowledged
sideband batch plus stable exact terminal must be rejected in each restore
phase without incrementing `send_attempts`. Tests must cover changed-condition clearing,
terminal-before-receipt, terminal-after-receipt, publish/clear failure,
clear-before-wake interpretation, ordinary receipt park, later exact-terminal
resume, semantic consumption, and deterministic lease release. The focused
EventPump/root-admission model must preserve one externally uninterrupted same-parent
bus-service episode across HAL admission, CYW43 protocol ownership, and SDIO
physical ownership. Open, bounded Closing, and exact handoff-boundary phases may
schedule required Serial/LocalSeat/Dispatch fairness checkpoints, but must admit
no idle gap, unrelated policy/NIC owner, second request, signal, grant, or
issue. Receipt and terminal are alternative handoff proofs, never simultaneous
authority. The request must remain `Issued` through incomplete completion
reads. GENET cannot acquire or inspect this state,
the finite op7 steady lease retains its separate identity, admission, and
budget while using the common condition-driven continuation route, and typed
cold provenance cannot authorize another publication lane. Tests must reject caller-supplied or partial
persistent flags, endpoint input, stale sequence, changed action/generation/body,
and replay of an issued parent.

The focused `bootstrap_op11_keeps_physical_identity_during_owned_context_replay`
case must additionally hold an exact issued bootstrap op11 while context replay
is owned and the steady Network lease is intentionally inactive. It must prove
that the outer policy classifier is inapplicable, the physical parent remains
`Waiting`, no pair restart is requested, and finite-op7 diagnostics do not
mislabel the persistent op11.

Focused Pi trace-normalizer coverage must reject incomplete or internally
contradictory SMP driver aggregates before they supersede lower-tier role
evidence, and recorder-scope coverage must exclude live-Net-superseded and
stale-generation recovery before labeling current gate rows.

A separate qualifying composition must start at cold bootstrap and use the
production EventPump, HAL admission, root client, CYW43 runtime, and SDIO runtime
dispatch. It may script AP/card responses only after the production path issues
the physical request, then must continue through the same scheduler across the
prior `q=0x408` DPC/RX frontier, Gates 1 through 10, and the raw-TCP software
consumer. Manual ring publication, owner advancement, wait-receipt publication,
synthetic terminal injection, or a completed-lifetime override remains focused
seam coverage and cannot qualify a hardware candidate.

For delegated CYW43-to-SDIO commands, tests must prove there is no usable
endpoint after handoff. Non-op11 delegated foreground work keeps its exact
acknowledged shared-grant coverage. Every child derived from the persistent
op11 instead requires the paired descriptor marker and advances from its
sequence-last committed command plus durable owner state, with zero delegated
grants. A completion miss retains the exact child frontier; no explicit `Grant`
phase or re-signal may appear. Every post-release DPC event carries the finite
steady-service marker bound to its exact event sequence and physical
generation, including before Gate 8, with zero recurrent grants. The owner uses
bounded helpers, issues each immutable hardware request at most once, and
continues whenever the current durable condition is locally runnable while
preserving the same request until one terminal.
Its physical deadline contains only the exact active autonomous phase or
ambiguous fault and cannot manufacture traffic work. Pre-issue
inhibit/status-clear coverage must not create a traffic cadence. HAL must mint the CYW43 send cap from the SDIO
owner's bound notification with send-only rights and badge 256; it must not copy
the owner endpoint.

Send-only reciprocal caps deliver CYW43-to-SDIO badge 256 and SDIO-to-CYW43
badge 2. SDHCI IRQ158 delivers its generated SDIO IRQ badge (currently 159),
and DMA channel-4 IRQ116 has a distinct generated IRQ source bound to that same
SDIO physical owner. Every notification is a coalescing prompt to re-read
durable state; none grants authority, counts work, records history, or replaces
the immutable request identity, generation, selected ordinary-grant,
persistent-marker, or DPC event-lease contract, hardware status, or sequence-last committed ring
state. The reserved high notification
bit is excluded from service work. Tests must deliver, omit, coalesce, and
repeat each prompt at least 4,096 times and obtain the same terminal count,
frame order, and final durable state. Non-op11 CYW43 root commands outside
finite op7 keep their exact root grant plus reserved-root-badge hint; an exact op11 uses its derived
persistent marker and exactly one hint. Both reject endpoint authority, while
non-CYW43 retained commands keep their endpoint coverage.

The production reciprocal-ring/controller test must publish the complete body,
clean it, execute the barrier, commit the new nonzero sequence last, and signal
only after commit. For an op11-derived child, the delegated owner stable-reads
the paired persistent marker, exact command identity, and durable completion
state, then preserves that identity across bounded owner turns without a grant.
For non-op11 delegated work, the existing exact-grant ACK-before-I/O contract
remains covered. PIO must reach one terminal with IRQ158/host state and zero DMA
use. For external DMA, IRQ158 and IRQ116 may arrive in either order or together,
and the same owner joins them into exactly one terminal.
For the generation's release-time, activation-absent or mask-skewed state, or
exact ACK debt bound to an already-submitted immutable activation frontier,
the `DPC_ACTIVATE` bounded ordered transaction masks, inspects, commits or
coalesces durable work, acknowledges the exact IRQ, and rearms. Healthy ordinary controls must reuse the
existing activation with no such child. When activation repair is a
one-way linked child, those steps emit no inner hint; only the generic
sequence-last child-terminal commit may signal. A failed exact IRQ
acknowledgement may retain only that immutable
ACK transaction for a later retry. Inject one failure followed by an exact
successful retry: `ack_failures` must remain one as per-pair telemetry while
current ACK-pending/fault flags clear, and op11, steady TX, DPC urgency, and RX
batching must all remain admissible. Pending, poisoned, and overrun state must
still fail closed. Pair replacement must zero the DPC ring counters and retain
the old cause only in root's first-cause recovery record. Before `Wait`, the owner must re-read
the matching persistent-child, DPC event-lease, or ordinary-grant identity,
completion sequence, committed work condition, and sequence-last one-way
command ring relative to the last consumed sequence. Visible work or a fresh
ring child continues without another
notification. A pending CARD_INT is serviced first; afterward the owner
rechecks durable state and may continue the exact persistent child or DPC event
lease, or preserve an unconsumed ordinary grant, without waiting for a new edge.
The consumed grant cannot replay. The actual card-init `HOST_CONFIG` producer must
cross the production reciprocal ring and retained owner cursor under this
ordering. Telemetry must distinguish the post-generation-admission
one-shot `sdio-owner-command-admitted` marker from generic root engine-init
`command-observed` history, and retained grant acceptance must not be
overwritten by another intake marker. The production `HOST_CONFIG` test must
drive the exact-grant owner to terminal, publish and consume its sequence-last
completion, and advance the CYW43 card-init parent to CMD0. Malformed idle
badges must not be retained. Empty, stale, consumed, mutated, or
wrong-generation grants and acknowledgement failure must execute zero owner
operations and must not produce a private retry loop.

Physical-deadline coverage must use the 20-byte
`DriverRuntimeSdioDeadlineArm` at owner-ring offset 2,028. Tests must prove the
phase plan exports the exact counter expiry for every timed autonomous owner
wait, including pre-issue inhibit/status-clear, issued polling,
containment/reset, and host-clock polls; containment clock settle must select
the earlier settle/overall expiry, while a blockable phase without a counter
fails closed. Publication writes epoch, request, and expiry body before
committing request sequence last; phase progress refreshes or clears the arm,
and terminal/reset clears commit before publication. Root stable-reads the
record and may send one reserved-root fault hint only for an unchanged expired
exact identity. CYW43 must recheck before forwarding its existing badge-256
hint, and SDIO must recheck terminal before deadline. Missing, torn, stale,
cleared, repeated, restarted, wrong-request, and terminal-racing arms must
produce no second hint or device action. Ordinary traffic must produce zero such hints.
The diagnostic assertion is the exact `sdio_deadline_hints=0` field.
CARD_INT coverage must also prove that terminal deferred service masks the host
source before IRQ acknowledgement.
The real SDIO post-claim priority-failure hook must prove that one episode
cannot reclaim its sticky cutover, that each failure/restart action consumes one
outer turn, and that only the exact SDIO-first/CYW43-second pair restart resets
the latch before a later recovery episode may claim cutover. A precondition
rejection without a valid restart context remains terminal.
Autonomous committed-ring condition checks must preserve non-CYW43 root-command
intake when a best-effort endpoint send is lost. Non-op11 CYW43 root commands
outside finite op7, including non-op11 bootstrap and recovery work, retain their reserved-root-badge plus
exact-root-grant cadence and reject endpoint continuation. An exact op11 instead
commits once, signals once, remains `Issued`, and advances from its durable
parent/child state with no recurrent root or delegated grant. Delegated initial
intake uses the coalescing badge-256 notification and sequence-last ring
command. Before a stable `DriverRuntimePersistentWaitReceipt` proves that the
child is armed on its local notification, the issued parent is a runnable
notify/wake handoff and retains the existing open Network lease for prompt
re-admission without a second signal or request. After `Pending`, a
persistent op11 parent or child continues whenever its current durable
condition is locally runnable, including equal deterministic private state; an
exact post-TX `WaitReply` blocks on
CARD_INT/DPC/RX/credit/child-terminal state with no forced source probe or
ordinary-traffic deadline hint. Interleaved EVENT/DATA uses the durable
sideband batch and exact ACK without terminating op11. Once the exact wait
receipt is committed, while HAL reports op11 `Waiting`, root must mask
only that parent's descriptor/logical-owner/HAL-lease self-demand; independent
DPC/RX/sideband/deadline/terminal work remains schedulable, and
`TerminalVisible` restores the
exact terminal consumer. Non-op11 delegated foreground producers retain their
existing `Poll -> Grant -> Poll` coverage. Production DPC instead uses the
event-sequence steady lease from its first post-release event with no mutable
Gate-8 mode switch or recurrent grant. Non-CYW43 and GENET retain their existing
phases; finite op7 keeps its separate identity and bounds while continuing
its current durable runnable state locally.
Pending-command DPC arbitration, reciprocal SDIO child-ring submission,
completion checks, and owner admission remain bounded retained work. The SDIO
owner must stable-read the durable condition and selected persistent-marker,
DPC event-lease, or ordinary-grant identity and recheck completion/work state
plus the sequence-last one-way command ring immediately before blocking. A
fresh ring child re-enters intake without another hint. No immutable hardware request may issue twice, and
no private fallback or recurrent yield/resignal/poll lane may form; consecutive
bounded helpers selected by the current durable runnable condition remain the
same transaction.
Issued-unknown completion reaping must run
before a pair-restart hold: a late exact child reply may prove terminal
ownership and release the claim, but its result/payload is quarantined and only
one exact old-parent terminal may be emitted before restart, with no
same-generation replay. The real root
reciprocal-ring
tests must cut the logical connection epoch once while a CYW43 command is
`Prepared` and once after it is `Issued`. Every active command, selected grant
or persistent marker, and completion in both cuts must retain the cursor's original request and
`aux1`; an active-state check using the replacement generation must fail, a
stale terminal completion must not update replacement state, and no
replacement-generation command may be published for the retained payload.
Stubbed association or WSEC completions cannot satisfy this coverage. Retained
lease tests must also prove that the CYW43/SDIO pair epoch cannot alias serial,
USB, HDMI, PCIe, or GENET transport identity, and that non-pair failures stay in
their contract-local recovery domain. Serial tests must classify RX, staged TX,
and transmitter-idle terminal transport failures as typed `Failed`, poison once
without replay, and never misreport them as `Pending` or backpressure.

The production DPC word-write test must cross the real SDIO descriptor and
controller seam and prove that interrupt-status W1C plus mailbox ACK/NAK use
one incrementing, four-byte Function 1 CMD53 with the exact little-endian
payload, never four bytewise CMD52 commands. Its adversarial cut must publish a
new interrupt cause after sampling but before commit and prove that only the
sampled bits clear. The release-order test must drive the production retained
cursor through `HOSTINTMASK`, the separate Gate 10 `FUNCTIONINTMASK` phase,
watermark, `DEVICE_CTL` read-modify-write adding `F2WM`, `MESBUSYCTRL`,
`WAKEUPCTRL` read-modify-write adding `HTWAIT`, `CARDCAP`, and exact
`FORCE_HT`. It must prove one controller operation per outer turn, preservation
of unrelated read-modify-write bits, stale/cached completion isolation, and no
controller reissue when a fresh pending turn replays only the cached completed
prefix. The final card-interrupt test must cross the same real seam as three
distinct turns: read CCCR `IENx`, write `current | 0x07` without clearing upper
bits, and read it back before DPC activation. A missing required bit, failed
access, or stale completion must terminate as exact fault `0x5339`, poison the
generation, and perform no opportunistic steady-state repair. The complete
catalogued Pi runtime suite covers the atomic DPC
word-write and Linux-ordered post-F2 production-chain invariants; do not replay
those tests as name filters.

The Pi 4 manifest defaults place both `bcmgenet-v5` and `cyw43455` on core `3`;
hardware captures must show `DRIVER_TASK_BOOT ... contract=<selected-network>
... affinity_core=3` for the selected network contract before claiming
fourth-core driver placement. Under MCS, every selected driver must instead
emit `DRIVER_TASK_MCS_ACTIVE ... core=<manifest-core>
timeout_policy=<generated-policy> timeout_endpoint=<installed|omitted>` followed by
`DRIVER_TASK_AFFINITY_MCS ... source=sched-control-sc-bind
direct-set-affinity=no status=configured`; any `TCB.SetAffinity` or affinity
failure marker invalidates placement proof because the per-core SchedControl/SC
bind is the sole MCS placement mechanism. This is distinct from the retired
classic-SMP child-TCB affinity path and from the root-authority affinity wrapper
used around legacy in-process operations. Any
`DRIVER_TASK_AFFINITY_DEFERRED ...
reason=pi4-child-tcb-affinity-boot-stall-guard` line is stale mitigation
evidence and must fail placement proof. Non-CYW43/SDIO runtimes may still emit
`DRIVER_TASK_NOTIFICATION_BIND_DEFERRED ...
reason=pi4-early-tcb-notification-bind-boot-stall-guard`, which keeps their
notification lifecycle proof red while their endpoint-backed command-ring
startup proceeds. The generated CYW43 and SDIO peers must instead emit
`DRIVER_TASK_NOTIFICATION_BOUND ... source=generated-cyw43-sdio-topology`; a
deferred bind for either peer fails Wi-Fi proof because ordinary exact grants
and the persistent op11 parent/child contract use their bound notifications
only as scheduling prompts. QEMU virtio compatibility boots may prove isolated
VSpace/ASID allocation, runtime-image transport-region mapping, and
pointer-free ring transport after virtio networking is online, but that is
transport-substrate evidence only. Fresh Pi hardware proof is still required
before claiming Wi-Fi/DHCP, GENET/DHCP, USB keyboard, HDMI, or strongest
isolated-driver hardware acceptance.

For the selected Pi profile, serial, USB, HDMI, CYW43, and SDIO must report
`timeout_policy=NaturalPostpone timeout_endpoint=omitted`; PCIe and dormant
GENET must report `timeout_policy=Terminal timeout_endpoint=installed`. The
standard fault endpoint, reserved timeout cap/badge/resource, registry entry,
and supervisor authority remain present for every row. Any timeout containment
from a selected natural-postpone runtime is a constructor-policy failure unless
a later explicit device deadline independently classifies the operation.

For the physical Wi-Fi profile with required local-seat, CYW43 bootstrap
admission must not overtake an active PCIe/USB controller-owner publication.
The trace must reach PCIe descriptor completion and owner registration, USB
descriptor completion, and USB controller-ready before the first
`CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=begin`. Each retained transition
occupies its own ordinary EventPump turn. USB keyboard command-ready and
first-byte evidence remain independent gates; neither is required to start
Wi-Fi. This ordering is Pi-only and cannot be inferred from the QEMU VirtIO
transport path.

Strict Pi SDIO command/data calls, fixed-layout SDIO CMD52/CMD53 descriptors, CYW43 firmware/NVRAM/SDPCM command records, direct-root-port xHCI keyboard polling, GENET RX/TX descriptor-ring service, and PCIe port read/write/flush helpers now compile in isolated runtime code before any root hardware execution; host coverage must keep proving those ring turns while preserving the fresh-Pi board-proof boundary.

Non-network cadence coverage must preserve the 48-byte ABI footprint and
version-2 validation, reject staged/torn/unknown-flag records, and prove the
second runtime entry carries a valid previous-entry sample. Formatter and
normalizer tests must distinguish modulo-32-bit entry-to-entry `gap` from
in-episode `run`, render `gap=na` before a previous entry exists, and never
restore the ambiguous schema-v1 `dt` interpretation. Cadence remains passive
source evidence; only fresh target behavior can qualify scheduling.

Physical Pi cadence convergence must separately prove the root-control
composition policy. Deterministic state-machine tests must require the exact
physical-owner plus linked-serial cut, preserve a single poll for QEMU and
every nonphysical path, cap preflight at four polls, and cap every ordinary
physical rotation at five polls. The composed path must stop on reboot,
containment, a phase retaining itself, or return to its starting phase. Active
GENET coverage must perform exactly one NIC poll while admitting the other
useful physical phases before Yield; a retained CYW43 Network action must stop
the composition immediately rather than receiving a second driver turn.
Quarantine coverage must prove persistent display debt cannot create an
unbounded redraw loop or poll quarantined CYW43. The next fresh exact-image Pi
boot must pair serial and captures and compare same-driver cadence against the
generated 10 ms periods: USB must retain Gate 10, one-deep HID/parser proof,
zero drops, zero current no-reply streak, and materially lower controller,
enumeration, command, and total startup latency; serial command response and
HDMI echo/completion must materially improve without a driver fault or root
timeout. The same boot must report whether SDIO request 2 advances beyond
phase 157 `sdio-hw-entry` to first MMIO or a typed terminal. Offline tests,
QEMU, a changed counter gap, or USB readiness alone cannot qualify the Pi
repair, and no SDIO/CYW43 device-semantic change is authorized until that
fresh cadence boot is harvested.

Physical Pi bootstrap-output coverage must select exactly one
ownership-aware sink for a row that would otherwise be emitted through both
`DebugConsole` and forced UART. Nonphysical profiles retain their established
debug-console copy. Tests must prove the selector, and source review must leave
each affected row's complete content on the forced-log path so linked-runtime
handoff still routes it to `queen.log`. Fewer source emissions are not serial
or HDMI performance evidence: the next boot must show one physical copy per
affected row and separately measure prompt latency, response latency, display
first draw, and scroll cadence.

Exact physical image
`1199991dc6bc4da9f2f349251169f6326035b6f709c22ff2bda530a77d055ae6`
harvested that boundary: USB retained Gate 10, HDMI returned all submitted
receipts, and SDIO request 2 stopped after phase 157 without entering the first
power-sequence phase. The corresponding MCS capability audit must prove that
the pair-recovery command cap is minted from the same endpoint object with the
same task-key command badge and `Write + GrantReply` rights as the ordinary
root command cap. The next fresh exact-image Pi boot must observe a later
power-sequence phase and then either a typed terminal or SDHCI progress. A host
test or QEMU result proves only the shared capability policy; it cannot promote
SDIO, CYW43, or network gates.

Current Wi-Fi acceptance also requires one exact
`CYW43_SDIO_DPC generation=<n> captures=<n> published=<n> consumed=<n>
rearms=<n> overruns=<n> epoch_errors=<n> sequence_errors=<n>
ack_failures=<n> owner_active=yes|no poisoned=yes|no masked=yes|no` diagnostic
in the current boot
slice and `WIFI_DPC_PROOF=yes` from
`scripts/pi4_trace_normalize.py --gate-summary`. `wifi dump-state` preserves that
bounded accounting grammar and immediately follows it with
`CYW43_SDIO_DPC_SCOPE captures=event-attempts published=ring-events
poisoned=aggregate-client-or-ring source=card-int-or-source-probe
physical_card_irq=not-exported`. The scope line
is mandatory because the compatibility key `captures` means
`ring.producer + ring.overruns`, includes hardware CARD_INT and authorized
SOURCE_PENDING attempts, and is not a physical IRQ counter. The fixed v3 ring
exports no cumulative physical CARD_INT count. Its overrun and ACK-failure
fields are scoped to the current physical pair and reset at replacement;
cross-pair first-cause history belongs to root recovery diagnostics. Current
flags/pending state remain authority, but accepted hardware evidence requires
`overruns=0 ack_failures=0 owner_active=yes`. `owner_active=yes` is the v3
ring's durable proof that SDIO admitted the generation-long physical activation
state; the exact child terminal separately proves transaction completion.
Valid/empty/unmasked without owner-active state must fail closed. The scope line is followed by
`CYW43_SDIO_DPC_TRUTH generation=<n> owner_active=yes|no ring_poisoned=yes|no
client_sample_stale=yes|no ring_consumer=<n> sample_consumer=<n>
sample_reason=<reason> authority=live-ring action=<action>` plus
`CYW43_SDIO_DPC_REARM generation=<n> counter=client-signal-attempts count=<n>
owner_irq=masked|unmasked action=<action>`. Current client truth comes from the
cache-isolated 128-byte sequence-last `DriverRuntimeCyw43DpcClientRecord` at
shared offset 49,984, accepted only when its exact physical epoch and live
consumer match identical stable owner-ring reads before and after it. That
record keeps the scope and rearm lines byte-stable, revises the accounting/truth
lines with the explicit activation state, and supplies the cause counters from
the existing initialization, quiescent owner-rearm, and terminal-fault
publication checkpoints. A transient raced or stale record is rerun-required
and acceptance-red, never recovery or owner authority. The record supplies
`CYW43_SDIO_DPC_CAUSE samples=<n> frm=<n> hm=<n> fcc=<n> fcs=<n> ca=<n>
other=<n> spur=<n> done=<n> dpc=<n> child=<n> owner=<n> fdpc=<n>
fown=<n>`. The additive v11 completion trace retains those fields only for
historical/compatibility decoding and cannot satisfy current proof. All five
lines must remain complete at maximum counter widths. The
same `wifi dump-state` lifetime must report `sdio_deadline_hints=<count>` from the
fault-only arm relay; ordinary accepted traffic requires zero and a nonzero
value cannot be normalized away as a successful transport wake. The
accounting `poisoned` value is the
fail-closed aggregate of a live poisoned ring, a stale client sample, and
client epoch errors; the truth line distinguishes those causes without
weakening old-capture parsing. The normalizer accepts that distinction only for
an exact adjacent accounting/scope/truth triplet in one ring generation and
exports `WIFI_DPC_RING_POISONED`, `WIFI_DPC_CLIENT_SAMPLE_STALE`,
`WIFI_DPC_TRUTH_AUTHORITY`, and `WIFI_DPC_TRUTH_LINE`; malformed, reordered,
mismatched, or live-poison truth fails closed. A clean live ring with
`client_sample_stale=yes` proves that recovery is unnecessary but remains
acceptance-red with `client-sample-stale` until a current bounded rerun. `rearms` remains telemetry: the
authoritative final rearm condition is `masked=no`, not equality between event
attempts and client-signal attempts.

The same `netstats` snapshot must include complete maximum-width
`wifi_tx_phase_counts gen=<n> accepted=<n> issued=<n> terminals=<n>
successor_issues=<n>` and
`wifi_tx_phase gen=<n> us=n/last/max/avg a2i=<...> t2n=<...>`
records, followed by
`wifi_tx_phase_i2t gen=<n> us=n/last/max/avg i2t=<...>` and
the passive `rx2a_mod32=n/last/max/avg` DPC-admission-to-TX-acceptance metric,
plus
`wifi_tx_phase_rxsplit_q11a gen=<n> us=n/last/max/avg
s2q=<...> q2p=<...>` and
`wifi_tx_phase_rxsplit_q11b gen=<n>
p2r=<...> r2a=<...> sat=<n> inv=<n>
slow=<total>/<s2q>/<q2p>/<p2r>/<r2a>`,
then
`wifi_tx_queue gen=<n> depth=<n> reserved=<n> hwm=<n> drops=<n>
stale_purged=<n>`; `wifi dump-state` must emit equivalent `wifi: tx_phase*` and
`wifi: tx_queue` records. Focused tests must prove generation reset, ticket
deduplication, same-turn issue/terminal ordering with `i2t=0`, later terminal
sampling, terminal-to-successor timing, saturating counters, bounded formatting,
FIFO HWM/drop/stale-purge accounting, and no GENET output or scheduling change.
For these metrics, coverage must prove that accepted batch-v3 source and stage
provenance survives
both direct and transferred exact copied-RX response paths and records once only
after successful paired-TX admission; ordinary TX, nonmatching/stale RX,
failed admission, and legacy records must produce zero samples. It must also
prove that raw zero is present evidence rather than absence, low-word wrap uses
wrapping subtraction, `0xffff` is saturated/UNKNOWN, the immutable packed word
is excluded from behavioral stable-sample identity, a timing-only mismatch
degrades to `0xffff/0xffff` without rejection or recovery, the first DATA entry
is selected after any preceding EVENT, and root's private stable-copy word stays
attached to that same entry. Valid samples must partition source-to-acceptance
into `s2q`, `q2p`, `p2r`, and `r2a`; the Q11 source stages are independent
floors in 2^11 ticks (about 37.9 us per unit). `p2r` is the exact
source-to-root-copy interval minus those two floors, so it includes their
combined quantization remainder of less than 4,096 ticks (about 75.9 us at
54 MHz) as well as actual precommit-to-root delay. Every split field's leading
`n` must remain visible coverage but may legitimately be below `rx2a_mod32 n`,
because only the first DATA entry in each batch carries packed stage evidence.
Each missing, saturated, or invalid sample is individually UNKNOWN. Coverage
is decision-complete for the bottleneck only when the valid same-sample
`slow.total` equals `rx2a_mod32.max`; if `slow.total` is smaller, the worst
sample remains UNKNOWN. `slow` must retain all five values from that one sample
so independent maxima can never be mistaken for one episode. Source measures
runtime DPC-event admission, not radio reception or
physical IRQ arrival; the first stage ends only after the durable queue-state
commit, while the second ends at the final precommit evidence-word sample
before body clean/barrier and sequence-last parent commit.
No passive timing value may feed a
wake, queue, scheduling, issue, retry, deadline, or recovery predicate. This
passive proof field is scoped to reopened Milestone 26b task
`m26b-wifi-join-owner-forensic-decision`, within
`m26b-wifi-sdio-notification-dpc-closure` and
`m26b-net-control-priority`.

The J4 per-child extension must additionally prove the physical two-region
layout and sequential writer handoff. `DriverRuntimeSdioChildTimingMailbox` is
exactly one 64-byte cache line at SDIO owner-ring offset 1,920; tests must show
that owner fault telemetry ends at 1,912, the clock snapshot starts at 1,984,
and the shared payload starts at 4,096. CYW43 stages the immutable child
sequence, descriptor fingerprint, physical epoch, DPC event, typed action/I/O
phase/engine, publication flag, and publication CNTVCT before sequence-last
command handoff. SDIO must validate and preserve that body, add intake,
issue, and terminal flags and CNTVCT words, and commit the matching child
sequence last before the normal completion publication. CYW43 must accept only two identical committed
samples and add its acceptance timestamp without mutating the mailbox. Root is
read-only. Tests must reject a concurrent or late CYW43 mailbox mutation and
any SDIO mutation of the staged identity. Neither side may confuse numeric
offset 1,920 with the parent descriptor on CYW43's physically distinct local
ring.

Capability/layout tests must prove the reciprocal bus link exports only the
SDIO owner ring and eight payload pages covering offsets 4,096-36,863. It must
not export CYW43-private RX-batch pages to SDIO.
`DriverRuntimeCyw43DpcChildTimingRecord` begins exactly at shared offset 49,472,
after the 128-byte bus-episode record at 49,344, is exactly 512 bytes, retains
at most sixteen 28-byte child entries, and ends at 49,984 before the
RX-batch-region end at 53,248. Compile-time and ABI tests must prove 64-byte
record alignment, fixed bounds, no overlap with the RX-batch ACK or bus-episode
records, CYW43 as sole trace writer, root as stable read-only consumer, and no
SDIO mapping. Each entry must preserve the same child sequence and typed
metadata with publication, SDIO-intake, issue, terminal, and CYW43-acceptance low CNTVCT
words.

The following `DriverRuntimeCyw43DpcClientRecord` begins exactly at offset
49,984, is exactly 128 bytes and 64-byte aligned, and ends at 50,112 without
overlapping the 53,248-byte CYW43-private region. ABI/runtime/HAL tests must
prove CYW43-only body-before-sequence-last publication at existing
initialization, quiescent owner-rearm, and terminal-fault checkpoints, stable
root double-read without a write or child turn, and rejection of zero, torn,
wrong-version, wrong-size, reserved-nonzero, wrong-physical-epoch, stale live
consumer, or changed surrounding-ring state. Diagnostic construction must use
the order `live ring -> client record -> live ring`; transient staleness is
rerun-required and acceptance-red, not recovery, scheduling, or owner authority.

Sequence-last tests must reject torn publication, wrong version, stale physical
epoch, wrong DPC event, child/fingerprint/typed-metadata mismatch, overflow,
and wrap-ambiguous deltas without rejecting or delaying the underlying packet.
Healthy coverage must carry exact source, child publication, SDIO intake,
physical issue,
joined physical terminal, CYW43 acceptance, between-child, and queue-commit
evidence from the same DPC episode and preserve the current
`s2q/q2p/p2r/r2a` result. Decision tests must classify a dominant
source-to-first-publication interval as CYW43's local pre-child DPC path,
publication-to-intake as the reciprocal CYW43-to-SDIO handoff/admission seam,
intake-to-issue as SDIO preissue, issue-to-terminal as the selected SDHCI/PIO-or-DMA
engine. Terminal-to-acceptance must classify the final mailbox publication plus
normal completion handoff and CYW43 acceptance, while only between-child or
final-acceptance-to-queue may classify CYW43 local continuation. Missing,
invalid, non-worst, or mixed samples
remain UNKNOWN. Every timing field and classification must be data-only:
mutation or absence must leave notifications, runnable decisions, command
publication, physical issue count, retry/rearm/deadline/recovery state, queue
delivery, and scheduler choice identical. Hardware may admit one minimal
correction only after repeated valid same-seam dominance and a direct
production-code counterfactual. Otherwise stop; historical `494e9cb0e9ad` is
not eligible for merge or restoration because its fresh raw-TCP p95 was about
1.5 seconds with retransmission and first-ACK delay.
Hardware analysis must use high `a2i` for acceptance/FIFO/owner wait through
first issue, high `i2t` for issued runtime/SDIO service including runtime
`WAIT_CREDIT`, and high `t2n` for the post-terminal EventPump handoff only when
queue/acceptance evidence proves a successor was already waiting. Without that
evidence, `t2n` includes ordinary idle time and later TxToken arrival.
`successor_issues` counts only a later actual op7 issue, not TxToken admission,
an earlier local promotion, or general smoltcp delay.

`wifi dump-state` emits the proof only after identical stable reads of the admitted
SDIO owner ring surround one stable valid read of the current 128-byte CYW43
DPC-client record for that exact physical bus-link epoch and live consumer.
The normalizer binds that record only within the exact production command
window `wifi: debug subcommand=dump-state action=begin` -> adjacent
accounting/scope/truth triplet -> matching dump-state completion. A missing,
malformed, clipped, or cross-command sample is acceptance-red and requires a
bounded rerun; no prior command's healthy triplet may be reused. Historical
verbose `wifi diag` windows remain parseable but do not define the new command
split.
The v11 completion layout preserves the complete v10 prefix for old-capture
parsing only; neither version supplies current client truth.
`rearms` counts generation-scoped owner-rearm signal attempts, not separately
delivered wakes or hardware re-enables; the older
source-asserted-empty episode counter cannot satisfy Gate 10. Acceptance also
requires the stable live ring to report `masked=no`. It fails closed with
`WIFI_DPC_REASON=no-activity` unless the exact proof has both event-attempt
`captures > 0` and `published > 0`; this proves DPC ring activity, not a
physical CARD_INT. It also fails when the accounting line is missing,
poisoned, or masked, any overrun/epoch/sequence/ack failure is nonzero,
captured and published totals differ, consumed and published totals differ, or
the final IRQ service state is unrearmed. The DPC diagnostic `generation` is
the linked SDIO/CYW43 ring epoch, not Gate 8's association/control generation.
The normalizer establishes freshness after the current atomic Gate 8 commit
and same-generation bootstrap service-Ready lifecycle; it never compares those
independent generation domains. DPC failures are retained within one ring
generation, but a prior supervisor attempt or superseded association
generation cannot poison the latest exact attempt's healthy accounting.
Exploratory summaries and wired-only historical evidence remain readable
without this Wi-Fi-only proof.

Cause-line coverage must count one `samples` episode per exact raw initial SDIO
interrupt-status capture before the W1C ownership mask is applied. `frm`, `hm`,
`fcc`, `fcs`, and `ca` may overlap and cover FRAME, HOSTMAIL, FC_CHANGE,
FC_STATE, and CHIPACTIVE respectively. `other` advances when any nonzero raw
bit falls outside those classes and may overlap a known cause; `spur` advances
only when the complete raw initial status is zero. `dpc` counts
event-associated CYW43 DPC turns, `child` counts distinct SDIO child
submissions, and `owner` counts the initial and each fresh-grant owner quantum
issued. `done` counts completed DPC-admitted frames. `fdpc` and `fown` must
accumulate turn deltas only at frame completion, so their ratios to `done`
exclude all work after the newest completed frame. Tests must prove the
252-byte v11 layout is an additive suffix with no 196-byte v10-prefix drift,
generation-reset behavior is unchanged, and the five diagnostic lines remain
untruncated at `u32::MAX`. The counters are passive telemetry and must not
change DPC authority or timing. Focused coverage is
`cyw43_rx_idle_trace_v11_layout_is_additive`,
`cyw43_rx_idle_trace_v11_appends_dpc_causes_without_v10_prefix_drift`,
`cyw43_dpc_cause_and_frame_turn_accounting_is_episode_exact`,
`rx_idle_trace_v11_decodes_dpc_causes_and_retains_v10`,
`sdio_dpc_cause_diagnostic_preserves_generation_scoped_ratios`,
`dpc_client_counters_require_same_generation_v11_sample`, and
`wifi_sdio_dpc_diagnostic_lines_preserve_truth_without_truncation`.

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
Start QEMU (source tree or bundle), then verify:
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
  - No unexpected `ERR` lines or reconnect loops.
  - ACK/ERR/END ordering stable.

### Conditional A — TCP reliability smoke
Stage 04 is self-contained for local QEMU. If no `COHESIX_GATEWAY_URL`
(`HIVE_GATEWAY_URL`, `COHSH_REST_URL`, or `COH_REST_URL`) is supplied, the stage
boots a local QEMU instance, starts `hive-gateway` against that TCP console, and
uses a stage-local request-auth token. Local mode allocates free loopback ports
by default; override the local bind/port with `TP_STAGE4_GATEWAY_BIND` and
`TP_STAGE4_QEMU_TCP_PORT`. Supplying an explicit gateway URL keeps the
external-gateway path and requires
`HIVE_GATEWAY_REQUEST_AUTH_TOKEN` (`COHSH_REST_AUTH_TOKEN` or
`COH_REST_AUTH_TOKEN`). Stage 04 uses the same canonical absolute Python 3.11+
selection contract as Stage 01 for its local probes and `cohesix-py` REST smoke.

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

The retained exact-source boundary for this gate is explicit. State
`out/test-plan/m26e-console-qemu-v35-v18-rest-timeout-ddscope-20260814T163813Z`
passed Stages 01-05 for source
`sha256:ea8bc5458e4ccbbcd1ecb2731030b66e7ec05782750534c0bf4a683a3fba9b60`:
Stage 01 `21/21` with manifest
`sha256:4320add56bd34ce7c4461507703eb6be4a22ee44b70424c4a8f5dedb5acc1f5f`;
Stage 02 `2/2` with manifest
`sha256:c9ae96134c2d2e2211c51e4524a98f1f6aa0654b75130a8ac5e596ef42ac6c03`
and resolved manifest
`sha256:72b6fdbd175150ec352f9345d99791a1d576cf01de47363aed2a64ad0c463a93`;
Stage 03 passed the complete selection of fixed matrix `7/7` plus 18 `.coh`
scripts and bound manifest
`sha256:71092307be0d3501b47b12191604d698656fcb773007ede12fda798ae961499d`,
aggregate
`sha256:b65136f01a90d0f06ba192cdfe7252a29dff5e5f113d72f613a2d6609dc4976e`,
base artifact
`sha256:2491dc27b6b27a939b4427742c517405376516c27fbcb00962da03f1036f3012`,
and gated artifact
`sha256:1b0345bc385ef50480a439fb274e72f11877e40ef70f9ce5bae58ca00bd9939e`.
Stage 04 passed core `3/3`, parity `1/1`, and Python smoke with manifest
`sha256:f0c960c1a4b4103b851b71fb928fa54a5c96b30a43abf35558d3c9bf81456bee`
and result
`sha256:a8cd9ded3369ba74daf4e21bdad144a1d49eba3eb8a92e6d13d069836fb3d246`;
Stage 05 passed with manifest
`sha256:f03756e37da6eaef8edea929da5512144d80f4608a4b7296c486b12dabeceb88`.

The exact-version post-pass at
`out/host-tool-postpass/ddscope-stage03-20260815` later stopped at its first
failure: gated `cas-tool` REST upload packed the 1,073-byte retained trace into
1,152 bytes and nine 128-byte chunks, then received
`ERR ECHO reason=quota detail=buffer-full` for ninth digest
`247fbe6cb8e8f54887478fb4b4ffc1c8ca21324c5b0640db57fe638c0023196f`.
That is failure evidence for the former host projection, not a reason to raise
the eight-chunk target bound or retry. The five-stage pass remains valid only
for its exact source and workloads; it did not close Conditional B, Python
target projection, the Conditional D host-model comparator, or Pi. The capacity-projection
repair is authorized by active discovery task
`m26e-console-network-service-isolation` together with Reopened tasks
`m17-cas-regressions` and `m24e-cas-tool-rest`; changed source requires fresh
artifacts and every applicable gate.

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
  - A concurrent "core" batch (boot/proc/pool coverage): `scripts/cohsh/boot_v0.coh`, `scripts/cohsh/observe_watch.coh`, `scripts/cohsh/session_pool.coh`.
  - A strict "parity" batch (control-plane smoke): `scripts/cohsh/rest_control_plane_smoke.coh`.
    - Note: `scripts/cohsh/busy_backpressure.coh` and `scripts/cohsh/policy_gate.coh` remain covered by the TCP/QEMU regression matrix (Stage 03), where console-parser semantics are validated directly.
- Stage 04 also runs a Python REST smoke (`tools/cohesix-py` `RestBackend`) that performs `LS /` and reads `/proc/lifecycle/state` against the same gateway.
- Logs:
  - Scripted Stage 04 writes REST batch logs under the stage state dir (for example `out/test-plan/<run-id>/rest-regression-logs/`).
  - Manual runs of `scripts/cohsh/REST_regression_batch.sh` default to `out/regression-logs/<batch>/<script>.run*.log` unless `COHSH_LOG_ROOT` is set.
- Verify logs show no unexpected errors or disconnects.
- From Milestone 25 onward, use the REST batch above; the TCP/QEMU batch remains a local bring-up tool only.

The fresh V35/V18 staged state
`out/test-plan/m26e-console-qemu-v35-v18-full-20260814T134855Z`, source identity
`sha256:0a1c64ec92fc9f80d74e423972c2579872fc067fbf76370c54daf14e823b2821`,
passed Stage 01 `21/21`, Stage 02 `2/2`, the fixed matrix `7/7`, and all 18
selected `.coh` scripts. Its Stage 03 base/gated artifact IDs were
`sha256:d7f978d66935e93318892a09a7be426bc6083e55fc7237aaaea5d9ff332523f9`
and
`sha256:d0141863625f009280bab8f3bcbc085c0adc36dcdf978c116c9b42f0ac67c981`,
and its Stage 03 aggregate result was
`sha256:3cad4f939982e0d892ed9a92299bf9082345a601ee001f9489df032b61109e6b`.
Stage 04 attempt `20260814T142002.169146Z-32864-8dc11c8651a4` then passed REST
`boot_v0.coh` and failed `observe_watch.coh` line 8 with
`ERR TAIL path=/proc/ingest/watch reason=gateway transport error`. The gateway
kept one target connection and one upstream ATTACH, QEMU continued answering
ECHO operations, and no root-emergency, fail-stop, panic, or target fault was
recorded. The client expired at three seconds while the gateway was still
inside its legal five-second queue plus 120-second broker-response envelope.
This is exact Stage 04 failure evidence for the bounded deadline restoration
owned by active `m26e-console-network-service-isolation`, reopened
`m24e-rest-client`, `m24e-cohsh-rest-transport`,
`m25-smp-rest-regression-batch`, and the deadline-composition portion of
`m25f-gateway-broker-refactor`; it is not Stage 04 acceptance or performance
evidence.

The next fresh state
`out/test-plan/m26e-console-qemu-v35-v18-rest-timeout-enumbound-20260814T150747Z`,
attempt `20260814T153840.420898Z-83984-3692079da030`, bound source digest
`sha256:9e782b7b531895b5e30954e16bb28b6fd460b29e42a2d0fadfabb5251e057f56`.
It verified the Stage 03 base artifact, brought local QEMU and Hive Gateway to
readiness, passed the concurrent core batch (`boot_v0.coh`,
`observe_watch.coh`, and `session_pool.coh`), passed parity
`rest_control_plane_smoke.coh`, passed the Python REST smoke with 12 root
entries and a 12-byte lifecycle state, and wrote and verified Stage 04 result
`sha256:6c971c409a8b352d5aef197e3cf40fca475f18f3b1a0275173b5502f7fe5a277`.

Finalization correctly refused acceptance with status `stale-inputs`. Initial
context digest
`sha256:ee0a82e7448afebb6e8daaa10f9c85979dc8f39da7722b5609549de9ef4f6fc2`
became
`sha256:dbe746163ea96ed2d8b6485d3d8512f38d0cfbbaff2eeac42f9896f1a1898bde`
solely because Stage 04 retained seven values it had exported after initial
capture: `COHESIX_GATEWAY_URL`, `COHSH_REST_RESPONSE_TIMEOUT_MS`,
`COHSH_REST_URL`, `COH_REST_URL`,
`HIVE_GATEWAY_BROKER_CONTROL_RESPONSE_TIMEOUT_MS`,
`HIVE_GATEWAY_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS`, and `HIVE_GATEWAY_URL`.
Git HEAD, status, tracked diff, untracked inventory, source digest, action
digest, toolchain digest, and Stage 03 dependency attestation remained exact.
This is runner-owned provenance self-mutation, not a target, REST,
external-concurrent-edit, or generated-restoration failure. Every behavioral
result remains diagnostic only; Stage 04 did not pass and Stage 05 was not
reached. Restore the pre-export environment before either final-context path,
pass the focused regression above, and rerun Stage 04 from a fresh source-bound
state.

That fresh rerun is
`out/test-plan/m26e-console-qemu-v35-v18-rest-timeout-envrestore-20260814T154759Z`,
bound to source digest
`sha256:c9736b7be1f5c14d16a2606f1259588e523b3fa03e33c774ac3f5040a80a4886`.
Stage 04 attempt `20260814T161701.848576Z-19430-73991892f2e6` recorded byte-identical
initial and final contexts with context digest
`sha256:9b90429e67bf72aec97941ee7e93eba99eeb0379624e8672b14f1c2f82a6a782`.
Its immutable passing stage manifest is
`sha256:f475d77ecda220df2d1bb992d724a574175b0638691d87175bbdccc4051462de`.
The Stage 03 base artifact verified, the local gateway declared the exact
`5,000 + max(120,000, 120,000) + 5,000 = 130,000 ms` response window, and the
core batch passed `3/3`: `boot_v0.coh`, `observe_watch.coh`, and
`session_pool.coh`. Parity `rest_control_plane_smoke.coh` passed `1/1`; the
Python REST smoke returned 12 root entries and a 12-byte lifecycle state; and
Stage 04 result
`sha256:de30a40d14f9e2e075c515c9e1f83cf3cd94d0662238c9f23afa045a0a64e98f`
wrote and verified. This is immutable Stage 04 QEMU REST integration PASS for
that exact source, dependency, artifact, and context. It does not establish a
five-stage run, Stage 05, performance, release, or Milestone 26e acceptance.

### Conditional B2 — Milestone 26e QEMU executable-Worker REST pressure

Run the canonical `scripts/m26e_qemu_pressure.sh` command in
[BENCHMARKS.md](BENCHMARKS.md). The macOS lane cleans repository `target/` and
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
- pre/post state contains every ordered generated Worker row with five-part
  identities, image hashes, READY/control/receipt/completion sequences, cores,
  passive executor/Reply identity, and the full generated per-slot admission
  object bundle (not an observed retype census), plus hash-bound
  `/proc/schedule/{summary,queue}` and `/proc/lease/{summary,active,preemptions}`
  snapshots;
- one bounded Heartbeat kill/recreate cycle proves terminal teardown and a
  larger supervisor generation; GPU and LoRA retain their identity while their
  receipt and completion sequences increase through real host-ticket-v2 work;
- exact per-run UART/fault bytes match `fault_artifacts`, the marker index is
  complete, and the target transcript independently contains all role faults,
  all seven actions with confirmed/rejected/stale outcomes, exact teardown
  booleans, service containment, and the GICv3 target/session markers;
- `/gpu/bridge/status` and the bounded LoRA export job identify only the
  QEMU/bootstrap-trace fixture path. Missing, expired, production-labelled, or
  provider-live-labelled fixture input blocks the run;
- `report.workload.control_write_outcome` is `admitted`; no ACK, HTTP success,
  provider result, or control write is described as accepted or READY;
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
      either record/prompt ordering. The canonical command-ready receipt must
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
      viewport is materialized, each repeat advances it by one bounded CSI
      `S`/`T` row;
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
      Wi-Fi progress suppression during USB boot activity and after USB
      first-byte proof. These checks introduce no console command or USB/HDMI
      authority change. The reserved 48-byte USB old-good slot is the only ABI
      evidence extension described above; runtime publication is dormant.
      The Pi write-only damage-compositor regression must independently prove
      fixed row-ring scroll semantics, zero scanout reads during scroll,
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
      The Pi GENET profile regression must require exact IRQ 189, badge 1024,
      and default queue 16 while the QEMU profile retains exactly three
      driver-runtime IRQ entries, no GENET IRQ, and unchanged scheduling.
      Runtime tests must prove
      a maximum 16-frame/24,576-byte child DPC quantum, private-queue overflow
      preservation and wrap, bounded control/data fairness, exact-badge
      admission, durable masked continuation, device-store/unmask readback,
      final source/ring recheck, and no handler acknowledgement while accepted
      work remains. A separate regression must place a complete bounded frame
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
      Generated-profile tests must also
      prove that no direct
      GENET-console link/caps or nine-page export-helper bypass is emitted.
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
    - when the compiler-declared console-network child owns TCP/IP, `nettest` must not inherit the root adapter's default `unsupported` result. Its existing 15-second generation remains peer-assisted: a physical backend requires a post-admission exact child response drain, a later NIC TX completion, later RX/TCP counter progress, matching authenticated connection identity, and listener readiness; direct VirtIO requires the same child drain but no synthetic root NIC completion. Historical traffic cannot satisfy a new run. The unchanged terminal schema reports `udp_echo_ok=false` when only peer-assisted proof is present. Native ICMP echo response is a separate reachability check and cannot be relabelled as the UDP self-test.
    - isolated child liveness triage must retain the bounded `netstats: isolated_progress`, `netstats: isolated_units`, and `netstats: isolated_state` rows. They distinguish selected child observation/output/disconnect/ingress/tick/egress/diagnostic turns, material-progress time, command/output queue depth, pending egress, response-drain state, and ingress backpressure/drop. These counters diagnose where progress stopped; they are not TCP, performance, or acceptance evidence by themselves.
    - `tx_submit=<count> tx_complete=<count> tx_free=<count> tx_in_flight=<count> tx_double_submit=<count> tx_zero_len_attempt=<count> arp_rx=<count> arp_tx=<count>`; on CYW43, `tx_complete` is the root release count from exact joined Function-2 terminals. `tx_submit > tx_complete` means an outstanding root TX owner, not a missing firmware-credit acknowledgement.
    - `wifi_assoc=<0|1> wifi_link=<0|1> eapol_rx=<count> eapol_start=<count> eapol_secure=<0|1>`
    - driver-task scheduling evidence for the active hardware path in reopened 26a/26b acceptance captures: contract name, service class, isolation mode, poll/service count, budget exhaustion/yield count, RX/TX queue depth, drop count, manifest-selected affinity core, observed service latency, and timer backend proof. The normalizer exposes this as `TIMER_BACKEND`, `TIMER_CLOCK_HZ`, `TIMER_EL0_COUNTER`, `DUMMY_TIMER_SEEN`, `DRIVER_TASK_CONTRACTS`, `DRIVER_TASK_DEDICATED`, `DRIVER_TASK_COMPATIBILITY`, `DRIVER_TASK_DEDICATED_READY`, `DRIVER_TASK_SERIAL_DEDICATED`, `DRIVER_TASK_USB_DEDICATED`, `DRIVER_TASK_DISPLAY_DEDICATED`, `DRIVER_TASK_NET_DEDICATED`, `DRIVER_TASK_SDIO_DEDICATED`, `DRIVER_TASK_PCIE_DEDICATED`, `DRIVER_TASK_SUBSTRATE_READY`, `DRIVER_TASK_FAILED_COUNT`, `DRIVER_TASK_CAPSET_PROOF`, `DRIVER_TASK_FAULT_PROOF`, `DRIVER_TASK_REVOKE_PROOF`, `DRIVER_TASK_SCHED_PROOF`, `DRIVER_TASK_AFFINITY_PROOF`, `DRIVER_TASK_AFFINITY_CONFIGURED`, `DRIVER_TASK_AFFINITY_APPLIED`, `DRIVER_TASK_AFFINITY_MANIFEST_PROOF`, `DRIVER_TASK_AFFINITY_MANIFEST_MATCHES`, `DRIVER_TASK_AFFINITY_MANIFEST_MISSING`, `DRIVER_TASK_AFFINITY_MANIFEST_MISMATCHES`, `DRIVER_TASK_VSPACE_PROOF`, `DRIVER_TASK_POINTER_FREE_IPC_PROOF`, `DRIVER_TASK_OWNER_STATE_PROOF`, `DRIVER_TASK_DMA_PROOFS`, `DRIVER_TASK_DMA_BLOCKER`, `PI4_RUNTIME_DMA_PROOF`, `PI4_RUNTIME_DMA_PROOF_REASON`, `PI4_RUNTIME_DMA_COUNTER_PROOF`, `DRIVER_TASK_ACTIVE_NET`, `DRIVER_TASK_BUDGET_OVERRUNS`, `DRIVER_TASK_LATENCY_PROOFS`, `DRIVER_TASK_RING_CALL_BEGIN`, `DRIVER_TASK_RING_CALL_RETURN`, `DRIVER_TASK_RING_CALL_OUTSTANDING`, `DRIVER_TASK_RING_CALL_TIMEOUT`, `DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT`, `DRIVER_TASK_BOOTSTRAP_DEFERRED`, `DRIVER_TASK_RESOURCE_INIT`, `DRIVER_TASK_RESOURCE_BLOCKER`, and `DRIVER_TASK_RESOURCE_CURRENT_BLOCKER`. `DRIVER_TASK_OWNER_STATE_PROOF=yes` must be backed by per-hot-path owner-state descriptor lines for serial, USB, HDMI, PCIe, and the selected network owner set (`cyw43-wifi` plus `sdio-host` when `DRIVER_TASK_ACTIVE_NET=cyw43`, or `genet-nic` when `DRIVER_TASK_ACTIVE_NET=genet`). Pi 4 performance evidence must report `TIMER_BACKEND=arch-counter`, `TIMER_CLOCK_HZ=54000000`, `TIMER_EL0_COUNTER=vct`, `DUMMY_TIMER_SEEN=no`, `DRIVER_TASK_DMA_BLOCKER=none`, and `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`; otherwise latency proof is red even if driver-task owner-state proof is present. `DRIVER_TASK_RESOURCE_BLOCKER` is the first lost resource proof in the capture; `DRIVER_TASK_RESOURCE_CURRENT_BLOCKER` is the latest non-ready resource-init blocker. The source `DRIVER_TASK_RESOURCE_INIT` line carries the current isolated runtime owner/action, active request, `expected_request_valid` / `expected_aux0_valid`, expected aux/request values when present, same-request flag, and child progress marker needed to diagnose the live turn. Any positive `DRIVER_TASK_RING_CALL_OUTSTANDING`, `DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT`, `DRIVER_TASK_BOOTSTRAP_DEFERRED`, or non-`none` resource blocker is an isolated runtime no-reply/deferred-proof frontier; raw `DRIVER_TASK_RING_CALL_TIMEOUT` counts remain diagnostic when a later return closes the same request. Contract-only root-task compatibility evidence, resource-init breadcrumbs, and declared `max_service_us` budgets are diagnostic and must not be counted as dedicated driver-task closure or latency proof.
    - routine WiFi, GENET, and HDMI call begin/return chatter may be absent in steady, nonblocking, prompt-slice, and retained modes. Initialization, descriptor non-acceptance, fault, budget-exhaustion, and non-quiet timeout evidence remains required; absence of a routine call row cannot be treated as progress or failure proof.
    - a Wi-Fi pair-recovery diagnostic may retain four pre-scrub rows named `scheduler_sdio_fault`, `scheduler_sdio_status`, `scheduler_sdio_dma`, and `scheduler_sdio_regs`. `captured=yes` is valid only for two stable reads of an exact Fault completion with version-3 116-byte payload, aligned in-ring cursor, contained or owner-poisoned flag, matching magic/version, and matching terminal result. `captured=no` is unavailable evidence, and all-zero rendered words in that case are not register values. These passive rows can distinguish SDHCI inhibit/status from DMA4 `CS.ERROR`, control-block, or debug state, but cannot satisfy owner, DPC, association, traffic, performance, or acceptance gates.
    - driver-task counter evidence for performance triage is separate from owner-state proof. Activity-gated `DRIVER_TASK_COUNTER` lines are normalized as `DRIVER_TASK_COUNTER_SNAPSHOTS`, `DRIVER_TASK_COUNTER_INVALID`, `DRIVER_TASK_COUNTER_BUSY`, `DRIVER_TASK_COUNTER_SAME_REQUEST`, `DRIVER_TASK_COUNTER_TIMEOUTS`, `DRIVER_TASK_COUNTER_KEEP_ACTIVE`, `DRIVER_TASK_COUNTER_ABORTS`, `DRIVER_TASK_COUNTER_STAGED_BYTES`, `DRIVER_TASK_COUNTER_CACHE_OPS`, `DRIVER_TASK_COUNTER_CACHE_BYTES`, `DRIVER_TASK_COUNTER_RX_FRAMES`, `DRIVER_TASK_COUNTER_TX_FRAMES`, `DRIVER_TASK_COUNTER_RX_BYTES`, and `DRIVER_TASK_COUNTER_TX_BYTES`. `DRIVER_TASK_COUNTER_SNAPSHOTS` counts distinct `(contract, hot_path)` owners and the totals aggregate only each owner's latest cumulative snapshot; repeated diagnostic commands therefore cannot inflate activity. `DRIVER_TASK_COUNTER_INVALID` still counts every observed empty, truncated, non-root-ring, or otherwise malformed line, including an invalid sample superseded by a later valid one. Reopened 26b performance evidence must keep `DRIVER_TASK_COUNTER_INVALID=0`, and selected Wi-Fi counter qualification requires current CYW43 plus SDIO owner snapshots rather than unrelated USB, HDMI, or PCIe activity.
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

### Conditional G — Release bundle validation (macOS + Ubuntu)
Run the catalogued host-tool, replay, and UI bundle checks from a clean
extraction directory, never from repository build output.
- macOS bundle: `releases/Cohesix-0.9.0-beta-MacOS.tar.gz`
- Ubuntu bundle: `releases/Cohesix-0.9.0-beta-linux.tar.gz`
- Ensure headless Linux uses `xvfb-run` for SwarmUI.
- The release bundle includes Python tests and fixtures for running `python3 -m pytest tools/cohesix-py/tests`.

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

In the Stage 04 PASS state above, Stage 05 attempt
`20260814T161937.357994Z-20575-399d58a7497f` retained source
`sha256:c9736b7be1f5c14d16a2606f1259588e523b3fa03e33c774ac3f5040a80a4886`.
`required-audit-assets` and `staged-evidence-state` passed. The first failed
proof layer was `stage-01-attestation`: recorded context
`sha256:bc69f60f93bd1a670a3eb13e71476bb9c01cb8d086805f3d255599a75f1bec7d`
recomputed as
`sha256:a40a397cce804241bad65a1d8b0da96c0f5ee4167f6fb83985dbc7a00dc7a973`.
Forensic reconstruction established that the verifier received exactly three
wrapper-injected additions: `DD_GATE_LOG_DIR`,
`DD_REUSE_STAGED_EVIDENCE_FROM`, and `DD_REUSE_STAGED_EVIDENCE_TARGET`.
Source, toolchain, action, and dependency identities were unchanged. The gate
failed closed at that check; no later Stage 05 gate ran.

The reused-stage verifier remains a direct call. Context capture passes the
stage into `test_plan_evidence.selected_environment(scope, stage)`, which
records `DD_*` controls only for Stage 05 and ignores them for Stages 01-04,
where they do not govern behavior. Applicable `TP_*` selectors, toolchain,
source, and target remain bound in every stage context; inherited documented
Stage 05 controls are neither cleared nor hidden from the stage they govern.
Focused regression
`test_due_diligence_selectors_bind_only_stage_five_context` passed, proving
inherited `DD_COLLECT_ALL`, `DD_GATE_LOG_DIR`, and
`DD_REUSE_STAGED_EVIDENCE_TARGET` are absent from Stage 01, exact in Stage 05,
and `TP_HOST_JOBS` remains exact in both. The complete evidence suite passed 30
tests plus 5 subtests, and the complete due-diligence lifecycle suite passed 27
tests plus 2 subtests. These host tests close the selector-isolation defect
only. Stage 05 and full acceptance remain open until a fresh source-bound
all-stage run passes.

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
scripts/release_bundle.sh --check-manifest
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

`scripts/release_bundle.sh --check-manifest` validates the inventory-selected
version, exact host-tool architecture, selected GICv3 kernel, target-image
sources, and every individually listed document, script, Python artifact, UI
asset, trace/transcript fixture, support file, and versioned migration. Bundle
creation then compares every regular-file destination against
`release.expected_bundle_files` and emits `MANIFEST.sha256`; recursive copies,
ignored files, missing files, and unexpected files fail.

GPU bridge tests cover real-or-empty live registry behavior, no first-model
activation, placeholder-secret rejection before connection, exact manifest and
CAS identities, base/adapter compatibility, source epoch/sequence monotonicity,
activation receipt validation, stale snapshot rejection, and TTL withdrawal to
`unavailable`. The target begins with no GPU, model, lease, temperature, node,
unit, or provider fixture state. QEMU acceptance must use the selected GICv3
MCS closure and fresh build output; this gate is not Pi hardware evidence and
does not modify or reclassify CYW43 behavior.

### Milestone 26e executable-slot and critical-TCB admission gate

Run the compiler/model gate before any QEMU target boot:

```bash
cargo test -p coh-rtc worker_admission
cargo test -p root-task --test schedule
cargo test -p root-task --test worker_resource_admission
cargo test -p root-task --test worker_fault_registry
cargo test -p root-task --test critical_tcb_reserves
cargo test -p root-task --test critical_tcb_handoff
cargo test -p root-task --test driver_supervisor_handoff
cargo test -p root-task --test mcs_fault_lanes
cargo test -p root-task --test mcs_activation_order
scripts/check-generated.sh
```

The resource test recomputes the maximum live role mix rather than multiplying
the namespace/model capacity. It must prove, for every object class and untyped
bytes, that `fixed + maximum mix + post-construction reserve <= capacity`.
The selected seL4 16 AArch64 SMP+MCS header values must be TCB 11 bits,
endpoint 4, notification 6, Reply 5, minimum scheduling context 7, CNode slot
5, and page/page-table/VSpace 12; stale classic notification or Reply sizes are
an admission failure.
Reject zero/aliased/out-of-range Worker and critical retention slots, duplicate
SC slots, mismatched Worker prefixes/cores/counts, an undeclared role mix, and
any capacity or byte overflow. Injected allocation failure at each constructor
stage must leave every child suspended and must not make a partially populated
slot eligible for reuse.

The temporal compiler test recomputes per-core budget demand and every active
task's fixed-priority response-time recurrence. It must reject stale response
results, a missed deadline, overflow, non-convergence, a core/SchedControl
mismatch, fewer than two total refills, missing consumed-time evidence, and an
active SC that aliases another task. QEMU and Pi each reserve 1,000 us of every
10,000 us core window. The Pi table includes seven linked-driver records; the
default QEMU table deliberately does not fabricate those hardware TCBs.
Manifest `root_task.schema = "1.14"` retains exactly one fixed, root-retained
NineDoor bootstrap SC outside the steady temporal-task topology. Resource
admission must therefore total 18 SCs for QEMU and 25 for Pi; the former 17/24
totals, a zero-object NineDoor SC inventory, or double-counting the one-shot
object must fail.

The critical topology tests must account for the init TCB exactly once as the
real `root-control` domain and exactly four distinct restricted children:
`root-fault`, `root-emergency`, `root-worker-supervisor`, and
`root-driver-supervisor`. Reject a duplicate, phantom fifth control child,
missing kernel object, shared child CNode/TCB/SC/Reply/retention cap, wrong core,
idle/trampoline entrypoint, or activation before registry seal. Critical
permanent-domain retention caps are not grouped reclaimable untyped anchors.
The QEMU compiler fixture must assert root-control remains on core 0 with
`5500 us / 10000 us`, `5000 us` WCET, `7600 us` response, and
`m26e-qemu-root-adjacent-refill-natural-postpone-candidate-v35` provenance. Pi must retain
core 0, `2750 us / 10000 us`, `2500 us` WCET, `5100 us` response, and
`m26e-pi4-root-adjacent-refill-natural-postpone-candidate-v24` provenance. The QEMU root row must select
`virtio_operator_serial_io_bytes_per_turn = 64`; the Pi root row and every
non-root row must select zero. Validation must reject every QEMU root value
other than exact `64`, including `0`, `65`, `1024`, and `u32::MAX`, plus a
nonzero non-VirtIO root bound or a nonzero non-root bound. The QEMU fixture
must place the active `console-network-service` task, matching service config,
and SchedControl on core 2 with `3000 us / 10000 us`, `3000 us` WCET,
`3000 us` response, and
`m26e-qemu-console-received-progress-retention-candidate-v18` provenance; its
GPU and LoRA peers must each derive `3600 us` response. It must
assert exact core-0/core-2 demands `8750/3800 us`. Pi must retain its earlier
console core 0, derive `8100 us` response, retain GPU/LoRA `1200 us`
responses, and preserve exact `9000/9000 us` admission truth while selecting
the same V18 child provenance and ABI v3. Both fixtures must select
`NaturalPostpone` for the active console child and their selected root-control
row, retain each timeout cap/badge/resource/registry identity, and prove both
TCB timeout-handler slots are left empty while their standard fault endpoints
remain terminal. All budgets, periods, deadlines, WCETs, response bounds,
priorities, MCPs, refill counts, placements, reserves, and the separate QEMU
24 MHz/Pi 54 MHz clocks remain unchanged.
The focused runtime regression must authenticate over the real smoltcp socket,
drive the child-side TCP state through `FinWait2`, queue complete late output,
and prove that free send capacity without `can_send()` neither calls
`send_slice` nor returns an error, commits bytes, or publishes
`OutputDrained`. A subsequent peer close must reach `Closed`, publish exactly
one `Disconnected`, clear the ended connection state, and relisten. The normal
Established-state complete-frame send remains covered, and no other Session
backpressure or error source may be reclassified as successful continuation.
A distinct real two-stack regression must authenticate, close the client socket
without root disconnect control, observe the server enter `CloseWait`, and prove
that V12 requests the existing graceful disconnect. Exact-generation output
remains queued until `close_ready`; the existing close path must then reach
`LastAck`/`Closed`, publish exactly one `Disconnected`, clear the ended
generation, and restore LISTEN so a replacement client can connect. The test
must add no retry, polling loop, timeout, or fabricated root wake.
A separate V13 real-stack regression must drive a locally initiated close
through `FinWait2` into `TimeWait`, call `TransportSession::end` before aborting
the completed TCP control block, restore LISTEN immediately, and authenticate a
replacement connection with identity 2. It must prove one old-generation
`Disconnected`, cleared old authentication/output state, unchanged `Closed`
handling, and no modeled `10 s` expiry, retry, polling loop, or root wake.
They must independently assert the exact `root-fault` candidate:
`3000 us / 10000 us`, `2400 us` per-unit WCET, `2600 us` per-unit response,
and `m26e-qemu-root-fault-service-units-candidate-v6` provenance. No
fixture may reinterpret that per-unit response as the end-to-end latency of
the terminal-critical receive/classify, suspend, and emergency-signal
sequence.
The fixtures must additionally preserve the target-specific supervisor
admission split. QEMU must assert `root-worker-supervisor` at
`3000 us / 10000 us`, `2400 us` WCET, `4800 us` response, and
`m26e-qemu-root-worker-supervisor-cold-activation-candidate-v15` provenance,
and `root-driver-supervisor` at `3000 us / 10000 us`, `2400 us` WCET,
`2400 us` response, and
`m26e-qemu-root-driver-supervisor-cold-activation-candidate-v15` provenance.
Their steady active core-1 demand must be exactly `6000 us`. With the
root-retained NineDoor bootstrap budget of `3000 us` at priority 128 below both
supervisors, transient activation demand must equal the exact `9000 us` usable
capacity of the `10000 us` window after its `1000 us` reserve. Pi must retain
the earlier compiler truth unchanged: Worker supervisor
`750/10000 us`, `600 us` WCET, `1400 us` response; driver supervisor
`1000/10000 us`, `800 us` WCET, `800 us` response; both retain
`m26e-qemu-candidate-v1` provenance pending the required Pi checkpoint.
The WCET is a per-phase live candidate because the v2 whole-turn interpretation
exhausted the complete 2750-us refill at the console-network control wake, and
the live compact-page run at source `00bf02540` timed out `root-control` at the
console-network control poll, disproving the v3 combined Network/runtime-IPC
phase. The live run at source `4d1a47b89` retained three outer phases but timed
out in `VirtioTxToken::consume` after queue notify, disproving v4's multi-unit
Network turn. The later live v5 run raised child timeout badge `0x26ee0007` at
`Send` after `publish_exchange` with exactly `3000 us` consumed, then raised
root timeout badge `0x26ee0001` at the sole outer `seL4_Yield` after
containment/quarantine with exactly `2750 us` consumed. Those failures disprove
the multi-material child turn and whole-containment Recovery turn. The
historical `5100 us` V24 response was per-phase scheduler admission, not
end-to-end host/TCP latency. The exact V24 snapshot at
`out/m26e-qemu/ack-split-v24-20260813T122117Z/v24-gdb-readonly-snapshot.txt`
binds current-fault label `5`, badge `0x26ee0001`, FaultIP `0x5361c` in
`KernelSerialDriver::read_byte`, LR `0x5391c` in
`SerialPort::poll_rx_current_tcb`, outer successor Runtime, and Operator
successor SerialDispatch. The child remained healthy at Wait `0x21360c` while
root-fault received and emergency was fail-stop. V18 retains V17's temporal
envelope and authenticated oversize transition while retaining private service
after nonzero bounded socket receive progress: current
fixtures must assert root
budget/WCET/response `5500/5000/7600 us`, child budget/WCET/response
`3000/3000/3000 us`, GPU/LoRA response `3600 us`, and core-0/core-2 demand
`8750/3800 us`; stale V15 child values fail closed. Passing offline recurrence proves admission for this
conservative pending envelope, not that the values are numerically minimal.
The canonical v6 root ELF SHA-256 was
`0059fd675b476106888d6ca62c8bba21f9b340b9aa607e000fbf96997fd29900`.
That run raised root timeout badge `0x26ee0001` after exactly `2750 us` at the
sole outer yield. Its saved state proves the preceding Network visit composed
an empty ObserveChild, no-op StageOutput and Disconnect, then committed and
signalled the first 60-byte ARP ingress as sequence 1. The child was healthy and
no Recovery ran. Treat this only as failure evidence that a no-op lower unit
must consume its own Network visit.
The canonical v7 root ELF SHA-256 was
`d2f69bddbf56deef6919ec6ea802e9d3c44a691c2dbe05aa59428854bbf7a6ae`.
It emitted the startup command list while the UART-visible
`[mark] root-console.start.ok` record remained queued; that wire absence does
not locate the source before the console lifecycle transition. Before any
ordinary Network or Recovery phase, `root-control` consumed exactly `2750 us`
and raised current-fault class `Timeout`, badge `0x26ee0001`, at serial queue
`inner_dequeue` (PC `0x43e84`) called by `SerialPort::flush_tx_unlocked`
(LR `0x77b74`). Treat this only as v7 failure evidence that every serial poll
and flush within one VirtIO Operator must share one bounded credit.
The canonical v8 root ELF SHA-256 was
`5052e7a5070987c252d3c1f5cf6f27172bd5ece1836a8f6c2a5c329c789a0a61`.
With the generated `64`-byte limit active, `root-control` still consumed its
complete `2750 us` refill and raised current-fault `Timeout`, badge
`0x26ee0001`, at PC `0xede84` immediately after `emit_prompt_now`. Treat this
only as v8 failure evidence that the byte limit must also admit at most one
retained output-record attempt in each isolated VirtIO Operator.
The canonical v9 root ELF SHA-256 was
`fa488c9367136f0eadef7182a18691664c3ae51c2ac2974e12000ff5d27f38ed`;
its CPIO SHA-256 was
`aca549e99e0d86299e9f98348d896b730259277654544ebd22a74595b61e9bfb`.
The direct command list completed under the bootstrap SC. At the first
post-bind Operator, serial was idle, the one-record cursor was full, and the
retained marker plus initial prompt were still queued. `root-control` consumed
the complete `2750 us` refill and raised current-fault `Timeout`, badge
`0x26ee0001`, at PC `0x13a798`, the first instruction of `compiler_builtins`
`memmove`. LR `0x79ccc` was
`heapless::Vec<PendingConsoleOutput, 72>::remove(0)` and `x2 = 0x110` described
the prospective 272-byte move, but zero bytes were copied. Treat this as
aggregate first-post-bind refill exhaustion across activation tail work,
no-work containment probes, and the Operator prefix. Do not classify it as
copy-cost evidence or as a failure of the one-record admission rule.

The canonical v10 root ELF SHA-256 was
`022908395c954f73a67136f70fe4404d96e0cf1ff16f4531fa95eae7a6f57cb5`.
Its post-activation yield completed, and UART emitted the retained startup
marker and prompt in separate bounded Operator visits. The second fresh Runtime
consumed the complete `2750 us` and raised root timeout badge `0x26ee0001` at
PC `0xce98c`, the `seL4_NBWait`/nonblocking receive on root endpoint `0x0a70`.
The saved successor was Network, the output FIFO was empty, its record cursor
was inactive, and the response barrier had crossed the prompt. Treat this as
failure evidence for v10's composed Runtime responsibilities; do not classify
it as activation, output, Network, or Recovery failure.

The same run recorded exact console timeout sequence 1, badge `0x26ee0007`,
with Terminal policy. The child was saved at `seL4_Wait` with
`service_pending = 1` and `control_pending = 1`, proving one completed logical
unit composed with its pending successor on residual SC. Recovery reached
Complete with the TCB suspended, SC unbound, mappings scrubbed, capabilities
revoked, objects deleted, and generation fenced; NineDoor remained healthy.
Treat this as failure evidence for the v3 child replenishment boundary, not
containment completeness.

The canonical v11/v4 run used root ELF SHA-256
`44971429e4941d751248c216082256f01e187930d9a6d40028e5c89d8611b597`,
console child ELF SHA-256
`af08f817191cc51c9354b61f09f3eeb50c8cdf875c660c7231987a426886666d`,
and CPIO SHA-256
`9fbb58e1dc6dc508361f37ce0c24219e3e9029dae101e2be789df1bcb1a5b11d`.
There were four TCP connects. The first three completed authentication attempts
each wrote 18 bytes and read zero; the fourth connect had no completed
authentication record. The child consumed the complete `3000 us`, raised timeout
badge `0x26ee0007`, and stopped at PC `0x213458`, the `seL4_Yield` immediately
after the composite `PollService` completed and cleared; saved retained state
identified `PollService` as that completed unit. Root completed console
containment through cursor discriminant `Complete(6)`, then consumed the
complete `2750 us` and raised timeout badge `0x26ee0001` at PC `0xf5fbc`, the
sole recurring outer `seL4_Yield` after an empty Operator. The stored ordinary
successor was `Runtime`; the retained Runtime successor was `ControlEndpoint`,
proving the previous Runtime selected `Worker`; output was empty. The child
fault is the chronological initiating fault and the later root-control timeout
is the fatal escalation; the emergency fail-stop is downstream, not an
independent primary fault. V11/v4 fail the live per-unit gate and are not
qualification evidence. Stage 03, the canonical `.coh` batch, Hive Gateway
pressure, and complete host-tool validation remained withheld.

The next non-claiming convergence run,
`out/test-plan-convergence/v12-v5-auth-20260812T010200Z`, bound root ELF
SHA-256 `7cec5bd582d063adc73830af8cc62e0ec8dbbb33d91bd4701db09ca69e32e6ca`,
console child ELF SHA-256
`920883c5e706688a65e7f168a643dbc527d09d7f48584bfb41fbd0c0ae823cb6`,
and CPIO SHA-256
`dc36495a5de0df13bfb853ffa33fdc6e7ccc3bbf3a1a3c8c4cd74c8551160c16`.
All four authentication attempts wrote 18 bytes and read zero. The only target
timeout was root-control badge `0x26ee0001`, with exact consumption `2750 us`
and outer-Yield PC `0xf612c`. Stored ordinary successor `Network(2)` proves the
completed phase was Runtime; stored Runtime successor `StreamFlush(3)` proves
the selected unit was `BootstrapDrain`, whose staged `Option` was `None`.
Fault sequence 2 and the console child healthy at Yield-then-Wait prove no
earlier child fault or Recovery. The result embedded dirty source commit
`a533290ffe264f0a2bf0af3db4bb4c45d1a4a278`; repository HEAD later advanced to
`84934dda6`. Treat the run only as immutable diagnostic/failure evidence for
the generic Runtime-without-control prelude composed with an empty selected
unit. It cannot qualify either source identity or any acceptance stage.

The next non-claiming convergence run,
`out/test-plan-convergence/v13-v5-auth-20260812T014607Z`, bound dirty source
commit `84934dda6fcffbfa536d4e437cc1904c7fdeb0b1`, root ELF SHA-256
`0275cd7d701263cc1731ca3301d9aeab8a0393651745659f192106a0d558d78f`,
the unchanged v5 child SHA-256
`920883c5e706688a65e7f168a643dbc527d09d7f48584bfb41fbd0c0ae823cb6`,
and CPIO SHA-256
`142e2aec64662888a9872ff77ff85d1f5f7c351b7aaa478ded8cf99ba9e64f29`.
All four authentication attempts wrote 18 bytes and read zero, and the first
failed proof layer was `real-target-operation`. The initiating timeout was
root-control badge `0x26ee0001` at `sel4::poll` SVC PC `0xce98c`, with caller
`0x108910` immediately after the child-to-root notification poll inside
`IsolatedVirtioConsole::poll`. Committed ordinary successor `Operator` and
lower successor `StageOutput` identify selected unit `ObserveChild`. The child
remained healthy at `seL4_Wait`. Root-fault then timed out with badge
`0x26ee0002` at `suspend_tcb` SVC PC `0xce0cc` while targeting root-control TCB
cap `0x10`; root-emergency fail-stop was downstream. Treat this only as
diagnostic failure evidence for v13's generic Network-prelude/all-unit adapter
path and v2's composed terminal-critical containment. It does not qualify or
falsify child v5.

The exact v16 image bound root ELF SHA-256
`4fab7abc8707b9829ba66ac525efdfc7afefa812df4bab9abb8cb67d504a76a6`
and system CPIO SHA-256
`456558cac05e4d136d3cbc18d1290cc48bebf619ba5459cd623b667dbfff3e96`.
The prompt serial/output completed, but root-control consumed the complete
`2750 us` and raised timeout badge `0x26ee0001` at outer-Yield PC `0xf61c4`.
Saved successors `Runtime` and `ControlEndpoint` identify the completed phase
as Operator and the earlier Runtime unit as Worker. Exact target disassembly
showed an approximately `0x42c0`-byte generic EventPump frame and an
approximately `0x12a0`-byte generic Operator frame still preceded the retained
output leaf. Root-fault then consumed the complete `3000 us` at the first
post-classification Yield, PC `0x113938`, before suspension or emergency
signalling. Treat both as diagnostic failure evidence for v16 root-control and
v3 root-fault, not child-v5 or interface failure.

The exact v17 non-claiming run
`out/test-plan-convergence/v17-v4-auth-20260812T041428Z` bound root ELF
SHA-256 `3d0641bac42d21ce383c47f38628a05db0d2474fab69fc6e14b67ba39a71bd47`,
the unchanged v5 child SHA-256
`920883c5e706688a65e7f168a643dbc527d09d7f48584bfb41fbd0c0ae823cb6`,
and CPIO SHA-256
`fa478638d6d2b93b654a2615e4dcd1e1d7f666d0945d4e012adcf28da2292af1`.
All four authentication attempts wrote 18 bytes and read zero, and the first
failed proof layer was `real-target-operation`. Current fault `.1` was
root-control at outer-Yield PC `0xf6624`; saved ordinary, Runtime, and Operator
successors were `Runtime`, `Worker`, and `SerialDispatch`. Treat this as exact
failure evidence that v17 reached its compact successor-before-work path but
still composed serial driver admission/RX and TX flush inside `SerialIo`. It
does not qualify or falsify root-fault v4 or child v5.

The exact v18 non-claiming artifact bound root ELF SHA-256
`e7d34f018ff308c575fedb79ca7cef5542a7da8e753c09ddb9d55cf9daa79d4e`
and system CPIO SHA-256
`0dca41cc6fdd9a877144dcd2db610beaeafef95423a81ce6896b01bb9b8f5cf5`.
All four authentication attempts wrote 18 bytes and read zero. Root-control
consumed exactly `2750 us` and raised timeout badge `0x26ee0001` at outer-Yield
FaultIP `0xf66e4` after a completed Network phase. The ordinary successor was
`Operator(0)`; isolated lower successor `Disconnect(2)` proves selected unit
`StageOutput(1)`. Pending egress was zero, deferred diagnostic state was `2`,
and no child signal occurred. Root-fault timeout `.2` at `suspend_tcb` SVC PC
`0xce1f4`, with its retained cursor already `SignalEmergency`, was downstream.
Treat this as exact failure evidence for the v18 split Network prelude, not its
Operator split, root-fault v4, or child v5.

The exact clean v19 non-claiming artifact bound root ELF SHA-256
`0737a6f008197fd5b931af104c95164ddcd925fa04a8440439895c1e76b26fca`
and system CPIO SHA-256
`51e7b955b449b42b7a0cad569aa187e19a0f71464ffb81080d29733a589e7ed0`.
All four authentication attempts wrote 18 bytes and read zero. Root-control
timed out at outer-Yield PC `0xf66dc` after a completed Network phase. Lower
successor `Ingress(3)` proves selected `Disconnect(2)` was a no-op and did not
signal the child. Pending egress was empty, the child remained healthy at Wait
PC `0x21343c`, and root `smoltcp_polls` was `250098`. Treat this as exact
failure evidence for the composed post-leaf counter refresh, NETDIAG, and
NineDoor ingest aggregate. It does not falsify the timer prelude, selected NIC
unit, root-fault v4, or child v5.

The exact immutable v20 launch set bound root ELF SHA-256
`ed5cb9f587d0d63e6121f8b00b083e68f5a0a7dd23dd6d2bbf0c899e1e85e80f`,
system CPIO SHA-256
`ca2a52038eb0814a17c8609f03bec32ff357fdd524edee3e7080ac69ceb7823b`,
kernel SHA-256
`865b5a0614f1633ca636800705f97339e78f47065fdaffd2cb4139e4a25630c0`,
and resolved-manifest SHA-256
`6dd92e0015a1ec7c14af5321fc35ebc9143673dbe71b09766a83bf3b77a36e0a`.
Its four authentication attempts wrote 18 bytes and read zero. A same-artifact
no-input run reached the root marker and prompt before root-emergency
fail-stop. Root-control timeout `.1` was at outer-Yield PC `0xf680c`; successor
Operator and retained NETDIAG `Some` prove the timer-plus-NIC visit completed
and the diagnostic successor had not run. Exact lower-cursor, pending-egress,
and child state are unconfirmed; stale layout offsets cannot qualify them.
Treat this only as failure evidence for composing Timer and Nic in one Network
visit. It does not falsify the selected NIC/lower unit, exclusive diagnostic
visit, child v5, root-fault v4, or v15 supervisors.

The exact immutable v21 launch set
`out/cohesix-v21-qemu-20260812T070200Z` bound root ELF SHA-256
`c3d45ee5650373ed6064de1a7d13691d9473f300ac5ffec11d0f1719ab877de2`,
system CPIO SHA-256
`4d164fcc7c9a3605d3d5ca7430895e3cf911f4a294ddb0432d8afafb0966ed64`,
console child ELF SHA-256
`920883c5e706688a65e7f168a643dbc527d09d7f48584bfb41fbd0c0ae823cb6`,
and kernel SHA-256
`865b5a0614f1633ca636800705f97339e78f47065fdaffd2cb4139e4a25630c0`.
The official non-claiming run
`out/test-plan-convergence/v21-v4-auth-20260812T070901Z` bound dirty commit
`bf77d71946e958c5a4671db1c4f5bd9edd959aae`, source digest
`sha256:1b9cbf5125e24c9e36741bfc558d9d5a50cf5f193a4840158a007ab00ac11251`,
and failed first at `real-target-operation`. It and the controlled exact-image
run `out/live-diag/v21-20260812T071219Z` each completed four authentication
attempts that wrote 18 bytes and read zero. The controlled run remained healthy
at the prompt without TCP input; AUTH then reproduced terminal child timeout
current-fault `[0x5, 0x26ee0007]`. The child consumed the full `3000 us` refill
and stopped at post-unit `seL4_Yield` FaultIP `0x213434`. Retained
`service_pending=1`, `control_pending=1`, and `ingress_pending=0`, combined
with the v5 successor-before-work contract, prove completed `StackPoll`
returned `Continuation` with `Session` committed next. Smoltcp 0.13.1
explicitly documents `Interface::poll` as potentially unbounded because it
loops ingress and egress. Record this only as exact failure evidence for child
v5; it does not falsify the v21 root boundary, root-fault v4, or v15
supervisors.

The exact immutable V22 launch set
`out/cohesix-v22-qemu-20260812T074004Z` bound root ELF SHA-256
`d392ee72ada945f5b5ae52e4dee285ed3e7ff67fa9283103a234af9d15e389fc`,
system CPIO SHA-256
`7055a956cf84cbaa2c6b00b8c9fe59d73c52c7cd157e07b66960434ac66af64c`,
console child ELF SHA-256
`89c241b6198a140170744b02b707a6000486acf39dee0799a302d74605fde52b`,
and kernel SHA-256
`865b5a0614f1633ca636800705f97339e78f47065fdaffd2cb4139e4a25630c0`.
Non-claiming run `out/test-plan-convergence/v22-v4-v6-auth-20260812T074300Z`
bound dirty commit `bf77d71946e958c5a4671db1c4f5bd9edd959aae` and source digest
`sha256:4f7473411494f351def533a52e6bf8e3f02037b75c8945992171a16108cf7c28`;
all four authentication attempts wrote 18 bytes and read zero. Controlled
exact-image AUTH recorded initiating root-control timeout badge `0x26ee0001`.
Root-control was inactive at FaultIP/NextIP `0xe9ddc`, before the outer
phase-successor store after composed response reconciliation and prompt-tail
queueing; retained phase Operator proves no phase leaf ran. The child remained
healthy at `seL4_Wait` after `StackEgress`, with Session committed,
`service_pending=1`, `ingress_pending=1`, and `control_pending=0`.

The same snapshot recorded secondary root-fault timeout badge `0x26ee0002` at
FaultIP/NextIP `0x113e70`, immediately after the Receive turn's terminal Yield
SVC at `0x113e6c`. LR `0x113e5c` followed publication of initiating label `5`
and badge `0x26ee0001`; Classify was already committed. Record these as exact
failure evidence for v21's composed predispatch and root-fault v4's unprimed
first Receive respectively, not as child v6 failure or qualification evidence.

The later V23/V6 image exposed a systemic execution-profile defect that
supersedes further source-leaf attribution until repaired. Exact root ELF
`e3d757a79f2fa186a405e3199d8b62633dbf22fb9c12462341f8344e94c04731`
failed under ordinary wall-clock TCG at changing boundaries across otherwise
independent root and child owners. The byte-identical image under
`-icount shift=0,sleep=off` completed `OK AUTH` and the Secure9P write/read
portion of `9p_batch.coh`; that establishes diagnostic path reachability only.
Its first HVF exception was the pre-seL4 FP/ASIMD trap EC `0x07` at
`cpio_get_file+28` (`ldr q25`), where release libcpio had been auto-vectorized
without the elfloader's required `-mgeneral-regs-only`. The old profile also
embedded the TCG-oriented SMC DTB and 62.5 MHz timer value while this supported
Mac exposes 24 MHz under HVF. A historical macOS release independently reached
seL4 and root bootstrap under HVF on the same host. Therefore `-icount`,
wall-clock TCG, and CNTFRQ-dilated runs remain non-claiming diagnostics; they
must not trigger another owner split or qualify `.coh`, REST, or performance.

The immutable V23/V6 diagnostic artifact
`out/cohesix-m26e-hvf-20260813T054027Z` binds rootserver SHA-256
`48ae02bd3faae9626a03c29932ca15b809d30f9552404ddfbe81051d24dace95`,
console child SHA-256
`89c241b6198a140170744b02b707a6000486acf39dee0799a302d74605fde52b`,
and CPIO SHA-256
`0619e47f1157815f43629ab56c03b39f342e44460b931efd822bc93e42e0c689`.
The fresh TCG gateway map
`out/m26e-qemu/v23-v6-tcg-gateway-fault-map-20260813T172000Z` recorded the
console TCB at FaultIP/NextIP `0x213d58` (`BRK`), LR `0x21373c`, and `x0 = 5`
(`RuntimeError::Backpressure`) after `poll_session_unit`. A breakpoint on the
old image's shared send/backpressure exit at `0x21832c` then hit with smoltcp
TCP state `FinWait2`, proving that the capacity-only send predicate admitted a
closed transmit half. This is diagnostic localization of the v6 defect, not
target acceptance for v7.

The canonical HVF V24 artifact
`out/cohesix-v24-attach-fastpath-hvf-qemu10-20260813T081651Z` bound rootserver
SHA-256 `e2386dc044fb78ab2cf757db3304bf918847b99990f7aa7869eb366647cbdcb7`,
console child SHA-256
`b1689774784e6e2ab11d5ac878fee173f42150deb6c371baa26017a8a81f18db`,
CPIO SHA-256
`d9dc4058acb5f97d65af7ff17219dfc4034e0527c407b063f2bcd482ea97316b`,
and kernel SHA-256
`8aa06eaf462a87d3aa6c1f8500ad1d1ed5632f5ad8031c5eab53fdd2095257da`.
Sequential run `out/m26e-qemu/v24-attach-sequential-20260813T083057Z`
completed `OK AUTH` and `OK ATTACH role=queen`, then `boot_v0.coh` timed out
waiting for the first `TAIL /log/queen.log` response. UART explicitly reported
`terminal-fault class=Standard`. The read-only map placed the child at
FaultIP/NextIP `0x213d80` (`BRK`) with LR `0x2136cc`; the TCB fault payload had
already been consumed/zero. SC offset `+0x30` still contained the configured
timeout-handler badge `0x26ee0007`, which is configuration state and must not
be cited as evidence that the initiating fault class was Timeout. Exact
source/state mapping identified retained `ApplyControl(SendLine)` after the
owning session ended and `AuthState` became inactive. V7 read the control page
but discarded its connection identity, so the already-staged old-generation
record entered the current unauthenticated terminal-error path. This is exact
V7 stale-generation failure evidence, not V8 qualification and not evidence
that `ATTACH` failed.

The exact V8 HVF artifact
`out/cohesix-v8-stale-control-hvf-qemu10-20260813T090943Z` bound rootserver
SHA-256 `f200ecf56021e5eb712e28ecd3ea290d476552166ba2ce3a6b6b5d959f811910`,
console child SHA-256
`ed0b2b7ecbf2c9f302a30d087d54fbfe264fbfe6857f1717f94319ae4735b551`,
CPIO SHA-256
`991ea1d85fa8fb5a2d1146cbe9b183eaf3c204301d8348d4f18a722cb490156d`,
and kernel SHA-256
`8aa06eaf462a87d3aa6c1f8500ad1d1ed5632f5ad8031c5eab53fdd2095257da`.
Its live run `out/m26e-qemu/v8-stale-control-live-20260813T092000Z`
reached `root-console.start.ok` without a target fault. Two sequential TCP
connections each wrote the complete 18-byte AUTH frame, read zero bytes, and
timed out. This is exact failure evidence for V8's unconditional post-unit
blocking Wait: child-owned publication and service continuations could remain
durable but unrunnable until an unrelated later root signal. It does not
invalidate V8's stale-generation fence and is not later-candidate qualification.

The immutable V25/V11 artifact
`out/m26e-qemu/temporal-v25-20260813T125130Z/artifact` completed the first raw
AUTH, but replacement raw connections were reset at `+1 s` and again at
`+10 s`; `raw-auth-twice.log` and `raw-auth-after-10s.log` retain that evidence.
A live read-only snapshot, for which no transcript was retained, showed root
current fault `NullFault`, root-control fully replenished at budget `5500 us`,
and the healthy child blocked in core-2 Wait. Source comparison localized the
failure to peer FIN advancing the isolated socket to `CloseWait` without the
server half-close/relisten transition that M26b Complete `72288c7d` had owned.
This is V25/V11 reachability and failure evidence, not V12 qualification.

The immutable V12 target set
`out/m26e-qemu/peer-close-v12-20260813T133000Z` bound source digest
`sha256:c047b0886ba42ba1dfe0004009a8e9377d4d2cbd98e997e8dfd463e4bc80eaa0`.
Raw session-1 AUTH, host close, and same-boot raw session-2 AUTH passed. A third
`cohsh` session passed AUTH, ATTACH, four-line TAIL, END, and QUIT. Replacement
authentication timed out at `+5 s`; a raw replacement still timed out at
`+30 s`. UART recorded no fault. The read-only GDB reproduction under
`out/m26e-qemu/peer-close-v12-20260813T133000Z/attached-teardown-gdb-20260813T134100Z`
found all CPUs kernel-idle. Smoltcp 0.13.1 had the sole socket in `TimeWait`:
its fixed `10 s` close delay is re-armed by each incoming replacement SYN.
M26b Complete `72288c7d` treated `TimeWait` as terminal; V13 restores that
boundary by ending the old generation, aborting the completed TCP control
block, and relistening immediately. This is V12 failure evidence for V13, not
V13 qualification.

The immutable V13 target failure is retained in
`out/m26e-qemu/peer-close-timewait-v13-20260813T140319Z/same-boot-two-complete-sessions-20260813T141000Z`.
It binds rootserver SHA-256
`d91c89bc9e076097e45085c92a368760ecf39cb36fbf1fede2faa9099d87f31d`,
console child SHA-256
`4057c1b6a850ae806e2d9d5fde018ad88a831b66c606f4cf7a105ea38fe3f387`,
and CPIO SHA-256
`bb60bfaf2b9b0775361ecc111b972ab8f8f9ae1b0d9d9afd672bac3e56e4cdf1`.
Direct session A completed AUTH, ATTACH, four-line TAIL, END, and QUIT. Same-boot
session B connected twice, but each AUTH wrote 18 bytes and read zero before the
script failed after 21 seconds. QEMU remained alive, UART showed no runtime
fault, and exact child disassembly retained V13's `TimeWait -> end -> abort ->
listen` path. Root source and cursor audit found the earlier boundary: a
successful Disconnect left `disconnect_requested` set; `ControlCompleted`
reopened the control slot and `OutputDrained` marked that new control drained.
The next `ObserveChild -> StageOutput -> Disconnect` pass therefore published
Disconnect again, and each successful signal reset the lower cursor to
ObserveChild. This repeated transaction starved Ingress and ServiceTick, so the
peer ACK/FIN never reached the child and V13's packaged branch never ran. V26
adds a per-connection issued latch that commits only after successful
publication, remains clear on backpressure, survives completion/drain while the
Quit reason remains requested, and clears on the connection/generation terminal
paths. The post-completion Disconnect visit must then be a no-op and advance to
Ingress and ServiceTick. This is V13 failure evidence for V26, not V26
qualification.

The V35 root-control, V6 root-fault, V18 child, and unchanged V15 supervisor
candidates must start with a fresh, validator-clean HVF QEMU profile and
exact-artifact convergence canary through bounded TAIL/CAT response progress,
the fixed one-socket HELP/NETSTATS/CACHELOG9/SMP matrix, standard-fault
terminal containment, and budget-exhaustion natural-postponement
liveness/isolation. The
retained V26/V13 artifact already passed two sequential direct sessions on one
boot. Exact V27/V15 artifact
`out/m26e-qemu/bounded-response-v27-v15-20260813T164606Z/artifact` and evidence
`sendbatch-tail64-cat-20260813T165110Z` completed boot, AUTH, ATTACH, the
64-record TAIL, bounded CAT, and host-side script exit zero, then QUIT reached
`closing session` before UART fail-closed at `response-completion-sequence`.
That is exact failure evidence: child-side drain admitted Disconnect before the
root-owned terminal response lane had retired its exact completion/drain, ACK
debt, copied egress, and queued-output identity. Neither historical result
qualifies V30/V15. The immutable V28/V15 artifact
`out/m26e-qemu/terminal-disconnect-fence-v28-v15-20260813T171339Z/artifact`
and evidence `two-session-sendbatch-20260813T171736Z` recorded both sessions
exit zero, one connection per session, 25 retained TAIL/CAT records in session
one, and no UART terminal fault. That result qualifies only V28's terminal
Disconnect fence and same-boot reconnect; it does not qualify generalized
synchronous producers, Stage 03, REST, performance, or Milestone 26e. The
immutable V29/V15 artifact
`out/m26e-qemu/bounded-sync-response-v29-v15-20260813T190157Z/artifact`
binds rootserver SHA-256
`f60d716573afe3387360689b20f5e9012171a0c4e34ebdd7d4b21bf89811b9a6`
and resolved-manifest SHA-256
`983b5433ec4b4138ebc5bc05401393bcd6f623bae96e46d9ca4ad52694889adc`.
Its fixed matrix completed AUTH and ATTACH, then HELP returned zero bytes before
the unchanged 30-second timeout. A read-only snapshot showed clean root and
child state. Breakpoints proved HELP finished command handling with
`capture_started=false`, and the selected live `DefaultNetStack` vtable used
the trait-default bounded identity returning `None`. Source audit found V29's
four isolated response hooks and the preexisting pending-event hook were not
delegated through the selected wrapper. This is exact V29 failure evidence for
V30, not V30 qualification.
The immutable V30/V15 artifact
`out/m26e-qemu/default-netstack-response-v30-v15-20260813T200444Z/artifact`
completed HELP and NETSTATS, then the child terminalized at the Yield SVC with
two adjacent refills totalling exactly `3000 us`. The non-claiming local-Poll
diagnostic
`out/m26e-qemu/local-poll-diagnostic-v30-v15-20260813T204840Z/artifact`
completed HELP, NETSTATS, and the correct first-call SMP body count of 16 on
its first boot; the host's then-expected 26 was oracle drift. A second fresh
boot of the same bytes reached `root-console.start.ok` and emitted
`root-emergency fail-stop` before any TCP probe. These results disprove the
V15 terminal-timeout/local-Poll combination and are not V16 qualification.
The later non-claiming V30/V17 Stage 03 artifact
`sha256:466f8210e6aa98757f6393758bfed0cee24e15272f95e680a5263feab26ea90e`
bound root SHA-256
`47df96e2a444e507a6872bf43af5ddd47133f960c8daebdc2dba71d10f53b690`
and resolved-manifest SHA-256
`eb274d4358345ddef0d4a191d75fd2bd40ff98c6577f0135fd18cd599f9afbd0`.
Its fixed matrix, `boot_v0.coh`, `9p_batch.coh`, and `host_absent.coh` passed.
`observe_watch.coh` received the exact typed invalid-path CAT error, but retained
an uncommitted ordinary stream shell with no END pending; same-connection QUIT
then timed out before dispatch. This is V30/V17 failure evidence for V31, not a
Stage 03 pass.

The next non-claiming V31/V17 Stage 03 run
`out/m26e-qemu/stage03-v31-v17-20260814T005001Z` bound invocation/source SHA-256
`21b37c19572ef05accf6258c602442d21cd2ee24ff9594a2a3e0d2ab489bfae3`.
Its base launch record SHA-256 was
`099e8de70ece2c38f38ebf1cd7d1b1acaf730e5baa5d7ca20d3ba6021ce2d94e`,
with artifact ID
`sha256:a1f6953065daa1dac388ba249c302a0acf8563e3e75546da9f94a1ab9a1454f5`,
resolved-manifest SHA-256
`29f5c8c7a044fa3b9c611bad06be30904a6f77db43d17cabce53e30cfa89545e`,
rootserver SHA-256
`919de64dfe7e69e627e7461e1f6822cc7714f3fbaf2171cb84b59592189084f9`,
CPIO SHA-256
`62ba464d2613206526f8d2b3d001cbea233ba3b32d5a81e95885a2ec426bf12e`,
and kernel SHA-256
`8aa06eaf462a87d3aa6c1f8500ad1d1ed5632f5ad8031c5eab53fdd2095257da`.
The fixed matrix passed 7/7, then `boot_v0.coh`, `9p_batch.coh`, and
`host_absent.coh` passed. In `observe_watch.coh`, its first two rapid TAILs
passed; the third reached target command begin, `tail.start`,
`tail.stop reason=eof`, and command end with status OK, but `cohsh` received no
complete response before the unchanged five-second deadline. CAT and QUIT were
not reached. The gated image was built but never target-run: launch record
SHA-256
`2925c2c8540e3b1034d0d949c4852a16d97b64439bde8eb9e5ddfad0a5820f04`,
artifact ID
`sha256:650b5b5c171f2cc18c0ad2ba3ffa2f65157625cd49c11b951e57c69f41765000`,
resolved-manifest SHA-256
`2ae9b9afdcd3bf9c1cf90db81822636ca69cbbe66774bddb3aab542e099dd100`,
rootserver SHA-256
`e1dc0c5664c4834e451c5bbc396cd98b9dc30130802e0a209f8a04fb198df0ee`,
and CPIO SHA-256
`cfa93f1591b50928622ac8fc3a638afa2ab000c926e9830c6f099aad7270bfb8`,
with the same source and kernel. This is V31 failure evidence for V32, not a
Stage 03 pass or qualification of either image.

The subsequent immutable non-claiming V32/V17 Stage 03 run
`out/m26e-qemu/stage03-v32-v17-20260814T014606Z` bound invocation/source
SHA-256
`3e1d02dac334fbe0019f517b34a3aad13f483e52e9d07570b52d122391a14eb1`.
Its base artifact ID was
`sha256:105b33911515641a19cde0a7c670c848911c16e14134bdf00efcdeba418c0483`,
with launch-record SHA-256
`94d409d0b5bdae883c929defaf801ee3cbaeb2d88688bf690b71f40ff983199b`,
resolved-manifest SHA-256
`03f72150a556b78a897ae291752c81d9def8cf283a930a8cd8b55b92464270e1`,
rootserver SHA-256
`592b60dc45597f1ca29d646ae9cf0fd4c8df86298f393016ee915f0e91ce161e`,
CPIO SHA-256
`2fd94dad9e75b43ea7eb8f42c38a4a587dc5e07a1c8f33cb10c2a6444fa29500`,
console-child SHA-256
`513ec72cf5174c2765d46696a41d28610b1171ee880b23720cfab0b261f51f40`,
and kernel SHA-256
`8aa06eaf462a87d3aa6c1f8500ad1d1ed5632f5ad8031c5eab53fdd2095257da`.
The fixed matrix passed 7/7 and `boot_v0.coh` passed. `9p_batch.coh` then
successfully wrote `batch-1`, `batch-2`, and `batch-3` but failed its line-12
exact ordered-substring check because V32 routine command/session diagnostics
entered public `/log/queen.log` and contaminated the bounded CAT preview. One
of 17 selected operational `.coh` scripts passed before the runner stopped.
The gated image was built but never target-run: artifact ID
`sha256:6b9b281b4c872c588154f5d9a309797d4ec20e263943e0920e49ca0af492f1f0`,
launch-record SHA-256
`b4d7babfad21b2a70aa9311900e7f0692ecfbb6a4a772f5d933bd31909f8ace9`,
resolved-manifest SHA-256
`aeb4e263793c7b25bc1c4fc00983e25de033298c71fa8ce447a5a24eae19e101`,
rootserver SHA-256
`841ed16776c3dca1b1e3b1c6b7599d5a7b001e67f83f6a98cc889c33740688ad`,
and CPIO SHA-256
`22c24fc9d72c4c66a820986869786ca596efe9f0707a1404e089ed8c2636d40f`,
with the same source, child, and kernel. This is V32 failure evidence for V33,
not a Stage 03 pass or qualification of either image.

The next immutable non-claiming V33/V17 Stage 03 run
`out/m26e-qemu/stage03-v33-v17-20260814T024137Z` bound invocation/source
SHA-256
`4c186a1114d06b7fc1744ff49f7553363d49f6924fec9d4fc2a431cebfde94ec`.
Its base artifact ID was
`sha256:e3f5108fdeb3e4e190a879dd7ec63b7857cbffaf9b1a28bb08badad700ee1cf2`,
with resolved-manifest SHA-256
`aeb07bd7f8c78a2f070a156817f19d00d1789070feeb18c5cd5f9b61abc312e1`,
rootserver SHA-256
`8cdcfb9de60aa91f5e1758ee853e2835fc7c8dc94dc69b443d5c92022d64c508`,
CPIO SHA-256
`07f4a9eb864467e98aca709ac7e1bbc6851695adfa8830c841f2d49569d04d5e`,
and kernel SHA-256
`8aa06eaf462a87d3aa6c1f8500ad1d1ed5632f5ad8031c5eab53fdd2095257da`.
The fixed matrix and seven base `.coh` scripts through
`busy_backpressure.coh` passed. `cas_roundtrip.coh` then failed at line 22:
the first two 96-byte fragments were incomplete CBOR and correctly returned
`OK`; the third completed a manifest signed by fixture key
`207a067892821e25d770f1fba0c47c11ff4b813e54162ece9eb839e076231ab6`,
while the operational base verified against public key
`9d96cc26689b84db3ab037d159f040eb0375a74ec0af387337dd9323d3efede8` and correctly
returned `ERR ECHO reason=policy detail=denied ... EPERM`. The gated artifact
was built but never target-run: artifact ID
`sha256:9391ad8b68ec149909cd9364b723567d1efdd6da801d0acd1331a5bf6def84e3`,
resolved-manifest SHA-256
`d043d827ea98f57248d6838b2bff6d56f18fd4ba6fd2250545b5f76a8d66699f`,
rootserver SHA-256
`e91855a1814174ddb60d18d7f2c1f3108d92c8610a02714f609e91e25090162c`,
and CPIO SHA-256
`ebf9b4d377efd922e1c8c668530c0a7bfa221f971b53d104377ca4bfa74cdd33`,
with the same source and kernel. This is a harness trust-profile routing
failure, not a target CAS defect or Stage 03 qualification.

The subsequent immutable non-claiming V33/V17 Stage 03 run
`out/m26e-qemu/stage03-v33-v17-20260814T031936Z` bound source digest
`sha256:6e5a11b51a744d584a1d839421a047b21bd2093aa2d4840ecee2797100ca668c`.
Its base artifact ID was
`sha256:33007af1046bbf1bb4b09a60182104f5224d807e1f17c2c3ad0929f5675af238`,
with rootserver SHA-256
`7c4a8eaca8aba695a3aefd294e06379d831f5d2b8719627a08a5342a6df398ef`,
resolved-manifest SHA-256
`aeb07bd7f8c78a2f070a156817f19d00d1789070feeb18c5cd5f9b61abc312e1`,
CPIO SHA-256
`af683916fdbd2eba0bf6eca3c3e3e15613482484933ea763dcfbaf941441c938`,
and launch-record SHA-256
`0ea06f80a124dd474215e04214587236b504e65a15ea2dbdd3726d8e9883345b`.
Its gated artifact ID was
`sha256:de3a14760a8d6df84c69f24e289f0c51f54de16a4802d6ec880b9358e5055dbb`,
with rootserver SHA-256
`11460ae9449bcaa6a6ff462dc0f00ea58f454e09f101a630f9e41a46dfdb80d1`,
resolved-manifest SHA-256
`d043d827ea98f57248d6838b2bff6d56f18fd4ba6fd2250545b5f76a8d66699f`,
CPIO SHA-256
`de7bf9c197b39fc17325c2c0b36288f553d6f42e83a3ad1a89e3fb606acc7c01`,
and launch-record SHA-256
`b452eb2f1a671928899d711bf82d83c0e9506d21ffe411c195ae44eaa717fdde`.
The base fixed matrix passed 7/7, then `boot_v0.coh` and `9p_batch.coh`
passed. Immediately after the final successful QUIT audit and before
`host_absent.coh` authenticated, UART emitted
`[critical] root-emergency fail-stop`. Exact same-base-artifact GDB evidence
at `out/m26e-qemu/v33-post-9p-critical-gdb-20260814T035000Z` caught root-fault
classification at `commit_root_fault_suspend` with task index `0`, timeout
badge `0x26ee0001`, and seL4 Timeout label `5`: root-control exhausted its
temporal budget, and the emergency plus later AUTH timeouts were downstream.
No gated target workload ran after that first failed proof layer. This is V33
failure evidence for V34, not a Stage 03 pass or qualification of either image.

The next immutable non-claiming V34/V17 Stage 03 run
`out/m26e-qemu/stage03-v34-v17-20260814T040349Z` bound source digest
`sha256:9852bb0c7dfc5fb7029f84e434ee1831acac5eb6716d94d3a83223ae5d107fc6`.
Its base artifact ID was
`sha256:054abe6b2e33bb22d3ff30c18c35c1f6292c64a687d1b1308e0ca96de9ab0ef2`,
with rootserver SHA-256
`8d476189ee8f17c7d3514285082a2a9e0cab0bc7e3452b97523d8d5c4fff5ed3`,
resolved-manifest SHA-256
`df9fec5f01854a873437fe9938443ac8f67fc7a3f3778b5ad517fd3199110ae3`,
CPIO SHA-256
`e358bf089084a0fde0d7c7b76ebbaa50ad95f06b2d03fb5774a80de0332b6248`,
and launch-record SHA-256
`2b476508b62a48a693b8605c4becb16ceafe1ca683c269fceb0405e135eb8a50`.
The fixed matrix passed 7/7 and all ten base scripts passed. The fresh
base-telemetry boot then completed the expired-ticket, scope-denial, spawn,
twelve telemetry writes, wrap, and 64-line TAIL paths, but
`telemetry_ring.coh` line 56 expected `ERR ... ELIMIT` from a later CAT and
received `OK CAT`. The ticket had only a one-request-per-second `/log` scope,
the selected `131072`-byte default bandwidth, and a telemetry-only cursor
advance. A multi-turn TAIL may cross that rate window, `/log/queen.log` is not
a telemetry cursor path, and selected-QEMU routine diagnostics remain private.
This is a stale test oracle, not a target quota defect. The gated artifact ID
`sha256:b71799152fed6e06124a62933d256b22f4091557abf79c71c671168bbe2dcece`
was built but never target-run. This run remains failure evidence and cannot
qualify either image.

The later staged V34/V17 attempt
`out/test-plan/m26e-console-qemu-v34-v17-20260814T063134Z`, attempt
`20260814T074732.814568Z-34073-6859f9f8537f`, bound source digest
`sha256:599240eb411428ac7c973c8b346300ff3f9f9fa7e3b46bb2363573fbf5aa327c`.
Stage 01 passed all 21 actions and Stage 02 passed both actions. Its Stage 03
base artifact ID was
`sha256:61f24940ce81779d1372e801839e29ad5c36da805c1e58bbca8cb08762e5ca4a`,
with rootserver SHA-256
`792d3a79b475a0d103f96221a3cfe0c745bf6e26f82a9b2d84ea58dd5ec7a330`,
resolved-manifest SHA-256
`df9fec5f01854a873437fe9938443ac8f67fc7a3f3778b5ad517fd3199110ae3`,
CPIO SHA-256
`7d2d7afa5840adfde384c9e20769bd22c6a8082aec2527ef527a131806107912`,
and launch-record SHA-256
`2725b8d3fd7f51c53638fdd34c831c0f9e144bd46599038f7472d7f56f15eee5`.
Its packaged `cohsh` SHA-256 was
`689b523b32655681ae0005413c0d2d9c7e70dc39b58761d9c0728d36e3bec01d`.
The fixed matrix passed 7/7 and nine base `.coh` scripts passed through
`tcp_basic.coh`. `session_pool.coh` line 5 then failed during its ordinary
64-operation baseline, before the deliberate eight-byte short-write injection:
the peer closed, and four replacement TCP connections received no AUTH response.
No pool result, group result, later target group, root-emergency, fail-stop,
panic, or unhandled target-fault marker was recorded. The gated artifact ID
`sha256:ff567e69fef939430276775a0088921e1c1e08b530fd1ddcf30e82f08491fc85`,
with rootserver SHA-256
`9a7eb8a9d88778009bf3689049f5c30118e92056155e50ce59ee2137f5fd5c74`,
resolved-manifest SHA-256
`3c7956af210a2e4b131c853e90315ff61c8c1a3d02176af401b9c48493d836e3`,
CPIO SHA-256
`a7d3feb2b2da5b271b31a3304cf930b6ed33b8b3b6b36674ae438df8253ba235`,
and launch-record SHA-256
`1298815ff35ecc121d7e109a2a5458f97a64735a27ca354b90ccb1758f189c81`,
was built but never booted.

This first failure restores Reopened Milestone 20f task
`m20f-cohsh-tcp-pool-safety` within the discovering
`m26e-console-network-service-isolation` task. The pooled wrapper had regressed
from host-local logical lease allocation to one wire ATTACH per checkout; its
24 logical telemetry checkouts advanced the target from the script's primary
sid 11 through sid 35 before baseline ECHO progress stopped. A pooled checkout
must validate and normalize the requested ticket, reuse the wire only when the
live attached connection has the exact cached role and normalized ticket, and
then allocate a distinct host-local session ID without a wire frame. Cold,
disconnected, reset, or changed authority must still perform the existing real
ATTACH, and pooled QUIT remains a no-op. A deterministic framed-listener
regression must prove one AUTH plus one ATTACH for 24 exact-authority leases,
deterministic 25th-lease exhaustion, a later PING, a second ATTACH for changed
normalized authority, and zero pooled QUIT. Fresh same-boot Stage 03 must prove
that `session_pool.coh` emits no redundant pool ATTACH before its deliberate
short write, recovers the replacement connection, reports exact
`expected=64 observed=64` with zero failures, and completes QUIT/EOF before any
later group may qualify. A mixed old-target/current-host replay that failed
before reaching the pool workload is non-claiming and cannot close this gate.

The subsequent canonical V34/V17 staged state
`out/test-plan/m26e-console-qemu-v34-v17-poolfix-timerfix-20260814T092311Z`,
resumed Stage 03 attempt `20260814T094657.522207Z-26353-639b8740c198`,
bound source digest
`sha256:e69fb97d01a4199c77ff7f44b4a071d73e615a9d36e0f8013422545decbd0e62`.
Its base artifact ID was
`sha256:61453059dcc4f6eb91ee636e4eb3a7f52cb7ad056b90b640e0ffe7348e32eead`;
its built but unbooted gated artifact ID was
`sha256:f0b27ffef079cd5494d19e12e12797d969d1b050a55b514e668cd4c093d114a1`.
The canonical QEMU 10.1.0 resume passed the fixed response matrix 7/7 and
`boot_v0.coh`. `9p_batch.coh` then completed three writes, exact ordered CAT,
and authenticated oversize `ERR FRAME reason=invalid-length`, but line 15 QUIT
timed out with no response. The target recorded the frame error with no
root-emergency, fail-stop, panic, or unhandled fault. This is exact V17 failure
evidence for V18, not Stage 03 qualification.

The first broken transition was child-private receive retention. V17's Session
unit returned `Continuation` only after committing an outbound wire frame.
When a bounded read consumed only part of the already-buffered oversize body,
it returned `Complete`, child scheduler `service_pending` cleared, and the child
blocked even though later body fragments and QUIT remained in smoltcp's receive
queue. V18 makes every nonzero successful socket receive durable private
progress: it retains one fresh `StackIngress -> StackEgress -> Session` cycle,
while only zero received bytes plus no committed wire frame returns `Complete`.
The focused real two-stack regression
`buffered_oversize_body_and_quit_retain_service_until_same_connection_parse`
must buffer the oversize body and QUIT together, prove at least four bounded
Session reads without a fresh packet/control notification, parse QUIT on the
same authenticated connection, and quiesce on the first zero-progress cycle.
No poller, synthetic wake, retry, timer, delay, or clock-speed assumption may
substitute for retained state.

The first fresh V18 staged state
`out/test-plan/m26e-console-qemu-v34-v18-full-20260814T102424Z`, Stage 01
attempt `20260814T102527.190708Z-45453-417ba8e027f2`, bound test-plan source
identity
`4d08f51747f6a00ef04aeb858506436a91039b229f03e20f691692733bcc9ba2`.
It passed the first 17 actions through the driver-coverage contract, then
`host.python-tests` found the one stale V17 structural expression in
`tests/test_console_network_runtime_packaging.py` after 1,773 tests and 7
subtests had passed. The source guard now requires exact
`Ok(committed_wire_frame || received != 0)` ordering while preserving the
independent complete-send/commit flag transition. Its focused file passed
11/11 and the complete standalone Python gate passed 1,774 tests plus 7
subtests. This host repair is not Stage 01 qualification; restart with a fresh
state and source identity, and do not reuse the failed attempt.

The next immutable V34/V18 staged state
`out/test-plan/m26e-console-qemu-v34-v18-oraclefix-20260814T104728Z`, Stage 03
attempt `20260814T105938.465736Z-11947-27f75501fecd`, bound source digest
`sha256:f514ef3dfe5a76cd31e778a90b76e97b6cb78016a97be11b1be0b6587ed3ffab`.
Stage 01 and Stage 02 passed. Its Stage 03 base artifact ID was
`sha256:11921e2eedbf8e9c46f781c500b89acdcb9669ebda42eb6db0ed21a4eb47dac3`,
with rootserver SHA-256
`546de0283fd03b2bcfedc4876a96f5f41ea2071ce5b94aac627f4cf741638832`,
resolved-manifest SHA-256
`7d267475fa010c369cb47f60a201f77186505f05222b73bdc89645f2b0a392fc`,
CPIO SHA-256
`d44e33b75876129cdb60b8510088b48ad67d686bff2b943b13d11a286036281b`,
and launch-record SHA-256
`3cab273fd5bac9f936194090384e278e45698b032e1eda5ddd25b486fc3607bd`.
Its gated artifact ID was
`sha256:46ce91c8bffae218f557fedb19ec125cdded39118db641aee70db9e63949163b`,
with rootserver SHA-256
`f28934ee16357b1aef5ac201b1cd654097cbd9bc0d1cbb5b0dde439cf78fdfbe`,
resolved-manifest SHA-256
`8e3a9d1f2ab1b83715fbb42ee58672bcb57941ab76df98066c1bcafeded038fa`,
CPIO SHA-256
`54ee771686e8e89164c1591a0a49308c4697d05f069b59da2640881d59c86e05`,
and launch-record SHA-256
`5b8c27efeb8f2dbf4333392a93fe489cfa61f82521cda94b985fbbcc0ccd2de4`.
The fixed matrix passed 7/7. All ten base scripts passed: `boot_v0.coh`,
`9p_batch.coh`, `host_absent.coh`, `observe_watch.coh`,
`root_cut_basic.coh`, `session_lifecycle.coh`, `busy_backpressure.coh`,
`cas_fixture_signature_rejected.coh`, `tcp_basic.coh`, and
`session_pool.coh`. The fresh base-telemetry boot passed
`telemetry_ring.coh`. `telemetry_push_create.coh` then reached its expected
invalid-path CAT denial, the peer closed, and the oversized-push check failed
because each replacement connection wrote the complete 18-byte AUTH frame and
read zero bytes. The runner stopped at this first failed action; Stage 03 did
not pass and no later REST, host-tool, Python, performance, or Pi result may be
inferred.

Exact immutable replay of that base artifact proved root-control task index
`0`, timeout badge `0x26ee0001`, and seL4 Timeout label `5`; the saved PC was
immediately after the sole outer `seL4_Yield`. Retained successors identified
ordinary `Network` and selected `Timer`. Tick `356343` was not divisible by
8,000, so the timer-trace branch did not run. The adjacent refill amounts were
the exhausted current `38,090` counter ticks and the already-valid next
`93,910` ticks. Their sum is the unchanged configured `132,000` ticks, or
`5,500 us` at generated QEMU `24,000,000 Hz`. The installed terminal timeout
endpoint converted exhaustion of only the current refill, despite the valid
adjacent refill, into root-fault classification and downstream
`root-emergency` fail-stop. It does not falsify V18, timer arithmetic, the
console grammar, or a host surface.

Under active discovery task `m26e-console-network-service-isolation` and the
reopened Milestone 25 root-service temporal-restoration authority carried by
`m26e-root-tcb-target-proof`, selected QEMU root-control advances from
historical V34 to
`m26e-qemu-root-adjacent-refill-natural-postpone-candidate-v35`; selected Pi
root-control advances from historical V23 to
`m26e-pi4-root-adjacent-refill-natural-postpone-candidate-v24`. Both select
`NaturalPostpone`, retain their timeout cap/badge/resource/registry identity,
omit the TCB timeout endpoint, and keep the standard fault endpoint terminal.
Every numeric, placement, resource, and the distinct QEMU 24 MHz/Pi 54 MHz
clocks remain unchanged. Common child provenance remains V18. The repair
changes no schema, API, wire frame, namespace, command or ACK/ERR/END contract,
workload, retry, timeout, host-tool implementation, Python-library contract,
benchmark implementation, evidence record, or report schema.

Stop at its first failed target layer; run
the focused deterministic regressions below only after the target canary
reaches the changed live path. For the shortest target-first integration path,
then run the focused direct base `.coh` batch, Hive Gateway REST core/parity
batches plus the Python smoke, and Conditional B2's exact-three executable QEMU
pressure. Run Conditional D's no-retry `telemetry-1mb`, `telemetry-10mb`,
`telemetry-100mb`, and `telemetry-1gb` matrix separately against the exact
packaged gateway's explicit host-model backend. Broad host closure and complete
staged acceptance follow only after convergence passes.

The strict four-scenario Conditional D diagnostic retained at
`out/bench/conditional-d-strict-rolling-endpoint-final-20260815` kept
`strict_control_errors=true`, `no_retries=true`, `backend_class=host-model`,
`proof_class=host-model`, and observed the configured `120/120` endpoint.
`telemetry-1mb` recorded `9,730/9,687/43` attempted/success/error operations at
rate `0.004419321685508736`, summary SHA-256
`e81ea8d40043d4b5f3f9462e8a44d80242c64572f64360cf2a00bb557e240929`;
`telemetry-10mb` recorded `9,924/9,854/70` at `0.007053607416364369`,
SHA-256
`a690509e030adf62b9c2881b36f77caa77aa99f1cb6a53e52e8831a1532528f2`;
`telemetry-100mb` recorded `3,235/3,235/0`, SHA-256
`63e2e69cf594988de7a410fe51e2a20bc891fb8f8caf81b770d4e6c836021186`;
and `telemetry-1gb` recorded `528/528/0`, SHA-256
`ae269cf8e3102a4e09decf46854182085bdbd635f725a988c31819dcd92bb345`.
The first two runs admitted exactly 256 schedule entries, then classified all
43 and 70 semantic-capacity failures as generic HTTP 503 `other_errors`, with
`buffer_full_errors=0`. Their aggregate error budgets passed, although the
first interval-budget crossings were `0.02821522309711286` at the final
`telemetry-1mb` interval and `0.01749271137026239` at `telemetry-10mb` step 13.
Aggregate `report.reliability.error_budget_pass` remains the scenario verdict
and interval crossings remain diagnostics, but the required lossless
bounded-refusal classification did not pass. The matrix is diagnostic and
non-qualifying. The locally present release gateway was SHA-256
`e4a1a57ad868536b25ad17d9bfdabed7a7dd53b1a2d6d733515ca34835d56188`,
but the summaries contain no source, Git, or gateway-binary identity and cannot
establish exact-source Conditional D provenance.

Restore only the exact in-process `NineDoorError::Protocol { code: TooBig,
... }` schedule, lease, and export capacity cases to the canonical
`ERR ECHO reason=quota detail=buffer-full path=<path> error=buffer full`.
Retain the typed source error and leave unmatched paths, messages, and codes
generic. Hive Gateway preserves the semantic refusal as HTTP `200` with
`GatewayResponse.status=ERR`. Conditional D's joint strict-plus-no-retries
handling counts each returned refusal once and performs no harness retry; the
gateway's independent bounded retry window, cooldown, and counters remain
unchanged.

The following staged attempt retained at
`out/test-plan/m26e-console-qemu-v35-v18-strict-perf-final-20260814T214011Z/evidence/attempts/stage-01/20260814T214030.244940Z-23431-989e20fcd8a5`
bound source digest
`6fa1cb471dc3191c8b035ddda2cfc7842e0fb77298a0a66221f6474ec095bf25`
and context digest
`89edf724bc379cc2406a75040e597113c972a233698fd523318cc25eb0892076`.
Actions 1–20 passed; action 21, `host.rust-risk-ratchet`, stopped with exit
`130`. Its action-record SHA-256 is
`a5c6fcb017aee6dd8fc491265c0b5c59014e7fb42682aa5e50015e6efdc8b5bd`,
action-log-slice SHA-256
`c1e9a2f3ef002979417cab98044f949773d9b7bae78028cadaaa37dfa11a680f`,
failed Stage 01 record SHA-256
`a04d82aa5116c553cee929f47e5dc59f26e5beed2605729f692cd64c0bcc1c45`,
and complete stage-log SHA-256
`b6f6512724d754d683f8b78c86299941b740f4b74565895eb15ed4b64210a551`.
Stage 01 emitted no pass attestation; Stages 02–05 and exact-source
Conditional D did not run. Nothing from this state may be reused as staged or
Conditional D PASS evidence.

Compatibility review for V35/root-fault-V6/child-V18, Pi V24/V18, and bounded NineDoor containment covers
the complete host-tool suite, all `.coh` scripts, `tools/cohesix-py`,
`m26e_qemu_pressure.sh`, and `rest_perf_harness.py`. Those target repairs change no
external verb, line frame, ACK/ERR/END state, namespace or authority path,
client timeout, quota, Python contract, benchmark workload, retry policy,
evidence record, or report schema, so those public implementations, workloads,
and report schemas require no contract change. The separate Stage 04 repair
changes only the host filesystem-operation response window according to the
composed gateway deadlines documented above; it leaves every named external
contract, Python public default, benchmark workload, retry policy, and report
schema unchanged. The reopened M20f repair changes
only `cohsh` pooled connection reuse and must be exercised through Hive Gateway;
all other host-tool, Python, and performance implementations retain their
existing contracts and still require exact-version execution after full staged
target qualification. The pool repair introduces no target timer read, delay,
spin, or retry. V18 changes only child-private progress retention and likewise
adds no timer, wake, delay, retry, or clock-derived transition. QEMU's generated
24 MHz timer and Pi 4's generated 54 MHz timer remain separate unchanged truths;
the common child provenance remains selected on both targets. Fresh exact V35
QEMU must pass the complete staged path, fixed matrix, full `.coh` regression
harness, standard-fault terminal and root budget-exhaustion postponement
injections, REST core/parity and Python smoke, every host tool, the Python
library, Conditional B2 exact-three executable target pressure, and the
companion Conditional D host-model gateway matrix. QEMU cannot qualify Pi;
fresh V24 Pi acceptance separately requires a generated 54 MHz build, exact
flash/readback and cold boot, applicable staged/hardware proof, both fault
injections, host-tool/Python compatibility, and a Pi-selected target-performance
path. Conditional D may establish companion host-model gateway behavior but
cannot supply fresh-Pi target evidence.
`telemetry_ring.coh` must prove target `ELIMIT` before its operational workload
with a bootstrap-signed Queen ticket that grants rate-unlimited `/log` read
scope and explicitly caps `bandwidth_bytes=8`. After the existing write-scope
`EPERM`, one `cat /log/queen.log` must receive exact quota `ELIMIT`; the script
then detaches and uses the unchanged operational ticket for spawn, twelve
90-byte writes, wrap, TAIL, and the existing final telemetry-read refusal. It
must not derive a quota result from a second read completing within one
wall-clock rate window or from incidental public-log volume. The cohsh
structural regression must verify
the fixture MAC, decoded role/scopes/quota, early EPERM/ELIMIT order, absence of
a later log CAT, and exact token-stream hash. This changes only script authority
and coverage; target code, manifests, generated artifacts, quota semantics,
grammar, timeout, retry, and workload payload remain unchanged.
V12 changed only the child-internal peer-close lifecycle. V13 adds only the
completed active-close `TimeWait` terminal transition and provenance; it adds
no host timeout or retry and changes no REST or performance surface.
V26 changed only the root-internal successful Disconnect transaction from
repeatable to per-connection single-issue. V14 added internal binary SendBatch;
V15 admitted its observed 60-page ELF footprint; V16 changes only the
manifest-owned console timeout policy, local-work cadence, and derived temporal
inputs. V17 corrects only authenticated oversize rejection, exact declared-body
drain, and same-connection framing continuation. V18 retains the same framing
contract while keeping one fresh private service cycle after every nonzero
bounded socket receive. V27 adds the bounded
authenticated response lane and ordinary-debt turn, V28 adds the root-local
terminal-response Disconnect fence, V29 adds root-local capture, sealing, and
bounded later flushing for HELP, NETSTATS, SMP, and CACHELOG, and V30 delegates
the four response hooks plus `console_event_pending` through `DefaultNetStack`.
CACHELOG uses
one immutable newest-first snapshot taken under one bounded lock acquisition
and rendered after releasing the lock. No
declared retry, client timeout, grammar, evidence, workload, or report contract changes.
V31 additionally retires only a refused ordinary stream producer. V32 changes
only the internal delivery path of routine command, session, TAIL, and
NineDoor audit diagnostics after bounded logging or the linked serial owner is
active: their existing text is appended to the bounded log rather than emitted
per byte on the synchronous debug UART. Pre-handoff routine audit and
critical/fatal call sites retain the existing diagnostic helper; V32 does not
change those sites. The V32 target run disproved that route because those
records contaminated public `/log/queen.log`. V33 replaces it only on the
selected QEMU VirtIO composition with an EventPump-private capacity-four FIFO.
Saturation or an oversized record drops the new best-effort diagnostic while
retaining older FIFO order. The final-idle `RoutineAudit` Operator unit attempts
one nonblocking serial admission and removes the head only after success;
response/input/stream/flush, retained-output, containment, network, and display
work all take precedence. Once admitted, a QEMU-only `SerialPort` provenance
flag identifies the complete staged TX backlog as audit-only. Ordinary
`SerialDispatch` skips retrying that exact backlog while the UART would block,
so late `NetEvent` and `NetLine` work stays live; a later final-idle
`RoutineAudit` may retry it. Admission of any nonempty ordinary serial record
or bytes promotes the backlog to ordinary dispatch priority without changing
FIFO or exact `\r\n` byte order. The FIFO never enters public
`/log/queen.log`. Compile-time `net-backend-virtio` selection excludes its
type, storage, provenance flag, and unit from Pi, while linked-runtime, legacy/non-VirtIO, ordinary
console-failure, critical/fatal, and fail-stop paths retain their prior raw
diagnostic route. This requires no `coh`, `cohsh`, `.coh`, SwarmUI, Hive
Gateway, `tools/cohesix-py`, benchmark workload, evidence-record, or report-schema
change; the unchanged timeout, retry, command grammar, and response ordering
remain target gates.
This reviewed set includes V8's stale-generation disposition, V11's explicit
Observe-to-ACK publication credit, root unauthenticated-output retention, the
all-ready IPC fast gate, and the ATTACH
logger transition. Gateway cold readiness changes already present remain
compatible. The fixed functional response matrix remains a required target
execution; unchanged host schemas cannot substitute for it.
V34 additionally narrows only selected-QEMU physical routine-audit TX to one
byte per eligible final-idle Operator visit. The complete record and its
audit-only provenance tag remain retained across visits; every response,
input, retained-output, containment, network, display, and ordinary serial
priority remains unchanged. The NineDoor cursor changes only target recovery
scheduling. V34 preserves V33/V32/V31/V30/V29's capture contract, V28's terminal fence, V27's response cadence, V25's temporal envelope, V24's
ACK split, and V23's separate Timer and Nic
visits plus the exclusive NETDIAG visit and makes
publication ACK a distinct highest-priority QEMU Network unit. Successful
predispatch housekeeping returns before phase selection; bounded backpressure
may run exactly one Operator unit without ordinary-phase advance. The NIC still
runs once per three featured Network visits; the
unchanged performance workload must therefore be rerun rather than assumed equivalent. Their
compiler-owned default/QEMU/Pi projections must still regenerate exactly; any
unexplained drift blocks the target canary.

The HVF profile repair changes the internal target execution envelope but no
external protocol or report schema. `coh`, `cohsh`, `.coh` scripts, SwarmUI,
Hive Gateway, and `tools/cohesix-py` therefore require compatibility execution,
not grammar changes. QEMU artifact, pressure, convergence, release-runner, and
REST-performance launchers must all select the same HVF/off/HVC/24 MHz profile;
any TCG or 62.5 MHz QEMU record is a comparator only. After AUTH, the fastest
ordered gate is direct `boot_v0.coh`, direct `9p_batch.coh`, one gateway-owned
REST core batch, REST parity batch, Python SDK smoke, then serial no-retry
`telemetry-1mb`, `10mb`, `100mb`, and `1gb`. Do not run direct TCP `cohsh`
concurrently with the gateway, and reuse an existing QEMU/gateway in the
performance harness only with both `--no-qemu` and `--no-gateway`.

The intervening v14 artifact set at
`out/cohesix-v14-qemu-20260812T021853Z` bound root ELF SHA-256
`4265ee26a8a23b38851167aa046f4adce50764715131d044059b4e08211b9361`,
system CPIO
`85a1e211f5cb83ad8ace277d0a4cfe89c22317ffa148e0aeae64611f1bd315d6`,
kernel
`865b5a0614f1633ca636800705f97339e78f47065fdaffd2cb4139e4a25630c0`,
driver archive
`88a3a9f1df93cb560501ac13275efb20b52985db8878b54372e29a397539474d`,
and driver manifest
`ef168c902062ff1c9f08208bc1eadf92773991ee2ce297a28da5ee26e2cfa385`.
It failed before authentication: `root-worker-supervisor` first exhausted its
complete `750 us` refill at entry PC `0x1143dc`, timeout badge `0x26ee0004`;
root-fault received that record, then `root-driver-supervisor` exhausted its
complete `1000 us` refill at entry PC `0x113d98`, current timeout badge
`0x26ee0005`. Both generated rows select core 1. Compiler and kernel source
inspection confirms the core-specific SchedControl ConfigureFlags operation
precedes SC binding and the MCS bind migrates each TCB to the SC core; a direct
`TCB.SetAffinity` call is neither required nor valid evidence under MCS. Treat
this only as cold first-activation failure evidence for the v14 supervisor
  envelopes. The following v15 image proved both QEMU supervisors reached their
  healthy blocking waits; it does not qualify Pi, whose supervisor values remain
  unchanged pending its required image-bound checkpoint.

The following v15 non-claiming convergence run bound dirty source HEAD
`84934dda6`, root ELF SHA-256
`6c145a1d81bd57e791781a052f62dfc6dd5d34c7c7ca0aa4e3311a9b5696018c`,
system CPIO SHA-256
`07b84ff5dc2a40e2b9039d49b1e37bb88824909fe2fd902c9dd0165b4a643529`,
and resolved-manifest SHA-256
`46f3264e862944b84188064941bd581e60a78d80d9a7590dfe4b42fcfa3e7482`.
Root-control consumed exactly `2750 us` and raised timeout badge
`0x26ee0001` at the outer `seL4_Yield` after the second retained-output
Operator emitted and cleared the initial prompt. Saved successors `Runtime`
and `ControlEndpoint` prove the prior Runtime selected `Worker`; the output
cursor and FIFO were clear. The entry-time TX snapshot nevertheless kept the
completed-output Operator on the generic Runtime tail. Root-fault then consumed
exactly `3000 us` and raised `0x26ee0002` at the return from the
`SignalEmergency` send. Root-emergency received that signal, emitted its
fail-stop line, and remained healthy in its terminal yield. Both v15
supervisors were healthy at `seL4_Wait`, so their cold-activation repair reached
user entry. Treat this only as failure evidence for root-control composition;
the v16 root provenance recorded the QEMU-only exclusive retained-output cut.
The later exact v16 failure above showed that cut still entered the generic
EventPump and Operator frames. All timing, schema, child v5, supervisor v15,
capability, ABI, and authority values remain unchanged. Fresh v20/v4 QEMU
authentication and standard/timeout injection remain required before Stage 03
or pressure.

The ordinary EventPump regression must prove the QEMU-only cyclic order
Operator/Dispatch -> Runtime/IPC -> Network -> Operator/Dispatch, exclusive
per-phase ownership, successor commit before every early return, and the sole
recurring outer `seL4_Yield` between phases. The retained v17 dispatcher
regression must
first prove the public EventPump entry selects the exact isolated VirtIO path
before the generic EventPump frame can be allocated. Its tiny noinline phase
dispatcher must never call `poll_generic`, `poll_ordinary_operator_turn`, or a
generic Runtime body. Operator begins the existing shared `64`-byte serial and
one-record output credits, performs the bounded serial probe, and admits at
most one eligible material noinline leaf in this exact strict-priority order:
SerialIo/SerialDispatch, LocalSeat, response-ordered RetainedOutput, NetEvent,
NetLine, background/high-impact RetainedOutput, then DisplayAttach. The v18
serial regression must prove `SerialIo` performs exactly one bounded RX-only
probe, never flushes TX or dispatches input, suppresses the raw-UART RX trace
only while the admitted ordinary root-control turn is active, and retains `SerialDispatch` when
entry TX or admitted input is present. A later Operator must select that
retained `SerialDispatch` before a new probe, commit `RetainedOutput`, perform
bounded serial consume/echo plus TX flush, and return without another material
leaf. An idle RX-only probe restores `SerialIo` eligibility before continuing
the unchanged priority selector. Each
material leaf must commit its recorded successor before work. An idle compact
Operator must return directly; Pi, linked-runtime, physical-owner, and other
non-VirtIO counterfactuals must enter the unchanged generic path. Target
disassembly must show that the selected compact call chain cannot reach the
generic v16 EventPump or Operator frames. The QEMU Runtime regression must
prove one persistent Worker -> ControlEndpoint -> BootstrapDrain -> StreamFlush
-> RebootTail cursor and exactly one selected unit per Runtime visit, including
an idle/no-op. Worker must consume one pending mailbox operation or check one
retained Heartbeat/GPU/LoRA role slot; ControlEndpoint must perform at most one
poll and its immediate forward; BootstrapDrain must take one staged `Option`;
RebootTail must own its visit. No visit may search another Runtime unit for
work. The successor commits before the compact isolated-VirtIO Runtime prelude,
and Recovery preserves it. That compact prelude must read the HAL timebase and
perform one timer poll. An observed tick must update `now_ms`, increment the
timer metric, publish HAL timebase, and run the existing conditional timer
trace; without a tick, `now_ms` must take the read timebase. It must then
reconcile CYW43 network-ready HDMI state and must not execute the generic
Runtime-without-control tail. The MCS fault-endpoint poll must be absent from the
cursor. StreamFlush must use one visit per earlier retained
line, one visit for the retained final line, a later selected no-line visit for
cursor/bandwidth finalization only, and the following selected visit for END
only. Tests must reject a line plus finalization, finalization plus END, or two
line emissions in one visit. Legacy Pi/non-VirtIO Runtime must retain its
48-line/16-KiB behavior. Every isolated Network visit must attempt
exactly one internal unit, including a no-op. The persistent lower cursor must
cycle ObserveChild -> StageOutput -> Disconnect -> Ingress -> ServiceTick ->
ObserveChild. A no-op advances the cursor; any successful lower unit that
signals the child forces the next lower attempt to ObserveChild. A pending
compact normal-success diagnostic must emit first, at most one record on a
non-publish visit, and return. Otherwise retained TX preempts the lower cursor,
performs no more than two bounded reclaim checks, attempts exactly one TX, and
returns on success or backpressure. Both preemptions must preserve the lower
cursor and forced-observe state. The gate must queue normal-success records for
successful attempt sequences 0 through 63 and every 64th eligible success
thereafter, never every post-window TX, while counters remain continuous. An
ObserveChild attempt may copy and retain active output/event data but must
return without TX. Tests must prove Observe -> TX -> DeferredDiagnostic before
resuming the preserved lower state for a sampled success, with no second
publication able to overwrite or merge the pending diagnostic. Anomalies must
remain immediate. Tests must prove TX descriptor
initialization, avail publication, optional notify, and in-flight identity commit
are atomic; there is no post-publication buffer write, completion wait, duplicate
publication, or head reuse before bounded reclaim.
The v23 compact-predispatch behavior guard must prove that TailInFlight invokes
one response-barrier reconciliation and returns immediately if the barrier
clears; if it remains in flight, exactly one compact Operator unit runs and the
turn returns. An eligible prompt likewise invokes one queue attempt and returns
if `stream_prompt_pending` clears; bounded queue backpressure permits exactly
one compact Operator unit before return. Both outcomes preserve the ordinary
phase plus Runtime and Network cursors; only a fallback Operator subcursor may
advance. Ready reboot remains an exclusive pre-phase return. Only when none of
those duties applies may the dispatcher read and commit the ordinary phase and
run one phase leaf. Generic and Pi behavior and adjacent helper ordering remain
unchanged.
The v21 Network source guard must additionally prove that, when no retained
postlude exists, `poll_split_ordinary_virtio_network_turn` reads
`OrdinaryVirtioNetworkUnit::{Timer, Nic}`, commits `unit.next()` before work,
and executes only that selected unit. Timer must call exactly
`poll_runtime_timer_prelude`; Nic must call exactly one
`poll_one_split_ordinary_virtio_network_unit`, retain one compact observation
containing telemetry plus originating `now_ms` and last-RX-progress horizon,
and return. The former composite Network-prelude helper must be absent. Neither
unit may enter `poll_runtime_inner`, command dispatch, the generic Runtime tail,
or `reconcile_cyw43_network_ready_hdmi`; the distinct split Runtime prelude
retains timer/timebase update plus CYW43-ready HDMI reconciliation. A retained
observation must be taken before the Timer/Nic cursor is read or advanced;
counters are sampled after the intervening exact compact Operator and Runtime
visits, only NETDIAG runs, and the visit returns without timer or NIC work.
Immediate flush accounting, connection identity, and NineDoor ingest
accounting remain in the Nic visit unchanged. Quarantine clears the retained
observation but preserves the Timer/Nic cursor. Generic/Pi Runtime and Network
behavior remains unchanged. `select_isolated_network_turn` must commit the ordinary lower
successor before dispatch to one distinct noinline
`poll_{deferred_diagnostic,transmit_egress,observe_child,stage_output,disconnect,ingress,service_tick}_unit`
adapter; successful child signal may then force the retained cursor back to
ObserveChild. ObserveChild must not compile through one closure containing all
seven unit bodies. Console lifecycle-event admission moves to Operator and is
bounded to at most one event per Operator visit; Network may retain the event
but may neither drain it nor dispatch policy in the same visit.

The Operator serial regression must instantiate exactly one generated `64`-byte
I/O credit at isolated VirtIO Operator entry and share it across every
root-context serial RX poll and TX flush. Repeated helper calls must not reset
credit; each accepted or emitted byte debits the same total, and exhaustion
retains remaining bytes for a later Operator. When TX backlog exists at entry,
the test must prove an exact `32`-byte TX reservation and at most `32` bytes of
RX service. With no entry backlog, RX may consume all `64`. Counterfactuals
must prove that sustained RX cannot suppress pending ACK/ERR/END output, total
service never exceeds `64`, and the physical/linked serial driver's independent
`max_bytes=1024` contract and all non-VirtIO/Pi turns are unchanged.
When the generated VirtIO serial limit is nonzero, the same regression must
prove that exactly zero or one retained output record is attempted in an
Operator, a second FIFO or response-tail record remains queued and ordered for
the next Operator, and helper re-entry cannot admit another record. The
zero-bound Pi/non-VirtIO regression must preserve the existing two-record
attempt limit and its prior phasing.

The v12 isolated-QEMU Operator regression must require both the split VirtIO
path and a nonzero `OrdinaryVirtioConsoleOutputTurn` selector. After the first
bounded serial poll, local-seat priority consume, serial consume/flush, and one
buffered authenticated-line dispatch attempt, it must build the private pure
`OrdinaryVirtioOperatorWork` snapshot with exactly these fields:
`serial_input`, `serial_output`, `local_seat_input`,
`dispatchable_network_line`, `pending_console_output`, `physical_response`,
`stream_or_tail`, `reboot`, `serviceable_display`, `serviceable_frontier`, and
`serviceable_attach`. When `is_empty()` is true, the turn must return before
`poll_runtime_without_control_tail` and all repeated serial, local-seat,
output, display, attach, and frontier probes. A real value in any field must
prevent the cut. Raw post-prompt/frontier flags without an attached
`LocalSeatRuntime`, quarantine/terminal status itself, timer/Runtime/Network
work, and global atomic or HAL hints must not manufacture work. Zero-selector,
Pi, and non-VirtIO counterfactuals must retain their existing behavior, and the
ordinary and Runtime successors must remain unchanged.

The V18 active-child regression must prove retained-first priority: completion
publication, service-event publication, egress publication, service-poll
continuation, new ingress, then new control. Every unit must commit fully or
remain pending, and the badge/publication-credit gates must be rechecked before
the next unit. `seL4_Poll` is legal only for exact eligible internal
`PollService`, `IngestPacket`, or `ApplyControl` work, which mutates only private
state. Idle or publication-uncredited work must call blocking `seL4_Wait`
directly with no ordinary `seL4_Yield`; internal units preserve credit and one Publish consumes it before mutation.
Ready has no credit, and ordinary wakes never grant one. A
stable empty packet/control page must clear that exact hint before retaining
one three-unit service cycle, then reach idle without a self-Poll loop. Tests
must prove completion plus service-event and event plus egress cannot overwrite
either one-slot page, retained service survives publication preemption, and
root ACK debt is set only after every indicated record is valid and copied,
then cleared before post-retention Release+Signal, which forces ObserveChild.
Revoke parks without publication; shutdown waits for credit, publishes its
terminal record, retires debt without ACK, and starts bounded containment.
Tests must reject an ordinary child Yield, Poll when the exact local gate is
false, publication without credit, ordinary-wake credit, duplicate/late ACK,
an installed console timeout handler, or numeric drift.
V18 framing tests must preserve V17's pre-authenticated fail-closed rule while
proving both authenticated oversize classes: a payload above the 2304-byte
command bound and a declared payload above the child frame buffer. Each queues
exactly one `ERR FRAME reason=invalid-length`, emits no rejected-auth event,
retains the authenticated connection, and drains exactly the declared payload
across arbitrary fragments before accepting a following frame from the same
ingest buffer or a later read. The discarded bytes must never be interpreted
as frame prefixes or commands, and neither case may allocate from the declared
peer length.
Within the retained `ChildTurnUnit::PollService`, the v11 child regression must
prove a private `ServicePollUnit::StackIngress ->
ServicePollUnit::StackEgress -> ServicePollUnit::Session` cursor and public
`ServicePollOutcome::{Continuation, Complete}`. The successor must commit
before the selected work. `StackIngress` performs exactly one
`Interface::poll_ingress_single` call and returns `Continuation`;
`StackEgress` performs exactly one `Interface::poll_egress` call and returns
`Continuation`. The kernel retains `scheduler.service_pending`, rechecks the
gates, and uses the eligible local-Poll path when no publication preempts it, then later
dispatches the successor. `Session`
owns connection/session RX, tick, TX, close, and relisten work. A nonzero
bounded receive or complete wire-frame commit returns `Continuation` and
retains a fresh three-unit cycle; only zero receive plus no commit returns
`Complete`, and only that `Complete` clears `service_pending`. The focused
`buffered_oversize_body_and_quit_retain_service_until_same_connection_parse`
real two-stack regression must buffer the oversize body and following QUIT
together, span at least four Session reads without a fresh root notification,
parse QUIT on the same authenticated connection, then quiesce on the first
zero-progress cycle. Errors must not call scheduler completion. Retained
completion, event, or egress publication may
preempt either pending stack successor without losing it, and coalesced badges
remain hints rather than permission to bypass the gates.
Session output must require both `socket.can_send()` and complete-frame
capacity before `send_slice`. A `FinWait2` socket with free capacity must
retain output without commit, `OutputDrained`, or error; after peer close the
existing end path must publish one `Disconnected`, clear the ended connection,
and relisten. Other Session errors remain terminal.

The V12 lifecycle guard is separate from those V11 scheduling and historical
V7 sendability invariants. When peer FIN produces `CloseWait`, Session must set
the existing idempotent close-after-flush intent, retain exact-generation
output, and reuse `close_ready` plus the existing `Closed`/`end`/`listen` path.
It must emit one `Disconnected` and restore the sole listener without adding a
unit, cursor, wake, timeout, retry, or root-side disconnect command.

The V13 lifecycle guard is distinct from V12's peer-FIN `CloseWait` repair.
After a server-active close reaches smoltcp `TimeWait`, Session must call
`TransportSession::end` exactly once before aborting the completed TCP control
block and restoring LISTEN in that same bounded Session unit. A real two-stack
test must authenticate replacement connection 2 immediately and prove that an
incoming SYN cannot re-arm smoltcp's `10 s` close delay indefinitely. The old
generation must publish one `Disconnected` and clear its auth/output state;
the existing `Closed` path and all retry/timeout policies remain unchanged.

The V26 root lifecycle guard is distinct from V13's child `TimeWait` repair.
After one successful root Disconnect publication, neither `ControlCompleted`
nor `OutputDrained` may reopen that semantic transaction for the same
connection. Backpressure must leave the issued latch clear; success must set it
without clearing the requested Quit reason. The following lower-cursor pass
must skip Disconnect and reach Ingress then ServiceTick. `Connected`,
`Disconnected`, no-active, graceful terminal, and fail-closed transitions must
clear the latch at their existing generation boundaries.

Control-lifecycle tests must independently prove that the child reader returns
the committed nonzero `connection_id` with sequence/kind/payload, validates
kind and payload before stale disposition, and calls application with that
identity. For connection 1, exact pre-authentication `SendLine` remains
`Unauthenticated`; identity zero and malformed stale records remain
`ConsoleFrame`. After connection 1 ends, its well-formed retained `SendLine`
must return `StaleConnection`, enqueue zero bytes, and leave output empty. After
authenticated connection 2 begins, the same connection-1 control must neither
enter, clear, nor reorder connection-2 output, while connection-2 `SendLine`
returns `Applied`. Kernel/source coverage must prove both outcomes advance the
exact accepted control sequence and queue `ControlCompleted`; only stale sets
the same sequence as already drained, so it cannot later mint
`OutputDrained`. Root-boundary coverage must prove `Disconnected` does not
release or overwrite the one-slot in-flight control and only its exact
`ControlCompleted` does. The isolated root adapter must return `false` before
normalization or output-queue mutation when no authenticated connection exists,
retaining the pending stream cursor.

ATTACH coverage must prove target namespace preparation precedes NineDoor's
local role/ticket plus attached commit; that commit precedes audit, logger, and
tracer observation; root session authority and `OK ATTACH` occur only after the
bridge succeeds; and post-commit diagnostic failure cannot roll back the local
namespace context. Logger tests must prove bridge attachment selects
UART+EP mirroring without changing the ping token and that only a later
explicit promotion request can run the ping/ack self-test and select EP-only.
No test may weaken the namespace prepare gate or delete UART/EP functionality.

IPC source coverage must prove the wrapper reads endpoint ready, endpoint
validated, send unlocked, and post-commit unlocked before its all-ready return;
the fast section must contain no bootstrap counter increment, tracer snapshot,
formatting, allocation, lock, or UART emission. The separately cold outlined
path must retain the bounded trace/snapshot and diagnose the exact caller-read
values. This is a steady-state cost guard, not permission to bypass any failed
readiness bit.

Before each selected phase, the regression must prove
console-network mailbox precedence and permit a NineDoor probe only when the
console probe reports no work. If either containment owner consumes or attempts
work, the regression must classify the complete refill as one exclusive
Recovery turn, advance at most one material containment unit in fixed
owner-local order, persist the successor, execute no EventPump phase, preserve
the selected ordinary phase plus retained Runtime- and Network-unit states, and reach the same
sole outer yield. Console work must retain precedence until its sequence is
complete; only then may simultaneous NineDoor work advance. No Recovery turn may
fall through to the pump, and no new authority, SC, budget, refill, or internal
yield is permitted. It must also prove
that an attached VirtIO
contract suppresses the Pi/GENET-only synchronous
`SERIAL_INPUT_TRACE stage=idle` path before and after console-network
quarantine. Quarantine must retain all three exclusive phases, with Network
observing quarantine, preserving both Runtime and Network retained unit states,
and fencing NIC work
rather than polling or combining Operator and Runtime/IPC. An attached GENET
contract must retain the existing trace cadence,
and non-VirtIO/Pi phase behavior must remain unchanged. A complete idle line
immediately before a root-control timeout at the outer yield is a failed QEMU
phase, not qualification evidence.
The source-order regression must additionally prove that the bounded
synchronous bootstrap IPC trace completes after registry seal and before any
restricted child activation. It must reject root-control temporal activation
from kernel bootstrap: the one HAL transition is guarded in userland and may be
called only at the serial console, deferred-network supervisor, or non-serial
pump event-loop seam. After the successful MCS bind and timeout-endpoint setup,
the regression must prove exactly one universal activation-seam `seL4_Yield`
occurs before either containment mailbox probe or the first EventPump phase,
with no pre-arm retained-output drain and no other post-bind work interposed.
That yield must preserve Operator as the first ordinary phase and leave the
queued startup marker and prompt ordered for steady service. The existing one
outer yield per Recovery or ordinary phase remains the sole recurring boundary;
the activation yield occurs once per boot on both QEMU and Pi and adds one Pi
startup-period wait without changing Pi phase semantics.

Handoff tests must cover an eight-record root-control queue, one Worker fault
mailbox per admitted Worker, one owner mailbox per generated isolated service,
and the generated number of driver fault records (zero on QEMU, seven on Pi).
Root-control saturation refuses new work without blocking; Worker, service, or
driver fault saturation is fatal and never drops a containment record. Worker
and service mailboxes use the generated temporal-task ordinal, never a
role-local slot. Heartbeat, GPU, and LoRA intentionally share ABI slot zero. A coalesced
Worker wake may contain the root handoff bit and any
combination of the three generated child-completion bits. The supervisor must
reject every other bit, drain durable child records, and drain Worker faults
before Worker control records. Fault-path producers and consumers must remain
bounded and nonblocking. Root-fault suspends an isolated service before
publishing its durable record; only that service's root-control owner may take
the record and perform its typed caller-failure and revoke sequence.

The registry is exact and profile-qualified: QEMU seals 10 live sources (5
critical + NineDoor + console-network + 3 Workers), while Pi seals 17 by adding
its 7 live drivers. Reject duplicate task indices or TCBs, aliased
standard/timeout badges, zero generation components, overflow, underfill, and
registration after seal. A contained Worker generation may replace its sealed
entry in place only for the same task/badge pair, exact prior identity, nonzero
TCB, and strictly newer supervisor and capability generations; stale or
wrong-task replacement fails. Construction and registration errors are
boot-fatal; a source may not be inferred from a badge range at receive time.

Fault-receive tests must prove the acyclic graph and exact one-endpoint,
one-Reply cardinality. Standard and timeout send caps carry disjoint
exact-identity badges but target the same root-fault endpoint. Root-fault owns
the sole Read cap, supplies its one Reply object to blocking `seL4_Recv`, and
resolves the fault class only after the sealed registry accepts the nonzero
badge. There is no empty-receive polling path: tests must prove an idle receiver
blocks without consuming its full budget, while every compiler-admitted
standard and timeout badge is accepted. Fault send caps are Write + GrantReply,
receives are Read-only, and supervisor signals/waits are
Write-only/Read-only. Ordinary Worker faults are suspended and handed off
without Reply. Driver faults retain the single fault association while the
independent driver supervisor returns at most one command failure,
suspends/unbinds the TCB, and revokes the old generation; root-fault blocks on
its existing Read-only wake cap and may receive again only after the
supervisor's exact generated release-badge signal. Tests must reject an early,
wrong, aliased, or duplicate release and any attempt to reuse the Reply while
associated. Only an explicitly generated `replenish-once` timeout may reply
once; the current allowlist is empty. Root-fault faults route to
root-emergency, and root-emergency has no recovery edge.
For terminal critical-domain and service faults, the source and behavior guard
must prove private `FaultReplyDisposition::CriticalTerminal { task_index }` and
the Copy cursor `RootFaultCriticalTurn::{PrimeReceive, Receive, Classify,
ResolveService, SuspendService, RecoverPassiveService, PublishService,
SuspendCritical, SignalEmergency}`. The atomic default must map to
`PrimeReceive`. That one-time turn must commit Receive before exactly one Yield
and contain no receive, Reply-cap use, copied fault value, classification,
TCB-cap lookup, suspend, or emergency signal. `Receive` must commit `Classify` before the
blocking receive, copy only label and badge into the value-only pending record,
and yield without classification or TCB-cap lookup. A fresh `Classify` turn
must consume that record exactly once. `Released` must yield before another
Receive; `RetainedByDriver` must wait for and validate the exact release badge
and cleared busy state, then yield; `CriticalTerminal` must commit
`SuspendCritical` and yield before cap lookup or suspension. A fresh
`SuspendCritical` turn must commit `SignalEmergency` before the exact
child-local TCB-cap lookup and `suspend_tcb`, then yield; a fresh
`SignalEmergency` turn must commit `Receive`, signal root-emergency, and yield
before blocking receive can execute again. The single Reply association remains
serialized through every boundary. `handle_target_fault` must contain no yield,
and Worker, driver, and recoverable semantics must remain unchanged. A service
classification must commit `ResolveService` and yield. `ResolveService` must
perform exactly one fixed generated lookup plus a nonblocking registry
lock/scalar snapshot, retain the copied fault and retry itself on contention,
and otherwise commit `SuspendService`. `SuspendService` must perform one quiet
bounded suspend syscall and select `RecoverPassiveService` only for a passive
service with a donated Call; active console must select `PublishService`
directly. `RecoverPassiveService` may issue at most one recovery Reply and then
commit `PublishService`. `PublishService` performs one mailbox action, retains
the snapshot on backpressure, and commits `Receive` only after publication.
The focused source guards are
`root_fault_cold_activation_primes_receive_before_any_fault_association` and
`terminal_critical_fault_commits_one_resumable_action_per_refill`.

NineDoor, temporal-contract, console-ACK, SendBatch, and NaturalPostpone schema
tests must require `root_task.schema = "1.14"` and reject schema 1.13 or older. They must admit
exactly one root-retained, one-shot scheduling context with
object bits 8, `3000 us / 10000 us`, and `max_refills = 2`; any different value,
missing fixed SC accounting, child-Cspace SC or SchedControl cap, or NineDoor
`TCB.SetAffinity` path fails closed. The candidate is not target-qualified by
schema, code generation, compilation, or host tests.

Bootstrap tests must prove the exact transition `Resume -> validated empty Log
prepare -> atomic ReplyRecv queued -> SC unbind -> steady passive donation`.
Constructor/source-order coverage must first prove that root configures and
binds the one-shot SC while the child is suspended and before registry seal;
activation must not allocate, configure, or bind a second SC.

The empty probe has `path = ""` and `payload = ""`, consumes one ordinary
request sequence without leaving an outstanding exchange, and must not break
repeated Calls. Source-order coverage must prove one initial `seL4_Recv`, one
atomic `seL4_ReplyRecv` loop, no separate `seL4_MCS_Reply`, validation before
unbind, and unbind before `Passive`. Activation, probe, and unbind failure tests
must all revoke the namespace boundary and refuse admission; probe and unbind
failure must also attempt to suspend the child.

NineDoor also adds one separate recovery Reply object owned by its passive
receive loop and copied only into the compiler-selected root-fault CSpace slot.
Tests must reject reuse of the ordinary root-fault receive Reply object, reject
active console-network admission to the passive path, and prove one atomic
ready-to-replied transition. A fault with an outstanding Call returns exact
sequence plus typed `Closed` once before the durable service mailbox is
published; a fault between calls publishes containment without Reply. Both
paths suspend, fence, scrub all four shared frames, delete recovery/fault caps,
and revoke anchor 16137 before old authority can be reused.

After the host gate is green, the QEMU target check must use the selected
four-core MCS kernel, construct all 10 QEMU sources suspended, seal the exact
registry, and finish the bounded synchronous bootstrap IPC trace before
resuming any restricted critical child onto its generated SC. Root-fault is
the sole receiver on the shared standard/timeout fault endpoint; its
compiler-bounded child-local TCB
control caps must suspend the exact registered target without relying on a
root-relative CPtr. The init/root-control TCB retains its kernel-provided
bootstrap SC until userland reaches the selected serial console,
deferred-network supervisor, or non-serial pump event-loop seam. Only there may
it apply the generated root-control temporal policy, exactly once and before
steady polling. It must then yield the partially consumed initial refill and
wait for the next MCS replenishment before probing either containment mailbox
or entering the first Operator. The init/root-control MCS dispatcher never
receives or polls the fault endpoint because it owns no receive Reply object.
The boot is a failure if any
named duty does not execute on its generated TCB/SC, if root-control temporal
policy is armed during kernel bootstrap, if init continues polling an MCS fault
endpoint after transfer, or if a fault, timeout, simultaneous wake, saturation,
handler fault, or allocation failure loses attribution or forward progress.
None of this QEMU evidence is Pi hardware proof.

### Milestone 26e NineDoor service-isolation gate

Run the fixed ABI, one-shot bootstrap/passive child, generated inventory, image
binding, and target checks before the QEMU boot:

```bash
cargo test -p secure9p-transport -p nine-door-runtime
cargo test -p root-task --test ninedoor_service_isolation
.venv/bin/python -m pytest -q tests/test_ninedoor_runtime_packaging.py
SEL4_BUILD_DIR=out/sel4/profile-v2/qemu-smp-production \
COHESIX_WORKER_IMAGE_ARCHIVE=out/cohesix/worker-images/cohesix-worker-images.cpio \
COHESIX_WORKER_IMAGE_MANIFEST=out/cohesix/worker-images/cohesix-worker-image-manifest.json \
COHESIX_CONSOLE_NETWORK_RUNTIME_IMAGE=target/aarch64-unknown-none/release/console-network-runtime \
COHESIX_NINEDOOR_RUNTIME_IMAGE=target/aarch64-unknown-none/release/nine-door-runtime \
cargo check -p root-task --target aarch64-unknown-none \
  --no-default-features --features release-qemu
```

The selected four-core GICv3 QEMU run must then show all ten sources constructed
suspended, NineDoor registered last before seal, root-fault activated first,
and NineDoor resumed only after that receiver is live. It must observe the exact
post-validation marker
`[ninedoor-service] passive child active bootstrap-sc=unbound recovery-reply=installed`,
with no NineDoor `TCB.SetAffinity`/affinity-failure marker, then complete at
least two ordinary namespace requests so the first and repeated passive
donation/atomic-`ReplyRecv` cycles are both live. Before any injection, one fresh
exact-artifact boot must produce a non-claiming `ninedoor` convergence `PASS`,
including the live authenticated-cohsh UART marker and canonical `9p_batch.coh`
operation. The service runner must bind that frozen observation, UART, QEMU
launch record, emitted four-file target-session bundle, service ELF, root ELF
where used, and every byte count/SHA-256 before it may attach GDB.

Use two additional fresh exact-artifact boots because terminal NineDoor has no
replacement. In the first, redirect the live child request handler to its
standard-fault hook while a donated Call is active; that Call must return one
typed failure before containment. In the second, allow two ordinary Calls to
complete, stop at the root's post-prepare evidence hook after the second Reply
has returned the donated SC and the child is blocked in atomic `ReplyRecv`, and
request a root-local `NamespaceServiceBoundary` revoke. Root must consume that
flag in its normal control path before another Call. This is the mandatory
between-Calls no-Reply-association case; it is not, and must never be reported
as, a child standard fault. Neither path may leave a donor blocked, admit a
second Reply, preserve old mappings/caps, route the active console through the
passive path, or stop root-control progress.
Source and behavior guards must prove that the first exclusive root-control
Recovery turn only consumes the durable fault record, fences new Calls, retains
the four shared mappings, initializes the cursor at `SuspendTcb`, and returns
`InProgress`. Later Recovery turns must select exactly these 18 units in order:
`SuspendTcb`; request 0 `ScrubCleanRequestFrame`, `UnmapRequestFrame`; request 1
with the same two units; response 0 `UnmapResponseRead`,
`MapResponseWritable`, `ScrubCleanResponseWritable`,
`UnmapResponseWritable`; response 1 with the same four units;
`RevokeRecoveryReply`; `DeleteFaultCap(0)`; `DeleteFaultCap(1)`;
`RevokeAnchor`; and pure `Finalize`. Each selected unit must expose its
successor before work and restore only itself after a synchronous error.
Scrub/clean must use the bounded lock-free cache path, each writable response
remap must issue exactly one `Page_Map`, and Reply/fault-cap deletion must use
quiet bounded operations. `Finalize` must only advance to `Complete`; the next
idempotent `Complete` turn publishes the exact five-field proof, and removal is
permitted only after that proof. `InProgress`, incomplete proof, and error must
all consume the exclusive Recovery refill and fence ordinary EventPump work.
These are common MCS QEMU/Pi containment semantics. Ordinary Pi scheduling is
unchanged, but fresh Pi containment evidence is still required separately.
This QEMU qualification is still pending; the marker or a successful boot alone
does not satisfy this gate and cannot qualify Pi containment.

### Milestone 26e console-network service-isolation gate

Run the fixed compiler provenance, ABI, child transport, root boundary,
bounded synchronous-capture, harness, packaging, and target checks before the
QEMU boot:

```bash
cargo test -p coh-rtc root_control_turn_candidate_is_exactly_accounted
cargo test -p coh-rtc supervisor_cold_activation_candidates_are_profile_scoped
cargo test -p coh-rtc --test ai_lora_contract
cargo test -p console-network-abi -p console-network-runtime
cargo test -p nine-door --test policyfs --test host_sidecar_policy
cargo test -p root-task --test console_network_service
cargo test -p root-task --no-default-features --features driver-tests-qemu \
  isolated_response_lane_pays_exactly_one_ordinary_debt_after_eight_units \
  -- --test-threads=1
cargo test -p root-task --no-default-features --features driver-tests-qemu \
  isolated_help_capture_publishes_complete_body_then_one_terminal \
  -- --test-threads=1
cargo test -p root-task --no-default-features --features driver-tests-qemu \
  isolated_fixed_synchronous_producers_cross_batch_depth_without_end \
  -- --test-threads=1
cargo test -p root-task --no-default-features --features driver-tests-qemu \
  bounded_sync_capture_overflow_emits_only_typed_terminal_and_reconciles_metrics \
  -- --test-threads=1
cargo test -p root-task --no-default-features --features driver-tests-qemu \
  bounded_sync_cache_snapshot_crosses_batch_depth_and_tombstones_on_quiet_cut \
  -- --test-threads=1
cargo test -p root-task --no-default-features --features driver-tests-qemu \
  bounded_sync_response_is_retired_on_exact_identity_loss \
  -- --test-threads=1
cargo test -p root-task --no-default-features --features driver-tests-qemu \
  pinned_network_line_cannot_dispatch_to_a_replacement_connection \
  -- --test-threads=1
cargo test -p root-task --no-default-features --features driver-tests-qemu \
  physical_progress_is_bounded_while_heavy_producers_preserve_network_owner \
  -- --test-threads=1
cargo test -p root-task --no-default-features --features driver-tests-qemu \
  blocked_physical_producer_retains_an_ordered_busy_terminal_and_prompt \
  -- --test-threads=1
cargo test -p root-task --no-default-features --features driver-tests-qemu \
  hal::cache::tests -- --test-threads=1
cargo test -p root-task --no-default-features \
  --test isolated_virtio_network_phasing -- --test-threads=1
.venv/bin/python -m pytest -q \
  tests/test_console_network_runtime_packaging.py \
  tests/test_qemu_tcp_response_matrix.py \
  scripts/ci/test_run_regression_batch.py
cargo test -p cohsh --features tcp \
  pooled_tcp_uses_one_wire_attach_for_twenty_four_exact_authority_leases \
  -- --exact --test-threads=1
cargo test -p cohsh --features tcp -- --test-threads=1
cargo test -p hive-gateway -- --test-threads=1
SEL4_BUILD_DIR="$PWD/out/sel4/profile-v2/qemu-smp-production" \
  cargo check -p console-network-runtime --target aarch64-unknown-none
scripts/check-generated.sh
python3 scripts/ci/test_plan_catalog.py recommend \
  scripts/qemu_tcp_response_matrix.py tests/test_qemu_tcp_response_matrix.py
python3 scripts/ci/test_plan_catalog.py converge --target qemu --format json \
  scripts/qemu_tcp_response_matrix.py tests/test_qemu_tcp_response_matrix.py
scripts/cohsh/run_regression_batch.sh
scripts/ci/test_plan_run.sh --target qemu \
  --state-dir out/test-plan/m26e-console-qemu-v17
```

The compiler and root-boundary tests must agree on the exact image path and
entrypoint, retained anchor, one-MiB child untyped, 60 image pages, 32 stack
pages, one IPC page, one init page, and four shared pages: 98 frames total.
They must also agree on eight translation objects, 123 retained root slots, 16
child CSpace slots, the 32-page stack at `0x72030000..0x72050000`, notification
slots/badges including the distinct publication ACK badge 64, sole port 31337 listener, active SC `3000 us / 10000 us` budget,
`3000 us` WCET, QEMU core-2 placement with `3000 us` response candidate, Pi
core-0 placement with `8100 us` response candidate, refills, and
standard/timeout fault identities. The timeout cap/badge/resource/registry row
must remain reserved, but the console TCB timeout-handler slot must be empty.
Any drift, cap-slot alias, anchor collision, second listener,
W+X page, unbound or tampered image, broad fault/signal rights, root SC
borrowing, or non-pointer-free record fails before launch. These stack and
temporal values are a measured repair candidate, not qualified truth: live
four-core GICv3 QEMU must still complete authentication, the canonical `.coh`
regression, standard-fault containment, and budget-exhaustion natural
postponement without stack failure or loss of liveness/isolation.

The compiler resource tests must additionally derive QEMU fixed/maximum
frame-slot totals of `2018/4058` and `2066/4250`, and Pi fixed/maximum totals of
`4066/8962` and `4114/9154`, respectively. The post-construction reserve,
per-Worker costs, eight translation objects, untyped bytes, and fifth root
Write-only publication-ACK mint remain unchanged; V15 adds exactly one image
frame and its retained root slot. Adding the unchanged reserve must therefore
yield admitted capacity-check totals of `2578/6298` for QEMU and `4626/11202`
for Pi.

The exact V14 build failure is the oracle for this reconciliation, not target
evidence: child ELF SHA-256
`e16fc715975d4d73959c6536d9c4246058857040339c3c05a68721223a2d3f16` was
73,568 bytes with page-rounded PT_LOAD span `0x200000..0x23c000` and final
LOAD end `0x23b540`, while generated truth admitted only 59 pages. V15 must
bind that 60-page shape exactly; any further ELF growth, shrinkage, span drift,
or source/generated disagreement fails before root linking and QEMU launch.

The fixed-layout tests must also prove that console-network ABI v4 still uses
the same four 4096-byte pages, offsets, lengths, and record schemas while the
live helpers touch only a 40-byte packet or 64-byte control/event scalar header
plus the validated active payload. A publisher must clear commit, perform a
release fence, write the header and active bytes, perform a second release
fence, publish the final sequence commit, and signal only afterward. Readers
must reject an oversized length or a commit that changes across the bounded
copy without advancing the accepted sequence. Scalar reserved header fields
must validate as zero. Reserved page-tail bytes and inactive payload suffixes
are non-authoritative and must not be scanned or copied during a normal turn;
construction zeroing and containment scrub remain required. Capability
inspection must show that the child's packet-ingress and control mappings are
read-only and its packet-egress and event mappings are read-write.

Schema 1.14 must reject the immediate 1.13 predecessor and any ACK badge other
than the ABI v4 value 64. The root must retain the fifth Write-only cap on the
existing root-to-child Notification without adding a child slot or Notification
object. Source and deterministic state-machine tests must prove
`NaturalPostpone` is selected for the active console child and for both selected
QEMU V35/Pi V24 root-control records, standard faults remain terminal, and
each reserved timeout cap is not installed on that TCB.
Ready has no credit; ordinary wakes never grant credit; exact eligible internal
work may Poll and preserves credit; idle or publication-uncredited work calls
Wait directly with no ordinary Yield; one publication consumes its credit
before page mutation; root refuses another
poll while ACK is owed; and ACK debt is set only after all indicated records
validate/copy, then cleared before Release+Signal after adapter retention.
Malformed, stale, retention-failed, revoke, and containment paths grant no ACK.
Graceful ShutdownComplete consumes one credit, retires root debt without ACK,
starts bounded containment, and becomes teardown-terminal only after the exact
proof completes; terminal plus egress coalescing fails closed.

ABI v4 tests must prove `SendBatch = 3` binary encoding version 1 with exact
eight-byte header, one through eight records, exact `used_bytes`, reserved zero,
and each `1..=256`-byte UTF-8 record free of CR/LF. Empty, ninth, oversized,
malformed UTF-8, truncated, trailing, overlapping, stale-identity, and
second-batch-while-pending cases fail without partial queue mutation. A valid
batch is accepted atomically, but each replenishment-bounded child Session unit
stages at most one external frame. `ControlCompleted` and `OutputDrained` must
refer to the exact control sequence and connection; terminal drain evidence
from an old connection cannot release a replacement response.
After each successful complete-frame Session commit, including the last batch
frame, tests must observe exactly one retained following
`StackIngress -> StackEgress -> Session` cycle. The next no-progress Session
must complete and quiesce. A pending batch without a commit and a
capacity/sendability failure must retain zero new cycles; neither a root
ServiceTick-per-record dependency nor a local self-Poll loop is accepted.

ABI v4 tests must additionally prove `CommandBatch = 27` with encoding version
1, an exact eight-byte batch header, one through eight records, and per-record
`now_ms`, command length, and exact UTF-8 command bytes. The child may coalesce
only consecutive authenticated `Command` events for one connection, up to the
generated `max_commands_per_wake <= 8` and fixed 2368-byte payload bounds.
Connected, Authenticated, Rejected, Backpressure, Disconnected, and shutdown
events fence the batch. Root must validate the complete copied batch and prove
capacity for every command before the first queue mutation; malformed,
oversized, ninth, cross-identity, or over-capacity input fails closed without
partial command execution. Each command retains its original timestamp and FIFO
order, while the complete batch consumes exactly one existing child publication
credit and adds no page, capability, SC, timeout, retry, or public protocol.

Root V34 tests must preserve V33/V32/V31/V30/V29's capture plus V27's one exact
generation/connection-bound lane,
at most eight useful producer/Network response units, then exactly one
preserved ordinary Operator/Runtime/Network debt turn. HELP, NETSTATS, SMP, and
CACHELOG body plus exact ACK or ERR terminal are captured root-locally, sealed
immutable before publication, and drained on later Network visits through the
existing response lane. Backpressure retains the exact identity and cursor;
identity loss discards the response without publishing it to a replacement
connection. CACHELOG reserves a bounded owned vector before taking the live-log
lock, copies at most the existing 1920-record ring under one lock acquisition,
then renders newest-first after releasing the lock. Allocation failure becomes
one typed bounded terminal rather than a partial response. Conflicting physical
synchronous producers receive bounded BUSY while the network response owns
shared state; physical input, permitted bounded commands, fatal output, timers,
and ordinary cursors remain live. The terminal lane retires only after exact
terminal `ControlCompleted` plus `OutputDrained`. Disconnect publication must
remain ineligible while that lane retains terminal completion,
publication-ACK debt, copied egress, or queued output, even when child-side
drain is true. Backpressure must leave the existing issued latch clear; after
exact lane retirement one successful publication sets it and every later
Disconnect visit for that connection is a no-op. The 1920-record ring capacity
is an implementation bound, not a separate five-second promotion gate.
For CAT, LS, TAIL, and LOG only, a typed command refusal must retire the
provisional ordinary stream shell exactly when no stream END was committed.
The regression must queue a refused command followed by QUIT on one
authenticated connection, observe the exact typed ERR, then exact `OK QUIT`
and peer EOF without retry or reconnect. It must prove `pending_stream=None`
and `stream_end_pending=false` before QUIT dispatch. Successful streams,
committed END/prompt state, synchronous capture/sealed modes, and other failed
commands must remain unchanged.

The compatibility matrix is a hard promotion gate, not an assumed consequence
of source or host tests. Its fixed one-socket functional mode must authenticate
and attach once, then return HELP `11 + OK`, isolated QEMU VirtIO NETSTATS
`15 + OK`, first-call selected-QEMU SMP activity at `max_cores = 4` `16 + OK`,
and CACHELOG9 `9 + OK`, then complete PING and QUIT on that socket without
retry or reconnect and within the existing client timeout. Stage 03 base runs
the fixed matrix before its `.coh` group. A missing, duplicate, reordered, or
unexpected body or terminal frame, timeout, retry, or reconnect fails the
matrix. The authenticated oversize regression must then use one connection to
receive exact `ERR FRAME reason=invalid-length`, drain the complete declared
oversized body even when it spans reads, send QUIT on that same authenticated
connection, receive exact `OK QUIT`, half-close the client write side, and
observe peer EOF. Any `ERR AUTH`, close, reconnect, retry, parsing of body bytes
as a new length prefix, or missing QUIT EOF fails the gate.

The operational QEMU Stage 03 `.coh` selection must exclude
`scripts/cohsh/host_sidecar_mock.coh`. That script is an explicitly named,
non-production fixture whose predefined systemd topology contradicts the
operational `/host` contract: providers start `unavailable source=none`, with
no predefined units or controls until an authenticated host snapshot populates
them. Stage 03 still exercises the operational absent-host behavior. The
independent `policyfs` and `host_sidecar_policy` NineDoor fixtures retain the
non-Queen `EPERM`, missing-approval `EPERM`, approved Queen write, one-shot
approval consumption, and audit assertions; those fixture results cannot
become live target host-provider evidence.

The common gated `policy_gate.coh` and `replay_journal.coh` target lanes must
also remain independent of host snapshots. They use uniquely named,
session-local `/queen/ctl` binds from `/proc/boot`: PolicyFS requires one
approval, proves its consumption and alias readability, then refuses reuse;
ReplayFS records two approved controls, verifies a cursor-zero match, and
refuses a future cursor. `QUIT` clears those binds. The intended bounded audit
and decision records may persist for later same-boot scripts, but no Worker,
lifecycle transition, provider child, or predefined `/host` node may be
created or assumed.

CAS trust material is routed just as strictly. `cas_roundtrip.coh` contains
repository-fixture signatures and may run only in the QEMU gated artifact,
whose regression manifest selects
`resources/fixtures/cas_verification_key.hex`; it never runs in operational
base QEMU or Pi. Operational base QEMU and Pi instead run
`cas_fixture_signature_rejected.coh`, which requires the same first two
incomplete manifest fragments to return `OK` and the completing fixture-signed
fragment to return `ERR ... EPERM`. Do not replace either operational public
verification key with fixture trust material and do not add an operational
private signing key to the repository. Positive live operational upload
continues to require the external `COH_CAS_SIGNING_KEY` flow below. The full
QEMU selection is exactly 18 `.coh` scripts plus the fixed response matrix: 10
base, two telemetry, one shard, four common gated, and one QEMU-only gated CAS
fixture. The full Pi selection remains exactly 17 `.coh` scripts: 10 base, two
telemetry, one shard, and four common gated, with neither the fixed matrix nor
the fixture-positive CAS upload. Therefore focused QEMU base records 11
workloads, focused QEMU gated records five, focused Pi base records 10, and
focused Pi gated records four. Any cross-profile fixture routing or count drift
fails Stage 03 before a target claim.

Until fresh exact-artifact V35/V18 evidence passes the fixed matrix,
standard-fault terminal containment, budget-exhaustion natural-postponement
liveness/isolation, and all later
applicable stages, Stage 03, REST, performance, and Milestone 26e acceptance
remain withheld. A bounded TAIL/CAT QEMU operation is only a non-claiming
convergence diagnostic.

Retain target disassembly evidence for all four runtime page helpers. It must
show bounded small helper frames, active-length copies, and no compiler-expanded
4096-byte aggregate load, store, or copy; the maximum nested stack chain must
remain strictly inside the generated 32-page stack. This is a target-binary
acceptance check, not a host unit-test substitute. At source commit
`290ef6028`, the selected QEMU boot reached readiness, but the first
authenticated packet raised console-network timeout badge `0x26ee0007` while
the saved PC was in the compiler-expanded volatile read of a complete
`PacketPage`; that helper reserved roughly 96 KiB below the large start frame,
and root subsequently timed out before draining the containment mailbox. Record
that run only as the live failure that invalidated whole-page transfer. Fresh
authentication, the full canonical `.coh` batch, and standard/timeout fault
injection on four-core GICv3 QEMU remained pending after the compact repair.
The following compact-page run at source `00bf02540` reached the root prompt
but timed out `root-control` at the console-network control poll before
authenticated regression. Record that run only as the live failure that
disproved v3's combined Network/runtime-IPC phase. The next live run at source
`4d1a47b89` preserved the three-phase outer cycle but timed out `root-control`
in `VirtioTxToken::consume` after queue notify. Record that saved PC as failure
evidence for v4's multi-unit Network visit, not as a TX-completion wait: published
descriptors complete asynchronously. The v5 bounded-Network-unit candidate must
not be promoted: the following live v5 run consumed exactly `3000 us` in the
active console child and raised timeout badge `0x26ee0007` at `Send` after
`publish_exchange`, then consumed exactly `2750 us` in root Recovery and raised
timeout badge `0x26ee0001` at the sole outer yield after containment/quarantine.
The next canonical run used root ELF SHA-256
`0059fd675b476106888d6ca62c8bba21f9b340b9aa607e000fbf96997fd29900`.
Its child was healthy and no Recovery ran, but root timeout badge `0x26ee0001`
followed exact `2750 us` consumption at the outer yield after Network composed
empty ObserveChild, no-op StageOutput and Disconnect, and committed/signalled
60-byte ARP ingress sequence 1. Record that only as the v6 failure that requires
one attempted lower unit per visit. The following v7 run used root ELF
`d2f69bddbf56deef6919ec6ea802e9d3c44a691c2dbe05aa59428854bbf7a6ae`
and timed out before the queued UART-visible `[mark] root-console.start.ok`,
Network, or Recovery became visible; do not treat the missing marker as a
source-order boundary. Root consumed exactly `2750 us` and raised `Timeout`
badge `0x26ee0001` at serial queue `inner_dequeue` (PC `0x43e84`) from
`SerialPort::flush_tx_unlocked` (LR `0x77b74`). Record that only as the v7
failure requiring one shared Operator serial-I/O credit. The following v8 run used root ELF
`5052e7a5070987c252d3c1f5cf6f27172bd5ece1836a8f6c2a5c329c789a0a61`,
consumed the complete `2750 us` root-control refill, and raised `Timeout` badge
`0x26ee0001` at PC `0xede84` immediately after `emit_prompt_now`. Record that
only as the v8 failure requiring at most one retained output-record attempt per
nonzero-credit VirtIO Operator. The following v9 run used root ELF
`fa488c9367136f0eadef7182a18691664c3ae51c2ac2974e12000ff5d27f38ed`
and CPIO
`aca549e99e0d86299e9f98348d896b730259277654544ebd22a74595b61e9bfb`.
It consumed the complete `2750 us` first post-bind refill and raised `Timeout`
badge `0x26ee0001` at PC `0x13a798`, the first `memmove` instruction reached
from `PendingConsoleOutput::remove(0)` (LR `0x79ccc`, prospective length
`0x110`). Zero bytes were copied, the output cursor remained full, serial was
idle, and the marker plus prompt remained queued. Record that only as aggregate
first-post-bind refill exhaustion; do not attribute the timeout to copy cost or
the still-enforced one-record rule. The v10 root ELF
`022908395c954f73a67136f70fe4404d96e0cf1ff16f4531fa95eae7a6f57cb5`
then crossed its activation seam and emitted the marker and prompt, but the
second fresh Runtime consumed the full `2750 us` and raised root timeout badge
`0x26ee0001` at PC `0xce98c`, the root-endpoint nonblocking receive. The same
run recorded child timeout sequence 1, badge `0x26ee0007`, Terminal, with the
child at `seL4_Wait`, `service_pending = 1`, and `control_pending = 1`.
Recovery reached the six-field Complete proof and NineDoor stayed healthy. The
next canonical v11/v4 run bound root ELF
`44971429e4941d751248c216082256f01e187930d9a6d40028e5c89d8611b597`,
console child ELF
`af08f817191cc51c9354b61f09f3eeb50c8cdf875c660c7231987a426886666d`,
and CPIO
`9fbb58e1dc6dc508361f37ce0c24219e3e9029dae101e2be789df1bcb1a5b11d`.
There were four TCP connects. The first three completed authentication attempts
each wrote 18 bytes and read zero; the fourth connect had no completed
authentication record. The child consumed `3000 us`, raised `0x26ee0007`, and
stopped at PC `0x213458`, the `seL4_Yield` immediately after the composite
`PollService` completed and cleared; saved retained state identified
`PollService` as that completed unit. After
containment reached `Complete(6)`, root consumed `2750 us` and raised
`0x26ee0001` at outer-Yield PC `0xf5fbc` after an empty Operator, with
successors `Runtime` and `ControlEndpoint` and empty output. Record this only as
v11/v4 failure evidence. The later v12/v5 non-claiming run bound root ELF
`7cec5bd582d063adc73830af8cc62e0ec8dbbb33d91bd4701db09ca69e32e6ca`,
child ELF
`920883c5e706688a65e7f168a643dbc527d09d7f48584bfb41fbd0c0ae823cb6`,
and CPIO
`dc36495a5de0df13bfb853ffa33fdc6e7ccc3bbf3a1a3c8c4cd74c8551160c16`.
Its four authentication attempts each wrote 18 bytes and read zero. Root was
the only timeout: badge `0x26ee0001`, exact `2750 us`, outer-Yield PC
`0xf612c`, ordinary successor `Network(2)`, Runtime successor
`StreamFlush(3)`, and empty staged bootstrap `Option`. Fault sequence 2 and the
healthy child at Yield-then-Wait exclude child failure or Recovery. Because the
run embedded dirty commit `a533290ffe264f0a2bf0af3db4bb4c45d1a4a278` and
HEAD later advanced to `84934dda6`, record it only as diagnostic failure of the
generic Runtime-without-control prelude plus empty `BootstrapDrain`. The v13
root and v5 child candidates were therefore required to repeat target proof.
The subsequent v13/v5 non-claiming run bound dirty commit
`84934dda6fcffbfa536d4e437cc1904c7fdeb0b1`, root ELF
`0275cd7d701263cc1731ca3301d9aeab8a0393651745659f192106a0d558d78f`,
the unchanged child ELF
`920883c5e706688a65e7f168a643dbc527d09d7f48584bfb41fbd0c0ae823cb6`,
and CPIO
`142e2aec64662888a9872ff77ff85d1f5f7c351b7aaa478ded8cf99ba9e64f29`.
All four authentication attempts wrote 18 bytes and read zero. Root-control
initiated failure with badge `0x26ee0001` at child-notification `sel4::poll`
SVC PC `0xce98c`, caller `0x108910`; successors `Operator` and `StageOutput`
identify `ObserveChild`. The healthy v5 child remained at `seL4_Wait`.
Root-fault then raised badge `0x26ee0002` at `suspend_tcb` SVC PC `0xce0cc`
against root-control cap `0x10`; emergency fail-stop followed. Record this only
as diagnostic failure of v13 root-control and v2 root-fault.

The exact clean v19 artifact bound root ELF
`0737a6f008197fd5b931af104c95164ddcd925fa04a8440439895c1e76b26fca`
and CPIO
`51e7b955b449b42b7a0cad569aa187e19a0f71464ffb81080d29733a589e7ed0`.
Four authentication attempts each wrote 18 bytes and read zero. Root-control
timed out at outer-Yield PC `0xf66dc` after completed Network. Lower successor
`Ingress(3)` proves selected `Disconnect(2)` was a no-op without child signal;
pending egress was empty, the child was healthy at Wait PC `0x21343c`, and
root `smoltcp_polls` was `250098`. Record this only as failure evidence for the
post-leaf counter-refresh, NETDIAG, and NineDoor aggregate. The current
V35/root-fault-V6/child-V18 target canary must repeat the ordered
peer-close/relisten gate, prove the fixed response matrix, and run
standard-fault containment plus budget-exhaustion/postponement proof before
Stage 03, Hive Gateway pressure benchmarks, broad host closure, or complete
host-tool validation proceeds; none of the prior failures is qualification
evidence.

The exact v20 root/CPIO hashes were
`ed5cb9f587d0d63e6121f8b00b083e68f5a0a7dd23dd6d2bbf0c899e1e85e80f`
and `ca2a52038eb0814a17c8609f03bec32ff357fdd524edee3e7080ac69ceb7823b`.
That image reached the marker and prompt, then root-control timed out at
outer-Yield PC `0xf680c`. Successor Operator and retained NETDIAG prove timer
plus NIC completed and the diagnostic had not run. Lower-cursor, egress, and
child state remain unconfirmed. Record this only as failure evidence for the
v20 Timer/Nic composition; it does not qualify or falsify the exclusive
diagnostic, selected lower unit, child v5, root-fault v4, or v15 supervisors.

Child tests cover partial and oversized frames, malformed authentication,
constant-time token acceptance, command release only after `AUTH`, one-packet
and one-control backpressure, retained output until smoltcp accepts the complete
frame, an `OutputDrained` completion only after the child TCP send queue empties,
and byte-exact root response forwarding. Root tests cover READY and
connection transitions, stale generation/sequence rejection, completion-bound
slot reuse, fault closure, and complete suspend/unbind/scrub/revoke evidence.
The EventPump source test must also prove the deterministic
Operator/Dispatch -> Runtime/IPC -> Network -> Operator/Dispatch cycle for the
isolated VirtIO contract. The next phase is committed before any early return
within an admitted ordinary phase. Operator gives pending physical input/output
priority and dispatches at most one buffered network line; Runtime/IPC alone
selects one persistent Worker -> ControlEndpoint -> BootstrapDrain ->
StreamFlush -> RebootTail unit with serial and network command ingress
suppressed. Worker consumes one pending mailbox operation or checks one retained
Heartbeat/GPU/LoRA role slot; ControlEndpoint performs at most one poll and its
immediate forward; BootstrapDrain takes one staged `Option`; RebootTail owns its
visit. Each no-op still consumes its selected visit, and the successor commits
before the compact isolated-VirtIO Runtime prelude. The prelude reads HAL
timebase and polls the timer once. An observed tick updates `now_ms`, increments
the timer metric, publishes HAL timebase, and runs the existing conditional
trace; without a tick, `now_ms` takes the read timebase. It then reconciles
CYW43 network-ready HDMI state; the generic
Runtime-without-control tail must be absent. MCS fault polling is absent from
the cursor. StreamFlush emits at most one retained line per selected
visit; the visit after the final line performs cursor/bandwidth finalization
only and the following selected visit emits END only. Legacy Pi/non-VirtIO
Runtime retains its 48-line/16-KiB path. Network alone
performs VirtIO/NIC service and cannot execute a command or general runtime-IPC
dispatch. Each phase returns to the sole recurring outer yield before its
successor begins. Reboot, serial cutover, and linked-runtime routing remain
earlier owners and do not falsely advance this QEMU-only phase state. Equivalent
non-VirtIO and Pi turns must retain their existing behavior. Separately, every
MCS profile must perform one post-activation yield before the first containment
probe or Operator, preserving the retained startup FIFO and adding only the
documented one-period Pi startup wait. That activation seam is not one of the
recurring phase yields. The isolated
VirtIO Operator must create one generated `64`-byte serial-I/O credit, share it
across all root-context poll/flush calls, and retain unfinished bytes when it is
exhausted. Entry-time TX backlog must reserve `32` bytes for TX and cap RX at
`32`; without TX backlog RX may use all `64`. Tests must reject any helper-local
credit reset, aggregate service above `64`, suppressed pending output under
sustained RX, or application of this cut to linked-runtime/Pi paths.
With a nonzero `OrdinaryVirtioConsoleOutputTurn` selector, the v12 source guard
must also find the exact `OrdinaryVirtioOperatorWork` fields
`serial_input`, `serial_output`, `local_seat_input`,
`dispatchable_network_line`, `pending_console_output`, `physical_response`,
`stream_or_tail`, `reboot`, `serviceable_display`, `serviceable_frontier`, and
`serviceable_attach`, plus its pure `is_empty`. An empty snapshot after the
first bounded priority pass must return before
`poll_runtime_without_control_tail` and the repeated Operator probes. Raw
unattached-seat flags, quarantine, timer/Runtime/Network work, atomics, and HAL
hints cannot prevent the cut; Pi, non-VirtIO, and zero-selector behavior remains
unchanged.
Within each originating isolated Network phase, source and behavior tests must
prove exactly one attempted NIC unit, including a no-op, after the timer
prelude. That visit retains one compact diagnostic observation and returns. The
next Network phase must take the observation first, run only NETDIAG, and
return without timer or NIC work. The persistent lower cursor must select
ObserveChild -> StageOutput -> Disconnect -> Ingress -> ServiceTick ->
ObserveChild. Each lower attempt returns; a no-op advances, and a successful
unit that signals the child forces the next lower attempt to ObserveChild. A
deferred normal-success diagnostic preempts first and drains exactly one compact
record. Otherwise retained egress preempts for exactly one TX attempt and
returns on success or backpressure. Both preemptions must leave the exact lower
cursor and forced-observe state unchanged. An ObserveChild attempt may copy and
retain activity but returns without TX. A published TX must initialize the bounded
payload and atomically publish the descriptor, avail entry, optional notify, and
in-flight identity before returning to the sole outer yield. Tests must show no
post-notify buffer mutation or wait for completion, and that backpressure retains
egress without duplicate publication. The retained-TX unit may perform no more
than two bounded reclaim checks before its one attempt. Tests must prove the
bounded early 0-through-63 and every-64th-success diagnostic gate, continuous
counters, and Observe -> TX -> DeferredDiagnostic before resuming the preserved
lower state for sampled successes. A second publication cannot overwrite or
merge a pending diagnostic; anomalies remain immediate.
The v23 source guard must prove that `poll_split_ordinary_virtio_compact`
handles `PhysicalResponseBarrier::TailInFlight` through
`reconcile_physical_response_barrier`, tests the resulting barrier, and either
returns or calls exactly one compact Operator unit before returning. The
eligible `!stream_end_pending && stream_prompt_pending` path must similarly
call `queue_stream_prompt_tail_if_ready`, test the resulting pending bit, and
either return or call exactly one compact Operator unit before returning.
Neither path may read, commit, or dispatch `ordinary_service_phase`; both must
preserve the Runtime and Network cursors. Ready reboot must return before phase
load. Only the no-duty path may load the phase, commit `phase.next()`, and call
one existing Operator, Runtime, or Network leaf. No new phase, cursor, or helper
is permitted, and generic/Pi call ordering remains unchanged.
The v21 source guard must prove `poll_split_ordinary_virtio_network_turn`
first takes a retained diagnostic and returns without reading or advancing
`ordinary_virtio_network_unit`. Otherwise it reads
`OrdinaryVirtioNetworkUnit::{Timer, Nic}`, commits `unit.next()` before work,
and dispatches exactly one unit. Timer calls only
`poll_runtime_timer_prelude`; Nic calls only one noinline
`poll_one_split_ordinary_virtio_network_unit`, followed by retention of the
telemetry, originating `now_ms`, and originating last-RX-progress horizon.
The former composite `poll_split_ordinary_virtio_network_prelude` must be
absent. Neither unit may run a generic Runtime tail, command dispatch, event
drain, or NETDIAG; Timer cannot reconcile CYW43-ready HDMI state. A retained
diagnostic visit samples counters after the intervening compact Operator and
Runtime visits, runs only NETDIAG, and leaves the Timer/Nic cursor unchanged.
Immediate flush, connection-id, and NineDoor ingest accounting remain unchanged
in the Nic visit. Quarantine clears retained diagnostic state while preserving
the Timer/Nic cursor; generic/Pi behavior neither uses nor mutates it. The
adapter must use
`select_isolated_network_turn`, commit `selection.successor()` before work, and
dispatch to separate noinline `poll_deferred_diagnostic_unit`,
`poll_transmit_egress_unit`, `poll_observe_child_unit`,
`poll_stage_output_unit`, `poll_disconnect_unit`, `poll_ingress_unit`, or
`poll_service_tick_unit` helpers without one all-unit closure. A successful
child signal may force the final cursor back to ObserveChild. Operator, not
Network, drains at most one retained console lifecycle event per visit.

V18 child-loop source and behavior tests must prove the retained-first order
completion publication -> service-event publication -> egress publication ->
service-poll continuation -> new ingress -> new control, with badge and
publication-credit gates rechecked between units. Exact eligible retained
private work uses `seL4_Poll`; publication-uncredited and idle paths call
`seL4_Wait` directly with no ordinary `seL4_Yield`. SC exhaustion must rely on
native postponement, not a console Timeout handler. Ready and ordinary wakes
grant no publication credit; internal work preserves credit and one Publish
consumes it before mutation. Stable empty hints retire before a separate
service cycle. Root ACK is owed only after valid copy, sent only after durable
adapter retention, cleared before signal, and forces ObserveChild. Revoke parks;
graceful shutdown consumes credit, retires debt without ACK, and contains.
Within `ChildTurnUnit::PollService`, the v11 source guard must prove
`ServicePollUnit::StackIngress -> StackEgress -> Session` successor commit
before dispatch and `ServicePollOutcome::{Continuation, Complete}`.
`StackIngress` owns exactly one `Interface::poll_ingress_single` call;
`StackEgress` owns exactly one `Interface::poll_egress` call. Each returns
`Continuation` and retains `service_pending` across the gate recheck and
eligible local-Poll path. `Session`
owns connection/session RX, tick, TX, close, and relisten. Nonzero receive or a
complete wire-frame commit returns `Continuation`; only zero receive with no
commit returns `Complete` and clears the unit. The exact real-stack source
regression above must prove already-buffered oversize fragments plus QUIT make
progress without another packet/control wake. An error or preempting retained
publication must not lose the pending successor.

The V12 source and behavior guard must additionally prove that the exact
`TcpState::CloseWait` observation calls the existing idempotent
`request_disconnect`, that queued exact-generation output remains retained
until `close_ready`, and that the pre-existing close/end/listen path emits one
`Disconnected`, clears the generation, and restores LISTEN. No root wake,
additional poll unit, or modeled TCP shortcut may substitute for the real
two-stack peer-FIN regression.

The V13 source and behavior guard must prove that exact
`TcpState::TimeWait` observation performs old-generation `end` before TCP
`abort` and immediate `listen`, while `TcpState::Closed` retains its prior
path. The focused exact real-stack regression
`tests::fin_wait_retains_unsendable_output_until_peer_close_relistens` must
authenticate replacement connection 2 without advancing modeled time through
the smoltcp close delay and must observe exactly one old-generation
`Disconnected`.

The V26 source and behavior guard must prove that root's per-connection issued
latch is tested before publication and set only in the successful
`stage_disconnect` arm. `BoundaryError::Backpressure` must not set it. A focused
lower-cursor regression must model the exact first successful Disconnect,
subsequent `ControlCompleted` and `OutputDrained`, refusal to restage, and
progress through Ingress to ServiceTick while retaining Quit attribution until
`Disconnected`.

The V15 source and behavior guard must preserve V14's complete SendBatch validation
before mutation, exclusive pending-batch ownership, one external frame staged
per Session unit, and exact control/drain identity. The V34 guard must preserve
V33/V32/V31/V30/V29's capture, V28's terminal Disconnect fence, and V27 response-lane identity reset on
generation/connection transition. It must prove root-local capture and sealing
before publication, immutable terminal ordering, exact cursor retention across
backpressure, stale-identity discard, and CACHELOG snapshot allocation before
one lock acquisition followed by lock-free rendering. Each response Network
visit selects one useful unit without serializing routine success diagnostics,
and the ninth response opportunity pays exactly one ordinary phase debt before
resuming. Terminal completion must require the exact terminal batch's
`ControlCompleted` and `OutputDrained`; a conflicting physical synchronous
producer must retain its bounded BUSY terminal and prompt without replacing the
network owner. A selected-wrapper regression must additionally prove
`DefaultNetStack` delegates terminal-line publication, bounded identity,
response-lane inspection, response-budget polling, and
`console_event_pending` to the concrete VirtIO backend.

The V34 routine-diagnostic regression must preserve V33's private four-record
FIFO, FIFO order, drop-new saturation/overlength behavior, and exclusion from
the public log buffer. The event source guard must route
routine command begin/end, session attach/detach, TAIL start/stop, NineDoor
error, and CAT acknowledgment to that FIFO only for the selected QEMU VirtIO
composition. It must prove terminal response admission precedes diagnostic
retention. Serial/local-seat input, response ownership, stream END/prompt,
pending stream/flush, retained output, containment, network event/line, and
display work must each prevent the final-idle `RoutineAudit` unit. Initial
admission may attempt only the FIFO head and may pop it only after the complete
record is staged atomically; a full serial queue retains the same FIFO head for
a later idle turn. Each eligible visit with an admitted audit-only backlog may
physically transmit at most one byte. Every remaining byte and the audit-only
tag must survive across visits, and the tag may clear only after the final
`\r\n` byte drains. A focused stalled-UART regression must stage one exact
audit-only record, make device TX return would-block, and prove ordinary
`SerialDispatch` does not retry it ahead of a newly arriving `NetEvent` and its
following `NetLine`. Both network units must complete while the tagged bytes
remain unchanged and unemitted; after TX becomes writable, successive eligible
final-idle visits must emit exactly one byte apiece and reconstruct that record
once with exact `\r\n`, without loss, duplication, or reordering. A separate `SerialPort` regression
must prove every nonempty ordinary record, direct enqueue, and best-effort
enqueue promotes the tag, restores ordinary dispatch priority, and preserves
exact audit-before-ordinary bytes without loss, duplication, or reordering.
Compile-time `net-backend-virtio` guards must remove the FIFO storage,
provenance flag, and unit from Pi. Pi, linked-runtime, legacy/non-VirtIO, ordinary console-failure,
critical/fatal, and fail-stop sites must retain the existing raw diagnostic
helper. Existing `9p_batch.coh` must prove `/log/queen.log` still returns exact
ordered `batch-1|batch-2|batch-3` without routine-diagnostic contamination.
Target execution must repeat the rapid three-TAIL `observe_watch.coh` sequence
within the unchanged client deadline and prove that private diagnostic draining
does not alter response lines, terminal order, retry count, connection count,
or peer-EOF behavior.

The isolated VirtIO regression must repeat the same phase-order and ownership
assertions after console-network quarantine. Quarantine must preserve the outer
phase plus retained Runtime- and Network-unit states while fencing NIC polls; a combined
Operator+Runtime/IPC fallback fails.
Recovery-turn coverage must independently inject console-only, NineDoor-only,
and simultaneous pending records before each of the three ordinary phases. It
must prove console-first conditional probing, successor-cursor retention, zero
ordinary pump work during Recovery, phase-state plus Runtime/Network cursor
preservation, and one outer yield before the next containment turn or ordinary
phase. Console coverage must assert that mailbox `Retry` performs no authority
fence, the first latched turn performs only the value/resource latch plus the
lock-free scalar authority fence, and later refills execute exactly one of the
fourteen ordered material units: `SuspendTcb`; `UnbindSchedulingContext`;
`ScrubCleanSharedFrame(0)`, `UnmapSharedFrame(0)`, then the same pair for indices
1, 2, and 3; `DeleteFaultCap(0)`; `DeleteFaultCap(1)`; `RevokeAnchor`; and pure
`Finalize`. `Finalize` must leave `Complete` selected; the following idempotent
`Complete` turn alone publishes the exact proof and permits quarantine. A
containment error still consumes its exclusive Recovery turn and cannot fall
through into the pump. NineDoor coverage must separately
assert one latch-only first turn, then exactly one of the 18 ordered units per
later refill: suspend; request 0 scrub/clean then unmap; request 1 scrub/clean
then unmap; response 0 read-unmap, one writable `Page_Map`, scrub/clean, then
writable-unmap; response 1 with the same four units; quiet recovery-Reply
delete; two indexed quiet fault-cap deletes; anchor revoke; and pure Finalize.
Every unit must commit its successor before action and restore only that exact
unit on synchronous error. Scrub/clean must be bounded and lock-free.
Finalize must leave `Complete` selected; the following idempotent Complete turn
must publish the five-field proof and only then permit record removal.
`InProgress`, incomplete proof, and error all fence ordinary work.

Post-containment coverage must prove one quiet ordinary retained-output unit per
turn in the exact order `RootSessionTicket -> RootTicketUsage ->
NineDoorSessionTicket -> NineDoorSessionScope -> NineDoorSessionBinds ->
PendingStreamCursor -> PendingStream -> Finalize -> Complete`. Each successor
must be stored before its selected action; heap owners must move to
reboot-lifetime tombstones without drop, allocation, logging, or audit work.
Cleanup `Complete` may expose only the retained conditional
reboot/parser/serial/local-seat/tail/detach diagnostics. Service
fault/failure/teardown records must precede cleanup diagnostics, console must
precede NineDoor, queue admission must precede diagnostic commit, backpressure
must retain the record without eviction, and admission plus flush must occur on
distinct ordinary turns.
The QEMU run must additionally show that all generated sources are constructed
suspended and the exact fault registry is sealed before the console child
resumes. The retained V26/V13 lifecycle canary uses one exact boot and no concurrent gateway
owner. A raw session 1 must authenticate, the host must close that socket, and
a same-boot raw session 2 must authenticate, proving peer-close relisten before
two sequential direct `cohsh` sessions run. The first direct session
authenticates, attaches, begins a bounded streamed response, and closes only
after an exact control is staged or demonstrably retained; it must complete END
and QUIT so the server-active close reaches the changed `TimeWait` boundary.
Without a `10 s` delay, reconnect retry, or host-timeout change, the second
direct session must then authenticate and attach. The old connection-1 control
must produce exactly one completion,
zero session-2 bytes, zero child fault, and no false `OutputDrained`; session 2
must receive its own bounded ACK/TAIL/END response and the listener must remain
live. The transcript must bind both connection identities and the exact
root/child/CPIO/kernel hashes. A serial prompt, one successful ATTACH, or the
host client's timeout alone is insufficient. After that canary, the existing
ACK/ERR/END fixtures, malformed/authentication load, and separate fresh-boot
standard-fault containment plus budget-exhaustion/postponement injection must
prove root policy cannot be bypassed and both paths leave serial, local-seat,
root-fault, and root-emergency progress live. The postponement path must not
expect a console Timeout teardown.
The immutable V26 run and retained narrow V28 result completed that two-session
direct gate for their own transitions. The later V34/V17 and V34/V18 boots
remain failure evidence for their respective first failed layers. The current
V35/V18 boot must repeat the gate as a regression after the fixed response matrix;
historical success cannot
substitute for current artifact identity.
Simultaneous console-network and NineDoor fault injection must show two
or more root-control Recovery refills as required by their retained unit
cursors, with every console unit preceding every NineDoor unit, an outer yield
between material units, and no ordinary EventPump work interleaved.
The QEMU MCS transcript must contain no `TCB.SetAffinity` or affinity-failure marker:
root bridge attachment is bookkeeping, while execution placement comes only
from the generated SchedControl/SC binding. A live VM fault in the console
entry stack-zero loop or a root-fault timeout during `TCB.Suspend` is a failed
stack/temporal contract, not acceptable containment evidence. A standard
root-control fault in or after one admitted Runtime unit, Network unit, or
Recovery primitive remains terminal and fails the V35/root-fault-V6
candidates. Root-control and child V18 budget exhaustion must naturally
postpone rather than terminalize: their TCB timeout endpoint slots must be
empty, their already-valid adjacent refill must remain usable, and the
standard fault endpoint must remain installed. A Timeout label 5 at the root
outer yield fails V35. Passing compilation or compiler admission cannot
replace that live check.
Scale back network mirroring before command responses; never starve the
physical operator queues or fatal output. This gate deliberately performs no
Pi 4 execution and cannot qualify NineDoor containment, GENET, CYW43, or SDIO
behavior on Pi. The common MCS NineDoor containment code changes on QEMU and Pi,
but ordinary Pi scheduling remains unchanged and fresh Pi evidence is pending.

### Milestone 26e linked-driver MCS and coexistence gate

Run the QEMU-first driver gate against
`out/sel4/profile-v2/qemu-smp-production`; do not substitute Pi hardware or a
classic target build:

```bash
cargo test -p pi4-driver-abi -p pi4-driver-runtime --lib
cargo test -p root-task --test driver_task_mcs --test driver_faulted_call_recovery
.venv/bin/python -m pytest -q tests/test_driver_runtime_pipeline.py
SEL4_BUILD_DIR="$PWD/out/sel4/profile-v2/qemu-smp-production" \
  cargo check -p pi4-driver-runtime --target aarch64-unknown-none
```

The root target check additionally supplies the exact Worker manifest/archive
paths produced by the Worker pipeline. It must prove all seven compiler-owned
active-SC records, fixed command/Reply/completion/fault cap slots, disjoint
badge domains, `Write + GrantReply` command/fault caps, Read-only receive caps,
Write-only signal caps, one synchronous command association, and fault-before,
during, after, cancellation, reconstruction-generation, and normal-versus-
failure Reply exclusion. One-way bootstrap/background completions signal only
after their sequence-last ring commit and never consume the command Reply.
The runtime's nonblocking command seam is kernel-contract-specific: classic
seL4 uses `seL4_Poll`, while MCS must use `seL4_NBRecv` with the exact
compiler-generated child Reply slot 6. Once that receive retains a Call, both
kernels must sample or block only on the generated bound local notification in
slot 3; a second endpoint receive may cancel or replace the live Reply
association. An ordinary synchronous command that needs another bounded
service phase must preserve that association and yield locally because its
blocked caller cannot publish an endpoint continuation. Generation-bound
continuation commands must remain one-way and reject a Reply-bearing form. A
focused regression must reject an initial MCS NBWait/Poll spelling, reject
endpoint polling or blocking while Reply is live, and select local yield for a
synchronous multi-turn command. It must also prove that a zero-length,
nonzero-badge result from the initial MCS `NBRecv` preserves every exact
generated serial, GENET, SDIO, SDIO-DMA, and CYW43 notification, services at
most one routed owner quantum, rejects malformed or unowned badges, and leaves
the durable command for a later Reply-slot-6 receive. The exact MCS target
compile must exercise the real `NBRecv`, local `NBWait`, and local `Wait`
bindings. This source and target check proves Reply-association selection and
notification routing only; it cannot prove a child ran or that a pending
physical IRQ crossed its wait boundary on Pi.

The Pi serial regression must lock the hardware-validated BCM2711 mini-UART IER
values at RX `0x1`, TX-empty `0x2`, and combined `0x3`, despite the reversed
labels in the older BCM2835 peripheral PDF and in agreement with its published
errata; validate every `MU_STAT[27:24]`
fill level from zero through eight; reject an impossible level before the
generation-bound TX-SPSC consumer cursor advances; and preserve exact byte
order across partial, third, and later FIFO-empty wakes. Valid nonzero
occupancy with a full FIFO must select the exact slot-3 local-notification
wait; one through eight current free slots must re-enter one bounded owner
turn; zero occupancy must return to the combined endpoint-and-bound-
notification receive. A missing, poisoned, discontinuous, or over-capacity
cursor or impossible FIFO sample must select neither and fail closed. The
regression must admit root, serial-IRQ, and coalesced badges while rejecting
zero and foreign badges, prove that every bounded FIFO wake returns to the
outer command poll, and reproduce a source-polled `help` response whose first
eight bytes fill the FIFO. That turn must establish and retire the explicit
handler-rearm obligation before waiting on the long response tail; queued USB
input must remain ordered and undropped behind the shared physical-response
barrier. A separate terminal-poison regression must begin with failed linked
TX, then prove two successive queued USB commands reach the parser, the
physical-response barrier and serial-output queue return to idle after each,
one exact typed failure record is retained, response text remains HDMI-
mirrorable, and no root-UART MMIO fallback is invoked. The regression must
also prove the ordering device writes, completion
barrier, same-aperture IER/`MU_STAT` readback, completion barrier, then handler
ACK. Readback or ACK failure must fence TX, and the TX-idle probe must expose
that fence as `FAULT_DEVICE_UNAVAILABLE`. Host tests establish only register
arithmetic, cursor, route, and ordering invariants. Fresh boot-bound Pi serial
evidence is still required for physical IRQ delivery, losslessness, and
cadence.

`scripts/driver_runtime_manifest.py` must reproduce byte-identical newc and
JSON outputs from identical component bytes, validate the immutable
`configs/driver_runtime_classic_comparator.toml` source/component/archive
graph, and verify every new component and archive hash after staging. The
retired comparator digest is
`db2e353327cde2f91b37f40a7bf17905bb5f70cd27a999ba880a9fa7c2de9835`;
it is comparison identity, not MCS execution proof. The complete runtime model
suite is the QEMU/source
coexistence guard for unchanged CYW43/SDIO ownership, register order, fairness,
virtual-counter deadlines, retry ceilings, pair-restart cuts, rings, IRQ
ack/mask rules, and typed errors. This does not promote historical or current
Pi evidence. Until a live QEMU image passes, the MCS execution gate remains
open; until later fresh Pi tests pass, the Milestone task is not a Pi PASS.

The 256-executable-Worker Pi convergence candidate adds two deterministic
contracts without changing generated allocation or temporal values. First,
the deferred CYW43 supervisor may grant four consecutive Driver turns only
inside the exact finite `cyw43-cold-physical-lifetime` or
`cyw43-pair-restart` cursor. A pure regression must prove the preceding
Operator reconciliation, the four-turn hard cap, a return to Operator when a
Driver turn is not due, and immediate reversion to one-for-one cadence when
the cursor completes, is superseded, fails, or reaches ready state. Second,
routine successful Worker fault registrations may omit their individual UART
rows only after registration succeeds. Tests must retain detailed success
rows for every service, driver, and critical root domain, retain individual
missing/failure rows, and require the exact expected/registered seal before
activation. The serial reboot regression must additionally prove an
authenticated root prompt barrier between Queen attachment and `reboot`.

Exact source `f4fa54161bc959427eaeb805841fc8962c5c186a` and image
`2e74eff228fea5fb2125856774d8e9d922ac81070d44221dc398526fe959bdb9`
are failure evidence, not acceptance: their 272/272 registry seal and clean
USB/HDMI driver terminals coexist with Wi-Fi ending at supervisor turn 103,
`replay-sdio-engine`, `aggregate-deadline-expired`. The replacement must be
built and flashed from one exact clean source identity. On a fresh Wi-Fi boot,
boot-paired UART and Wi-Fi/USB captures must prove that the finite SDIO replay
cursor retires, association and Gate 8 complete, DHCP supplies the expected
address, authenticated `cohsh` TCP plus focused `.coh` scripts succeed, and no
unexpected fault, drop, no-reply, recovery, deadline, or quarantine counter
grows. The same boot must retain USB Gate 10 and keyboard input, bounded serial
command response, and terminal HDMI receipts; physical scroll/redraw quality
and bounded HDMI deferral debt must be observed separately because host tests
and a completed HDMI request do not prove a polished display. A distinct
GENET-selected boot must prove link, DHCP, authenticated `cohsh`, and the same
focused scripts without mixing child binaries or evidence between network
backends.

Pi- and QEMU-feature host tests, target compilation, static profile checks,
image construction, flash/readback, and the prior diagnostic cadence can each
reject the candidate but cannot establish physical Wi-Fi, GENET, USB, HDMI,
serial, network, performance, or Milestone acceptance. Compatibility review
must cover `coh`, `cohsh`, `.coh` workloads, Hive Gateway,
`tools/cohesix-py`, generated profile contracts, and the REST/QEMU pressure
harnesses. None consumes routine per-Worker success rows or the private
bootstrap cadence, so no grammar, schema, workload, or report change is
expected; `scripts/pi4_serial_reboot.py` and its focused tests are directly
affected by the prompt barrier.

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
- `configs/root_task.toml` — `sha256:92df0d52bc280aa56a0a254a6411bfb6c99f38d22411421e4a84b52ca00c5970`
- `configs/generated/root_task_resolved.json` — `sha256:cdbfdfa9f4de5c1cd8f8f9ef7233aff9465e15e5469cce6604bdde50872996ba`
- `configs/root_task_pi4_uboot_aarch64.toml` — `sha256:77c46ba8b66b805911c5eef1218ddb7348046ba58e3acb8bbde3b4eb54f67881`
- Pi `pi4_production` transient resolved binding — `sha256:a9916efc2ae0a11257a7b023ee559ede994fc943872454ad5d50d6cdde6c0c48`

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
- `tests/fixtures/transcripts/converge_v0/swarmui.txt` — `sha256:7c88f30c480d960990ccc741d775f8c13bb9fd4a29779e19a3445eb1f761cbdb`
- `tests/fixtures/transcripts/converge_v0/coh-status.txt` — `sha256:b026211888edf50538f61b66c79dc6ae1eaf59cc33b8dd3506e57ae60b3606c4`
- `tests/fixtures/transcripts/control_plane_v0/cohsh.txt` — `sha256:f43434e6b3071753596e919021e573cb7f6a9831123769dd7cefb5b0c115c1ef`
- `tests/fixtures/transcripts/run_demo_v0/cohsh.txt` — `sha256:d429aa09972892adaeabed60ef2a36e4fe366eb9e730a8467a85f27870957040`
- `tests/fixtures/transcripts/peft_roundtrip_v0/cohsh.txt` — `sha256:a761096db1c412e8b775f3bdb78a9aec79b95ef787d0e933406d23c20285f7db`
- `tests/fixtures/transcripts/trace_v0/cohsh.txt` — `sha256:56b97a2d8486ed783d7cb93d38ea67811d93df6efcc24d7ed97265a4df1b1c4f`
- `tests/fixtures/transcripts/trace_v0/swarmui.txt` — `sha256:56b97a2d8486ed783d7cb93d38ea67811d93df6efcc24d7ed97265a4df1b1c4f`
- `tests/fixtures/transcripts/trace_v0/coh-status.txt` — `sha256:a002a369390cc197714ac291ba08531966af658ed797c569f1ece4bab9b1820b`

## Trace fixture hashes
- `tests/fixtures/traces/trace_v0.trace` — `sha256:f5cd6eb44c1b4a51f5e1516dad9a7ec1f76fae148169744c9e8e3809f9b6c30b`
- `tests/fixtures/traces/trace_v0.hive.cbor` — `sha256:977113ebcfad69272cbb15ddc57e7ce1ccd1df87baa6568704253cacc55e8e2d`

## Guard
- `scripts/ci/check_test_plan.sh` verifies hashes, required scripted-stage references, and command alignment (`python3`, workspace/tests gates); `scripts/check-generated.sh` invokes it.
