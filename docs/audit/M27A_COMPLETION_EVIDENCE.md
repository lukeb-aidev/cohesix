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

## Outstanding acceptance

Required remaining evidence is the complete applicable Test Plan, final exact
host/target builds, production runtime surfaces, the equivalent 26d status
comparison, final security/audit checks and human Rust reviewer sign-off.
Production failover/federation remains disabled in the single-Jetson Release A
profile. No physical failover or future VM authority qualification is claimed.

BUILD_PLAN and STATUS may become Complete only after their definition of done
and applicable repository acceptance gates are satisfied. Commit and push are
authorized after that closure; no M27a commit or push has occurred.
