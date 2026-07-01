<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define Cohesix benchmark methodology, evidence lanes, report schema, and rolling performance interpretation. -->
<!-- Author: Lukas Bower -->
# Cohesix Benchmarking

This document defines how Cohesix performance evidence is produced, read, and
acted on. It is written for engineers who need to run benchmarks, compare
results across milestones, and decide whether a result exposes a defect or a
justified cost of additional system complexity.

Benchmarking Cohesix is not a single throughput race. Cohesix is a bounded
control-plane OS with append-only authority surfaces, Secure9P semantics,
role-scoped tickets, explicit backpressure, isolated driver runtimes, and
operator-liveness requirements. A useful benchmark must therefore preserve the
system contract while measuring where time, errors, queueing, and capacity are
spent.

## Active Scope

The current benchmark lane is Milestone 26d,
`m26d-benchmark-revalidation-and-tuning`: revalidate and, where safe, recover
the accepted 26b REST/driver-runtime benchmark envelope on the seL4 15
baseline. Harness changes made in this lane may improve provenance, strictness,
classification, and reportability. They must not relax the workload contract,
hide errors, bypass Secure9P, or turn benchmark-only shortcuts into production
semantics.

Post-26d work uses the accepted 26d evidence as the rolling comparison point.
Later milestones decide whether they need a full same-harness benchmark,
targeted microbenchmark, or no runtime benchmark according to
`docs/BUILD_PLAN.md`.

## Benchmarking Philosophy

### Preserve Semantics

Benchmarks must run through the same authority path as normal users:
authenticated `cohsh`/TCP, `hive-gateway`, Secure9P files, append-only control
nodes, read-only `/proc` nodes, and manifest-defined bounds. A faster result
that drops audit lines, coalesces append-only writes without a documented
contract, changes ACK/ERR/END behavior, or masks backpressure is invalid.

### Compare Like With Like

Every performance claim must state the workload, target, transport, seL4
baseline, manifest, gateway settings, worker envelope, request mix, retry
policy, error budget, and artifact path. QEMU, Pi 4 GENET, Pi 4 Wi-Fi, direct
TCP, REST, UI, storage, and host-tool microbenchmarks are separate proof lanes.
They can explain each other; they cannot substitute for each other.

### Measure Reliability And Latency Together

Throughput without error rate is not a pass. Error rate without latency hides
operator pain. Low latency with uncontrolled refusal can be correct under
overload, but only when the refusal is deterministic and accounted for.
Benchmark reports must include:

- successful and failed operations;
- overall and per-operation error rate;
- average, p50, p90, p95, p99, min, and max latency;
- target and observed throughput;
- configured and observed concurrency;
- gateway, VM, and target backpressure counters when available;
- top failing operations and top latency contributors.

### Treat Complexity Honestly

The old 1500-worker QEMU benchmark was taken when Cohesix had a simpler
surface. Since then, Cohesix has gained more namespaces, policy gates,
telemetry/lease/schedule controls, root-task audit behavior, host projections,
driver-runtime evidence, and seL4 15 compatibility work. Higher latency or a
lower worker ceiling can be acceptable when the added work is real and bounded.
It is not acceptable when the cause is stale sessions, avoidable gateway
blocking, broken cache behavior, unbounded retries, or an unintended queue.

### Action The Moved Layer

A benchmark result is useful only when it identifies the layer that moved:
client load generation, gateway queueing, gateway session pool, Secure9P
transport, root-task authority, VM control rings, driver runtime, physical
network, target timer, or UI/rendering. Fix defects in that layer. Do not tune
another layer to hide the symptom.

## Evidence Lanes

| Lane | Proves | Does not prove |
| --- | --- | --- |
| QEMU REST | Semantic capacity, gateway behavior, VM control-plane limits, same-harness regression stability. | Pi 4 physical-network latency, GENET/CYW43 driver health, HDMI/USB operator liveness. |
| QEMU direct TCP or `cohsh` | Console transport health, ACK/ERR/END stability, baseline command latency without REST overhead. | REST gateway overhead or browser/UI behavior. |
| Pi 4 GENET REST | Production Pi 4 throughput and latency for the wired transport, when paired with fresh serial/runtime proof. | Wi-Fi production capacity or QEMU-loopback parity. |
| Pi 4 Wi-Fi REST | CYW43/SDIO research envelope, field diagnostics, physical-link failure modes. | Production high-concurrency capacity unless a site-specific Wi-Fi envelope is freshly accepted. |
| Pi 4 serial/USB/HDMI proof | Operator liveness and local-seat behavior under load. | REST throughput by itself. |
| Driver-runtime counters | Where target service turns spend time and where bounded backpressure occurs. | Fresh boot proof or user-visible throughput without a same-harness run. |
| Gateway microbenchmarks | REST projection overhead, auth/policy additions, queue behavior, protocol adapters. | Full target throughput unless they expose a runtime-path regression that triggers a full rerun. |
| Host-tool/UI microbenchmarks | Field-tool latency, report rendering, replay or visualization cost. | VM or hardware transport capacity. |

## System Areas Tested

Benchmarks should be selected according to the changed surface:

- **Gateway and REST projection:** request admission, broker queues, session
  pool, cache behavior, control-write retry policy, HTTP status mapping, auth
  overhead.
- **Secure9P and console transport:** frame bounds, tag/window behavior,
  fid lifecycle churn, ACK/ERR/END grammar, direct TCP liveness.
- **Root-task authority:** append-only control processing, policy gates,
  schedule/lease/export control files, `/proc` snapshots, audit logging.
- **Worker scale:** spawn/kill/list behavior, worker telemetry reads, worker
  namespace listing, worker-cap refusal.
- **Telemetry ingest:** segment creation, append bandwidth, tail/readback,
  quota refusals, cursor behavior.
- **Lease and schedule controls:** grant/preempt/quota/schedule pressure,
  buffer-full accounting, deterministic refusal shape.
- **Pi 4 runtime and networking:** GENET/CYW43/SDIO/USB/HDMI/serial service
  turns, DMA/runtime proof, timer/counter validity, DHCP/static-IP evidence.
- **Operator liveness:** serial first, local-seat USB keyboard second, HDMI
  feedback third, authenticated TCP shell priority when active.
- **Future host/UI surfaces:** gateway authority, MCP/A2A projections,
  Live Hive render cadence, evidence-pack export, field-tool read latency.

## Canonical Scripts

| Script | Use |
| --- | --- |
| `scripts/rest_perf_harness.py` | Primary REST and direct performance harness. Use `--mode simulate` for mixed REST load and `--mode perf` for sequential-vs-parallel status/telemetry latency. |
| `scripts/pi4_compare_driver_models.py` | Compare matched QEMU and Pi artifacts and reject workload/provenance mismatches. |
| `scripts/pi4_trace_normalize.py` | Normalize Pi serial evidence and extract driver/runtime/counter proof fields. |
| `scripts/pi4_gate_proof.sh` | Produce target-qualified Pi 4 proof bundles after fresh serial evidence passes normalization. |
| `scripts/ci/test_plan_run.sh` | Target-qualified staged regression runner; use it when benchmark evidence must be tied to a full test-plan lane. |
| `scripts/check-generated.sh` | Guard generated artifacts after manifest or generated-output changes. |

## REST Harness Modes

### `--mode simulate`

`simulate` drives a mixed REST workload against `hive-gateway`. It can launch a
local QEMU plus gateway or attach to an existing gateway with `--no-qemu` and
`--no-gateway`. It writes:

- `<prefix>.log`: timestamped run log;
- `<prefix>.summary.json`: canonical machine-readable benchmark result;
- `<prefix>.ops.csv`: per-operation metrics for quick plotting;
- `<prefix>.ramp.csv`: ramp-step metrics for time-series plots;
- `<prefix>.ramp.svg`: lightweight visual smoke artifact.

Use this mode for worker cardinality, high-pressure mixed operations,
lease/schedule/telemetry pressure, and QEMU/Pi same-harness comparison.

### `--mode perf`

`perf` measures sequential and parallel status/telemetry reads. It is a
microbenchmark for gateway/read-path latency, not a worker-scale proof. It is
useful before and after gateway, auth, cache, or read-only namespace changes.

## Summary JSON Is The Source Of Truth

The `*.summary.json` file should contain every field needed to reproduce a
benchmark report. CSV and SVG files are projections for convenience. A report
or dashboard should load JSON first and derive all charts from it.

The current REST harness writes a `report` object with schema
`cohesix-benchmark-report/v1`:

| Field | Meaning |
| --- | --- |
| `report.workload` | Mode, scenario, worker range, intensity, base RPS, target RPS bounds, duration, retry policy, strictness, configured max in-flight requests. |
| `report.throughput` | Observed total, OK, and error operations per second. |
| `report.latency` | Overall average, min, max, p50, p90, p95, and p99 latency. |
| `report.reliability` | Count, OK, error, error rate, error budget, pass/fail. |
| `report.concurrency` | Configured max in-flight, observed high water, submitted/completed operations, and final in-flight count. |
| `report.backpressure` | Gateway counter deltas: pool exhaustion, checkout retries, timeout rejections, control-write retryable errors/retries/exhaustions, retry sleep, cache hits/misses. |
| `report.top_operations_by_p95` | Highest p95 latency contributors. |
| `report.top_operations_by_error_rate` | Highest error-rate contributors. |
| `report.visualization` | Stable series names and recommended chart types. |

The top-level legacy-compatible fields remain available: `overall`,
`operations`, `ramp`, `gateway_status_start`, `gateway_status_end`, and
`gateway_status_delta`.

## Visualization Guidance

A world-class benchmark report should show the shape of the run, not just the
final score. Recommended charts:

1. **Worker count and target pressure over time:** `ramp.workers`,
   `ramp.rps`, and `ramp.max_inflight_observed`.
2. **Observed throughput vs target pressure:** `ramp.throughput_ops_s` and
   `ramp.rps`.
3. **Reliability envelope:** `ramp.err_rate` with a horizontal error-budget
   line, plus final `report.reliability.error_rate`.
4. **Latency envelope:** overall p50/p90/p95/p99 and per-operation p95 bars
   from `operations`.
5. **Backpressure attribution:** gateway counter deltas from
   `report.backpressure`, grouped as queue, pool, timeout, control-write, and
   cache counters.
6. **Failure taxonomy:** top error strings by operation, preserving VM
   `ERR` details and HTTP status.
7. **Target comparison:** QEMU vs Pi GENET vs Pi Wi-Fi in separate lanes with
   matched workload metadata visible.

Do not plot a single "score" without the supporting error, latency, and
backpressure panels. Cohesix intentionally refuses work when bounded queues or
VM control buffers are full; that refusal is part of the contract and must be
visible.

## Current 26d Findings

### Historical 1500-Worker Context

The historical M25b QEMU result at 1500 workers remains useful as context, but
it is retired as an active target. It measured an earlier, less complex
Cohesix. The old raw artifacts under `docs/bench/` have been removed so they
are not mistaken for current seL4 15 rolling baselines.

The best historical fixed-1500 QEMU result was approximately:

- `1500` workers;
- `1m` duration;
- `base_rps=0.1`, intensity `1`;
- about `7356` total operations;
- about `0.068%` errors;
- average latency about `2.9 ms`;
- p95 latency about `6.1 ms`.

Those numbers should not be used as the pass/fail bar for the current
seL4 15 system without accounting for the added control-plane and proof
surfaces.

### Defects Found And Fixed

The seL4 15 revalidation found a real gateway-side defect class before the
current results were accepted: stale connection/pool state and cache hits that
could be blocked behind reconnect behavior. The safe fix was gateway-local:

- shut down the stale session pool after ping failure;
- serve valid cached `/proc` reads/lists before requiring reconnect;
- increase the bounded `/proc` cache TTL to `2000 ms`.

This changed latency and reliability without changing Secure9P semantics,
control-file behavior, error budgets, or the benchmark workload.

### Current QEMU Envelope

Accepted current QEMU evidence supports these practical limits for the mixed
REST harness:

| Workload | Result | Interpretation |
| --- | --- | --- |
| `600` workers, intensity `10`, `base_rps=0.1`, target about `600 RPS`, `max_inflight=64`, `2m` | PASS, error rate about `0.87%`, overall p95 about `22 ms`. | Realistic high-pressure aggregate target for the current mixed workload. |
| `400` workers, intensity `10`, `base_rps=0.2`, target about `800 RPS` | FAIL, error rate slightly above the `1%` budget. | Current quota/control pressure limit is below this target for the mixed workload. |
| `1200` workers, intensity `4`, `base_rps=0.1`, target about `480 RPS` | PASS, error rate below `1%`, p95 about `23 ms`. | Realistic high-cardinality target for the current QEMU profile. |
| `1300-1500+` workers under high load | Degrades through quota/control buffer pressure and worker-capacity timeouts. | 1500 workers is no longer a realistic high-load target for current Cohesix complexity. |

The dominant remaining failure class is `lease_quota` on `/queen/lease/ctl`
returning VM `buffer-full`. Other operations stayed clean in the accepted
high-pressure run. Gateway counters showed no pool exhaustion, no checkout
retries, and no timeout rejections, so the remaining limit is not a host
session-pool bottleneck.

### Latency Interpretation

The accepted QEMU p95 latency near `22 ms` is higher than the old M25b
loopback result, but it is consistent with a more complex control plane under
mixed high pressure. The important distinction is cause:

- gateway stale-session/cache defects were fixed;
- retrying VM `buffer-full` quota writes was tested and rejected because it
  worsened latency and error rate while producing no successful retries;
- remaining `lease_quota` errors are fast deterministic refusals from bounded
  VM pressure, not slow hidden timeouts;
- telemetry and log-tail p95 values in the accepted run remained bounded in
  the tens of milliseconds.

Higher latency is acceptable when it comes from real audited control-plane work
and bounded refusal. It is not acceptable when it comes from accidental retries,
stale pools, unbounded queues, or cache misses that should have been local.

## Running A Same-Harness QEMU Benchmark

Build the VM first with the selected seL4 build directory. Then run the harness
with explicit workload, retry, and artifact settings. Example high-pressure
QEMU run:

```bash
python3 scripts/rest_perf_harness.py \
  --mode simulate \
  --qemu-run /path/to/qemu/run.sh \
  --gateway-bin target/debug/hive-gateway \
  --auth-token bootstrap \
  --request-auth-token stage4-rest-token \
  --workers-min 600 --workers-max 600 \
  --intensity-min 10 --intensity-max 10 \
  --duration-mins 2 \
  --base-rps 0.1 \
  --max-inflight 64 \
  --error-budget-rate 0.01 \
  --summary-max-error-lines 2000 \
  --log-dir logs/bench \
  --log-prefix m26d_qemu_sel415_fixed600_i10
```

For a lower-worker, higher-pressure probe, keep the worker count fixed and
raise `--base-rps`:

```bash
python3 scripts/rest_perf_harness.py \
  --mode simulate \
  --qemu-run /path/to/qemu/run.sh \
  --gateway-bin target/debug/hive-gateway \
  --auth-token bootstrap \
  --request-auth-token stage4-rest-token \
  --workers-min 400 --workers-max 400 \
  --intensity-min 10 --intensity-max 10 \
  --duration-mins 2 \
  --base-rps 0.2 \
  --max-inflight 64 \
  --error-budget-rate 0.01 \
  --summary-max-error-lines 2000 \
  --log-dir logs/bench \
  --log-prefix m26d_qemu_sel415_fixed400_i10_rps800
```

## Running Pi 4 Benchmarks

Pi 4 benchmarks require target proof in addition to REST artifacts. A Pi REST
summary without fresh target evidence is a diagnostic run, not hardware
acceptance.

Required proof lanes:

- fresh non-empty serial log for the boot being benchmarked;
- selected seL4 15 Pi build tree and timer/counter configuration;
- `DRIVER_TASK_OWNER_STATE_PROOF=yes`;
- valid `DRIVER_TASK_COUNTER_*` snapshots with invalid count zero;
- runtime/DMA proof from `scripts/pi4_gate_proof.sh`;
- GENET or Wi-Fi link proof, including DHCP/static-IP evidence;
- raw direct `cohsh`/TCP liveness before REST interpretation;
- serial, USB/local-seat, and HDMI liveness under load when those surfaces are
  in scope.

Use Pi Wi-Fi only for the Wi-Fi research/diagnostic lane unless a fresh
site-specific Wi-Fi envelope is accepted. Production high-concurrency Pi
capacity should use GENET.

## Actioning Benchmark Insights

Use this decision path after every material run:

1. **Validate provenance.** Confirm target, seL4 version, manifest, workload,
   retry policy, duration, RPS target, max in-flight, and artifact paths.
2. **Check reliability first.** If error rate exceeds budget, inspect the top
   failing operations and exact error strings before reading throughput.
3. **Classify backpressure.** Gateway pool/checkout/timeouts point to
   `hive-gateway`; VM `buffer-full` points to bounded root-task/control-ring
   pressure; target counters point to driver-runtime or physical-link pressure.
4. **Read latency by operation.** Overall p95 can hide a single expensive
   control path. Use per-operation p95 and top error-rate tables.
5. **Separate capacity shapes.** High worker count with low pressure tests
   namespace/cardinality. Low worker count with high pressure tests aggregate
   control-plane throughput. Both are useful and neither replaces the other.
6. **Fix only the moved layer.** Do not increase queues, add retries, or lower
   workload pressure unless evidence shows that is the correct layer and the
   system contract allows it.
7. **Rerun the same harness.** A fix is accepted only when the same workload
   and report fields show the movement.

## Safe And Unsafe Performance Changes

Safe changes usually have these properties:

- preserve append-only control history and audit shape;
- preserve ACK/ERR/END and HTTP error mapping;
- improve local caching without hiding mutating state;
- close stale sessions or stale pools deterministically;
- add counters, provenance, or report fields without changing workload;
- reduce nonessential output under load while preserving operator liveness.

Unsafe changes include:

- transparent coalescing of `/queen/lease/ctl` quota writes without a new
  documented API/audit contract;
- broad gateway queue or pool increases when counters show no gateway
  starvation;
- retrying VM `buffer-full` writes when success-after-retry remains zero;
- changing the operation mix to make a result pass while calling it the same
  benchmark;
- treating Wi-Fi stress diagnostics as GENET production capacity;
- using stale local runs as active baselines.

## Artifact Retention

Raw benchmark outputs belong under `logs/bench/` or `out/bench/` during
iteration. Accepted benchmark reports should cite the exact summary JSON path,
log path, target proof bundle, manifest/seL4 provenance, and any comparator
output. Commit raw artifacts only when the active milestone explicitly requires
checked-in evidence; otherwise keep the benchmark document as the reviewed
interpretation layer and avoid carrying stale historical artifacts in
`docs/bench/`.

The old M25b raw artifacts in `docs/bench/` were removed as part of the 26d
benchmark-methodology refresh. Their conclusions are retained above as retired
historical context, not active pass/fail evidence.

## Benchmark Report Checklist

A benchmark report is ready for review only when it answers:

- What exact workload was run?
- Which proof lane does it belong to?
- Which artifact is the canonical `*.summary.json`?
- What was the configured and observed concurrency?
- What were target and observed throughput?
- What were p50, p90, p95, and p99 latency overall and by operation?
- What was the error budget and final error rate?
- Which operation produced the most errors?
- Which operation produced the highest p95 latency?
- Which backpressure counters moved?
- Did direct TCP/raw target liveness pass before interpreting REST?
- Are Pi serial/runtime/counter proof lanes fresh when hardware claims are
  made?
- Is the result a defect, expected complexity cost, or overload boundary?
- What concrete change or limit follows from the evidence?
