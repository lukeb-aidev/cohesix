<!-- Author: Lukas Bower -->
<!-- Purpose: Retain integrated M27g qualification decisions, exact evidence boundaries and defect restoration records. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# M27g implementation and qualification record

Milestone 27g remains In Progress. The major mixed physical-console/TCP stall
was repaired in `b832145070b708a0bfb1c7d430275c940adb40af`. That image completed
a fresh 7,200.000637-second operator burn-in, including 24 scheduled CUDA jobs,
four LoRA workflows, native Mac/Jetson coverage and reconciled incident segments.
Its sealed operational verdict is PASS. Both full staged Test Plans and both
source-native desktop walkthroughs subsequently passed for their recorded
profiles. Minor qualification-collector repairs retain those records and require
focused revalidation and exact final-source qualification, not another burn-in.
The release decision still requires the remaining pressure, physical media,
repeatability, assembled-artifact and human-review gates. Historical acceptance
and failed attempts retain their original verdicts.

## Pressure profile restoration

```text
Title/ID: m27g-pressure-profile-restoration
Milestone: 27g / assembled-journeys-and-recovery
Goal: Keep pressure authentication, compilation and final verification bound to the same provisioned manifest.
Inputs: out/m27g/pressure-25277757-01/runner.log; scripts/m26e_qemu_pressure.sh; TEST_PLAN Conditional B2.
Changes:
  - scripts/m26e_qemu_pressure.sh — retain the selected manifest through environment cleanup, exclude it from deleted output trees, freeze its resolved bytes, regenerate canonical projections before common checks, and verify the frozen pressure profile after staged builds.
  - tests/test_m26e_qemu_pressure_cli.py + tests/test_rest_perf_harness.py — cover selected-profile forwarding, cleanup exclusion and frozen-manifest tampering.
  - docs/TEST_PLAN.md + docs/BENCHMARKS.md — document provisioned pressure and staged profile selection.
Commands: .venv/bin/python -m pytest -q tests/test_m26e_qemu_pressure_cli.py tests/test_rest_perf_harness.py; scripts/check-generated.sh; scripts/ci/check_test_plan.sh; rerun canonical pressure with explicit provisioned manifests.
Checks: No placeholder accepted, no profile discarded or deleted, and no final claim derived from a later build's manifest. Runtime, authority, workloads and thresholds are unchanged.
Deliverables: Focused host checks, retained failed preflight and fresh canonical pressure evidence.
```

Compatibility review covers coh/coh-status, cohsh, Hive Gateway, SwarmUI,
host-ticket-agent, host-sidecar-bridge, gpu-bridge-host, cas-tool, sidecar-bus,
the Python SDK and benchmark runners. Only the pressure runner's manifest
handoff and evidence retention require changes; tool protocols, generated
interfaces and benchmark thresholds retain their existing contracts.

The same review found the pressure lane omitted the caller delegation now
required by protected REST reads and writes. Each owned gateway lifetime now
mints a finite read/write caller using an ephemeral environment-only issuer,
following the existing Stage 04 provisioning contract. The standard SDK and
host tools consume `COH_REST_TICKET`; no compatibility mode, authority check or
workload bound changes. Focused shell execution verifies the caller's declared
scope, lifetime, operation budget and issuer reference without real credentials.

## Native CUDA refusal restoration

```text
Title/ID: m27g-native-cuda-refusal-restoration
Milestone: 27g / m27g-two-hour-pi4-operator-burn-in; restoration: 27b / m27b-provider-action-registry
Goal: Preserve a native executor refusal as a refusal and reconcile bound admission failures without dispatch or replay.
Inputs: Pi image 23513e1c955e, host source 683587f6a, selected Pi manifest e38454f6667d923fc6e3368ca2b9d44aba7ec5407fd9695e4aa526aec035dcaf, m27g-run-01 and native UI revalidation evidence.
Changes:
  - apps/gpu-bridge-host/src/workload.rs — persist stale-input and insufficient-ticket-time failures with the exact input before replying; authenticate and type explicit refusal replies.
  - apps/host-ticket-agent/src/executors/workload.rs — preserve known pre-dispatch refusals; retain ambiguous handling for transport, authentication, retained-output and idempotency errors.
  - docs/GPU_NODES.md and docs/HOST_TOOLS.md — document failure reconciliation and inventory freshness during desktop admission.
Commands:
  - cargo test -p gpu-bridge-host --lib workload::tests -- --nocapture
  - cargo test -p host-ticket-agent --lib executors::workload::tests -- --nocapture
  - cargo clippy -p gpu-bridge-host -p host-ticket-agent --all-targets -- -D warnings
  - scripts/check-generated.sh
  - scripts/ci/check_test_plan.sh
Checks: Deterministic stale input and short ticket failures retain exact bytes and terminal time without creating a native child; replay/recovery return the same failure. Unauthenticated or mismatched replies never establish a refusal. Fresh native/Worker checks and a new full timed attempt remain required.
Deliverables: Focused command logs, new exact host packages, native refusal and ordinary-work evidence, failed-attempt records and a new 120-minute burn-in before full release testing.
```

The original timed native UI case omitted the required planning step. Its
workload was never admitted, and its exact lease was subsequently released with
a confirmed Worker receipt. A separate UI revalidation correctly refused stale
inventory. The next case admitted successfully, but its harness observed the
first evidence record before the complete grant prefix. Its delayed execution
then exposed a product defect: an explicit native refusal was mapped to pending
provider state, requiring expiry reconciliation. Each original failure remains
retained under `out/burn-in/m27g-run-01`; correcting a harness does not relabel it
as a pass. The product recovery change is classified as major and requires a new
complete 120-minute attempt, as authorized by the operator.

Compatibility review covers coh/coh-status, cohsh, Hive Gateway, SwarmUI,
host-ticket-agent, host-sidecar-bridge, gpu-bridge-host, cas-tool, sidecar-bus,
`tools/cohesix-py`, and performance benchmark scripts. The existing authenticated
socket response and job/journal/evidence schemas, manifest policy, five-second
inventory lifetime, quotas and performance thresholds remain unchanged. Recipe
consumers already reconcile failed jobs and preserve failed requested outcomes.
No other implementation change is required; rebuild affected package dependencies
and test their native composition. These focused checks do not replace the full
post-burn staged target, pressure, due-diligence or release promotion gates.

## Release A candidate selection

```text
Title/ID: m27g-release-a-candidate-selection
Milestone: 27g / m27g-adoption-overhead-and-release-cut
Goal: Select the reserved 1.1.0-beta candidate without modifying or promoting historical release evidence.
Inputs: Integrated source 7d395cf751db9c64fd49c44148a3502167bf985c; configs/implementation_surfaces.toml; release factory and publication guard.
Changes:
  - configs/implementation_surfaces.toml — select 1.1.0-beta and its current notes; retain linked, immutable 1.0.0-beta notes as historical documentation; include current adoption/journey guides, maintained CI workflow and CUDA/provider/use-case contracts in the exact release inventory.
  - releases/RELEASE_NOTES-1.1.0-beta.md — describe implemented Release A workflows and explicitly pending assembled qualification.
  - scripts/release_publication.py — permit current 1.1.0-beta notes and this qualification record to be finalized after testing while refusing edits to historical or future release notes and prior milestone records.
  - tests/test_release_bundle.py — verify reserved version selection, exact note inventory and publication refusal boundaries.
  - compiler outputs — regenerate inventory and dependent host/provider bindings from the changed source.
Commands:
  - .venv/bin/python -m pytest tests/test_release_bundle.py -q
  - .venv/bin/python -m pytest tests/test_release_inputs.py tests/test_release_qualify.py -q
  - scripts/check-generated.sh
  - scripts/ci/check_test_plan.sh
Checks: Candidate version and current notes agree; old release files remain byte-identical; publication reuse cannot accept changes to their notes or reserved future notes. These focused metadata checks do not start or replace the post-burn full Test Plan.
Deliverables: Source-selected candidate identity, draft notes and focused command evidence; release acceptance remains pending.
```

Compatibility review covers coh/coh-status, cohsh, Hive Gateway, SwarmUI,
host-ticket-agent, host-sidecar-bridge, gpu-bridge-host, cas-tool, sidecar-bus,
the Python SDK and performance scripts. CLI grammar, namespaces, authority,
runtime behavior, benchmark workloads and thresholds are unchanged. Packaging
consumes the new release inventory; dependent generated host/provider identities
must be rebuilt and qualified. Earlier default-profile builds of 7d395cf75 remain
preparation evidence for that source, not builds of this revised inventory.

The focused release-bundle checks passed (17 tests), as did the release-input
and qualification contract checks (28 tests), generated consistency and Test
Plan metadata checks. The original missing-adoption-document failure remains
retained in the local qualification logs. No staged target test or production
release acceptance is implied by these focused checks.

## Post-burn qualification entry

```text
Title/ID: m27g-post-burn-host-qualification
Milestone: 27g / m27g-assembled-journeys-and-recovery
Goal: Run the complete staged Test Plan after the successful operational burn-in and repair its first failed contract.
Inputs: Checkpoint 9837b9400; out/burn-in/m27g-run-02/retained-final-03/index.json; Rust 1.97.1; fresh post-burn Stage 01 attempt.
Changes:
  - crates/cohesix-authority/src/{mac_release,macos}.rs — borrow single-element test inputs instead of cloning them; preserve every assertion and all runtime code.
Commands:
  - TP_HOST_JOBS=4 TP_UI_WORKERS=2 TP_PYTHON_BIN=/opt/homebrew/bin/python3 scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m27g-post-burn-9837-qemu-01 --stage 1
  - CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings
  - CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 cargo test -p cohesix-authority --features std --lib
Checks: Preserve the original Stage 01 lint failure; strict lint and the existing authority contract tests must pass before a fresh source-bound staged attempt. This test-only repair changes no burned-in product behavior and does not require another two-hour session.
Deliverables: Retained failure/repair command logs, fresh complete staged evidence and subsequent release qualification; milestone closure remains pending.
```

The operational closure is scoped to the frozen Pi manifest
`e38454f6667d923fc6e3368ca2b9d44aba7ec5407fd9695e4aa526aec035dcaf`
and unchanged installed packages. Two minor timed harness incidents retain their
failed records and excluded repair intervals. Later native LoRA admission,
recovery and replay write refusal passed on both hosts; the delayed Jetson UI
observation was reconciled against the same signed nine-record graph and
confirmed Worker receipt without resubmission. Native shutdown logs report no
busy host operation. The exact supplemental Worker retired; Pi closure is
QUIESCED with zero active leases, and owned serving/gateway/tunnel processes
stopped. User-owned timestamped captures remain untouched.

The current integrated release inventory has a different generated graph and
requires its own assembled artifact qualification. Empty sidecar-bus endpoint
maps retain typed NotEnabled, with no physical field-bus claim; deferred 28a
providers remain outside the selected scope. No historical release evidence,
benchmark threshold, quota or runtime timeout changed. Compatibility review of
the complete host-tool suite, Python SDK and benchmark scripts finds no affected
runtime or interface surface from the two borrowed test inputs.

The second common-stage attempt retained a workspace-test failure at an older
attestation expectation. A complete diagnostic workspace pass then identified
five affected test targets. Contract review found stale tests and one stale
current build-script pin, with these repairs:

- Sealed-pack inspection has verified content integrity since `3452248de`.
  Attestation and attachment corruption must fail at that boundary. The positive
  diff test now exports two separately sealed observations and independently
  verifies that mutating the original sealed pack is rejected.
- `INTERFACES.md` and every checked-in selected manifest require schema 1.27.
  Legacy-schema tests now parse and modify the schema field explicitly, so an
  obsolete replacement string cannot silently leave the current schema intact.
  The expected retired-version errors and current-profile output remain exact.
- The same `3452248de` restoration forbids dropping federation identity to fit
  the legacy envelope. The agent test now requires its exact 224-byte refusal,
  no native dispatch and no published status/dead-letter result. The existing
  serializer test independently verifies retained identity in a sufficient bound.
- Canonical console help now says “Show commands available on this console.”
  The local-seat test retains its exact early-output refusal and later-output
  requirement using that current line; scheduling behavior is unchanged.
- `9bffe4c86` added the native SwarmUI source/asset hash to its build script.
  Review confirms sorted source paths/bytes produce the fixed hexadecimal
  `SWARMUI_SOURCE_SHA256`; the existing Tauri build remains its only build
  delegate. Refresh only that current script pin to
  `e0a9efd6ecb57cc364e3a6fcc89fb426b0af3d78de06df3129ae49104102fd19`
  in the scanner and matching baseline metadata. The historical pin, all risk
  counts/ceilings and historical replay obligations remain unchanged.

These repairs change test inputs/assertions and current audit metadata only;
all burned-in runtime implementations remain unchanged. The first and second
staged failures and the complete diagnostic failure are preserved under
`out/m27g-integration/post-burn-*`. Fresh complete staged evidence is still
required after focused verification and the repaired-source checkpoint.

Focused verification passed with unchanged assertion limits:
`cargo test -p coh --test attest --test operator` (14 tests),
`cargo test -p coh-rtc --test ai_lora_contract` (11),
`cargo test -p host-ticket-agent --lib` (70),
`cargo test -p root-task --lib` (317), and
`cargo test -p rust-risk-audit` (28). Strict workspace Clippy, formatting,
generated consistency, Test Plan metadata and
`env -u CARGO_HOME scripts/ci/rust_risk_gate.sh --baseline docs/audit/rust_risk_baseline.toml`
also passed. The risk gate reports no count increase: 234 `expect`, 99 `panic`,
823 `unsafe` and 38 `unwrap`, each at or below its unchanged baseline. These
focused results permit a fresh common-stage attempt; they do not complete the
staged Test Plan or replace required human review.

The third common-stage attempt exposed an invocation error: the local wrapper
inherited `COH_AUTH_TOKEN_REF` into mock TCP CLI tests, where it correctly took
precedence over their fixture token. Removing live target credentials from the
common host-test environment restored all six TCP fixture checks without a
product change. The subsequent clean-environment attempt passed every Rust
action and reached the complete Python inventory: 2,643 passed, two skipped,
110 subtests passed and one stale-fixture import failed.

That Python test imported `ReleaseOwnerWaiverTests`, removed by `c1f6a0d97` when
the owner permanently retired recurring DD30 acceptance. Consolidate its source
scope protection into the existing direct `validate_host_successor` test:
both an extra tracked implementation change and an untracked implementation
file must fail with the exact source-delta refusal. The obsolete cross-fixture
test is removed; the historical release's approved bytes, patch, path allowlist,
dates and evidence checks are unchanged. This preserves protection without
reintroducing the retired DD30 waiver or changing any historical decision.
`python -m pytest -q scripts/ci/test_release_stage5_acceptance.py scripts/ci/test_due_diligence_lifecycle.py`
passes 52 tests and 82 subtests. Python client/playbook example smokes also pass.
All seven hostile Rust audit-bootstrap checks pass, as do generated consistency
and Test Plan metadata checks.
This test-only repair leaves the complete host-tool suite, Python SDK, native
runtime behavior and performance contracts unchanged. A new clean-source
Stage 01 attempt remains required; all previous failed records are retained.

The clean `3010ec2ce` common Stage 01 passed all 22 actions, including the
complete Rust checks and Python inventory (2,643 passed, two skipped, 112
subtests). QEMU Stage 02 passed. Stage 03 then stopped before target launch:
`root_task_regression.toml` omitted the three CUDA workload actions introduced
in 27b, while the target-qualified Python contract correctly requires their
complete receipt matrix. Add `gpu.workload.submit`, `gpu.workload.cancel` and
`gpu.workload.observe` to that manifest's general and receipt allowlists. Extend
the existing selected-profile contract check to render the regression profile
and require the same independently specified six GPU actions on QEMU, Pi and
the gated regression target. Preserve distinct manifest hashes, existing gated
surfaces, default operational profiles and all bounds.

This repair belongs to 27g / `m27g-assembled-journeys-and-recovery`. The compiler
already generates the required host-tool and Python projections; fresh gated
artifacts must now be generated and compiled by the canonical regression runner.
Review of coh/coh-status, cohsh, Hive Gateway, SwarmUI, host-ticket-agent,
host-sidecar-bridge, gpu-bridge-host, cas-tool, sidecar-bus, the Python SDK and
benchmark scripts found no additional implementation changes: only the gated
regression manifest was missing the existing contract. The operational burn-in
profile already contained these actions, so its runtime remains unchanged.
The six Python-profile tests, formatting, generated consistency and Test Plan
metadata checks pass. Fresh staged evidence for the repaired source is required.
The independent Pi Stage 02 attempt stopped at an absent local U-Boot build;
that is a checkout dependency preparation failure, not Pi execution evidence.

On `0aeff41b0`, common Stage 01 passed all 22 actions and both QEMU and Pi
Stage 02 passed. The Pi Stage 03 matrix completed all 17 scripts: 16 passed,
and `host_absent.coh` retained an obsolete five-entry expectation. The selected
manifests enable `/host/snapshots`, documented in `HOST_SNAPSHOTS.md`, alongside
`tickets` and the four existing provider directories. The live transcript
independently shows all six names. Correct that exact fixture expectation;
the focused physical rerun passed against the unchanged image. This is a
27g / `m27g-assembled-journeys-and-recovery` test restoration, with no runtime,
namespace, policy, tool, Python SDK or benchmark implementation change.
The complete fresh staged campaign remains required for the repaired source.

The first `0aeff41b0` QEMU Stage 03 attempt built both variants but its selected
Homebrew emulator aborted in `hvf_arch_init_vcpu` before root-console readiness.
The local invocation now explicitly selects the QEMU 10.1 build already
documented in `TOOLCHAIN_MAC_ARM64.md`; its SHA-256 matches the documented
`a0471828f464116c51c1d29ebae12a2a0fc713b4edec5c52e81bd5388040135a`,
and strict code-signature verification passed. The original emulator failure
remains retained; changing this host selector does not establish target success.

Pi transport collectors also retain non-claiming first-connection raw samples.
The final base boot completed all 1,024 requests without a retry or reconnect,
but measured 531.441 requests/s and 5.157291 ms p95 during concurrent host
compilation. Those values miss the unchanged 600 requests/s and 5 ms performance
bounds and are not performance acceptance. Host scheduling interference remains
an unproven explanation; fresh controlled performance and repeatability evidence
is still required. Neither this observation nor a repaired fixture promotes the
failed Stage 03 attempt or changes the completed burn-in verdict.

The first common-stage attempt after that fixture repair (`744b0a097`, including
the owner's README reordering) caught its companion token-stream catalog pin.
Refresh only `host_absent.coh`'s exact pin for the reviewed `entries=6` assertion;
all other script hashes and the grammar feature inventory remain unchanged.
The original failed attempt remains retained. This completes the fixture's
same-change consistency work and does not alter executable product logic.

On `06e895348`, common Stage 01 and both target Stage 02 checks passed. The
separate production HVF guest reached the console, then the direct TCP matrix
rejected its complete help response because that test retained the pre-manual
15-frame command index. The published root-console surface now has 25 frames:
the command header, ten diagnostics, the session header, twelve session commands
and the host-command guidance. Restore that exact ordered expectation, including
the section and guidance lines. Keep the response-frame ceiling, ACK order,
same-connection requirement and QUIT/EOF checks unchanged. The host fixture
independently specifies the surface and rejects changed headers or guidance;
its local server credential is isolated from inherited authentication references.

This is a test restoration under 27g / `m27g-assembled-journeys-and-recovery`.
Ten focused host tests pass. A diagnostic replay of the corrected harness
against the unchanged `06e895348` production guest passes the complete response
matrix and all ten base scripts. It remains diagnostic evidence because the
harness changed after that guest's source identity was frozen; the original
failed run is retained. Product behavior, all host tools, the Python SDK,
generated contracts and benchmark thresholds are unchanged. No additional
implementation change was needed across those reviewed surfaces, and this
repair does not require repeating the successful operator burn-in.

The `06e895348` physical campaign subsequently passed all 17 Stage 03 scripts
and the complete Stage 04 REST gate. Stage 05 stopped at eight dependency
advisories in `aws-lc-sys`, `quinn-proto` and `rustls`; no ordinary Stage 05 PASS
was emitted. The reviewed lockfile update selects `aws-lc-rs 1.18.1`,
`aws-lc-sys 0.45.0`, `quinn-proto 0.11.15`, `rustls 0.23.45` and its required
`rustls-webpki 0.103.15`. No advisory suppression or exception is added.
`cargo audit --json` reports zero vulnerabilities against advisory database
commit `d5c17953a895cf19e8d3ce66eaa42b6fcfe1fb16`; `cargo deny check advisories`
passes with the existing warning policy.

This dependency restoration belongs to 27g /
`m27g-assembled-journeys-and-recovery`. The exact Pi production normal-dependency
tree contains none of those libraries, and Quinn is absent from the normal host
dependency graph. Host HTTPS clients do use rustls through reqwest/ureq, so
fresh host builds and transport checks remain required. The rustls advisory
[RUSTSEC-2026-0285](https://rustsec.org/advisories/RUSTSEC-2026-0285) concerns
TLS encryption-level enforcement while retaining transcript authentication.
The timed burn-in used the private HTTP gateway, local provider endpoints and
native CUDA/LoRA execution. Its original artifact identities and duration remain
unchanged; the dependency repair requires focused revalidation and the current
full release gates rather than relabelling that run as time on new binaries.
No major Cohesix execution defect has been identified in this advisory closure.
The focused cohesix-rest, attestation, coh and host-sidecar-bridge suite passes
127 tests with the updated dependencies. Generated consistency and Test Plan
metadata checks also pass; these focused results do not replace Stage 05.

Production-profile wheel qualification also exposed a stale smoke workflow:
it attempted the compatibility-only Worker spawn without a strict Queen intent.
The SDK correctly refused it. The repaired package gate checks that refusal and
the exact versioned intent bytes, identity, command and selected writer epoch
for all three executable roles. It explicitly reports serialization only:
MockBackend retains those bytes and does not execute strict intents. The
compatibility profile retains its spawn/READY/teardown checks, while live target
and release gates retain all execution obligations. Twenty-two focused package
and bundle tests pass. The unchanged target-neutral wheel, SHA-256
`ece24d1ac24147cd818cce93c93bc2797c375f7b3384662d8e4503fe542cb12f`,
passes the repaired gate on native macOS and Linux AArch64 with both
CPython 3.11.13 and 3.13.7. Those records retain the exact repaired harness and
profile hashes; they are package evidence only.

Compatibility review covers coh/coh-status, cohsh, Hive Gateway, SwarmUI,
host-ticket-agent, host-sidecar-bridge, gpu-bridge-host, cas-tool, sidecar-bus,
the Python SDK and benchmark scripts. The host HTTP dependency closure changes;
application grammar, generated authority, provider/Worker contracts, Python
implementation, benchmark workloads and thresholds remain unchanged. Fresh
native package builds and complete staged/release evidence remain required.

The `40af32d76` QEMU campaign passed Stages 01–03. Stage 04 then refused its
readiness probe because the retained `cohsh` expected its selected profile's
policy while the harness loaded the restored checkout default. The gateway
itself attached successfully. Stage 04 now defaults both client policy selectors
to the verified Stage 03 artifact, retains explicit operator overrides and
normal hash checks, and restores inherited selectors before finalizing evidence.
Eighteen focused REST security/lifecycle tests pass, including retained-policy,
explicit-override, missing-policy and environment-restoration checks. The failed
attempt remains unchanged; this harness repair does not establish target or
release acceptance.

Release preparation also verified that compiled target binaries contain the
resolved build-time ticket credentials, even when generated JSON exposes only
secret references. Preparation images built with private burn-in credentials
remain private and are excluded from public release selection. A separate
five-role qualification keyset has been provisioned for fresh candidate builds;
no key values are recorded here. The Quickstart, authority and security guides
now explain exact-image credential provisioning, image confidentiality and
rebuild-based rotation. Public evaluation delivery requires its own explicitly
shared credentials and cannot reuse private hive keys. This is existing
build-time behavior, not a newly introduced runtime or authority path.

The `1593b3dee` QEMU campaign passed Stages 01–02 and stopped in Stage 03
at `shard_1k.coh`: the first telemetry write followed spawn admission before
the executable Worker reached READY. The target correctly refused it. The
script now observes the existing bounded READY condition on each Worker before
writing either its sharded path or legacy alias. Both existing host namespace
tests pass, including alias-disabled refusal; a diagnostic replay against the
unchanged `1593b3dee` guest also passes. Admission, target readiness and host-model
observations retain their distinct proof classes. No runtime change, retry of a
write, longer deadline or relaxed assertion was introduced.

Candidate assembly independently rejected an interface-documentation example
whose illustrative policy digest repeated the published CAS test signing key.
The example now uses an unrelated illustrative digest and explicitly requires
the real policy-revision digest. The canonical private-fixture/canary scanner
remains unchanged; its four focused tests pass and the corrected document passes
the scanner. The original rejected payload is retained. Because this interface
document is outside the publication-only allowlist, qualification restarts from
a fresh source identity instead of expanding that allowlist or rebinding old
artifacts. These two restorations belong to 27g /
`m27g-assembled-journeys-and-recovery`; the operational burn-in verdict and
original identities remain unchanged. Compatibility review found no changes to
the complete host-tool suite, Python SDK, provider/Worker implementation or
benchmark workloads and thresholds.

The sharding-script restoration also updates its canonical token-stream
fingerprint. The full host workspace test lane (excluding the separately owned
SwarmUI and physical driver-runtime lanes), workspace Clippy with warnings denied,
and formatting checks pass with that companion fixture. The previous frozen
attempt remains failed at the stale fingerprint; no target artifact from it is
silently rebound. The earlier `1593b3dee` Pi campaign passed all 17 TCP scripts,
the REST gate and due-diligence checks, but final Stage 05 attestation refused the
missing fresh runtime/DMA proof. That required physical proof remains outstanding.


## Bounded driver proof retention restoration

Title/ID: m27g-driver-proof-log-retention
Milestone: 27g / m27g-assembled-journeys-and-recovery; restoration: 26e / m26e-driver-runtime-mcs-port-and-cyw43-coexistence.

The dedicated e571 Pi boot retained early bootstrap history but exported blank
lines in place of detailed scheduling, owner-state and DMA receipts. Inspection
confirmed that `LogRing::push_line` ignored a failed insertion of records larger
than 256 bytes. Some producer formatting buffers also could not hold their
complete record. The canonical normalizer correctly refused acceptance; the
unsupported `hdmi status` collector command is retained as a separate harness
failure. Evidence is in `pi4-runtime-e571-01` under the Pi qualification worktree.

Restore the existing boot-record producer and ordered-log handoff with checked
1024-byte formatting and
the existing bounded fragment mechanism, using a distinct `DRIVER_LOG` envelope.
Keep the 2048 ordinary entries, 256-byte entry bound, nonblocking lock, UART
ownership, scheduling and DMA behavior unchanged. Decode only complete,
boot-local driver observations. Required physical evidence remains pending.
This is an observability repair, not evidence of a workload execution defect;
the earlier two-hour run retains its original source identity and verdict.

Compatibility review: coh/coh-status, cohsh, Hive Gateway, SwarmUI,
host-ticket-agent, host-sidecar-bridge, gpu-bridge-host, cas-tool, sidecar-bus and
the Python SDK transport the unchanged log stream. Their command and authority
contracts need no changes. Pi qualification and benchmark proof readers use the
canonical normalizer, updated alongside the producer. Worker fragments retain
their exact envelope and decoder. Benchmark workloads and thresholds are unchanged.

Jetson Remote Desktop recovery is verified separately: authentication with the
existing VNC credential, a fresh desktop clock and a remote mouse action passed.
The Linux account password was not the VNC credential. No password was reset;
protected temporary credential copies were removed. Capture and result are in
`out/m27g/jetson-desktop-recovery-20260921` in the user checkout.


Focused validation: the Pi-feature driver subset passed 725 tests; the final
bounded-log suite passed 15 tests; the Pi normalizer and gate wrapper passed
1023 tests. `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all -- --check`, `scripts/check-generated.sh`, and
`scripts/ci/check_test_plan.sh` passed. Logs are under
`out/m27g-harness-repairs/driver-fragments-*`. An additional, noncanonical
`cargo clippy -p root-task --no-default-features --features driver-tests-pi4
--lib -- -D warnings` invocation reported 372 diagnostics in that broader
feature closure; its failed log remains retained and no lint suppression was
introduced. The canonical workspace lane above is the merge-required lane.

The prior e571 checkpoint completed all five QEMU Test Plan stages at
`out/test-plan/m27g-post-burn-e571-qemu-01` in the integration worktree.
Its native HVF build/base execution, KVM build, strict Pi build, and all eight
native Linux host-tool builds also passed their named scopes. These results
remain bound to e571 and do not qualify this later driver-log repair or complete
Milestone 27g.

The fresh `6c67f58b9` physical attempt (`pi4-runtime-6c67f58b-01`) recovered
complete driver records and reached all five linked runtimes. Its canonical gate
still failed: constructor affinity records had expired during ordinary boot log
traffic, and the host checker required descriptor ABI 8 while the target's
`pi4-driver-abi` emits ABI 13. Preserve that failed result unchanged. The follow-up
retains one compact original constructor identity per admitted driver through
the existing trusted boot-audit slots; it does not reconstruct records after
boot, expand ring bounds or claim DMA readiness from constructor identity. The
normalizer and synthetic current-ABI fixtures select exactly version 13 and
reject historical or unknown versions. The complete host-tool, Python and
benchmark compatibility review above applies; their transport and authority
contracts remain unchanged. Fresh target qualification remains required.

Follow-up focused validation passed the constructor identity test, all 1026 Pi
normalizer/gate tests, canonical workspace Clippy, generated consistency and Test
Plan metadata checks (`driver-identity-*` logs in the repairs worktree). The first
Rust compile exposed a capability pointer type mismatch, corrected to
`seL4_CPtr`; its failed log is retained. Diagnostic replay of the unchanged 6c67
capture recognizes all five ABI-13 descriptor seals, but remains short of
constructor affinity proof and cannot qualify the repair.

The newly installed HDMI capture card supplied original OBS frames before and
after a paced serial `ping` on that same 6c67 Pi. Both serial and live HDMI show
`PONG` followed by the prompt, with no observed display corruption. Evidence is
`out/m27g-integration/hdmi-capture-6c67-01` in the Pi worktree. This is display
responsiveness evidence only; physical keyboard input and later-image HDMI
qualification remain separate.

Fresh physical validation on `97ff6274d` passed the canonical driver gate for
image `ed46513479f451f057feb35af890cbf5740e75c43694e2966bd5bf907e20deb5`:
all five constructor affinities match, all five current descriptor seals qualify,
and no ring call or timeout remains outstanding. Original evidence is
`pi4-runtime-97ff6274-01` in the Pi qualification worktree. Paired live OBS frames
in `hdmi-capture-97ff-01` show U-Boot, root startup and final operator diagnostics
without observed display corruption. This closes the focused logging/decoder
restoration, not the remaining staged, pressure, media or release obligations.

## Native namespace response identity restoration

Title/ID: m27g-native-namespace-response-identity
Milestone: 27g / m27g-assembled-journeys-and-recovery; restoration: 27f / m27f-namespace-explorer.

The exact source desktop package at `6c67f58b9` passed the canonical native
Mac walkthrough through a real isolated HVF guest and authenticated gateway.
Credentials were refused correctly, live reads and Providers completed, reconnect
worked, retained LoRA/recovery references preserved their outcomes, and offline
control was refused. Evidence is `mac-native-source-6c67-01` in the native release
worktree. It is focused native evidence, not assembled release acceptance.

Visual inspection additionally found that direct `cat /proc/boot` after browsing
`/shard` displayed the correct response beneath the stale `/shard` heading and
breadcrumb. The shared read renderer now binds the preview identity to each
response for list, cat, tail and refusal paths. Original failed browser checks
and screenshots remain under `namespace-identity-*` in the repairs worktree.
Their bridge is explicitly a fixture; native verification of the corrected app
remains required before closure.

Compatibility review: this repairs frontend response presentation only. coh,
coh-status, cohsh, Hive Gateway, host-ticket-agent, host-sidecar-bridge,
gpu-bridge-host, cas-tool, sidecar-bus, the Python SDK and performance workloads
retain their command, authority, data and measurement contracts. Generated
contracts require no changes. Packaged SwarmUI must be rebuilt on each native
host because its embedded frontend identity changes.

Validation: the full source presentation matrix passed 90 checks with 12
configured skips; the final focused namespace matrix passed nine checks across
desktop WebKit, narrow WebKit and Chromium tablet. Generated consistency, Test
Plan metadata and formatting checks passed. Logs are retained as
`namespace-identity-full-ui-01`, `namespace-identity-final-focused-01`,
`namespace-identity-generated-01` and `namespace-identity-test-plan-01` in the
repairs worktree. The original failing fixture proves the stale heading and is
not promoted to native evidence.

## Mixed physical console and TCP stream liveness restoration

Title/ID: m27g-physical-prompt-stream-owner
Milestone: 27g / m27g-assembled-journeys-and-recovery; narrowly reopened
26e / m26e-console-network-service-isolation.
Goal: Complete each physical prompt without borrowing an unrelated TCP stream's
completion, preserving bounded response priority and display progress.

A comprehensive HDMI check on source
`f4730d39de591f2b8b02f59e0ca4cda0adff1051`, physical image
`1d1e9ec5b3dfa1199370f16359cef5ea9feb57450d5a042dd6f01b8c98ba829f`,
found a mixed-surface failure after repeated `netstats`, `help` and `ping`
commands and concurrent authenticated `/proc/boot` reads. Serial retained
`PONG` and `OK PING reply=pong` without the following prompt for 60 seconds.
The TCP read timed out after its ACK and partial body. HDMI stopped partway
through the preceding help text. A later passive observation and `usb status`
received no bytes. The packet capture still shows the isolated TCP child
acknowledging the client's FIN; this is not evidence that root resumed.

The original two-hour burn-in PASS remains historical evidence for its recorded
source and workload. This newly found control-plane liveness fault blocks
release qualification. The confirmed prompt-ownership defect is a major
operational issue requiring a fresh two-hour burn-in before the final full
Test Plan.

Evidence is retained in the Pi qualification worktree under
`out/m27g-integration/hdmi-output-stress-f4730d39-05/` and
`hdmi-mixed-console-first-fault-f4730d39-01/`. The latter holds first-fault and
post-fault records, original OBS video custody, a screenshot reference and
packet-header observations. The live user packet captures were preserved.
The original video is protected qualification evidence, not a public asset.
Earlier scratch attempts 01–03 failed their host invocation or script bounds;
attempt 04 incorrectly treated the documented network-owner busy refusal as a
product failure. Those attempts remain retained and do not establish a target
fault or successful mixed-pressure qualification.

The source defect was that `process_console_line` deferred a physical prompt
whenever any stream had an outstanding `END`. A network-owned stream must not
own that prompt. Physical response priority otherwise blocks the very
network/runtime turns required to finish the stream. Both direct and deferred
physical completion now wait only for their own stream. Deterministic regressions
retain the TCP owner, complete serial and local-seat prompt tails, and retire
the physical response barrier without advancing or falsely ending TCP output.
The display renderer, driver grants and scheduling reservations remain unchanged.
Fresh physical mixed-surface evidence is required after the focused regression.

Compatibility review: coh/coh-status, cohsh, Hive Gateway, SwarmUI,
host-ticket-agent, host-sidecar-bridge, gpu-bridge-host, cas-tool, sidecar-bus,
`tools/cohesix-py` and benchmark scripts retain their protocol, grammar,
authority, generated bounds and performance thresholds. The target fix restores
existing terminal/prompt ownership; host implementations and benchmark schemas
require no semantic change. Rebuild and qualify final exact-source artifacts.
Root and host help continue to index the same commands; the operator guide and
Test Plan describe the competing-surface refusal and independent PING contract.

The retained `physical-prompt-owner-red-04.log` reproduces the missing prompt
before the repair. Focused QEMU event tests (501) and Pi event tests (496),
workspace Clippy, and generated-contract checks pass after the repair. Exact
commands and exits are retained in
`out/m27g-harness-repairs/physical-prompt-owner-focused-pipeline-01.json`;
generated validation is in `physical-prompt-owner-generated-01.log`.

Current status: deterministic repair validated; fresh physical proof pending;
no fresh burn-in, final-source staged acceptance or release promotion claimed.

## Serial menu prompt collection after the completed burn-in

```text
Title/ID: m27g-serial-menu-fragment-restoration
Milestone: 27g / m27g-assembled-journeys-and-recovery
Goal: Preserve a U-Boot choice prompt split across consecutive host serial reads.
Inputs: b8321450 unchanged Pi image; post-burn pi4-entry-post-burn-b8321450-02 serial transcript and RAM transfer receipt.
Changes:
  - scripts/pi4_serial_reboot.py — carry the bounded prompt suffix into the next read, then append only newly observed bytes.
  - tests/test_pi4_serial_reboot.py — check each prompt split with an initial snapshot and a newly read menu; require the exact original byte sequence.
Commands: /Users/lukasbower/GitHub/cohesix/.venv/bin/python -m pytest -q tests/test_pi4_serial_reboot.py
Checks: 144 tests passed; fragmented prompts match without duplicate bytes or relaxed markers/timeouts.
Deliverables: Parser repair, deterministic regression, and separately retained physical collector revalidation.
```

The first read ended with `Se` and the second contained `lect option [1]:`.
The collector had discarded the matching context between its two waits. The
original failure remains retained; the target was waiting at its valid menu.
This changes host evidence collection only. It changes no target binary,
authority, timeout, performance threshold, or measured burn-in interval.

Compatibility review found no changes needed in coh/coh-status, cohsh, Hive
Gateway, SwarmUI, host-ticket-agent, host-sidecar-bridge, gpu-bridge-host,
cas-tool, sidecar-bus, the Python SDK, or performance workloads/report schemas.
The serial helper's command line and returned menu bytes are unchanged. Its
physical revalidation must identify the repaired collector separately from the
unchanged image; existing acceptance records retain their original source.

## Pressure report validation restoration

```text
Title/ID: m27g-pressure-report-restoration
Milestone: 27g / m27g-assembled-journeys-and-recovery
Goal: Validate retained pressure using the existing benchmark marker and namespace snapshot contract.
Inputs: Immutable 2796c0b6 medium/high pressure reports and raw fault logs; passing post-burn 482981ca staged QEMU evidence.
Changes:
  - scripts/worker_task_evidence.py — require the existing Heartbeat, GPU and LoRA ELF marker entries, exact canonical namespace paths, and hash-bound empty collection snapshots.
  - tests/test_worker_task_evidence.py — use independent canonical role/path fixtures; reject missing roles, empty summaries and tampered empty-collection hashes.
Commands: python -m pytest tests/test_worker_task_evidence.py -q; scripts/check-generated.sh; scripts/ci/check_test_plan.sh; re-run collect-qemu over the immutable pressure inputs into a fresh output directory.
Checks: Each role remains required; raw ELF/fault bytes, hashes, identities, outcomes, pressure thresholds and original failed collection remain unchanged.
Deliverables: Focused regression results and a separately identified corrected collection; no new burn-in or target-pressure window is required for this collector-only repair.
```

Compatibility review covers coh/coh-status, cohsh, Hive Gateway, SwarmUI,
host-ticket-agent, host-sidecar-bridge, gpu-bridge-host, cas-tool, sidecar-bus,
the Python SDK and benchmark scripts. The benchmark producer already emits
these role-qualified entries and full namespace paths. Schedule queues, active leases and preemptions may correctly be empty; their summary records remain mandatory. The corrected collection passes Worker-component and root-TCB acceptance on the untouched raw inputs; all 132 collector tests pass. No host implementation, runtime, generated
contract, wire format, authority, workload, resource bound or threshold changes.


```text
Title/ID: m27g-provisioned-python-contract-packaging
Milestone: 27g / m27g-adoption-overhead-and-release-cut
Goal: Retain the Python Pi contract from the same provisioned manifest as the selected release image.
Inputs: Native release builds, selected QEMU/Pi manifests and installed-wheel smoke records.
Changes:
  - scripts/cohesix-build-run.sh — select the companion Pi source through COH_RTC_PI4_MANIFEST before retaining generated release files; preserve canonical defaults and native QEMU profile selection.
  - tests/test_release_bundle.py — exercise both native host selections, explicit/default Pi routing and missing-input refusal.
  - docs/TOOLCHAIN_MAC_ARM64.md — document provisioned companion selection.
Commands: python -m pytest -q tests/test_release_bundle.py tests/test_release_inputs.py; bash -n scripts/cohesix-build-run.sh; scripts/check-generated.sh; scripts/ci/check_test_plan.sh.
Checks: Generated projections remain compiler-owned and bound to tested artifacts; no post-test replacement or evidence relabeling.
Deliverables: Focused checks and fresh affected native artifacts retained with their exact sources.
```

Compatibility review: coh/coh-status, cohsh, Hive Gateway, SwarmUI,
host-ticket-agent, host-sidecar-bridge, gpu-bridge-host, cas-tool and sidecar-bus
retain their runtime and authority contracts. The target-neutral Python wheel
is unchanged; its selected Pi projection now follows the provisioned image.
Performance scripts, workloads, bounds and thresholds are unchanged. This build
input correction does not invalidate the completed operational burn-in.

The retained host gateway cost attempt stopped before gateway startup because
its combined leaf read scope and write scope exceeded the compiled transport
ticket bound. The isolated benchmark now grants its fresh run-owned issuer
read scope `/proc` and write scope `/queen`. No production credentials, target
access, thresholds, operation counts or measured scenarios change. Validation
runs the canonical delegated-authority probe against the retained production
binaries; the original failed attempt remains preserved.

## Extracted-package and Linux replay collector restoration

```text
Title/ID: m27g-extracted-package-collector-restoration
Milestone: 27g / m27g-assembled-journeys-and-recovery; m27g-adoption-overhead-and-release-cut
Goal: Exercise the shipped artifacts with the existing installation and evidence contracts.
Inputs: Immutable a3217e25 native archives and retained failed installation attempts; matched 2796c0b6 Linux replay preparation failure.
Changes:
  - scripts/release_qualify.py — include the factory-bound public CAS key in exact provenance; use the shared installed-wheel smoke for both shipped profiles; let boot_v0 own its single ATTACH; allow capture drainage before forceful group cleanup.
  - scripts/ci/python_wheel_smoke.py and python_compat_run.sh — share the existing public API, target-neutrality and selected-authority checks without shipping development tests or importing the source package.
  - scripts/m26e_qemu_pressure.sh — preserve the original host launch record outside the not-yet-created session; move it into the exclusively created collector output afterwards.
  - focused tests and TEST_PLAN — preserve exact missing/extra/tampered payload refusal, exclusive output custody, strict intent serialization, single attachment and orderly capture shutdown.
Commands: python -m pytest -q tests/test_m26e_qemu_pressure_cli.py tests/test_release_qualify.py tests/test_python_package.py tests/test_release_bundle.py tests/test_release_inputs.py; scripts/check-generated.sh; scripts/ci/check_test_plan.sh; fresh extracted-native qualification.
Checks: 106 focused tests pass. Runtime source, archive bytes, authority, workloads and thresholds remain unchanged. Failed attempts remain retained; corrected live qualification is recorded separately.
Deliverables: Tested collector repairs and fresh artifact-bound results; no repeated operational burn-in is warranted by these collector defects.
```

Compatibility review: coh/coh-status, cohsh, Hive Gateway, SwarmUI,
host-ticket-agent, host-sidecar-bridge, gpu-bridge-host, cas-tool, sidecar-bus
and the Python SDK preserve their implementation and authority contracts.
The benchmark's pressure, population, receipt and performance criteria are
unchanged. The installed-wheel lane now uses the already-maintained smoke
against the two packaged contracts; neither its mock lifecycle nor strict
request serialization establishes target execution. Development tests remain
in the source tree under the full staged plan.

Fresh physical SD qualification additionally exposed a collector-only readiness
mismatch: production Pi serial emits `Cohesix console ready`, while the old
installation collector required the QEMU-only trace marker. The collector now
requires exactly one selected BUILD line followed by exactly one physical
console-ready line; repeated, stale, missing and reversed boot evidence fails.
The target code and its emitted bytes are unchanged.

The corrected extracted Mac attempt is retained at
`out/m27g/macos-package-a3217e25-06` in the development checkout: native tool,
replay, both installed-wheel profiles, packaged QEMU/TCP and 90 presentation
checks pass (12 existing optional snapshot skips). Its packet capture closes
cleanly. The preceding attempt is invalidated by an explicit cleanup-defect
record: the shell exited before its children and its logs were not final when
hashed. Graceful QEMU monitor exit now precedes capture drainage; failed
shutdown is a qualification failure. This is installation/presentation proof,
not native desktop workflow or pressure acceptance.

## Native Linux package header baseline

Title/ID: m27g-native-package-header-baseline
Milestone: 27g / m27g-assembled-journeys-and-recovery
Goal: Compare the packaged header against its supported native host rendering.
Inputs: Exact a3217e25 Mac/Linux archives; Linux package attempt 02; afc37390 collector.
Changes: The desktop header snapshot selects a reviewed Darwin or Linux baseline.
Commands: Native Playwright header tests on both hosts, followed by the exact Linux archive qualifier.
Checks: Existing 1% pixel tolerance, viewport and responsive checks remain unchanged.
Deliverables: Native platform baselines and retained package qualification logs.

Linux attempt 02 passed 89 UI checks and failed the shared header image by 870
pixels. The packaged font assets loaded on both hosts. Computed styles selected
Spectrum's native system font stack; measured button widths differed by about
five pixels between macOS and Linux. Visual review found the same text, controls,
height and layout without clipping. Preserve the failed comparison and its
font/geometry probes under `out/m27g/linux-package-a3217e25-02/ui-failure/`.
The Darwin baseline is unchanged byte-for-byte; Linux has its own reviewed
header image. No product code, native font choice or comparison tolerance changes.
Compatibility review: all host tools, Python SDK and benchmark workloads retain
their existing interfaces and behavior; only the package presentation test changes.

## Release installation and physical input follow-through

Title/ID: m27g-release-installation-follow-through
Milestone: 27g / m27g-assembled-journeys-and-recovery; m27g-adoption-overhead-and-release-cut
Goal: Validate delivered native archives and first-install SD behavior after the successful burn-in.
Inputs: Immutable a3217e25 artifacts; afc37390 and 156a69ce collectors; retained b832 burn and post-burn staged results.
Changes: Record completed installation, physical input and review evidence, and the remaining qualification frontier.
Commands: Native `scripts/release_qualify.py host` on Mac and Linux; `media` raw readback; `pi4` first-boot/TCP qualification; current Pi trace normalization; generated/Test Plan checks.
Checks: Archive/result/attachment digests verify; native installation and fresh exact SD boot pass; no broader release claim.
Deliverables: Immutable results under the paths below and an updated public status.

- `out/m27g/macos-package-a3217e25-06/qualification/result.json`: PASS,
  including native HVF/TCP, both packaged Python contracts and 90 UI checks.
- `out/m27g/linux-package-a3217e25-03/qualification/result.json`: PASS on
  Merlin2's native AArch64/KVM host, including both Python contracts and 90 UI
  checks. Twelve project-specific presentation skips remain explicit on each host.
  Attempt 02 retains its failed cross-platform header comparison.
- `out/m27g/sd-write-a3217e25-02/readback/result.json`: independent raw
  readback PASS before provisioning. Attempt 01 failed after macOS automatically
  mounted and modified the FAT image; a temporary device-specific mount guard
  protected the successful write/readback. It changed no global mount policy.
- `out/m27g/pi4-sd-package-a3217e25-01/qualification/result.json`: PASS for
  first-install provisioning, saved settings, exact a321 boot and packaged TCP.
- `out/m27g/pi4-sd-first-boot-a3217e25-01/physical-input-result.json`:
  149 physical keyboard bytes accepted, drained and echoed with zero drops;
  linked-runtime parser and post-diag liveness pass. The operator confirmed
  successful up/down arrows. The retained HDMI recording and stills show legible
  output. Current normalization yields five owner/descriptor/DMA proofs and zero
  invalid counter records. Missing timer-summary proof and under-load input
  coverage are not inferred from these observations.
- `out/m27g/installation-qualification-a3217e25-01/result.json` independently
  revalidates all three canonical result records and every retained attachment.
- `out/m27g/release-report-b832-02/` adds measured verification, delegated
  gateway and provider dry-run costs to the retained native/workflow report, with
  separate claim classes and source hashes. Interrupted maintainer preparation
  is not presented as a novice first-use timing study.

The first SD boot completed 1,024 raw requests with no errors: 663.179 requests/s
and 4.49725 ms p95. This is one observed GENET run, not the required multi-boot
performance or full pressure/repeatability result. Its preceding ICMP check
proved reachability, not the separate cold-neighbor ARP gate. Dedicated en8
capture succeeded; the attempted en0 capture failed on bpf3 permissions.

Lukas Bower confirmed human reviewer sign-off for the exact main-to-156a69ce
Rust diff in `out/m27g/release-review-156a69ce-01/human-review.json`. This does
not approve incomplete release gates. The successful b832 two-hour runtime and
post-burn staged results remain unchanged; collector fixes do not restart it.

Remaining blockers include strict-profile pressure setup: the legacy control
path used by population/lifecycle setup is disabled in production, while its
253 population admissions exceed the selected 64-entry intent table. This
mismatch requires explicit authority/profile/harness reconciliation; disabling
production controls, masking refusals or lowering the required population is
not a passing result. Physical Wi-Fi/repeatability, complete target acceptance,
matched native pressure and final promotion remain open. No release archive has
been promoted and Milestone 27g remains In Progress.


## Approved Release A intent capacity and pressure restoration

The user approved 512 retained outcomes and matching QEMU/Pi manifests on
22 September 2026. Discovery is M27g assembled qualification; the narrowly
reopened owner is `m27a-queen-ctl-idempotency`. Manifest schema 1.28 permits at
most 512 entries and the shared Release A materializer selects 512 on both
boards. Compatibility defaults remain 64. No identity is evicted, no consumed
approval is recycled, and no Worker population or benchmark threshold changes.
The complete pressure boot requires 291 identities, including fault injection
and expected refusals; preflight checks remaining live capacity before mutation.
The SDK projects the bound. The harness uses that selected policy and immutable
strict envelopes for direct-console and REST lifecycle operations, with no
legacy fallback or automatic fresh-identity retry after a refusal or lost ACK.

Compatibility review covers coh/coh-status, cohsh, Hive Gateway, SwarmUI,
host-ticket-agent, host-sidecar-bridge, gpu-bridge-host, cas-tool, sidecar-bus,
the Python SDK and pressure/raw benchmark scripts. The existing console ECHO
and SDK QueenIntent contracts carry the envelope unchanged; only the SDK's
capacity projection and benchmark routing need changes. Generated contracts
and exact host/target builds must be refreshed; old 1.27 artifacts are retained
under their original identities and cannot be relabelled as 1.28 qualification.

The existing physical timer backend summary is retained in the bounded trusted
boot reserve. This fixes loss of required evidence under startup log pressure;
it changes no timer source, period, elapsed-time arithmetic or driver ownership.
The prior two-hour burn remains sealed against its original 64-entry profile.
These bounded capacity/collector/diagnostic corrections do not justify another
full two-hour run; fresh affected target and post-burn full-plan evidence is
still required before milestone closure.

Strict-pressure attempt `capacity-pressure-df4063578-04` booted and answered
authenticated control but missed its service fault injection. The non-claiming
`service-race-diagnostic-03` injected and contained that fault after observing
the installed breakpoint. Service setup now uses the existing Worker runner's
explicit debugger-ready barrier instead of a one-second delay, and preserves
raw failed debugger output. Injection and teardown requirements are unchanged.
This collector correction belongs to `m27g-assembled-journeys-and-recovery`;
all runtime, host-tool, SDK and benchmark contracts remain unchanged.

Attempt `capacity-pressure-645dc47fd-05` passed all three service injections
and the critical-duty observation, then stopped before pressure mutation on a
collector `Path`/string mismatch in the new capacity preflight. Its path is now
converted at the helper boundary. The actual embedded preflight is exercised
against both available and exhausted capacity; all 50 CLI harness tests pass.
`strict-budget-diagnostic-03` independently read the live 512-entry contract,
validated the 291-intent budget, and observed a strict Heartbeat admission reach
READY. This bounded diagnostic does not replace full pressure qualification.
