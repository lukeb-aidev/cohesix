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
- `wifi diag` and `wifi dump-state` formatting coverage must preserve the
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
readback was not published, and passive `wifi diag` must render both complete
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
record without a write or child turn. `wifi diag` tests must prove a current
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

`wifi diag` and `wifi dump-state` must report stable RX queue generation,
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

Fresh exact-image acceptance must pair one power-off plus five first-pair warm
WiFi boots with one GENET control and then run the same sequential request and
no-retry pressure workloads. Before ping, TCP, `cohsh`, or benchmark traffic
warms the Pi's host-neighbor entry on each lifetime, send exactly one ICMPv4
Echo Request. The boot-paired pcap must show that request, the Pi's ARP request,
the matching host ARP reply, and exactly one Echo Reply with the original
identifier, sequence, and payload, without a second host Echo Request or
duplicate Pi reply. Failure of that semantic cold-neighbor gate fails the
lifetime independently of CYW43 ingress and warmed cadence. Record its elapsed
time separately; use only subsequent ARP-warmed ping, SYN, and
request-to-first-payload samples for the CYW43 cadence threshold. The mandatory
tenfold floor is WiFi request-to-first-payload p95 at most 40 ms and at least
29 sequential requests/s. The aggressive low-overhead target is p95 at most 10
ms and at least 100 requests/s. At true idle the committed queue must be stably
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
The pcap must additionally show no warmed-traffic loss, sequence gap,
out-of-order delivery, reset, zero window, SACK recovery block, SYN retry, or
reconnect, and the pressure run must have zero timeout masking. GENET must pass
the same common cold-neighbor semantic gate while latency, throughput, and
scheduler counters remain within its control contract. Until a rebuilt/read-back
image produces this evidence, the cold-neighbor repair and both performance
thresholds remain source claims rather than hardware results.

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
and transfer generation. Polling the same identity must not refresh the
deadline. Before expiry the result remains first-report pending; exact expiry
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

The Pi 4 manifest defaults place both `bcmgenet-v5` and `cyw43455` on core `3`; hardware captures must show `DRIVER_TASK_BOOT ... contract=bcmgenet-v5 ... affinity_core=3` and `DRIVER_TASK_BOOT ... contract=cyw43455 ... affinity_core=3` before claiming fourth-core driver placement. Physical Pi owner-state boots apply `seL4_TCB_SetAffinity` directly to each driver child TCB. That is distinct from the root-authority affinity wrapper used around in-process NineDoor and Worker-model operations; neither NineDoor nor a general Worker has a separate TCB in the current profile. Any `DRIVER_TASK_AFFINITY_DEFERRED ... reason=pi4-child-tcb-affinity-boot-stall-guard` line is stale mitigation evidence and must fail placement proof. Non-CYW43/SDIO runtimes may still emit `DRIVER_TASK_NOTIFICATION_BIND_DEFERRED ... reason=pi4-early-tcb-notification-bind-boot-stall-guard`, which keeps their notification lifecycle proof red while their endpoint-backed command-ring startup proceeds. The generated CYW43 and SDIO peers must instead emit `DRIVER_TASK_NOTIFICATION_BOUND ... source=generated-cyw43-sdio-topology`; a deferred bind for either peer fails Wi-Fi proof because ordinary exact grants and the persistent op11 parent/child contract use their bound notifications only as scheduling prompts. QEMU virtio compatibility boots may prove isolated VSpace/ASID allocation, runtime-image transport-region mapping, and pointer-free ring transport after virtio networking is online, but that is transport-substrate evidence only. Fresh Pi hardware proof is still required before claiming Wi-Fi/DHCP, GENET/DHCP, USB keyboard, HDMI, or strongest isolated-driver hardware acceptance.

Strict Pi SDIO command/data calls, fixed-layout SDIO CMD52/CMD53 descriptors, CYW43 firmware/NVRAM/SDPCM command records, direct-root-port xHCI keyboard polling, GENET RX/TX descriptor-ring service, and PCIe port read/write/flush helpers now compile in isolated runtime code before any root hardware execution; host coverage must keep proving those ring turns while preserving the fresh-Pi board-proof boundary.

Current Wi-Fi acceptance also requires one exact
`CYW43_SDIO_DPC generation=<n> captures=<n> published=<n> consumed=<n>
rearms=<n> overruns=<n> epoch_errors=<n> sequence_errors=<n>
ack_failures=<n> owner_active=yes|no poisoned=yes|no masked=yes|no` diagnostic
in the current boot
slice and `WIFI_DPC_PROOF=yes` from
`scripts/pi4_trace_normalize.py --gate-summary`. `wifi diag` preserves that
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
owner_irq=masked|unmasked action=<action>`. The additive v11 client trace keeps
the scope and rearm lines byte-stable, revises the accounting/truth lines with
the explicit activation state, and appends
`CYW43_SDIO_DPC_CAUSE samples=<n> frm=<n> hm=<n> fcc=<n> fcs=<n> ca=<n>
other=<n> spur=<n> done=<n> dpc=<n> child=<n> owner=<n> fdpc=<n>
fown=<n>`. All five lines must remain complete at maximum counter widths. The
same `wifi diag` lifetime must report `sdio_deadline_hints=<count>` from the
fault-only arm relay; ordinary accepted traffic requires zero and a nonzero
value cannot be normalized away as a successful transport wake. The
accounting `poisoned` value is the
fail-closed aggregate of a live poisoned ring, a stale client sample, and
client epoch errors; the truth line distinguishes those causes without
weakening old-capture parsing.

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
stale_purged=<n>`; `wifi diag` must emit equivalent `wifi: tx_phase*` and
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

`wifi diag` emits the proof only after a stable valid read of the admitted SDIO
owner ring and a current v11 CYW43 client-counter sample for that same physical
bus-link epoch. The v11 layout preserves the complete v10 prefix for
old-capture parsing.
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
    advance during that interval but cannot substitute for readiness. Missing
    first-report or command-ready proof alone is USB service debt, not physical
    input; it may schedule one bounded `LocalSeat` turn but cannot retain the
    selected-network operator fence without a decoded or buffered byte or
    physical response. A
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
    - USB keyboard proof reaches `USB_GATE=10` / `USB_BLOCKER=none` with `USB_COMMAND_READY=yes`, `USB_FIRST_REPORT_READY=yes`, `USB_LOCAL_SEAT_STATE=ready`, `USB_BUSY_AFTER_READY=no`, and the single interrupt-IN lane stably armed as `queued_reports=1`; any larger active depth is an invariant failure. Hardware acceptance also reaches `USB_OLDGOOD_REPLAY=yes` / `USB_OLDGOOD_MISSING=none` for the isolated hub-keyboard sequence before claiming the local-seat keyboard experience is complete. The first HID report and first byte must be sourced from `linked-runtime-hid`; `usb status` must remain honest with `physical_input_proven=no` until that linked-runtime byte also reaches parser ingress. A linked first-byte latch or parser ingress reported only as `local-seat-queue-diagnostic`, local-seat queue text, or `source=first-byte` is diagnostic by itself and never sets the proof. A printable-key line such as `runtime keyboard first-printable-byte ...`, `physical_input_proven=yes`, visible HDMI echo, and a post-`usb diag` `USB_DIAG_LIVENESS_STATUS=pass` remain required user-experience evidence. Sustained USB acceptance additionally requires `USB_POST_FIRST_BYTE_BLOCKER=none`, no `recovery-failed` report status, no post-first-byte queue collapse, and no growing no-reply or dropped-byte pressure during typing, arrow-history, and lock-key bursts. `USB_EVENT_LOOP_RUNTIME_SKIPPED` may grow when those turns intentionally service input first and is not itself a blocker.
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
      either record/prompt ordering. Prompt release itself still requires USB
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
      first-byte proof. These checks introduce no console command, driver-task
      ABI field, or USB/HDMI authority change. Pi 4 manifest-default boots must use
      `hw.local_seat.enabled=true`, `hw.local_seat.required=true`, and matching
      `usb-kbd0`/`hdmi0` `hw.devices[] required=true` declarations so missing
      declared devices fail visibly. Runtime backend attach failures may
      degrade with `required=yes action=serial-shell`; that keeps the UART root
      shell reachable but does not satisfy HDMI/USB acceptance.
  - `netstats` must report:
    - `mode=<off|static|dhcp> policy=<wired|wifi|auto> active=<iface> standby=<iface|none> addr_src=<source> ip=<ipv4> gateway=<ipv4> dhcp=<phase>`; the normalizer exposes the selected state as `NET_ACTIVE`, `NET_ADDR_SRC`, and `NET_DHCP`, and separately exposes command/listener proof as `NET_TCP_READY` and `NETTEST_PROOF`.
    - exactly one complete `nettest: generation=<connection> run_generation=<run> enabled=<bool> running=<bool> verdict=<none|running|pass|peer-assisted-pass|fail> tx_ok=<bool|na> udp_echo_ok=<bool|na> tcp_ok=<bool|na> console_ok=<bool|na> peer_assisted_ok=<bool|na>` status line. `OK NETTEST detail=started run_generation=<run>` admits one immutable run; only a terminal line for the same positive run generation is proof. An internal-only asynchronous log, an incomplete or truncated line, or a prior connection/run-generation verdict is not terminal proof; backend and target strings remain on the separate `nettargets:` line.
    - `tx_submit=<count> tx_complete=<count> tx_free=<count> tx_in_flight=<count> tx_double_submit=<count> tx_zero_len_attempt=<count> arp_rx=<count> arp_tx=<count>`; on CYW43, `tx_complete` is the root release count from exact joined Function-2 terminals. `tx_submit > tx_complete` means an outstanding root TX owner, not a missing firmware-credit acknowledgement.
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
- `configs/generated/root_task_resolved.json` — `sha256:2f840b864656017ba036810ff61bf3ff4abe2974bc95666b41be6cac01150054`

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
