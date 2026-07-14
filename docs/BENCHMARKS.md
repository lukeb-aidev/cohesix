<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define Cohesix benchmark methodology, evidence qualification, artifact requirements, and current findings. -->
<!-- Author: Lukas Bower -->
# Cohesix Benchmarking

Cohesix benchmarks measure a bounded control plane, not an unconstrained
throughput service. A valid result preserves the same tickets, namespace
semantics, audit behavior, backpressure, console grammar, and target ownership
model used in normal operation.

This document owns benchmark methodology and qualified findings. Target boot
and device proof belongs in [HARDWARE_BRINGUP.md](HARDWARE_BRINGUP.md), staged
acceptance in [TEST_PLAN.md](TEST_PLAN.md), and milestone authorization in
[BUILD_PLAN.md](BUILD_PLAN.md).

## Active Scope

The active performance task is Milestone 26d,
`m26d-benchmark-revalidation-and-tuning`. It may improve provenance,
classification, strictness, and bounded implementation defects exposed by the
same workload. It must not relax error accounting, change authority semantics,
hide `buffer-full`, or convert a benchmark shortcut into production behavior.

## Current Qualified State

| Evidence | Qualification |
| --- | --- |
| Current QEMU Test Plan | Stages 01-05 pass under `out/test-plan/m26d-repository-gates-qemu`. This qualifies the current QEMU regression environment, not a mixed-load numerical baseline. |
| Focused seL4 15 QEMU read microbenchmarks | The retained M26d provenance ledger records status-suite runs and their exact artifacts. Telemetry was skipped because the selected run exposed no discoverable worker entries. These results qualify only the measured status read path. See [M26D_SEL4_15_PROVENANCE.md](audit/M26D_SEL4_15_PROVENANCE.md). |
| Historical Pi 4 wired GENET | M26c retains a coherent GENET Stage 01-05, runtime/DMA, DHCP, raw TCP, and authenticated `cohsh` proof chain. It is accepted historical target readiness, not current-tree throughput proof. See [M26C_AS_BUILT_BLOCKERS.md](audit/M26C_AS_BUILT_BLOCKERS.md). |
| Current Pi 4 source | Pi-qualified offline Stages 01-02 pass under `out/test-plan/m26d-repository-gates-pi4`. No current-tree live Pi benchmark is qualified. |
| Current Pi 4 Wi-Fi | Rebuild, flash/readback, current-image association, EAPOL, DHCP, ARP, raw TCP, authenticated `cohsh`, and repeatability revalidation remain pending. Any older Wi-Fi performance result is diagnostic or historical. |

No current mixed-load `simulate` worker envelope is accepted by this document.
The former 1500-worker result and later local 400/600/1200-worker observations
lack a retained, current, fully qualified artifact index here. They may guide a
new experiment but must not be quoted as the active capacity limit.

## Qualification Rules

Every performance claim must identify:

- milestone task and harness version or commit;
- target and transport: QEMU, Pi GENET, Pi Wi-Fi, direct TCP, REST, or host-only;
- selected seL4 build and manifest fingerprint;
- workload mode, operation mix, worker range, intensity, duration, random seed,
  target RPS, and maximum in-flight requests;
- gateway bind, session pools, timeouts, cache settings, and auth mode;
- retry policy and whether `buffer-full` is counted as an error;
- overall and per-operation success, errors, latency, and throughput;
- backpressure counter deltas;
- exact summary, log, target-proof, and comparator artifact paths.

If any field needed to reproduce or interpret a result is missing, label the
result **diagnostic**, not accepted.

## Evidence Lanes

| Lane | Proves | Does not prove |
| --- | --- | --- |
| QEMU REST `simulate` | Gateway plus VM mixed-workload capacity, cardinality limits, bounded refusal, and same-harness regressions. | Pi physical-network or local-seat behavior. |
| QEMU direct TCP or `cohsh` | Console and grammar latency without REST projection. | Gateway, browser, or hardware transport cost. |
| REST `perf` | Sequential-versus-parallel status or telemetry read behavior. | Worker-scale mixed mutation capacity. |
| Pi GENET | Wired target latency and throughput only when paired with fresh current-image runtime, network, raw TCP, and `cohsh` proof. | Wi-Fi capacity or QEMU parity. |
| Pi Wi-Fi | Site-specific CYW43/SDIO behavior and failure modes with paired packet evidence. | Wired capacity or a general production envelope. |
| Driver-runtime counters | Service-turn, deadline, ring, IRQ, and bounded-backpressure attribution. | User-visible throughput without a same-boot workload. |
| Host microbenchmark | Gateway, parser, report, replay, or UI cost outside the VM. | VM or physical-target capacity. |

Lanes may explain one another, but they are not interchangeable.

## Canonical Tools

| Tool | Purpose |
| --- | --- |
| `scripts/rest_perf_harness.py --mode simulate` | Mixed REST load, worker cardinality, mutation/read pressure, and QEMU/Pi same-harness runs. |
| `scripts/rest_perf_harness.py --mode perf` | Sequential-versus-parallel status and telemetry read microbenchmarks. |
| `scripts/pi4_compare_driver_models.py` | Reject mismatched QEMU/Pi comparison metadata and compare accepted model lanes. |
| `scripts/pi4_trace_normalize.py` | Extract current-boot Pi device, network, timer, and driver proof. |
| `scripts/pi4_gate_proof.sh` | Produce fail-closed Pi target proof from a fresh serial capture. |
| `scripts/ci/test_plan_run.sh` | Qualify the source and target environment around a benchmark. |

## Harness Artifacts

`simulate` writes a stable set under the chosen `--log-dir` and prefix:

| Artifact | Use |
| --- | --- |
| `<prefix>.summary.json` | Canonical machine-readable result. |
| `<prefix>.log` | Timestamped execution and failure detail. |
| `<prefix>.ops.csv` | Per-operation projection for analysis. |
| `<prefix>.ramp.csv` | Time-series ramp projection. |
| `<prefix>.ramp.svg` | Quick visual smoke output; never the sole evidence. |

The summary contains `cohesix-benchmark-report/v1`. Review its workload,
throughput, latency, reliability, concurrency, backpressure,
`top_operations_by_p95`, and `top_operations_by_error_rate` fields. Legacy
top-level fields are compatibility projections; derive decisions from the
versioned report object.

`perf` writes a `*.perf-summary.json` artifact. Always state that it is a read
microbenchmark and name whether status, telemetry, or both suites ran.

## Running a Mixed REST Benchmark

First establish an accepted QEMU or physical-target boot and a gateway backed
by that target. Configure real secrets through environment variables rather
than command arguments:

```bash
test -n "${COH_AUTH_TOKEN:?set the target console secret}"
test -n "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?set the REST mutation token}"
test -n "${COH_REST_URL:?set the accepted gateway URL}"

python3 scripts/rest_perf_harness.py \
  --mode simulate \
  --no-qemu \
  --no-gateway \
  --rest-url "$COH_REST_URL" \
  --workers-min 100 \
  --workers-max 100 \
  --intensity-min 4 \
  --intensity-max 4 \
  --duration-mins 2 \
  --base-rps 0.1 \
  --max-inflight 32 \
  --seed 26 \
  --no-transient-retries \
  --strict-control-errors \
  --error-budget-rate 0.01 \
  --log-dir out/bench \
  --log-prefix candidate-fixed100-i4
```

The numbers above define an example workload, not an accepted Cohesix target.
Change one dimension at a time and retain the complete report for every
candidate envelope.

For a focused read-path run:

```bash
python3 scripts/rest_perf_harness.py \
  --mode perf \
  --suite all \
  --runs 5 \
  --no-qemu \
  --no-gateway \
  --rest-url "$COH_REST_URL" \
  --log-dir out/bench \
  --log-prefix candidate-read-path
```

## Pi 4 Preconditions

A Pi result is not interpretable until the same boot proves:

- exact read-back build marker and selected Pi manifest;
- valid virtual-counter frequency and no dummy timer;
- isolated runtime owner-state, DMA, ring, and driver counter proof;
- current serial prompt and input responsiveness for in-scope operator surfaces;
- selected GENET or Wi-Fi link, address, and bidirectional packet evidence;
- raw TCP and authenticated `cohsh` before REST;
- boot-paired packet capture and normalized proof bundle;
- target-qualified Test Plan state appropriate to the claim.

For Wi-Fi, additionally require the current DPC/IRQ proof, association and host
EAPOL completion, DHCP, ARP/data progress, and accepted same-image repeatability.
Do not substitute a historical wired boot, another Wi-Fi image, or ambient
traffic for the benchmark boot.

## Reading a Result

Review in this order:

1. **Provenance:** reject stale, mismatched, or incomplete artifacts.
2. **Reliability:** compare total and per-operation error rate with the declared
   budget. Preserve exact `ERR` and HTTP classifications.
3. **Bounded refusal:** distinguish gateway pool/timeout pressure from VM
   `buffer-full`, driver-ring pressure, and physical-link failures.
4. **Latency:** inspect p50, p90, p95, and p99 overall and by operation. A single
   overall percentile can hide one pathological control path.
5. **Throughput and concurrency:** compare observed work with target pressure
   and the maximum in-flight high-water mark.
6. **Target health:** correlate the workload with console, driver, network, and
   local-seat counters from the same run.
7. **Moved layer:** name the client, gateway, Secure9P, root-task, driver
   runtime, physical transport, or presentation layer that changed.

Do not reduce a run to one score. A useful report shows pressure over time,
observed throughput, error budget, latency percentiles, backpressure deltas, and
top failing operations.

## Safe Interpretation and Tuning

Acceptable tuning preserves all authority and proof semantics. Examples include
closing stale sessions deterministically, serving valid read-only cache entries
without blocking behind reconnect, reducing avoidable copies, or rate-limiting
nonessential output under load.

Do not accept a result produced by:

- retrying or hiding `buffer-full` without accounting for every attempt;
- widening queues or timeouts without evidence that the bounded layer is wrong;
- changing the operation mix while calling it the same workload;
- dropping audit, ticket, or receipt work;
- relaxing ACK/ERR/END or Secure9P behavior;
- using QEMU, Wi-Fi, or host-only evidence as a proxy for another lane;
- quoting an unarchived local run as the canonical baseline.

## Acceptance Checklist

A benchmark is ready for documentation only when all answers are explicit:

- What exact workload, target, transport, manifest, and seL4 build were used?
- Which `*.summary.json` or `*.perf-summary.json` is canonical?
- Did the target proof and raw console pass on the same boot?
- What were success, error rate, latency percentiles, throughput, and observed
  concurrency overall and per operation?
- Which backpressure counters moved, and which layer owns them?
- Were retries disabled or completely reported?
- Does the comparator have matching workload and provenance?
- Is the conclusion an accepted baseline, a diagnostic, an overload boundary,
  or a blocker?
- What exact engineering or operating decision follows?

Accepted reports must cite their artifact paths in this section or in a linked
checked-in audit ledger. Raw iteration files may remain under `out/bench` or
`logs/bench`, but an uncommitted path alone is not durable documentation.
