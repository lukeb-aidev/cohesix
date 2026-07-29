<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document Cohesix test fixtures, hashes, and convergence guardrails. -->
<!-- Author: Lukas Bower -->

# Test Plan

## Mandatory Agent Execution Contract

This contract is normative for contributors and automation.

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
| `qemu` | 01-05 | Stage 03 builds one immutable artifact per unique manifest, then uses a fresh boot for every regression group. Stage 04 reuses the validated default artifact but starts another fresh boot. Result manifests bind source, profile, manifest, image, scripts, boot identity, counts, and log hashes. |
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
| `target.root-task-qemu-release` | 2 | `qemu-integration` | provisioned-target / qemu | `SEL4_BUILD_DIR="${TEST_PLAN_ROOT}/out/sel4/profile-v2/qemu-smp-production" cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-qemu` |
| `target.pi4-profile` | 2 | `pi4-transport` | provisioned-target / pi4 | `"${TEST_PLAN_ROOT}/.venv/bin/python" scripts/sel4_profile.py validate --repo-managed --profile pi4_diagnostic --build-dir "${TEST_PLAN_ROOT}/seL4/build_UBOOT" --require-artifacts --for-runtime` |
| `target.root-task-pi4-release` | 2 | `pi4-transport` | provisioned-target / pi4 | `SEL4_BUILD_DIR="${TEST_PLAN_ROOT}/seL4/build_UBOOT" cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-pi4` |
| `qemu.tcp-regression` | 3 | `qemu-integration` | target / qemu | `scripts/cohsh/run_regression_batch.sh` |
| `pi4.tcp-regression` | 3 | `pi4-transport` | target / pi4 | `scripts/cohsh/run_regression_batch.sh` |
| `qemu.rest-regression` | 4 | `qemu-integration` | target / qemu | `scripts/ci/test_plan_stage_04_rest_multiplexer.sh` |
| `pi4.rest-regression` | 4 | `pi4-transport` | target / pi4 | `scripts/ci/test_plan_stage_04_rest_multiplexer.sh` |
| `release.unique-governance` | 5 | `release` | target / qemu, pi4 | `scripts/ci/due_diligence_gate.sh` |
| `ui.swarmui-playwright` | conditional | `ui` | conditional / qemu, pi4 | `scripts/ci/swarmui_ui_gate.sh --run` |
| `performance.gateway-telemetry` | conditional | `performance` | conditional / qemu, pi4 | evidence-only: telemetry-summary-matrix, ops-csv, ramp-csv, ramp-svg |
| `federation.three-hive-relay` | conditional | `federation` | conditional / qemu, pi4 | evidence-only: federation-result-manifest, relay-counter-snapshots, evidence-timeline, scale-summary |
| `pi4.hardware-acceptance` | conditional | `pi4-hardware` | conditional / pi4 | evidence-only: pi4-image-readback-identity, pi4-gate-proof, pi4-capture-manifest, pi4-repeatability-report |
| `release.bundle-validation` | conditional | `release` | conditional / qemu, pi4 | evidence-only: macos-bundle-result, ubuntu-bundle-result |
<!-- test-plan-catalog:end -->

## GitHub Actions gate mapping

`.github/workflows/ci.yml` is the sole repository-authored workflow and keeps
the stable aggregate check `ci`.

- Source/integrity, consolidated hermetic Rust/Python production-feature
  coverage, replay-mode UI coverage, and dependency advisories run as
  independently cacheable jobs. The aggregate waits for every required job.
- The workspace test lane uses the bounded host-wide concurrency policy rather
  than consuming every CPU. Root-task feature suites and
  `pi4-driver-runtime` remain serialized at their known stateful boundaries.
- Cargo registries, build outputs, Playwright browsers, and exact-version audit
  tools use content-keyed caches. Cached tools are version-checked; mandatory
  `cargo audit` and `cargo deny check advisories` are never skipped.
- Hosted CI compiles `pi4-driver-runtime` for `aarch64-unknown-none` and runs
  the complete host-safe QEMU/Pi production-feature suites. Exact root-task
  release builds and QEMU packaging remain on the provisioned macOS ARM64 lane
  because the canonical external seL4 trees are not vendored.
- The weekly workflow repeats the hermetic matrix and fresh advisory checks.
  Provisioned QEMU Stage 03/04 cadence is recorded separately and must not be
  represented as hosted-runner boot evidence.

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
  `ElfloaderRootserversLast=ON` and an embedded QEMU `virt` DTB generated with
  `virtualization=on`, so PSCI records `method = "smc"` for the Cohesix QEMU
  launcher.
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
- If the host lacks EL2/virtualization support or KVM cannot provide the
  selected GICv3 configuration, set `COHESIX_QEMU_VIRT=off` and/or
  `COHESIX_QEMU_MACHINE_EXTRA=kernel-irqchip=off` when invoking the release
  `qemu/run.sh`; the launcher must still agree with the generated GICv3 truth.
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
Run in order. Skips produce INCOMPLETE markers and the stage will fail.
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
with exact `pytest`/`pyserial` pins. Repository, client, due-diligence, and
runner contract tests execute in one pytest process; four mock examples execute
once in a separate smoke action. A missing Python lane is INCOMPLETE, never
PASS.

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
  `release-qemu` AArch64 root-task check.
- Pi 4 profile validation against
  the immutable `seL4/build_UBOOT` `pi4_diagnostic` artifacts, followed by the
  `release-pi4`
  AArch64 root-task check.

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

Dedicated-driver-task closure is stricter than contract declaration: `DRIVER_TASK_DEDICATED` must cover the required active roles, `DRIVER_TASK_COMPATIBILITY` must be `0`, `DRIVER_TASK_DEDICATED_READY=yes` must be present, `DRIVER_TASK_FAILED_COUNT=0` must be present, serial, USB/local-seat, display, selected network, selected-role SDIO (`DRIVER_TASK_SDIO_DEDICATED=yes`) for Wi-Fi, and PCIe role booleans must all be `yes`, and substrate/capset/fault/revoke/scheduling/per-driver-affinity/VSpace plus pointer-free IPC, owner-state proof, sealed runtime descriptor proof, and active-network identity fields must all be `yes` when `scripts/pi4_gate_proof.sh --require-driver-task-proof` is used. Physical Pi bootstrap is limited to the selected generated isolated runtime hardware contracts; RTL8139 and virtio-net remain QEMU compatibility contract coverage only. Owner-state proof requires one `DRIVER_TASK_OWNER_STATE ... hot_path=<exact> owner_state=driver-owned descriptor=present descriptor_version=5 descriptor_seal=valid artifact_hash=nonzero root_pointer=no` line for each current acceptance hot path: `serial-console`, `usb-keyboard`, `hdmi-text`, `pcie-root`, and the selected network path (`genet-nic` for wired or `cyw43-wifi` plus `sdio-host` for Wi-Fi). The canonical sealed descriptor fragment is `DRIVER_TASK_OWNER_STATE ... descriptor=present descriptor_version=5 descriptor_seal=valid`. Split clients must carry `bus_link_seal=valid` for USB-to-PCIe or CYW43-to-SDIO while non-split roles report `bus_link_seal=none`. Aggregate owner-state text, inferred hot paths, inactive-network hot paths, truthy aliases such as `owner_state=yes`, or pre-seal `descriptor=present root_pointer=no` logs without descriptor-seal fields must fail current closure.
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
staging coverage must traverse all eight published root aliases for the exact
32-KiB shared aperture. Clearing a partial transport must also zero cached
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

The CYW43 software-closure gate is authorized by active Milestone 26d task
`m26d-cyw43-hardware-free-closure`, where the latency defect was discovered,
and restores Reopened Milestone 26b task
`m26b-wifi-sdio-notification-dpc-closure`. It exercises the host-testable
production transaction data and state transitions (`begin_turn`, frontier
reservation, retained submit, completion miss, continuation grant, immutable
ticket/completion validation, completion commit, and cached replay). The host
ring adapter executes the production sequence-last command publication, stable
owner intake, sequence-last completion publication, and stable client read,
stages the reciprocal owner descriptor, and obtains the completion from the
real descriptor/controller service path rather than fabricating a direct
result.
Physical mapped addresses, cache-maintenance effects, seL4 notification
send/receive, and target transaction entry/exit remain target-compile checked
and require Pi proof. Under the ordinary EventPump, each
outer turn opens one monotonic CYW43 operation permit and may execute no more
than one child-runtime or HAL operation; a rejected second attempt must leave
the retained ticket, deadline, payload fingerprint, generation, and cursor
unchanged.

Ordered Gate 8 coverage must exercise one production diagnostic snapshot with
these exact subgates: `8a-pair-generation`, `8b-control-program`,
`8c-join-terminal`, `8d-association-link`, `8e-bssid-refresh`,
`8f-eapol-keys`, `8g-post-key-maintenance`, and `8h-data-admission`. Tests must
prove all of the following:

- 8a and 8b are derived from one current linked pair/control epoch; 8c through
  8h are derived from one current logical connection generation.
- Snapshot evaluation is passive, immutable, and performs no HAL, SDIO,
  runtime, retry, completion, or owner mutation. All eight records are formatted
  from that single value and admitted with the immediately following Ready
  record as one all-or-nothing retained transaction.
- Ready requires the same stable pair epoch and logical generation on two
  consecutive ordinary control turns. Both observations must have no
  current-generation pending host-EAPOL event or queued pre-secure EAPOL RX
  frame, no host-EAPOL prompt, session work, deferred-reauthentication, or BSSID
  work, no maintenance or logical control owner, no prompt-poll or
  terminal-drain cursor, no retained HAL driver-task request, and no
  recovery/rejoin. The linked SDIO DPC ring must have producer equal to
  consumer, zero current-generation flags, and the same nonzero epoch, producer
  watermark, and cumulative overrun/IRQ-ACK-failure counts on both
  observations. Stable historical nonzero counters after typed recovery are
  admissible; new counter movement is not. Any owner activity, DPC publication,
  counter movement, DPC epoch change, or logical/pair generation change resets
  the candidate. The producer revalidates the exact
  pair/generation/DPC/history receipt and commits that snapshot before `ready`,
  then rechecks it after consumer-token publication. Tests must reject pending,
  flagged, torn, zero-epoch, producer-advanced, epoch-advanced, and
  counter-advanced DPC snapshots while allowing stable historical counters and
  normal DPC activity after accepted Ready.
  First-cause deferred-recovery and terminal-drain diagnostics must survive a
  rejected exact receipt, consumer publication, or Ready output preflight and
  clear only after the complete retained Ready transaction linearizes.
  Partial, reordered, duplicate, mixed-generation, generation-regressing,
  cross-recovery, and changed-before-commit snapshots fail closed.
- Transport attachment publishes `stabilizing`. Initial Gate 8 publication in
  the sole `attempt=1` outer boot episode uses one absolute
  `now + 90,000 ms` deadline. Gate 8 is passive: a logical subgate failure
  remains inside its bounded gate-local policy and cannot request pair repair.
  A consumed-once typed pair repair and a material Ready retraction before Gate
  10 retain that original deadline; Gate 10 alone arms a later fresh, bounded
  steady-state recovery episode. Deadline exhaustion must retain the complete
  eight-line snapshot and adjacent
  `CYW43_GATE8_TERMINAL ... action=quarantine`, emit terminal
  `status=permanent`, and quarantine attached Wi-Fi while serial, local-seat,
  HDMI diagnostics, authentication, and reboot remain live. Only a separately
  typed runtime/SDIO fault or issued-unknown physical operation may invoke the
  consumed-once pair repair, and it cannot extend the Gate 8 deadline. Tests
  must prove output backpressure permits only hardware-free operator-output
  turns; schema/route/capacity preflight mutates no output and invokes no
  terminal decision; the final typed-recovery probe may decline terminal
  policy; a clear probe commits the explicit decision cut immediately before
  atomic retention; and no child/network poll, automatic whole-bootstrap
  backoff, reset, second `begin`, or attempt 2 is admitted after that cut.
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
- A fresh non-stable or different-generation observation retracts `ready` to
  `stabilizing` when it represents a material pair, generation,
  control-program, carrier, security, handoff, recovery, rejoin, or
  post-publication loss-invariant failure. The old snapshot becomes
  non-authorizing and a later `ready` requires a complete new snapshot. A
  delayed HDMI Ready/prompt, including bytes already handed to the local-seat
  queue, is superseded by a canonical Stabilizing redraw. Conversely, bounded
  same-pair/current-generation post-secure key maintenance must keep `ready`
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
  pre-Ready publication-quiescence check must nevertheless wait for that exact
  request and its terminal-drain/HAL ownership to finish; after Ready, one exact
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
- Root and child tests must bind their queue capacity and the child bounded RX
  drain budget to
  `pi4_driver_abi::DRIVER_RUNTIME_CYW43_RX_QUEUE_CAP=50`. They must reject
  divergent private capacities, prove the root can preserve one complete child
  backlog, and keep queue saturation subject to the Gate 8h rules above.
- `wifi diag` and `wifi probe-ht` formatting coverage must preserve six
  untruncated `wifi: data_handoff` records: generation/commit/baseline tokens,
  baseline generation, and `queue=<used>/50`; current/baseline root-drop and runtime-overflow counters;
  total/last-token/last-count stale-purge state; boot-first loss state; and
  current-handoff post-commit first-loss state; plus explicit
  `consumer=<blocked|open>` control-lane state. It must also preserve
  boot-cumulative association service-turn/Join-start counters and the latest
  complete non-recovery Gate 8 frontier so sticky recovery cannot replace the
  causal subgate with only a generic pair-failure state. The passive
  maintenance snapshot must render as adjacent state and action records,
  preserve generation/current/pending, all four masks, next stage, exact action
  generation/request/issued/turn fields at their maximum widths, and never
  truncate either record. A positive loss record must retain sampled
  generation, commit state where applicable, reason,
  queue length, channel, EtherType, and priority and end with
  `attribution=current-epoch-sample`. Tests and normalizers must not reinterpret
  that sampled generation as producer, runtime, SDIO, or physical-owner proof.
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
versus reset/regression, `ready -> stabilizing -> ready` reproof, and rejection
of Gate 9/10 evidence from any other generation.

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
its own 220-millisecond containment deadline. Shared-ABI tests must derive the
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
bounded preissue/issue owner quantum: SDHCI block-gap
inspect/repair/verify, DMA-authority/idle snapshot, full immutable control-block
staging, status clear, timeout, block size, block count, argument, transfer
mode, exactly one COMMAND, and then exactly one BCM2835 DMA `ACTIVE` write.
The status-clear step must include the same request-owned readback fence.
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
DPC, grant, continuation, fault-telemetry, and cumulative-counter byte outside
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
changed. The passive
`Cyw43ServiceWorkSnapshot` and EventPump durable-resume identity must include
the same epoch with connection generation and pair epoch; changing only the
physical epoch must invalidate both durable Network resume and any pending
operator fence without admitting Wi-Fi work under the replacement identity.

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
executes at most one CYW43/SDIO child physical operation.

Closing the quantum must fence fresh pair work. An exact already-`Prepared` or
already-`Issued` parent may drain alone. Tests must prove that an ABI-invisible
sequence-zero `Prepared` parent keeps an open quantum actionable and that a
closing parent advances in successive bounded Network turns rather than one
turn per physical-operator rotation. Every turn must admit at most one child
physical operation and revalidate the same request plus a monotonic
`Prepared`-to-`Issued` state. A time/turn cap or pending physical or buffered
console response may yield, but the next admitted slice must resume only the
same fenced parent. Restore order is CYW43 then SDIO after its exact terminal.
Tests must reject a fresh or switched parent during close, issue-state
regression, wrong-generation reuse, torn phase/reservation state, invalid
active-parent state, and partial acquisition or restore. A clean partial
acquisition must roll back the exact reservations; any state that cannot be
rolled back completely must poison the lease and request pair recovery.
Pair-epoch advance, quarantine, and reboot must either complete the same exact
close/drain or enter pair recovery; none may silently clear or alias ownership.
GENET must remain outside this state machine, retain its ordinary
single-`Network`-turn rotation, and emit no WiFi priority-lease telemetry.

For every root-to-CYW43 generation, including zero, the authority edge remains
one exact 24-byte continuation grant followed on a separate turn by a
reserved-root-badge notification scheduling hint; endpoint input is rejected.
Tests must prove sequence-zero prepare is invisible to the autonomous runtime,
and neither commit nor authority publication can repeat after issued-unknown
ownership. The request-bound lane must still reject a nonphysical profile,
zero/unpublished priority, or an already-bootstrap-priority runtime.
After the one-way owner handoff deletes root's SDIO endpoint, delegated
CYW43-to-SDIO foreground and DPC commands use the same exact grant authority
shape and a distinct badge-256 notification scheduling hint. Tests must
exercise the real shared record and owner cursor: publish magic, request
sequence, every-action fingerprint, independently authoritative SDIO
generation, and a monotonic nonzero grant id sequence-last; let the owner publish
`consumed_grant_id` irrevocably before spending one quantum; re-signal an
unacknowledged id without replacement; publish a new id only after exact
acknowledgement; classify the exact acknowledged predecessor as a
non-authorizing wait while its replacement is unpublished; and fail closed on
torn, stale, mutated, wrong-generation, mismatched-consumed, replayed, aliased,
or exhausted-id state. The notification is only a wake hint. Both foreground
and DPC paths must alternate separately admitted `Poll -> Grant -> Poll` turns,
with no acknowledgement poll or second child/service/HAL operation composed
into the grant turn.
An event's source/frame-length hint must be copied into the DPC cursor only on
the first admission of that exact sequence. Later grant and completion turns
must not reapply it: doing so can resurrect `I_HMB_FRAME_IND` after a completed
F2 read and create an endless same-frame drain. A different sequence while one
is active must poison the generation rather than merge event identities.
Production-chain coverage must also drive queue-empty hintless op10
`CONTROL_POLL` and op8 `RX_POLL` commands through the real reciprocal
`DPC_ACTIVATE` owner ring, exact source-event watermark, ordinary DPC
controller seam, and post-probe FIFO read. It must prove an association event
and a data frame are delivered, an event/probe race coalesces without a second
acknowledgement, stale work is rejected without mutating the replacement
generation, malformed current and issued-unknown completions poison without
replay, and a non-hintless empty poll remains queue-only. Every child
submission, grant, completion poll, DPC action, and post-probe read must consume
a separate outer turn; no foreground Function 1 or Function 2 receive is
permitted. A zero-status `SOURCE_PENDING` event must inspect status through the
ordinary DPC lane, perform zero Function 2 reads, ignore stale shared-aperture
bytes, consume the event, and rearm the sole SDIO owner. A real
`I_HMB_FRAME_IND` or validated retained frame hint remains mandatory for the
fixed first read. The durable present-or-exact-consumed predicate must also be
tested directly against verified same-generation consume observations for
event N and then N+1: the live consumer watermark and still-valid retained slot
must continue to authorize N without replay while both a future unconsumed
event and a sequence outside the finite retained range reject. A blind ring
advance, overwritten slot, stale generation, mismatched sequence, or recovery
state must still fail closed. This direct state cut is not a permitted
production interleaving between cached foreground completion and watermark
refresh.
The adversarial production chain must exercise the real scheduler seam: a
foreground source probe commits its event and exact owner completion; ordinary
DPC service must remain deferred while foreground ownership is active; cached
completion replay ends that ownership; the next scheduler iteration must
refresh the watermark before admitting DPC; only then may the DPC consume and
rearm the event. The final op8/op10 turn must reach `PostProbe` without a second
activation, poison, or replay. Zero result, wrong sequence or generation,
mismatched ring consumer, and recovery-poisoned state must all reject.
An injected DPC child fault must retain its primitive detail, result, frame,
event sequence, action, and I/O phase through the later prompt quarantine.
Exactly one fresh child ticket is allowed only for a telemetry-bound
`CONTAINED` entry-inhibit fault that proves no command issue; the second such
failure and every command-or-later, owner-poisoned, malformed, timed-out, or
issued-unknown cut must fence the pair without advancing the event.

Backplane-attach coverage must drive the production retained cursor through
ALP request, every ALP read, FORCE_ALP, the 65-microsecond settle, the Pi
pull-up clear, LOW/MID/HIGH window programming, the first ChipCommon read, and
completion. Each child submission, continuation grant, child completion poll,
retained deadline observation, and pull-up-clear operation must consume its own
outer EventPump turn.
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
one new child issue per outer EventPump turn and exact Linux ordering from
stale interrupt clear to DPC activation. Exercise 51 retained ARMCR4
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
Production-chain coverage must additionally drive normal control and EAPOL TX
through one cached-window F2 CMD53 child, drive a genuine cache miss through
exact LOW/MID/HIGH CMD52 writes followed by F2 with no per-packet IORx child,
drive all 20 post-F2 release children into real retained DPC activation, and
drive one DPC event through owner-backed status, F2 read, empty-confirmation,
and post-status work before foreground queue consumption.
Exactly 256 subsequent control/RX polls must consume 256 outer turns and issue
zero SDIO-owner operations. Pre-issue terminal, post-issue unknown,
stale-generation, action-fingerprint, timeout, and continuation-grant cuts must
not issue a second child or mutate the replacement generation. Exercise the
shared op11 outcome classifier through the real association, PTK/GTK, and
SCB/filter/BSSID maintenance consumers: pre-TX `NOT_READY` and decoded firmware
replies are terminal, while every encoded post-TX reply timeout must suppress
Gate 7a/cursor advancement, publish the immutable ambiguity ticket, and enter
the exact pair restart with no same-generation replay. Association coverage
must additionally prove that an exact HAL-issued Join at
`CONTROL_TX_BEGIN` remains event-unarmed, stale sequence or route progress
cannot arm it, and only its exact post-Function-2 progress can do so. Inject an
EVENT after the initial pre-TX drain while the cursor is waiting for credit;
that event must complete with zero Function 2 writes before the single Join
write is admitted. At the later final SDIO pre-issue boundary, assert host
`CARD_INT` for a Join-marked Function 2 child and prove a typed not-issued
terminal, zero controller/DMA/FIFO work, unchanged operation-11 parent and
absolute counter deadline, no SDPCM advance or pair recovery, one forced
`DPC_ACTIVATE` consumed through DPC, and exactly one later Function 2 issue
after source clear. The same asserted source on an unmarked Function 2
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
readback was not published, and passive `wifi diag` must render both complete
GPFSEL3/GPPUPPDN2 words and the expected masked values.

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
control/data/any-frame polls, generation, association, and host-EAPOL recovery,
data TX, ARP/GARP output, and the ordered pre-Join drain snapshot. The 256-poll
drain must consume exactly 256 separately opened outer turns. Tests must prove
the Join-only final pre-issue source fence closes the interval after that
snapshot without extending the policy to generic control/data descriptors.
The typed not-issued edge must preserve the same logical parent and route the
level source through the sole DPC lane before one later source-clear issue.
Failure-cut tests must reject stale completions, forbid same-generation replay after
issued-unknown ownership, and resume or fail deterministically at every
retained action. EventPump/NetStack tests must prove Wi-Fi urgency is retained
across later turns rather than implemented as private pre-root, EAPOL,
tail-ingest, TCP-flush, hot-dispatch, or smoltcp device bursts. A root-wake hit
must act only as edge urgency: consume it on the first admitted Network turn,
leave the current-physical-lifetime/generation/pair durable work level armed,
and continue bounded Network service without another notification until an
exact same-identity idle snapshot clears it. Coverage must exercise every
reason-mask class, coalesced/lost/repeated notifications, stale idle snapshots,
physical-lifetime changes, pair and connection generation changes, quarantine,
reboot, and selected-NIC change.
A complete TCP command, physical response/input, turn cap, and time cap must
retain unfinished Wi-Fi work behind a fence and prove `Serial`, optional
`LocalSeat`, and `Dispatch` each receive their bounded turn before Network
re-admission. A queued USB report and a buffered complete network command must
not be bypassed. GENET must neither sample nor retain the CYW43 snapshot and
must leave the CYW43 urgency, durable-resume, and operator-fence counters/state
untouched. The passive `rx_wake` state and `rx_wake_counters` records must
remain separate and untruncated at maximum counter values. Tests must also
stage a child-invisible sequence-zero
NetData request at the Gate 8 handoff, prove the next outer turn decodes it
through HAL's immutable retained identity and advances it beyond `Inactive`,
then prove host-EAPOL receives the next fresh prompt-poll turn without a pair
recovery latch.

Parent-replay coverage must table every CYW43 operation against transfer
stages 1 through 7. Only stage-1 `0x5103` on the seven single-action parents may
retry in-generation. Stage 7 is admitted only for the Join-marked Function 2
child and is a proven not-issued DPC deferral, not recovery or replay of an
issued action. `TRANSPORT_INIT`, `FIRMWARE_PREP`, `RELEASE`, and every other
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
pair recovery, one child operation on each successive outer turn, and allow
authentication suspension/backoff only after that retained action is gone.
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
`u64::MAX`. The episode admits at most one ordered full CYW43/SDIO pair repair
and one corresponding `status=recovery` while remaining `attempt=1`, but only
for a typed runtime/SDIO or issued-unknown physical fault.
Successful context replay alone must preserve the spent repair count and the
original absolute Gate 8 deadline; an injected recurring transport fault then
returns typed `cyw43-pair-recovery-limit` and terminates bootstrap instead of
starting another repair or outer attempt. A pre-issue lease conflict that
performed no child action and changed no scheduler state must clear locally;
issued or scheduler-mutating uncertainty must request the one bounded repair.
Gate-local association, DHCP, and protocol retries remain independently
bounded and must not mutate the boot-episode identity.
Separate lifecycle coverage must hold every logical Gate 8 failure until the
original 90-second deadline, retain `CYW43_GATE8_TERMINAL`, publish one
`status=permanent`, and quarantine without entering `status=recovery`.

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
After quarantine, the ordinary linked-runtime phase test must also inject a
pending CYW43 root wake plus HDMI work and prove that the wake and NIC remain
untouched while one bounded `Display` turn remains reachable before `Serial`.
Serial and qlog must contain the exact machine record byte-for-byte; each HDMI
line must begin `[drivers] WiFi` and contain no
`CYW43_BOOTSTRAP_SUPERVISOR`. Coverage must delay display long enough to fill
the ordinary FIFO, add a terminal transition at saturation, prove that the
FIFO plus terminal reserve does not overwrite start/progress transitions, lose
the readiness release, or affect serial/qlog, and drain at most one rendering
per later `Display` turn. A second boot `begin`, any `backoff`, attempt greater
than one, a second recovery before same-generation Gate 10, a recovery that
renews the Gate 8 deadline, repeated terminal record, same-turn display
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
zero backoff and no deadline renewal. `failed` requires `backoff_ms=0` and the
exact no-next-attempt sentinel. It also permits
`attempt=1 status=permanent` as the sole pre-`begin` record when fallible
construction or immutable configuration/artifact validation fails. A
maximum-length terminal record must survive
saturated background breadcrumbs without evicting a response tail or prompt.
Only same-generation Gate 10 plus attached address/TCP readiness may
open one distinct steady-state runtime-recovery episode with one fresh
consumed-once pair repair; that lifecycle remains `attempt=1` and cannot reset
the boot result.

Local-seat retained-service coverage must classify `Pending`, `Complete`, and
`Failed` through the production HAL wrapper. Every normal `Pending` phase must
leave the immutable USB command, readiness flags, no-reply counters, and
recovery state unchanged; a pre-issue terminal `Failed` must clear the active
command and fail closed exactly once, while issued-unknown retains its poisoned
identity without replay. Tests must also prove that sustained Pending traffic
cannot manufacture the pressure signal that suppresses HDMI. Adversarial lease
faults before and after the issue boundary must prove that USB, serial, and HDMI
never request CYW43/SDIO pair recovery: pre-issue requests fail locally, while
issued-unknown requests retain their immutable identity in a poisoned slot.
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
bounded CYW43 network weighting. Selected-CYW43 edge-admission coverage must
inject one empty-to-nonempty root wake from `Display`, `LocalSeat`, and
`Dispatch`, prove that `Serial` owns the first bounded turn and `Network` is
admitted on the next turn when no physical input/recovery owner is pending, and
prove that the admitted turn still uses the sole existing CYW43 owner. A queued
WiFi HDMI status may be preempted but must remain queued. Keeping the
notification level latched without incrementing its hit epoch must not re-arm
the admission or skip another physical phase. Real serial/local-seat input and
USB recovery retain their operator precedence; quarantine and reboot clear the
local cursor without a NIC turn or wake poll. The same injected wake under
GENET must leave its ordinary phase result, CYW43 wake counters, and CYW43
quantum counters unchanged.

The central `network_contract_service_admissible` check must fence both direct
EventPump service entries, ordinary `poll_runtime` and pre-root
`poll_pre_root_network`. Coverage must prove the check runs before Network
service and again immediately before either CYW43 polling or retained TCP
flushing. Missing, active, failed, or replaced physical epochs and
recovery-active snapshots must clear the Wi-Fi wake and durable-resume tokens
and admit neither operation; the same cases must leave GENET service unchanged.

`Serial` may perform one TX-first reciprocal-ring turn; `LocalSeat` then polls
one retained USB keyboard turn so fresh physical input is buffered before the
network quantum. `Dispatch` may consume one serial, buffered local-seat, or
buffered network command without polling the NIC or flushing TCP. `Network` may
perform exactly one ordinary NIC service
or one retained GENET response flush and must leave any received command
buffered for a later `Dispatch` turn. NIC polling, TCP flushing, and command
dispatch must occupy distinct outer turns. A dispatched GENET command must
schedule zero same-turn flushes and retain a cursor owned by its active
connection. Each later
`Network` phase consumes exactly one flush attempt, bounded to eight phases
normally or sixteen while the display reports backlog pressure. A second
buffered command stays behind the first cursor, and a changed or absent active
connection rejects stale cursor work. A data-ready CYW43 connection must not
create the GENET cursor. A response flush, exact socket/parser work,
runtime/root RX backlog, current valid pending or masked SDIO DPC event, or
retained CYW43 NetData/TX/fairness continuation may retain `Network` for at
most the compiler-declared CYW43 `max_ops_per_turn` service bound (currently
192 successive outer turns) and 25 ms on the seL4 virtual counter, whichever
comes first. The fairness predicate includes one generation-bound post-TX
cursor. `RequiredPoll` must block fresh TX until an exact nonfault op8 terminal;
`FrameReady` clears it, while an initially empty `Idle`/`Progress` terminal
must release fresh TX and transition the same cursor to an eight-millisecond
counter-deadlined receive watch. During the watch, the existing NetData op8
lane remains weighted but copied RX, pending TX, ARP, maintenance, and control
work take precedence. A frame, expiry, generation invalidation, or recovery
must clear it. Tests must prove wrong request/generation and fault terminals
cannot advance it, a new accepted TX rearms `RequiredPoll` without allocating
a second cursor, and GENET never reads or reports this state. Authentication
without pending work must not extend the quantum. Tests
must prove the first five exact-work turns remain contiguous without a forced
four-turn operator rotation. They must advance time between outer turns and
prove that a quantum already at its deadline returns to `Serial` with zero
additional NIC/SDIO operations. An idle selected interface must not acquire the
pair priority lease. The first actionable selected-WiFi turn must reserve and
boost SDIO then CYW43 once; later exact current-generation parents in the same
quantum must add no scheduler writes. Every quantum exit path must latch the
fresh-work close fence. An exact active parent, including an ABI-invisible
sequence-zero `Prepared` parent, must prevent an open lease from closing between
stages. If close has already fenced it, successive admitted `Network` turns may
advance only that same parent, by at most one child physical operation per
turn, while rechecking request identity and monotonic issue state after every
turn. A cap or operator response may yield to `Serial` and `LocalSeat`, but the
next Network slice must resume the same parent; request substitution,
`Issued`-to-`Prepared` regression, or disappearance without a typed terminal
must request pair recovery. After the exact parent terminates, restore order
must be CYW43 then SDIO before the EventPump exits the slice.
This bounded service is available before TCP authentication so raw DPC and
retained owner work cannot be starved while establishing a connection. Every
turn must still admit no more than one CYW43 physical operation, and either cap
must release to `Serial` and `LocalSeat`. A complete buffered TCP command and a
pending physical response must also exit immediately. Tests must prove idle,
stale-epoch, poisoned, overrun, acknowledgement-failed, and inconsistent CYW43
DPC work plus GENET do not enter the quantum. GENET must retain its ordinary
single-Network-turn rotation and all CYW43 quantum counters must remain zero.
At `Network` entry, quarantine and an already owned physical response must skip
NIC inspection and polling, open no CYW43 quantum, consume no CYW43 turn, and
return directly to `Serial`. The sole exception must be the exact
network-origin reboot acknowledgement drain; after that required NIC service
turn, or when a physical response becomes pending during an admitted operation,
the next phase must be `Serial` rather than `Display`. `netstats` must expose
quantum count, turns, maximum duration, zero-valued compatibility
`operator_yields`, and exit reasons. Selected WiFi must additionally emit:

```text
netstats: cyw43_priority_lease state=<inactive|acquiring|open|closing|restoring|poisoned> pair_epoch=<n> active=<yes|no> close_pending=<yes|no>
netstats: cyw43_priority_lease_counts opens=<n> closes=<n> restores=<n> recovery_revocations=<n> amortized_requests=<n> failures=<n>
```

The focused acceptance tests
`cyw43_sdio_network_priority_lease_amortizes_scheduler_transitions`,
`cyw43_sdio_network_priority_lease_closing_drains_exact_parent_and_blocks_fresh_pair_work`,
`cyw43_sdio_network_priority_lease_partial_failure_is_rolled_back_or_poisoned`,
`cyw43_sdio_network_priority_lease_rejects_generation_aliases`,
`cyw43_rx_fairness_transitions_to_bounded_receive_watch`, and
`cyw43_receive_path_drains_post_tx_fairness_before_queued_arp` must pass.
Together they must prove exactly four scheduler writes for a clean quantum
regardless of how many exact parents it covers (`SDIO boost`, `CYW43 boost`,
`CYW43 restore`, `SDIO restore`), close-time fresh-work rejection and
exact-parent drain, clean rollback versus poisoned recovery, current-generation
binding, and GENET non-applicability. The WiFi `netstats` fixture must preserve
both complete records at maximum counter widths and a quiescent clean sample
must report `state=inactive active=no close_pending=no` and `failures=0` with
`opens=closes=restores`; after steady traffic, `amortized_requests` must be
nonzero. A recovery revocation is acceptable only with matching typed
same-slice pair-recovery evidence. The GENET fixture must omit this WiFi-only
line and keep all CYW43 quantum counters zero.

CYW43 device tests must also prove that a retained TX or unproved credit window
withholds smoltcp's paired RX/TX token, preserves the copied RX frame, advances
only the sole retained owner, and produces zero fabricated TX drops before the
frame is later delivered.

The console socket pack must cover the maximum enabled profile: active and
standby console acceptors, DHCP, two UDP self-test sockets, two TCP self-test
sockets, and the optional outbound probe. All application close origins enter
one `Draining`/`PeerCloseWait`/`Closing` state machine. Clean `QUIT` must drain
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
fallback, USB backend polling, HDMI/echo re-entry, and network polling must
remain absent. Accepting reboot must fence all later physical and network
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
the 2,048-byte serial bound. Target-shaped coverage must distinguish a terminal
slice from a bounded command-owned continuation, advance that continuation by
one operation per later `LocalSeat` turn, and restore the prior polling policy.
The real linked-serial path must also prove that the passive compact `usb diag`
performs no USB poll, emits Gates 1 through 10, preserves `OK USB` and the
prompt within the three-record protocol-tail reserve, retires the physical
response fence, and then accepts fresh commands from both serial and buffered
USB input. Saturation coverage must prove ordinary response-body lines cannot
consume those tail slots even while response ordering is active.
Display coverage must prove an attach miss is retained and that attach and frame
submission cannot share an outer turn. The current synchronous PCIe HAL
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

SDIO runtime tests must prove the sole owner seals its engine from normalized
host-block geometry: at most two blocks use retained PIO and more than two use
external DMA. Every issued request remains in one immutable cursor. A
preissue/issue owner quantum may batch the finite Linux-ordered setup and
exactly one issue under the shared 256-operation contract; every later external
DMA continuation performs at most one retained snapshot, while every fresh
PIO-ready edge may move one complete normalized host block of at most 512 bytes
and 128 FIFO accesses without crossing into a later block. Common completion
requires response/R5, exact payload movement, authoritative `DATA_END`, and
host quiescence. PIO tests must prove direction-correct
ready/present-state ownership, zero DMA accesses, block-granular progress, and
that every owner quantum remains within 256 modeled HAL operations.
External-DMA tests must prove control blocks split
only at admitted physical-page boundaries, COMMAND is followed by exactly one
DMA activation, each later turn consumes one SDHCI/DMA snapshot, lone PIO-ready
bits remain outside its W1C ownership, and request-local `DMA_END` cannot
replace terminal `CONBLK_AD == 0` plus this request's `CS.INT`. Timeout or
selected-engine failure must perform bounded containment without engine
switching or post-issue replay; malformed external-DMA resources must fail
before command issue without falling back to PIO or root-owned service.
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
`sdio_external_dma_join_rejects_missing_dma_completion`,
`sdio_dma_abort_failure_still_attempts_sdhci_reset`,
`sdio_data_requests_without_dma_authority_fail_before_command_for_both_directions`,
`sdio_dma_error_telemetry_is_immutable_across_containment_reset`,
`sdio_stale_dma_generation_is_contained_before_fixed_memory_reuse`,
`sdio_descriptor_rejects_unrepresentable_cmd53_byte_mode_before_issue`,
`sdio_wifi_power_sequence_advances_one_bounded_action_per_turn`,
`sdio_engine_init_turn_withholds_completion_until_pwrseq_terminal`,
`sdio_retained_bootstrap_cmd52_and_card_commands_issue_once_without_private_polls`,
`sdio_retained_host_config_runs_recovery_and_set_ios_across_outer_turns`,
`sdio_retained_dpc_activation_rearms_one_policy_register_per_outer_turn`,
`firmware_parent_reciprocal_ring_drives_retained_sdio_owner_as_511_plus_one`,
`cyw43_linked_f2_tx_uses_cached_window_without_per_packet_iorx`,
`cyw43_linked_control_and_eapol_tx_use_one_cached_window_f2_issue`,
`control_and_eapol_tx_cross_reciprocal_ring_and_retained_sdio_owner`,
`control_tx_cold_window_crosses_exact_three_writes_then_f2`,
`release_post_f2_crosses_exact_linux_order_to_real_dpc_activation`,
`production_dpc_event_drains_real_owner_rx_before_foreground_poll`,
`production_control_and_rx_polls_consume_only_dpc_owned_queue`,
`firmware_terminal_and_issued_unknown_cuts_never_reissue_a_child`,
`stale_foreground_completion_cannot_mutate_replacement_generation`,
`mutated_action_fingerprint_poisoning_never_replays_issued_child`,
`issued_unknown_timeout_retains_one_child_without_same_generation_replay`,
`corrupted_continuation_fingerprint_fences_real_owner_without_second_quantum`,
and `cyw43_foreground_baseline_requires_release_published_snapshot`.

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

Milestone 26c Pi runtime/DMA proof states are machine-checkable and must not be inferred from adjacent evidence. `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml --venv .venv` consumes the immutable `seL4/build_UBOOT` artifacts and writes `out/pi4-sd/pi4-runtime-dma-proof.env` with `PI4_RUNTIME_DMA_PROOF=target-build`, `PI4_RUNTIME_DMA_PROFILE=bounded-no-iommu`, manifest hash, runtime CPIO hash, runtime uImage hash, staged image hash, and the hash of `pi4-image-identity.json`; this proves repository artifact identity, packaging, and exact legacy-image identity only. Under Milestone 26d, that Pi tree must validate independently as a repo-managed `pi4_diagnostic` seL4 16.0.0 `bcm2711` artifact set with its completed build-input stamp, `KernelRootCNodeSizeBits=14`, `KernelArmExportVCNTUser=ON`, physical counter/timer-control exports off, `TIMER_CLOCK_HZ=54000000`, and no retained one-domain `KernelDomainSchedule` cache entry. The 14-bit root CNode is required for the bounded capability inventory of the linked-runtime images and isolated framebuffer mapping; a 13-bit external Pi tree is stale and cannot satisfy image or hardware proof. The static `seL4/build_UBOOT` PASS proves only the canonical diagnostic artifact contract and cannot substitute for release proof, staged/read-back image identity, boot, Wi-Fi, TCP/`cohsh`, or benchmark lanes. The image wrapper must validate one complete relink tool family against the tracked baseline oracle and must never invoke CMake or Ninja in the immutable tree. `scripts/pi4_trace_normalize.py --gate-summary` emits `DRIVER_TASK_DMA_PROOFS`, `DRIVER_TASK_DMA_BLOCKER`, `DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF`, `DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_BLOCKER`, and `PI4_RUNTIME_DMA_PROOF=absent`, `diagnostic`, `qemu-or-stale-log`, or `fresh-pi` from serial evidence. `scripts/pi4_gate_proof.sh --require-driver-task-proof --runtime-dma-proof-out out/test-plan/<run-id>/pi4-runtime-dma-proof.env` writes the live proof bundle only after normalization passes. Only `fresh-pi` counts as live hardware runtime/DMA proof, and it requires driver-task dedicated readiness, cap/fault/revoke/scheduling/affinity proof, isolated VSpace, pointer-free IPC, per-hot-path `DRIVER_TASK_OWNER_STATE ... descriptor=present root_pointer=no`, sealed descriptor version/hash/identity proof for every active hot path, sealed bus-link proof for USB and CYW43 split clients, per-hot-path `DRIVER_TASK_DMA_PROOF` with bounded no-IOMMU profile and cache/bus-address policy, aggregate `DRIVER_TASK_DMA_BLOCKER=none`, no compatibility service roles, no unresolved ring timeouts/deferred bootstrap, no resource blockers, a fresh Pi cold-boot marker, and a live prompt. Raw `DRIVER_TASK_RING_CALL_TIMEOUT` events remain diagnostic, but `DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT` must be `0` after later return proof closes any bounded keep-active turn. It also emits `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified` only when `TIMER_BACKEND=arch-counter`, `TIMER_CLOCK_HZ=54000000`, `TIMER_EL0_COUNTER=vct`, `DUMMY_TIMER_SEEN=no`, every observed `DRIVER_TASK_COUNTER` line is valid, and the latest activity-bearing snapshot exists for every selected network owner. A selected CYW43 path therefore requires both `contract=cyw43455 hot_path=cyw43-wifi` and `contract=sdio-host hot_path=sdio-host`; repeated cumulative snapshots cannot substitute another driver's activity or be added together.

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
this episode. Supervisor `ready` is the Gate 8 driver frontier only; successful
interactive Wi-Fi readiness remains withheld until current-generation DHCP is
bound with a nonempty address, the TCP console listener is bound and admitted,
and USB command admission is proven.

Bootstrap has exactly one outer episode, always `attempt=1`. Its raw lifecycle
is `status=begin|recovery|stabilizing|ready|failed|permanent`;
`attempt=0 status=preflight` remains the linked-serial admission state. A failed
or permanent episode may release a diagnostic HDMI console and prompt, but
must never render the Wi-Fi `Ready to use` banner. The episode admits no
automatic whole-bootstrap backoff, reset, second `begin`, or attempt 2. Once
both linked-runtime restart contexts exist, a typed runtime/SDIO or
issued-unknown physical fault may emit one `status=recovery`
and consume one complete fenced CYW43/SDIO pair repair with retained
firmware/control replay. That repair does not renew the absolute Gate 8
deadline, and replay success does not replenish it. Gate-local association,
DHCP, and protocol retries remain independently bounded. Gate 8 itself never
requests that repair; logical failure waits to the absolute deadline, then
emits `CYW43_GATE8_TERMINAL ... action=quarantine` and terminal
`status=permanent`. A terminal failure quarantines network service and returns
to the ordinary EventPump so
diagnostics, authentication, reboot, serial, local-seat, and HDMI remain
responsive while Wi-Fi stays acceptance-red. Only
same-generation Gate 10 plus attached address/TCP readiness authorizes one
later independent steady-state runtime-recovery episode with one fresh
consumed-once pair repair; that lifecycle cannot emit or reset a boot attempt.
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
not poll USB, re-enter HDMI or echo, or start network work. It must then advance
HID discovery only through the explicit keyboard-enumeration aux while ordinary
background polls report the current frontier without re-entering enumeration.
No-reply background USB polls must produce `DRIVER_TASK_RING_CALL_TIMEOUT` plus
`[local-seat] isolated USB runtime keyboard poll suspended contract=usb-local-seat source=linked-runtime reason=driver-task-no-reply action=serial-shell`
rather than repeated blocking `usb-local-seat` calls. `usb probe-kbd` must
retain its bounded keyboard-enumeration cursor and must not replay the whole
isolated local-seat attach/init chain; each attempt consumes one later
`LocalSeat` outer turn and is permitted only while the child USB enumeration
marker advances, stopping at the finite cap, keyboard readiness, or no new
marker.

Root-console startup must emit UART-visible `[mark] root-console.start.begin`,
publish `cohesix>` after bounded non-Wi-Fi driver startup settles or fails
closed, and emit `[mark] root-console.start.ok` before persistent Wi-Fi
bootstrap, `/log/queen.log`, or NineDoor log-stream handoff; host-EAPOL,
association, DHCP, and retained gate-local progress cannot hold the serial
shell hostage. Once `cohesix>` is published and USB polling is armed, serial
UART and USB keyboard input must both feed the shared parser concurrently after
USB proof succeeds; during a Wi-Fi HAL turn local-seat dispatch remains
buffered and command-fenced even though HDMI has not yet claimed interactive
readiness.

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

Every root-to-CYW43 generation, including zero, instead advances only through
the stable acknowledged exact shared grant. Root publishes or re-signals the
grant and sends the reserved-root-badge notification in separate outer turns;
the notification is a coalescing scheduling hint and cannot grant authority.
Tests must reject endpoint input, stale-sequence, changed-action,
changed-generation, changed-flags, consumed or mutated grants, and prove an
issued action cannot be replayed.

For delegated CYW43-to-SDIO commands, tests must prove there is no usable
endpoint after handoff and that only the stable acknowledged shared grant can
advance the retained owner cursor. The grant must match the command sequence,
complete action fingerprint, and the independently retained SDIO generation;
its nonzero id is published last and must exceed the last consumed id. The
consumer acknowledgement is irrevocable, an unacknowledged id may only be
re-signalled, and the producer may publish the next id only after observing the
exact acknowledgement. The production foreground and DPC cursors must each
show one poll, one later grant action, and one later poll, never two operations
in one turn. HAL must mint the CYW43 send cap from the SDIO owner's bound
notification with send-only rights and badge 256; it must not copy the owner
endpoint.

Send-only reciprocal caps deliver CYW43-to-SDIO badge 256 and SDIO-to-CYW43
badge 2, and the SDIO IRQ delivers badge 159. Badge 2 and badge 159 are
service wakes. Badge 256 is a coalescing scheduling hint for an already-published
command or grant, but is not foreground authority by itself. The reserved high
notification bit is excluded from service work. Tests must prove that one
pending service source can consume at most one service quantum, that at least
4,096 standalone level reassertions produce no second service quantum and lose
no durable source, and that an exact later root or delegated grant admits only
one foreground phase on its respective path. CYW43 root commands use the exact
root grant plus reserved-root-badge scheduling hint and must reject endpoint
wakes; non-CYW43 retained commands keep their endpoint coverage. Tests must
also coalesce badge 159 with badge 256, service exactly one IRQ quantum,
preserve the exact grant across a scheduler handoff, and release exactly one
owner quantum only after validating the already-published grant. If
deferred notification service consumes a root scheduling edge, the same
unconsumed exact grant remains eligible only on a later foreground turn.
The production reciprocal-ring/controller test must also schedule CYW43 far
enough ahead that the initial command signal and first-grant signal are both
published before SDIO intake and collapse to one badge-256 wake—or that the
only edge is consumed before the grant becomes observable. The delegated owner
must then show distinct `CheckWake`, `CheckGrant`, `Service` or `Wait`, and
`Execute` outer turns. A CARD_INT pending at `CheckWake` must receive exactly
one service turn before grant admission; a later CARD_INT may follow at most
one already-admitted owner quantum and must be observed at the next
`CheckWake`. `CheckGrant` may perform one stable grant read but no device
operation. Immediately before blocking, `Wait` must recheck the durable shared
grant so a publication between the empty probe and receive advances to a later
`Execute` turn without needing a second notification edge; the recheck itself
performs no owner I/O, and the consumed grant cannot replay. `Execute` must
perform ACK-before-I/O and at most one owner quantum without another poll. The
actual card-init `HOST_CONFIG` producer must
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
CARD_INT coverage must also prove that terminal deferred service masks the host
source before IRQ acknowledgement.
The real SDIO post-claim priority-failure hook must prove that one episode
cannot reclaim its sticky cutover, that each failure/restart action consumes one
outer turn, and that only the exact SDIO-first/CYW43-second pair restart resets
the latch before a later recovery episode may claim cutover. A precondition
rejection without a valid restart context remains terminal.
Autonomous committed-ring polling must preserve non-CYW43 root-command intake
when a best-effort endpoint send is lost. Every CYW43 root generation,
including zero, must use a reserved-root-badge notification plus an exact
root grant and must reject endpoint continuation. Delegated initial intake must
use the coalescing badge-256 notification and sequence-last ring command. After
`Pending`, only an exact root or delegated shared grant for the retained intake
may grant the next foreground quantum; the notification only schedules a later
grant check. A missed poll can schedule only the matching later wake/grant
action and cannot recommit the command sequence. Pending-command
DPC arbitration, reciprocal SDIO child-ring submission, every shared grant, and
every reciprocal completion poll must each consume separately released retained
quanta, with no private yield/resignal/poll loop. The real root reciprocal-ring
tests must cut the logical connection epoch once while a CYW43 command is
`Prepared` and once after it is `Issued`. Every active command, exact grant,
and completion in both cuts must retain the cursor's original request and
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

The Pi 4 manifest defaults place both `bcmgenet-v5` and `cyw43455` on core `3`; hardware captures must show `DRIVER_TASK_BOOT ... contract=bcmgenet-v5 ... affinity_core=3` and `DRIVER_TASK_BOOT ... contract=cyw43455 ... affinity_core=3` before claiming fourth-core driver placement. Physical Pi owner-state boots apply `seL4_TCB_SetAffinity` directly to each driver child TCB. That is distinct from the root-authority affinity wrapper used around in-process NineDoor and Worker-model operations; neither NineDoor nor a general Worker has a separate TCB in the current profile. Any `DRIVER_TASK_AFFINITY_DEFERRED ... reason=pi4-child-tcb-affinity-boot-stall-guard` line is stale mitigation evidence and must fail placement proof. Non-CYW43/SDIO runtimes may still emit `DRIVER_TASK_NOTIFICATION_BIND_DEFERRED ... reason=pi4-early-tcb-notification-bind-boot-stall-guard`, which keeps their notification lifecycle proof red while their endpoint-backed command-ring startup proceeds. The generated CYW43 and SDIO peers must instead emit `DRIVER_TASK_NOTIFICATION_BOUND ... source=generated-cyw43-sdio-topology`; a deferred bind for either peer fails Wi-Fi proof because exact root and delegated grants use their bound notifications for scheduling. QEMU virtio compatibility boots may prove isolated VSpace/ASID allocation, runtime-image transport-region mapping, and pointer-free ring transport after virtio networking is online, but that is transport-substrate evidence only. Fresh Pi hardware proof is still required before claiming Wi-Fi/DHCP, GENET/DHCP, USB keyboard, HDMI, or strongest isolated-driver hardware acceptance.

Strict Pi SDIO command/data calls, fixed-layout SDIO CMD52/CMD53 descriptors, CYW43 firmware/NVRAM/SDPCM command records, direct-root-port xHCI keyboard polling, GENET RX/TX descriptor-ring service, and PCIe port read/write/flush helpers now compile in isolated runtime code before any root hardware execution; host coverage must keep proving those ring turns while preserving the fresh-Pi board-proof boundary.

Current Wi-Fi acceptance also requires one exact `CYW43_SDIO_DPC generation=<n> captures=<n> published=<n> consumed=<n> rearms=<n> overruns=<n> epoch_errors=<n> sequence_errors=<n> ack_failures=<n> poisoned=yes|no masked=yes|no` diagnostic in the current boot slice and `WIFI_DPC_PROOF=yes` from `scripts/pi4_trace_normalize.py --gate-summary`. `wifi diag` preserves that bounded accounting grammar for normalizer compatibility and immediately follows it with `CYW43_SDIO_DPC_TRUTH generation=<n> ring_poisoned=yes|no client_sample_stale=yes|no ring_consumer=<n> sample_consumer=<n> sample_reason=<reason> authority=live-ring action=<action>` plus `CYW43_SDIO_DPC_REARM generation=<n> counter=client-signal-attempts count=<n> owner_irq=masked|unmasked action=<action>`. All three lines must remain complete at maximum counter widths. The accounting `poisoned` value is the fail-closed aggregate of a live poisoned ring, a stale client sample, and client epoch errors; the truth line distinguishes those causes without weakening old-capture parsing. `wifi diag` emits the three-line proof only after a stable, valid read of the admitted SDIO owner ring and a same-generation v10 CYW43 client-counter sample. The v10 `rearms` value counts generation-scoped owner-rearm signal attempts, not separately delivered wakes or hardware re-enables; the older source-asserted-empty episode counter remains separate and cannot satisfy Gate 10. The rearm line labels that metric explicitly, while a healthy masked final state renders `sample_reason=owner-rearm-pending action=service-sdio-owner` and `owner_irq=masked`. Acceptance therefore also requires the stable live ring to report `masked=no`. It fails closed with `WIFI_DPC_REASON=no-activity` unless the current exact proof has both `captures > 0` and `published > 0`; it also fails when the accounting line is missing, poisoned, or masked, any overrun/epoch/sequence/ack failure is nonzero, captured and published totals differ, consumed and published totals differ, or the final IRQ service state is unrearmed. The DPC diagnostic `generation` is the linked SDIO/CYW43 ring epoch, not Gate 8's association/control generation. The normalizer establishes freshness by requiring the sample after the current atomic Gate 8 Ready edge and must never compare those independent generation domains. DPC failures are retained within one ring generation, but a prior supervisor attempt or superseded association generation cannot poison the latest exact attempt's healthy accounting. Exploratory summaries and wired-only historical evidence remain readable without this Wi-Fi-only proof.

### Automated Stage 03 — QEMU or Pi transport regression
- `scripts/ci/test_plan_stage_03_qemu_tcp_regression.sh`
- Stage 03 sets resilient defaults for clean hosts: `TP_STAGE3_READY_TIMEOUT=900`, `TP_STAGE3_PORT_TIMEOUT=60`, `TP_STAGE3_AUTH_READY_TIMEOUT=120`, `TP_STAGE3_QUIT_CLOSE_TIMEOUT=60` (override as needed).
- `scripts/cohsh/run_regression_batch.sh` builds one immutable artifact for the
  default manifest and one for the gated manifest. Base, telemetry, and shard
  groups reuse the default artifact bytes; every group still receives a fresh
  QEMU boot.
- The batch snapshots generated projections and restores them in an EXIT trap,
  including failure and interrupt paths. Each artifact and boot result has a
  machine-readable source/profile/manifest/image/action/log binding.
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
  - The `performance` tier is qualified only by Conditional D's explicit
    no-retry matrix and numeric error budget. Any regression claim also needs
    reviewable baseline artifacts indexed in `docs/BENCHMARKS.md`; never
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
  - Pad the trace to a 128-byte multiple (matches `cas.store.chunk_bytes`):
    ```bash
    python3 - <<'PY'
    from pathlib import Path
    src = Path("tests/fixtures/traces/trace_v0.trace")
    dst = Path("out/cas/trace_v0.padded")
    data = src.read_bytes()
    pad = (-len(data)) % 128
    dst.write_bytes(data + b"\0" * pad)
    print(f"padded {len(data)} -> {len(data) + pad} bytes")
    PY
    ```
  - Source tree: `./bin/cas-tool pack --epoch 1 --input ./out/cas/trace_v0.padded --out-dir ./out/cas/1 --signing-key ./resources/fixtures/cas_signing_key.hex`
  - Release bundle: pad `./traces/trace_v0.trace` into `./out/cas/trace_v0.padded`, then run `./bin/cas-tool pack --epoch 1 --input ./out/cas/trace_v0.padded --out-dir ./out/cas/1 --signing-key <path>`
  - `./bin/cas-tool upload --bundle ./out/cas/1 --host 127.0.0.1 --port 31337 --auth-token changeme --ticket "$QUEEN_TICKET"`
- `gpu-bridge-host`:
  - `./bin/gpu-bridge-host --mock --list`
  - Optional NVML: `./bin/gpu-bridge-host --list` (enabled by default on Linux builds; omit NVML with `--no-default-features`)
  - Live publish: `./bin/gpu-bridge-host --publish --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --interval-ms 1000 --registry demo/peft_registry`
    - On macOS without NVML, use `--mock --publish` to avoid NVML load failures.
- `host-sidecar-bridge`:
  - `./bin/host-sidecar-bridge --mock --mount /host --provider systemd --provider k8s --provider docker --provider nvidia`
  - `./bin/host-sidecar-bridge --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --watch` (requires `/host` enabled in `configs/root_task.toml`)
- `host-ticket-agent`:
  - `./bin/host-ticket-agent --mock --run-once`
  - `./bin/host-ticket-agent --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --run-once`
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
    - Failure classification is part of the contract: `HTTP 429` means bounded broker queue backpressure, `HTTP 503` means transport unavailable, and `HTTP 504` means the broker accepted work but the backend response exceeded its response timeout.
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
    - `./bin/cas-tool pack --epoch 1 --input ./out/cas/trace_v0.padded --out-dir ./out/cas/1 --signing-key ./resources/fixtures/cas_signing_key.hex`
    - `./bin/cas-tool upload --bundle ./out/cas/1 --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN"`
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
- Live Hive performance harness (bounded render cadence and backlog checks).
- Failure UI (auth error, disconnected state) as UI-only states.

#### 4) Determinism Rules
- Replay-first: all UI assertions are driven from replay fixtures.
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

### Conditional C — SMP parity (Milestone 25+)
- Boot QEMU with a single core: `COHESIX_QEMU_SMP=1 scripts/cohesix-build-run.sh --transport tcp`
- Run `./cohsh --transport tcp --tcp-port 31337 --script scripts/cohsh/smp_parity.coh > out/smp_parity_1.txt`
- Reboot QEMU with multiple cores (match the SMP kernel build): `COHESIX_QEMU_SMP=4 scripts/cohesix-build-run.sh --transport tcp`
- Run `./cohsh --transport tcp --tcp-port 31337 --script scripts/cohsh/smp_parity.coh > out/smp_parity_4.txt`
- Compare transcripts: `diff -u out/smp_parity_1.txt out/smp_parity_4.txt` (must be byte-identical).

### Conditional D — Gateway large-telemetry reliability (Milestone 25f)
When the `performance` claim is selected, run this matrix with `hive-gateway`
attached and **no retry paths**. Qualification requires both the local and G5g
evidence named by the active milestone.
- `python3 scripts/rest_perf_harness.py --mode simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-1mb --error-budget-rate 0.01`
- `python3 scripts/rest_perf_harness.py --mode simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-10mb --error-budget-rate 0.01`
- `python3 scripts/rest_perf_harness.py --mode simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-100mb --error-budget-rate 0.01`
- `python3 scripts/rest_perf_harness.py --mode simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-1gb --error-budget-rate 0.01`

Pass criteria:
- Every run exits `0`.
- Summary artifacts exist (`*.summary.json`, `*.ops.csv`, `*.ramp.csv`, `*.ramp.svg`).
- `error_budget_pass=true` and `error_rate <= 0.01` in each summary JSON.
- `no_retries=true`, `fast_ramp=true`, and `scenario` equals the requested preset in each summary JSON.

Failure policy:
- Any scenario above the error budget is a release-blocking defect.
- Do not use retry flags or ad-hoc rerun wrappers to mask failures; tune/fix code and re-run the same matrix.
- On slower physical targets, keep `--ready-timeout-secs` greater than the gateway broker response timeout and pass explicit harness overrides such as `--gateway-broker-control-response-timeout-ms 120000 --gateway-broker-telemetry-response-timeout-ms 120000` rather than lowering the error budget or enabling retries.

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
  benchmark traces, credit-gated CYW43 TX/RX ordering, and GENET service
  budgets. Conditional F adds only image, boot, capture, repeatability, and
  live-hardware proof.
- Pi 4 image / U-Boot gate:
  - `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml`
  - `scripts/uboot/qemu-uboot-smoke.sh --net user`
  - Confirm U-Boot env control remains deterministic (`ipaddr`, `serverip`, `coh_net_mode`, `coh_net_interface`), generic persistent `uboot.env` import is disabled with `CONFIG_ENV_IS_NOWHERE`, `CONFIG_PREBOOT` stays on the serial/video console path, the staged Pi 4 boot script owns the first menu/input USB bootstrap, reloads `cohesix.env`, mirrors `coh_net_*` values into the staged padded `bcm2711-rpi-4-b.dtb`, and boots the seL4 elfloader through U-Boot `bootm` with that DTB. Host coverage must also prove that a post-erase mount interruption retains the private saved-policy copy, prints an explicit `--policy-recovery-file` retry path, rejects recovery over a different non-empty policy, enforces the 384-byte bound, and consumes the recovery file only after verified flash completion and unmount.
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
  - `--require-usb-ready`, `--require-wifi-ready`, and `--require-ready` are
    stricter than gate/blocker success. They require the isolated runtime
    old-good replay fields from `scripts/pi4_trace_normalize.py --gate-summary`:
    `USB_OLDGOOD_REPLAY=yes`, `USB_OLDGOOD_MISSING=none`,
    `WIFI_OLDGOOD_REPLAY=yes`, and `WIFI_OLDGOOD_MISSING=none` for the selected
    full-ready path. Wi-Fi proof also requires
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
    association, M1, M2/M3/M4/PTK/GTK, or secure-release step. USB ready proof
    also requires `USB_LOCAL_SEAT_STATE=ready`, `USB_COMMAND_READY=yes`,
    `USB_FIRST_REPORT_READY=yes`, and `USB_BUSY_AFTER_READY=no` so parser
    admission cannot hide missing first-report or post-ready busy evidence. A
    decoded held-key/modifier report while the attach/recovery idle guard is
    closed must remain `FIRST_REPORT_PENDING`; recovery must revoke stale
    first-report, first-byte, parser, and HDMI command-ready latches until a
    fresh decoded all-zero release reopens them. Endpoint-health counters may
    advance during that interval but cannot substitute for readiness. A
    replay miss reports the first missing translated May/U-Boot/Linux behavior
    through `*_OLDGOOD_MISSING`; gate 10 without replay remains triage evidence
    only. USB replay requires distinct ordered endpoint, interrupt-IN,
    first-report, first-byte, and runtime-gate proof, and the first report/byte
    must be isolated runtime HID sourced. Wi-Fi replay rejects failed readiness,
    failed join, generic EAPOL message tokens, firmware-supplicant shortcuts,
    and started-only nettest output.
  - Existing logs may be normalized for triage only:
    - `scripts/pi4_gate_proof.sh --normalize-only --log <existing-log> --allow-summary-only`
    - `--allow-summary-only` is not acceptance proof and must not be combined with any `--require-*` hardware acceptance flag.
  - `scripts/pi4_trace_normalize.py --boot-summary` is a fail-closed boot ledger, not an alternative proof path. A `pass` slice requires clean serial, prompt/root-console readiness, arch-counter timer proof, dedicated driver-task owner/DMA/counter proof, selected network proof, `NET_TCP_READY=yes` or `NETTEST_PROOF=yes`, USB cold-boot and old-good local-seat proof, USB burst proof, and HDMI/serial responsiveness. Console-only boots, DHCP-only wired boots, and Wi-Fi boots without `WIFI_OLDGOOD_REPLAY=yes` remain failed slices even when the prompt is usable.
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
    - USB keyboard proof reaches `USB_GATE=10` / `USB_BLOCKER=none` with `USB_COMMAND_READY=yes`, `USB_FIRST_REPORT_READY=yes`, `USB_LOCAL_SEAT_STATE=ready`, and `USB_BUSY_AFTER_READY=no`, and hardware acceptance also reaches `USB_OLDGOOD_REPLAY=yes` / `USB_OLDGOOD_MISSING=none` for the isolated hub-keyboard sequence before claiming the local-seat keyboard experience is complete. The first HID report and first byte must be sourced from `linked-runtime-hid`; parser ingress reported as `local-seat-queue-diagnostic`, local-seat queue text, or `source=first-byte` is diagnostic only. A printable-key line such as `runtime keyboard first-printable-byte ...` remains required user-experience evidence. Sustained USB acceptance additionally requires `USB_POST_FIRST_BYTE_BLOCKER=none`, no `recovery-failed` report status, no post-first-byte queue collapse, and no growing no-reply/runtime-skipped pressure during typing, arrow-history, and lock-key bursts.
    - if the attached keyboard exposes lock LEDs, Caps Lock, Num Lock, and Scroll Lock testing either proves the preallocated EP0 OUT DMA path (`xhci-control-out-prealloc` plus `pi4 keyboard led sync ready ...`) or cleanly logs `keyboard led sync unavailable ... action=disabled` without blocking input.
    - HDMI local-seat acceptance observes typed USB keyboard bytes echoing at
      parser ingress on the live prompt row, boot/progress messages refreshing
      at the documented 5-10 s cadence, and new output scrolling the isolated
      HDMI viewport like a serial terminal without full-screen blink. On a
      successful deferred Wi-Fi boot, Gate 8 and supervisor `ready` remain
      progress only: the HDMI Wi-Fi `Ready to use` banner and interactive prompt
      must follow current-generation DHCP Bound, a nonempty address,
      TCP-listener admission, and USB command-ready proof. `failed` or
      `permanent` may expose a diagnostic prompt but must never show Wi-Fi ready.
      Preflight may report diagnostics available but must not claim Wi-Fi or
      interactive-console readiness. The first attached viewport snapshot is
      one-shot, and asynchronous driver milestones arriving during a partial
      command must use the bounded row-preserving update and restore the exact
      prompt, typed bytes, backspace floor, and cursor. USB up/down arrow escape
      sequences navigate the bounded root-owned HDMI history and trigger
      cursor-home redraws from canonical scrollback; redraws must use the
      framebuffer-derived safe-area row count even when the payload spans
      multiple bounded HDMI service turns. Each rendered row must use
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
      Gate 10. Stage 01 driver coverage guards the cadence constants, serial
      runtime ring RX/TX turns, HDMI prompt/input/history/no-reply behavior, and
      Wi-Fi progress suppression during USB boot activity and after USB
      first-byte proof. Pi 4 manifest-default boots must use
      `hw.local_seat.enabled=true`, `hw.local_seat.required=true`, and matching
      `usb-kbd0`/`hdmi0` `hw.devices[] required=true` declarations so missing
      declared devices fail visibly. Runtime backend attach failures may
      degrade with `required=yes action=serial-shell`; that keeps the UART root
      shell reachable but does not satisfy HDMI/USB acceptance.
  - `netstats` must report:
    - `mode=<off|static|dhcp> policy=<wired|wifi|auto> active=<iface> standby=<iface|none> addr_src=<source> ip=<ipv4> gateway=<ipv4> dhcp=<phase>`; the normalizer exposes the selected state as `NET_ACTIVE`, `NET_ADDR_SRC`, and `NET_DHCP`, and separately exposes command/listener proof as `NET_TCP_READY` and `NETTEST_PROOF`.
    - exactly one complete `nettest: generation=<connection> run_generation=<run> enabled=<bool> running=<bool> verdict=<none|running|pass|peer-assisted-pass|fail> tx_ok=<bool|na> udp_echo_ok=<bool|na> tcp_ok=<bool|na> console_ok=<bool|na> peer_assisted_ok=<bool|na>` status line. `OK NETTEST detail=started run_generation=<run>` admits one immutable run; only a terminal line for the same positive run generation is proof. An internal-only asynchronous log, an incomplete or truncated line, or a prior connection/run-generation verdict is not terminal proof; backend and target strings remain on the separate `nettargets:` line.
    - `tx_submit=<count> tx_complete=<count> tx_free=<count> tx_in_flight=<count> tx_double_submit=<count> tx_zero_len_attempt=<count> arp_rx=<count> arp_tx=<count>`; on CYW43, `tx_complete` is credit-backed SDPCM completion proof and `tx_submit > tx_complete` is a Wi-Fi TX credit anomaly until host TCP/cohsh evidence proves the path recovered.
    - `wifi_assoc=<0|1> wifi_link=<0|1> eapol_rx=<count> eapol_start=<count> eapol_secure=<0|1>`
    - driver-task scheduling evidence for the active hardware path in reopened 26a/26b acceptance captures: contract name, service class, isolation mode, poll/service count, budget exhaustion/yield count, RX/TX queue depth, drop count, manifest-selected affinity core, observed service latency, and timer backend proof. The normalizer exposes this as `TIMER_BACKEND`, `TIMER_CLOCK_HZ`, `TIMER_EL0_COUNTER`, `DUMMY_TIMER_SEEN`, `DRIVER_TASK_CONTRACTS`, `DRIVER_TASK_DEDICATED`, `DRIVER_TASK_COMPATIBILITY`, `DRIVER_TASK_DEDICATED_READY`, `DRIVER_TASK_SERIAL_DEDICATED`, `DRIVER_TASK_USB_DEDICATED`, `DRIVER_TASK_DISPLAY_DEDICATED`, `DRIVER_TASK_NET_DEDICATED`, `DRIVER_TASK_SDIO_DEDICATED`, `DRIVER_TASK_PCIE_DEDICATED`, `DRIVER_TASK_SUBSTRATE_READY`, `DRIVER_TASK_FAILED_COUNT`, `DRIVER_TASK_CAPSET_PROOF`, `DRIVER_TASK_FAULT_PROOF`, `DRIVER_TASK_REVOKE_PROOF`, `DRIVER_TASK_SCHED_PROOF`, `DRIVER_TASK_AFFINITY_PROOF`, `DRIVER_TASK_AFFINITY_CONFIGURED`, `DRIVER_TASK_AFFINITY_APPLIED`, `DRIVER_TASK_AFFINITY_MANIFEST_PROOF`, `DRIVER_TASK_AFFINITY_MANIFEST_MATCHES`, `DRIVER_TASK_AFFINITY_MANIFEST_MISSING`, `DRIVER_TASK_AFFINITY_MANIFEST_MISMATCHES`, `DRIVER_TASK_VSPACE_PROOF`, `DRIVER_TASK_POINTER_FREE_IPC_PROOF`, `DRIVER_TASK_OWNER_STATE_PROOF`, `DRIVER_TASK_DMA_PROOFS`, `DRIVER_TASK_DMA_BLOCKER`, `PI4_RUNTIME_DMA_PROOF`, `PI4_RUNTIME_DMA_PROOF_REASON`, `PI4_RUNTIME_DMA_COUNTER_PROOF`, `DRIVER_TASK_ACTIVE_NET`, `DRIVER_TASK_BUDGET_OVERRUNS`, `DRIVER_TASK_LATENCY_PROOFS`, `DRIVER_TASK_RING_CALL_BEGIN`, `DRIVER_TASK_RING_CALL_RETURN`, `DRIVER_TASK_RING_CALL_OUTSTANDING`, `DRIVER_TASK_RING_CALL_TIMEOUT`, `DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT`, `DRIVER_TASK_BOOTSTRAP_DEFERRED`, `DRIVER_TASK_RESOURCE_INIT`, `DRIVER_TASK_RESOURCE_BLOCKER`, and `DRIVER_TASK_RESOURCE_CURRENT_BLOCKER`. `DRIVER_TASK_OWNER_STATE_PROOF=yes` must be backed by per-hot-path owner-state descriptor lines for serial, USB, HDMI, PCIe, and the selected network owner set (`cyw43-wifi` plus `sdio-host` when `DRIVER_TASK_ACTIVE_NET=cyw43`, or `genet-nic` when `DRIVER_TASK_ACTIVE_NET=genet`). Pi 4 performance evidence must report `TIMER_BACKEND=arch-counter`, `TIMER_CLOCK_HZ=54000000`, `TIMER_EL0_COUNTER=vct`, `DUMMY_TIMER_SEEN=no`, `DRIVER_TASK_DMA_BLOCKER=none`, and `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`; otherwise latency proof is red even if driver-task owner-state proof is present. `DRIVER_TASK_RESOURCE_BLOCKER` is the first lost resource proof in the capture; `DRIVER_TASK_RESOURCE_CURRENT_BLOCKER` is the latest non-ready resource-init blocker. The source `DRIVER_TASK_RESOURCE_INIT` line carries the current isolated runtime owner/action, active request, `expected_request_valid` / `expected_aux0_valid`, expected aux/request values when present, same-request flag, and child progress marker needed to diagnose the live turn. Any positive `DRIVER_TASK_RING_CALL_OUTSTANDING`, `DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT`, `DRIVER_TASK_BOOTSTRAP_DEFERRED`, or non-`none` resource blocker is an isolated runtime no-reply/deferred-proof frontier; raw `DRIVER_TASK_RING_CALL_TIMEOUT` counts remain diagnostic when a later return closes the same request. Contract-only root-task compatibility evidence, resource-init breadcrumbs, and declared `max_service_us` budgets are diagnostic and must not be counted as dedicated driver-task closure or latency proof.
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

## Trace replay limits
<!-- coh-rtc:trace-policy:start -->
### Trace replay limits (generated)
- `trace.format.version`: `1`
- `trace.hash`: `sha256`
- `trace.max_bytes`: `1048576`
- `trace.max_frame_bytes`: `8192`
- `trace.max_ack_bytes`: `256`

_Generated by coh-rtc (sha256: `c502a57721e43d5c38f5499767a8668eb593ac74f25cb2389632804c4d7f15f2`)._
<!-- coh-rtc:trace-policy:end -->

## Manifest fingerprints
- `configs/generated/root_task_resolved.json` — `sha256:fb62b38622b289d3f9cd3bcd7171f270b3d45849f9e556f46d8fde381b423561`

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
- `tests/fixtures/transcripts/converge_v0/swarmui.txt` — `sha256:dafd88f7d7e984454e12815ccffd203f98c446d0eb1e8a364d79805aa69de017`
- `tests/fixtures/transcripts/converge_v0/coh-status.txt` — `sha256:dafd88f7d7e984454e12815ccffd203f98c446d0eb1e8a364d79805aa69de017`
- `tests/fixtures/transcripts/control_plane_v0/cohsh.txt` — `sha256:f43434e6b3071753596e919021e573cb7f6a9831123769dd7cefb5b0c115c1ef`
- `tests/fixtures/transcripts/run_demo_v0/cohsh.txt` — `sha256:d429aa09972892adaeabed60ef2a36e4fe366eb9e730a8467a85f27870957040`
- `tests/fixtures/transcripts/peft_roundtrip_v0/cohsh.txt` — `sha256:a761096db1c412e8b775f3bdb78a9aec79b95ef787d0e933406d23c20285f7db`
- `tests/fixtures/transcripts/trace_v0/cohsh.txt` — `sha256:56b97a2d8486ed783d7cb93d38ea67811d93df6efcc24d7ed97265a4df1b1c4f`
- `tests/fixtures/transcripts/trace_v0/swarmui.txt` — `sha256:56b97a2d8486ed783d7cb93d38ea67811d93df6efcc24d7ed97265a4df1b1c4f`
- `tests/fixtures/transcripts/trace_v0/coh-status.txt` — `sha256:56b97a2d8486ed783d7cb93d38ea67811d93df6efcc24d7ed97265a4df1b1c4f`

## Trace fixture hashes
- `tests/fixtures/traces/trace_v0.trace` — `sha256:0f5a1935e973fbdb57e73a952b9cd02d1060086167efb4b9e79b28169f308561`
- `tests/fixtures/traces/trace_v0.hive.cbor` — `sha256:977113ebcfad69272cbb15ddc57e7ce1ccd1df87baa6568704253cacc55e8e2d`

## Guard
- `scripts/ci/check_test_plan.sh` verifies hashes, required scripted-stage references, and command alignment (`python3`, workspace/tests gates); `scripts/check-generated.sh` invokes it.
