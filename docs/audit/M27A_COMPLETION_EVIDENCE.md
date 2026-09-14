<!-- Author: Lukas Bower -->
<!-- Purpose: Retain scoped implementation and qualification evidence for Milestone 27a. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# Milestone 27a implementation and qualification record

Status: **In Progress**. Owner authorization: Lukas Bower, 14 September 2026.
Base: `b848b5cbb` on `main`. This record does not inherit M27 qualification gaps
or Rust approval. No M27a target acceptance or publication is claimed yet.

Title/ID: `m27a-authority-hardening`
Milestone: [Milestone 27a — Authority Hardening: Delegated REST Identity, Fenced
Failover, Idempotent Queen Intents](../BUILD_PLAN.md#27a).
Goal: Close the single-host Release A authority floor while preserving existing
transport and accepted Worker/driver boundaries.
Inputs: canonical target manifests, manifest schema 1.21, upstream selected seL4
profiles, existing v2 execution journal, exact pre-change gateway binary at
`out/m27a/baseline-target/debug/hive-gateway` from detached `b848b5cbb`.

## Scoped task coverage

| Task | Implementation and remaining qualification |
| --- | --- |
| `m27a-rest-delegated-identity` | MAC validation, role/mount/write-scope intersection, finite budgets, issuer rotation and bounded cache pass host tests. HTTP probe passes. Target-backed projection pending. |
| `m27a-queen-ctl-idempotency` | Shared bounded reservation, stable duplicate terminal, changed-envelope refusal, explicit legacy compatibility and versioned introspection. Host tests pass; target qualification pending. |
| `m27a-failover-epoch-fencing` | Local/relay stale rejection, durable epoch floors and crash-safe cutover state tests pass. Release A excludes production failover/federation; no multi-host qualification claimed. |
| `m27a-production-secret-profile` | Shared references, pre-connect placeholder refusal, production profile generator, compiler-bound public key and exact release inventory. Compiler/release negative tests pass; assembled production image validation pending. |
| `m27a-audit-replay-production-default` | Production generator requires bounded audit/replay and evidence captures authority/dedupe. Target surface qualification pending. |
| `m27a-host-ticket-validation-replay` | Generated provider field contracts, validated operands, bounded v1/v2 journals, durable terminal replay and interrupted-execution deadletter. Crash/writeback and relay recovery tests pass. |
| `m27a-python-authority-contract-parity` | Shared generated policy/provider contracts, delegated mutation headers, strict intents, epoch/correlation and bounded responses. Python suite passes. |
| `m27a-gpu-bridge-auth-frame-caps` | Selected secret sources fail closed; generated frame length checked before allocation. Focused tests pass. |
| `m27a-console-debug-memory-gate` | Diagnostics default off and release features independently deny arbitrary memory reads. Earlier QEMU target compilation passed; current image/runtime checks pending. |
| `m27a-deferred-vm-authority-gates` | Compiler rejects unaccepted VM identity, 28b ledger/quarantine, AI actuation and production failover claims. Existing accepted task/driver authority remains distinct. |
| `m27a-gateway-authority-performance` | Current/pre-27a host-model comparison passes without retries. Accepted equivalent 26d status comparison remains unlocated. |

Optional admission correlation is retained in strict intents, host tickets,
execution journals, receipts, audit and evidence: `admission_id`, `intent_hash`,
`policy_hash`, `state_epoch`, `resource_generation`, `decision_expiry`.
No component evaluates or fabricates a Milestone 28a decision. `writer_epoch`
remains an independent writer fence.

## Compatibility review

| Surface | Same-change review |
| --- | --- |
| `hive-gateway` | Per-request caller delegation intersects one upstream session; bounded cache/quotas, typed refusals, hash-bound audit and status counters. No parallel console owners. |
| `cohsh` | TCP secret references resolve before connect; REST attach selects the current caller ticket; minting supplies finite caller claims; strict writes use the existing ECHO grammar. |
| `coh`, including FUSE and evidence | Shared REST client supplies delegation for mutations; evidence adds bounded authority/dedupe snapshots; filesystem and GPU operations retain their documented paths. |
| `coh-status` | Read-only status projection and terminal rendering unchanged; shared client validates any selected credential before HTTP. No new mutation authority. |
| SwarmUI | Placeholder TCP credentials now fail before connection; REST operations inherit shared caller delegation and bounded responses. Existing replay mode remains explicit. |
| `gpu-bridge-host` | Live TCP/REST credential preflight and generated console-frame bound; provider/registry publication and unavailable-state semantics preserved. |
| `host-sidecar-bridge` | Existing TCP transport receives shared pre-connect refusal; REST transport selects `COH_REST_TICKET`. Provider scheduling and publication paths need no change. Tool-specific token aliases remain documented. |
| `host-ticket-agent` | Provider allowlists and operands checked before executor dispatch; durable execution recovery, epoch floor, bounded journal reads, result/correlation binding and whole-request relay deadline. |
| `cas-tool` | Explicit bounded signing-key references, no fallback, file rotation; release bundles retain only the public key emitted with the selected root manifest. CAS protocol unchanged. |
| `sidecar-bus` and provider libraries | Use the existing transport/provider contracts; no independent network auth path or new host authority introduced. |
| `tools/cohesix-py` | Generated authority and provider fields, strict intent/epoch validation, delegated REST mutation preflight, response bounds, reference rotation and evidence parity. |
| Benchmarks and regression runners | Mutation workloads pass delegated tickets; selected manifests resolve exact private credentials; no bootstrap fallback. Authority probe preserves every sample/refusal and does not supply target throughput proof. |
| Build, release and due diligence | Selected manifest drives Python projection; generated snapshots include compiler-bound public key; release policy and payload scan reject fixtures/private keys; historical release directories remain immutable. |

The public migration contract is [M27A_AUTHORITY.md](../M27A_AUTHORITY.md).

## Retained command evidence

All paths below are local retained evidence, not portable release attestations.
Earlier failed attempts remain in place and are not relabeled.

| Command / result | Evidence |
| --- | --- |
| `scripts/check-generated.sh --update-pi4-test-profile` PASS | `out/m27a/check-generated-4.log`; current Pi generated fixtures refreshed by the compiler. |
| `scripts/ci/check_test_plan.sh` PASS | `out/m27a/check-test-plan-2.log`; 47 catalog actions. |
| `cargo test -p rust-risk-audit` PASS, 28 tests | `out/m27a/risk-tests-16.log`. |
| `cargo run -p rust-risk-audit -- --baseline docs/audit/rust_risk_baseline.toml` PASS | `out/m27a/risk-ratchet-16.log`; counts below. |
| `cargo test -p host-ticket-agent -p cohesix-rest -p hive-gateway -p coh-rtc` packages PASS in combined run | `out/m27a/authority-tests-15.log`; that run's additional risk-audit package failed on the stale current build fingerprint, subsequently repaired and retested. |
| Python auth/SDK/benchmark/failover/release tests PASS, 421 tests | `out/m27a/python-15.log`; command uses `PYTHONPATH=tools/cohesix-py out/m27a/venv/bin/python -m pytest`. |
| Release and regression-runner tests PASS, 40 tests | `out/m27a/release-runner-12.log`. |
| `cargo test -p gpu-bridge-host --features rest` PASS | `out/m27a/gpu-tests-17.log`; includes selected-source refusal and pre-allocation frame bounds. |
| Gateway current/pre-27a probe PASS | `out/bench/m27a-gateway-authority-04/report.json`, raw samples and gateway audit logs beside it. |
| `cargo audit` and `cargo deny check advisories` PASS under existing advisory policy | `out/m27a/cargo-audit-1.log`, `out/m27a/cargo-deny-1.log`; later lockfile changes require final refresh. |
| Full workspace run 13 failed only the stale build fingerprint | `out/m27a/workspace-tests-13.log`; final workspace run remains required. |
| Stage 01 attempt 01 stopped at an unused relay test import in Clippy | `out/m27a/stage1-01.log`; import removed, final staged refresh pending. |

### Current build-script audit contract

The selected-manifest preflight added to `apps/root-task/build.rs` resolves each
configured secret reference before compilation and registers its env/file source
through the existing validated Cargo directive emitter. The six existing
OUT_DIR generators and their include contracts are unchanged. The reviewed
current build-script SHA-256 is
`8b70f0ab92bbc18c4e63b25aedabd6527ab448d056f1eacd7fded42125463b14`.
The scanner constants and current baseline bindings are updated together;
historical fingerprints and every risk ceiling remain unchanged. This records
implementation review, not human Rust approval.

Measured `unsafe / unwrap / expect / panic` counts are global
`823 / 38 / 242 / 100`, linked HAL `173 / 0 / 2 / 0`, and outside HAL
`650 / 38 / 240 / 100`. Each is within its existing independent ceiling.

### Gateway comparison

The 32-iteration host-model run has no unexpected HTTP/protocol responses,
client retries or backpressure. Current status p50/p95 is 0.433/0.623 ms;
strict delegated writes 1.288/2.612 ms; exact duplicate acknowledgements
2.115/2.155 ms. Conflicting identities, non-current future epochs (2 against 1), and missing
delegation each produce exactly 32 expected refusals. This run does not prove
refusal of an older writer epoch. Against the same-host pre-27a binary,
status p95 increases 0.182 ms and write p95 increases 1.015 ms. This is a small
absolute host-model latency cost; it does not establish equivalent target
performance or dismiss the relative status increase. The accepted 26d
status-read comparator remains required and has been requested from the owner.

### Direct production QEMU observation

`out/m27a/qemu-production-5/authority/result.json` passes the direct production
profile diagnostic: compiled epoch 7, a documented policy approval, one fresh
strict write, exact duplicate ACK, identity conflict, stale epoch 6 refusal,
legacy path refusal and readable full audit/replay records. It uses the pinned
QEMU 10.1.0 HVF binary and the canonical four-core 24 MHz seL4 build. The packaged
CPIO is 152,064 bytes. Build log and exact retained image bindings are beside the
result. This is convergence evidence, not staged acceptance. The subsequent
console-shortcut and durable dedupe-label changes require a fresh image.

The first compatibility authority diagnostic refused the missing policy
approval. The prior TCP response-matrix failure was a client sending a literal
secret reference; the repaired probe resolves it before connecting. The shared
Pi reboot and raw TCP credential preflights now follow the same contract;
`out/m27a/probe-auth-20b.log` records 381 passing focused tests.

Host and root audit checks now retain `fresh`/`duplicate` with the complete
strict envelope, and preflight journal capacity before reservation or effects.
`out/m27a/ninedoor-audit-22.log` and `out/m27a/root-intent-22.log` pass. Rust/Python
C1 reassembly accepts bounded 8192-byte logical records while preserving the
256-byte frame and 64-chunk ceilings; focused validation is retained in
`rust-chunks-19.log`, `python-chunks-19.log`, and `root-chunk-21.log` under
`out/m27a/`.

`out/bench/m27a-gateway-authority-05/report.json` verifies actual stale epoch 6
refusal against a compiled epoch-7 gateway, plus all expected positive/negative
HTTP outcomes. Its release-versus-debug baseline and overlapping root compile
make its comparative latency unsuitable for final write-overhead qualification.
A matching optimized baseline and an idle-host rerun remain required.

### Source-freeze qualification boundary

`out/m27a/check-generated-23.log` and `check-test-plan-23.log` pass. The second
Stage 01 attempt passes generated consistency, formatting, Clippy, workspace
check and the catalogued workspace tests. It was stopped before completion
because concurrent main documentation commits changed its Git source identity;
`out/m27a/stage1-02-cancellation.txt` records the reason. No stage marker from
that attempt is accepted. Remaining qualification uses a fixed isolated source.

### Frozen candidate and current qualification

Implementation candidate `275922fde5c607de38f767e8c19115093c27e9a1` was frozen
without moving `main`. Candidate `72bffffd1b35dffaf65d1ae1525a8ac7112a1ec5`
adds only the missing public verification keys to the release-qualification
fixture; production implementation bytes are identical. The original archive
SHA-256 is `5ca00473f80a89e7efd32f0a387a7d3e80d43923a77b8a85b7e06cf728491af2`.
`out/m27a/qualification-candidate*.json` retains both source records.

The native Linux AArch64 release build and 449 host tests pass on Merlin2 for
the implementation candidate. The canonical Python SDK gate passes 139 tests
with one skip on Python 3.12.3. Logs, toolchain versions and binary hashes are in
`out/m27a/merlin2-275922fde5c6/logs/`. This proves native host contracts, not GPU
provider execution or target behavior. An earlier manually selected Python
command named a nonexistent test file and collected no tests; the subsequent
canonical SDK command supplies the reported evidence.

The corrected Mac Stage 01 attempt passes formatting, Clippy, workspace check,
the catalogued Rust suites, 2,559 Python tests and example smokes. It stops at
the Rust bootstrap guard because a checkout nested under the main repository
inherits that parent's Cargo configuration. The guard correctly refuses this
external configuration. Both candidate checkouts were moved to sibling
directories outside the parent repository; all seven bootstrap tests then pass
in `out/m27a/candidate-risk-bootstrap-41.log`. Full staged qualification is
restarted there. No failed attempt supplies a stage marker. Earlier absent
local Python environments and externally resolving compiler caches were also
repaired without weakening their checks.

The compatibility Pi image built from the clean corrected candidate is
`sha256:35512e394a9266b9c0df28ff129d1679d15bf3b7f3fc449ed9650848f5ef623d`.
`out/m27a/pi4-boot-36/` retains its fresh RAM-load/reset CRC checks, exact BUILD
marker, settled serial log and first raw TCP sample: 64/64 requests, one
connection, zero application retries or reconnects. The same boot passes strict
intent first/duplicate acknowledgement, identity-conflict refusal and full
`fresh`/`duplicate` audit labels. Its compiled writer epoch is 1; the diagnostic
named `stale-epoch` actually tests future epoch 2, so it is only a non-current
epoch refusal. Actual older-epoch and disabled legacy-path checks require the
separate production epoch-7 run. The first raw CLI invocation rejected a missing
ticket before connection; the retained successful invocation supplies a minted
ticket and is the first target TCP connection. These observations remain
non-claiming diagnostics until the applicable staged target checks complete.

The production Pi image also builds with clean source binding:
`sha256:e863b609e4c6ea8b462f69d3155f652669ae8a51ee10187e63198849f7e609a2`.
Build evidence is `out/m27a/pi4-production-build-40.log`; its current hardware
run is separate from the compatibility boot. `out/m27a/pi4-production-42/`
now records a fresh exact BUILD, CRC checks before and after reset, a settled
GENET lifetime and a first-connection 64/64 raw sample without retries or
reconnects. The production authority diagnostic passes epoch 7, true stale
epoch 6 refusal, stable duplicate acknowledgement with full dedupe audit labels,
legacy-path refusal and explicit `SPAWN`/`KILL` refusal. A subsequent bounded
check refuses both upper- and lowercase memory-dump commands on the production
operator surface; it does not exercise the emergency fallback or a kernel fault.
Paired continuous captures are retained under `out/m27a/pi4-capture-36/` and
must be closed and sealed before final capture claims.

Relocation also exposed cached host binaries with their old compile-time paths;
the owned build caches were cleared before the clean staged rerun. Copied QEMU
kernel trees fail the path-bound profile contract and remain unqualified. A
fresh canonical toolchain setup and 303-step seL4 build in the sibling Pi
checkout passes `qemu_smp_production` validation in
`out/m27a/pi-worktree-qemu-profile-{configure,build,validate}-48.log`.
The QEMU acceptance checkout needs its own corresponding profile build.

The clean sibling Mac Stage 01 now passes all 22 common actions in
`out/m27a/candidate-stage1-43.log`, including the Rust bootstrap and risk ratchet.
The immutable common attestation is retained in
`/Users/lukasbower/GitHub/cohesix-m27a-qualification/out/test-plan/m27a-qemu-43/`.
The Pi checkout imports that exact verified common evidence in
`out/m27a/pi4-stage1-import-50.log`; target-bound stages remain separate.

The refreshed production QEMU diagnostic also passes for candidate
`72bffffd1b35dffaf65d1ae1525a8ac7112a1ec5` in the sibling Pi checkout at
`out/m27a/qemu-production-49/authority/result.json`. Compiled epoch 7 admits one
fresh strict intent and the exact duplicate, retains one dedupe entry and one
duplicate, refuses actual stale epoch 6, conflicting identity, the legacy path,
SPAWN/KILL shortcuts and the normal HEXDUMP console command, and exposes full
fresh/duplicate audit records plus replay status. The canonical pinned HVF
QEMU build, source identity, selected manifest and runtime logs are retained
beside it. This is target convergence evidence, not staged acceptance or a
dynamic kernel fault/wake test.

The QEMU sibling's own toolchain setup and fresh seL4 profile build now pass
in `out/m27a/qemu-worktree-toolchain-50.log` and
`qemu-profile-{configure,build,validate}-52.log`. Both target Stage 02 actions
pass initially (`pi4-stage2-51.log`, `qemu-stage2-55.log`). The first attempted
Pi Stage 03 refuses changed environment selectors before a hardware boot;
`pi4-stage3-56.log` preserves that failure. Stage 02 is rerun with the complete
stable target settings before either target progresses to Stage 03. The
immutable source identity and acceptance checks remain unchanged.

`out/m27a/production-payload-scan-54.log` passes selected production policy and
private-fixture, canary and symlink scans for both built target artifact sets.
This is an artifact payload check, not a full release assembly claim.

The stable-settings Stage 02 refresh passes on QEMU and Pi in
`out/m27a/{qemu,pi4}-stage2-57.log`. QEMU Stage 03 then starts normally.
Pi Stage 03 attempt 58 records a fresh exact compatibility boot and a successful
first raw sample, then refuses the production-built client's mismatch with the
restored compatibility policy before regression operations reach the target.
The matching compatibility clients are built and the policy check passes in
`pi4-compat-clients-62.log` and `pi4-client-policy-62.log`. The earlier temporary
production `coh` client installation is superseded by this ordinary Cargo build;
its retained production artifact remains unchanged. Pi qualification restarts
with those clients and a newly bound Stage 02; failed results are preserved.

Merlin2's additional production-profile build first refuses the archive's
missing Git tracked-file inventory. The exact candidate Git objects are
transferred with pack SHA-256
`00c0ad7a951dda722399ffa2e9603efef2a90c39d0566ba01958e657c55f9d3e`.
Two vendor metadata files omitted by Git archive's export-ignore rules are
restored from those immutable objects. The earlier host build's generated Linux
UI schema is retained under ignored evidence. The resulting checkout is clean
at candidate `275922fde5c607de38f767e8c19115093c27e9a1`, tree
`37f84e6de0503cacda6aeafc3a2381ad29d987fe`; native production build 61 remains
pending. This does not change or relabel the original native compatibility tests.

Native production host compilation completes for all nine selected packages
in `out/m27a/merlin2-production-61/host-build.log`. Artifact collection initially
mistakes the `coh-status` library package for an executable; the eight actual
executables and `libcoh_status.rlib` are subsequently retained without a rebuild.
The correction does not relabel the failed collection process. The native
production gateway probe passes all 32 positive and negative cases per scenario
in `gateway-probe-65/report.json`, including actual stale epoch refusal, with no
operation retries or backpressure. The original native compatibility logs remain
separate; production evidence is not a physical GPU/provider execution claim.

The matching optimized Mac comparison in
`out/bench/m27a-gateway-authority-70/report.json` passes on an idle build host.
The production epoch-7 gateway has status p50/p95 0.567/0.698 ms, delegated-write
2.020/2.510 ms and duplicate-ACK 1.888/2.088 ms. All 32 conflicts, stale-epoch
writes and missing-ticket writes are refused as expected; no unexpected error,
client retry, reconnect or backpressure occurs. The queue high-water mark is 1;
one ticket cache entry records 127 hits and one miss. The 160 authority audit
records cost 4.636 ms in total. Against the matching optimized pre-27a binary,
status p95 increases 0.273 ms (about 64%) and write p95 1.118 ms (about 80%).
These are measured host-model latency regressions with bounded absolute costs;
there is no target-throughput or improvement claim. The equivalent accepted 26d
status comparator is still missing and remains required.

QEMU Stage 03 attempt 58 passes the complete base group and fixed response
matrix, then fails the telemetry fixture: its published-key MAC is correctly
refused by the provisioned issuer before the expected expiry check. The
`m27a-production-secret-profile` compatibility closure therefore adds
`coh-rtc-regression-tickets` and runner integration. It verifies the original
fixture MAC, replaces only the MAC with the selected issuer's signature, and
asserts exact claim-byte preservation. Original scripts and expected outcomes
remain unchanged; transport records retain the private materialized copies and
hash bindings. Five independent fixture/bound tests, 21 wrapper tests and
compiler Clippy pass in `out/m27a/regression-ticket-{tests,clippy}-67.log` and
`regression-ticket-runner-tests-66.log`. Regeneration and Test Plan integrity pass
in `check-generated-69.log` and `check-test-plan-69.log`.

Pi Stage 02 attempt 63 passes with matching compatibility clients. The queued
Stage 03 attempt 64 is interrupted after its fresh exact-image boot and first
raw sample while the fixture mismatch is corrected; its incomplete record is
not accepted. This is distinct from the earlier policy-file refusal.
All target results retain their original source identity. The fixture workflow
change requires a new frozen candidate and refreshed staged qualification.

Candidate `cargo audit` and
`cargo deny check advisories` pass under the existing advisory policy in
`out/m27a/candidate-cargo-{audit,deny}-38.log`.

The DD30 preflight still refuses the changed protected `kernel.rs` fingerprint.
The existing approval is scoped to Milestone 27 and does not approve 27a's
secret-registration and debug-memory changes. Fresh human Rust review and an
explicit applicable DD30 disposition remain required before merge. The dynamic
fault/wake test remains unexecuted; no static check or policy refusal replaces it.

## Outstanding acceptance

Required remaining evidence is the complete applicable Test Plan, final exact
host/target builds, production runtime surfaces, the equivalent 26d status
comparison, final security/audit checks and human Rust reviewer sign-off.
Production failover/federation remains disabled in the single-Jetson Release A
profile. No physical failover or future VM authority qualification is claimed.

BUILD_PLAN and STATUS may become Complete only after their definition of done
and applicable repository acceptance gates are satisfied. Commit and push are
authorized after that closure; no M27a change has been committed to main or pushed.
