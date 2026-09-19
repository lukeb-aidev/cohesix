<!-- Author: Lukas Bower -->
<!-- Purpose: Bind recoverable CUDA recipe closure to focused host contracts, live native evidence and explicit proof limits. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Recoverable CUDA recipe implementation record

**Status: Complete — 19 September 2026.**

Scope: Milestone 27c, `m27c-recipe-lifecycle-and-identity` and
`m27c-stage-reuse-accounting-and-diagnostics`. The owner's 19 September 2026
instruction activates the milestone, requests minimum completion testing and
authorizes publication to main. Base source is
`3452248de6b8faa2598a3629910e3c41d2c6178c`; the concurrent documentation commit
`cdcb9916ebc87f2cf0f4fc0aa105bdb1223e8925` is preserved as the publication parent.
No whole-workspace suite or staged
release campaign was run. Qualification is component-scoped under TEST_PLAN;
27g retains integrated release acceptance.

## Implemented contracts

The compiler emits the independent `cohesix-cuda-recipe-contract/v1` host contract.
`coh plan/apply/watch/explain/verify/recover cuda-reference --recipe` and the
Python wrapper share one implementation. The checked Python example constructs
bounded deployments from existing enrolled stage files; it cannot execute a
native command or manufacture a receipt. Public usage and recovery are integrated
into HOST_TOOLS, GPU_NODES, INTERFACES, SECURITY and the shared operator manual.

The private, exclusively locked journal commits logical operation/configuration,
stage/attempt identity, exact original ticket/idempotency and reservations before
submission. Unknown submission/termination remains ambiguous. Reconciliation
uses the existing host-ticket/native journals and independently enrolled evidence;
there is no new queue, native executor, scheduler, VM verb or receipt vocabulary.
A separately admitted cancellation confirms release only after native termination.
Recovery grants may change independently of workload configuration.

Reuse keys bind code/helper, deterministic data/parameters, output expectation,
runtime/device/topology and predecessor keys/output hashes. Cached labels are
recomputed and actual signed output bytes are rehashed. Live reuse requires
scoped visibility and current compatibility; historical verification refreshes
no grant. Changed inputs invalidate descendants while independent verified work
survives. Requested, measured, unknown and released resources remain distinct and
cumulative. Bounds are eight stages, 32 cumulative attempts/revisions, one active
workload, zero automatic retries, 256 KiB journal/output bounds, 8 MiB retained
output and one-hour reuse across revisions. Native child termination supplies
resource cleanup; retained artifacts are bounded, not automatically deleted.

## Focused contract evidence

All paths below are retained under `out/m27c/` unless otherwise stated.

| Required contract | Command/evidence | Result |
| --- | --- | --- |
| Durable intent, lost ACK/restart, current authority/resource/visibility refusals, cancellation, corrupted output/evidence, journal validation, DAG/reuse age and descendant invalidation | `cargo test --locked -p coh --test recipe --test workflow --test evidence_case`; `focused-tests-05.log` | 8 recipe, 1 existing workflow and 2 canonical case tests PASS. |
| Generated finite bounds reject parallelism/retry/retention widening | `cargo test --locked -p coh-rtc --lib recipe::tests`; `compiler-tests-01.log` | PASS. |
| Python routes the same deployment/credentials and rejects invalid cancellation/recipe selection | `PYTHONPATH=tools/cohesix-py .venv/bin/python -m pytest -q tools/cohesix-py/tests/test_playbooks.py -k 'workflow_credentials or recipe_lifecycle'`; `python-tests-02.log` | 2 focused tests PASS. |
| Rust compilation and warnings for changed implementations | `cargo clippy --locked -p coh -p coh-rtc --all-targets -- -D warnings`; `clippy-02.log`; final `cargo clippy --locked -p coh --all-targets -- -D warnings`, `clippy-03.log` | PASS. |
| Formatting | `cargo fmt --all -- --check`; `format-03.log` | PASS. |
| macOS ARM64 executable/manual consumers | `cargo build --locked --release -p coh -p cohsh -p swarmui`; final catalog build `build-mac-04.log` | PASS. |
| Linux AArch64 executable/manual consumers | Native `cargo build --locked --release -p coh -p cohsh -p swarmui`; final catalog build `build-linux-05.log` | PASS. Exact source archive and supplemental inventories in `native-source-01.json`, `native-support-01.json`, `native-resources-01.json`, `native-final-delta.json`, `native-catalog-final.json`. |
| Generated/source agreement | `scripts/check-generated.sh`; `generated-check-03.log` | PASS, including inventory, host graph, Test Plan and NIST consistency. |
| Test Plan alignment | `scripts/ci/check_test_plan.sh`; `test-plan-03.log` | PASS. |

The selective-reuse contract performs three initial independent/dependent stages,
changes A, and executes only A and its descendant C again. B is reused: five
submissions across both revisions, versus six if all three were repeated. The
cumulative requested/released total remains 5 MiB. This is deterministic host
contract evidence, not a native speedup or throughput benchmark. Native timing,
raw TCP/REST reports, thresholds and target performance acceptance are unchanged.

## Fresh CUDA recipe and recovery

`live-session-03/recipe-01/` retains one real checked `vadd` recipe with 64 values,
one iteration, 1 MiB requested allocation budget and a 30-second child deadline.
The fresh host observation `native-versions-01.log` identifies Linux AArch64,
kernel `6.8.12-1021-tegra`, CUDA toolkit `13.2.2-1`, nvcc `13.2.86`, JetPack
`7.2.1-b49` and NVIDIA container toolkit `1.19.1-1`. Jetson remains the maintained
reference for the portable Linux AArch64 CUDA contract.

The selected unchanged QEMU image is
`out/m27b/qemu-worker-evidence-12/artifact`, target resolved-manifest digest
`d502977c8b61c5ecb273d5bbcaced92e1b4b631d60e47641e98bb950d0d20de9`,
with its original 27b provider graph
`bd7b9438c83deb657b1aa092759677800cf7d48b7807541691a2f679be4dcaa1`.
This host-only change does not relabel that image as a newly built target.
The new catalog rows are a diagnostic Python example and this evidence record;
regenerated catalog digests are separate from the unchanged selected runtime contract. Target no_std
provider-field constants are unchanged; only the host std registry projection
changes. No new physical Pi evidence or target compilation is required by this
host-only delta.

The admitted host-ticket agent and GPU bridge retain the accepted 27b binary hashes:

- Agent: `56afe6098f9931305c5525cad891177157413211c40b06dd12e3151ce7871285`.
- Bridge: `08371b77d75c34e7c99ec8277ca53e8878848e2c5a47fb249d60eaf7fbf15fb5`.
- Recipe contract: `64d196005839fcf149d5a938e1a7d15d7ef0bf5e6a9f32896c8460e4b80a0ddd`.
- Final macOS `coh`: `4b5b0b8fc6b023d16e13eef2f0c95b7c5add486355c84a44050846eb2a78a721`.

A disposable loopback proxy forwards exactly one submission then drops its ACK.
The durable attempt retains `acknowledged=false` and a 1 MiB reservation. A new
controller invocation reconciles the original ticket and verified native/Worker
graph; it does not enqueue a second CUDA action. Output verification compares the
real 256-byte CAS object with the independently checked signed output digest.
Requested reservation is 1,048,576 bytes, measured allocation is 768 bytes,
confirmed released capacity is 1,048,576 bytes, unresolved capacity is zero and
retained output is 256 bytes. Original idempotency remains `systemd-vadd-once`.

Native identity is
`gpu-job:m27c-systemd-vadd:2c5065d627de9cbe11fd375ee2e4aa1133404240747f1c6c685c5571d826624f`.
The independently observed systemd invocation is
`78ddfce964df4ff780aceb4bd5b05620`, unit
`cohesix-recipe-m27c-systemd-03.service`. Signed native controls retain 1 GiB
MemoryMax, 200000/100000 microsecond CPU quota, 64 tasks, NoNewPrivs, one CUDA
context and owned-child kill/reap. The stopped unit is independently observed
`inactive/dead`, result `success`; signing keys and owned publisher/credentials
were removed. CUDA allocation admission is not a hard hardware memory partition.

The causal graph digest is
`864deb453deb8b64172e97daf7ca1d11d1908d85e3f335c6d79d431d60ae85a4`.
Agent retry leaves the graph unchanged; forged/missing Worker records are refused.
`recipe-completed.json` SHA-256 is
`1249d8dd22a233419c15755cc467d84ad98f2ea70a921c009cc9954aa53a4121`.
Final CLI `verify` and `explain` recheck that same operation in
`final-recipe-verify.json` and `final-recipe-explain.json`. The later CLI-only
partial-case attachment option changes no recipe or verifier implementation.
After indexing this record, the final catalog build verifies the same operation
in `final-catalog-recipe-verify.json` and reproduces the same diagnostic case
digest in `final-catalog-diagnostic.log`.

The selected 27b executor/native/container wrong-device, stale inventory,
oversize, deadline, revoke/cancel, safe OOM and bridge/agent recovery evidence is
reused at its original component identities, as linked by
[M27b's closure record](M27B_IMPLEMENTATION_RECORD.md#selected-executable-foundation-closure-19-september-2026).
Fresh recipe-level fixtures exercise those composition boundaries without
repeating unchanged provider qualification. Admission remains operator-approved;
formal machine-checked admission, Queen reboot persistence and production Worker
bundle binding are not claimed.

## Canonical diagnostic and retained failures

The real lost-ACK report has SHA-256
`42ea2faad2a2dd77d7130ab2495feb8440383a7cfcab36b955340d8cc6b5aeda`.
Its canonical diagnostic is `live-session-03/recipe-01/diagnostic-case/case.json`,
SHA-256 `44cb2cc8b07461f079247f253fc333de49089063bfb8d06ff35016d67e3142cf`.
`diagnostic-case-01.log` records successful export with:

```sh
target/release/coh evidence timeline \
  --input out/m27c/live-session-03/recipe-01/diagnostic-case \
  --recipe-report out/m27c/live-session-03/recipe-01/ambiguous-report.json \
  --scenario incident
```

The pack is explicitly **partial**: 40 paths captured, one missing and one failed
lease-detail read (`/proc/lease/by-id/m27c-systemd-lease`). Its original capture
returned nonzero and the harness summary remains FAIL for that capture error.
The successful recipe and signed output are distinct from pack completeness.
The canonical case retains the capture error, the earlier ambiguous submission,
its exact stage/ticket, remaining execution uncertainty and separately authorized
recovery. It stays `proof=none`; resealing does not promote missing evidence.
The milestone requires a diagnostic case, not a complete release evidence pack.

Earlier attempts are also preserved. `live-session-01` completed CUDA and
recovered from a harness command-variable error in a new controller invocation.
`live-session-02` refused stale inventory before submission (zero attempts);
loopback HTTP-server hostname lookup delayed setup after snapshot capture.
The passing lane binds its local proxy without that unnecessary lookup and
captures inventory 588 ms before apply; no freshness bound was changed.
Native build attempts 01/02 exposed incomplete temporary source transfer, fixed
by copying the existing workspace tests and embedded resources. No production
source, test predicate or native prerequisite was weakened.
The second generated check detected that this newly added evidence record had
not yet been indexed. Regeneration added its documentation row; the third check
and both final host builds use that complete catalog.

## Compatibility review

| Surface | Disposition |
| --- | --- |
| `coh` | New opt-in recipe lifecycle and canonical diagnostic attachment; existing workflow, GPU and local `run` behavior preserved. |
| `cohsh`, SwarmUI | Shared manual explains external-host command ownership; native builds include it. No root console/SwarmUI dispatch or graphical recipe workbench is added. |
| `hive-gateway`, REST/OpenAPI, `host-ticket-agent`, `gpu-bridge-host` | Existing current scoped reads, admitted v2 actions, native WAL and signed results are reused. No endpoint, wire schema, action or execution authority changes. |
| `host-sidecar-bridge` | Existing systemd observation verifies native identity/cleanup; no adapter change. |
| `cas-tool`, `coh-status`, signed evidence core | Existing immutable CAS and graph verification; optional canonical case projection preserves legacy byte fixtures. No new receipt or status authority. |
| `tools/cohesix-py` | Same Rust lifecycle/verifier, explicit credential references, checked source example. Existing package inventory remains unchanged; the new example is source-distributed diagnostic content. |
| Performance scripts | No workload, report schema, threshold, retries or transport gate changes. Recipe reuse/accounting is separately classified above. |

The source-authoritative metadata, generated inventories, implementation and
public references are changed together. Historical failed runs and target/source
identities remain intact. The full workspace suite, cargo audit/deny and staged
release gates were not run under the owner's explicit minimum-testing instruction.
