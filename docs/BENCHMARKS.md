<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define Cohesix benchmark methodology, evidence qualification, and artifact requirements. -->
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

See the [Glossary](GLOSSARY.md) for Cohesix-specific backend, role, and evidence
terms.

## Scope and proof classes

Cohesix performance evidence is classified by the system that produced it.
Results from different classes are useful for different questions, but they are
not interchangeable.

| Class | What it measures | What it cannot establish |
| --- | --- | --- |
| Host model | Gateway, REST projection, report generation, and synthetic population handling on the host. | Target execution, seL4 scheduling, driver behavior, or physical-network performance. |
| QEMU | The selected seL4 image, executable target Workers, console transport, and host projection under the recorded QEMU configuration. | Raspberry Pi timing, devices, or physical-network behavior. |
| Raspberry Pi 4 | The exact read-back image and physical transport used for the run. | A different image, transport, board, or later source tree. |
| Host microbenchmark | A named parser, replay, report, visualization, or host-tool operation. | End-to-end target capacity. |

The report's `backend_class` identifies the execution backend;
`proof_class` records the strongest evidence actually imported and validated.
A connected gateway, successful build, or reachable target does not by itself
upgrade the proof class.

The canonical macOS QEMU lane uses HVF with
`virt,gic-version=3,virtualization=off`, `cortex-a57`, QEMU-native HVC PSCI,
and the generated timer frequency. TCG, `-icount`, and artificial timer
variants are diagnostic execution models and must not be compared as accepted
latency or throughput evidence.

Target scheduling values, Worker bounds, console descriptors, and component
identities come from the selected generated build. Benchmark commands must
record them; this document does not duplicate their current values. See
[STATUS.md](STATUS.md) for the current implementation boundary and
[ROLES_AND_SCHEDULING.md](ROLES_AND_SCHEDULING.md) for the scheduling contract.

Exact measured findings belong in immutable benchmark artifacts and their
qualified audit records. A result becomes a public claim only when its source,
image, target, workload, comparator, and required Test Plan state are complete.

## Qualification Rules

Every performance claim must identify:

- milestone task and harness version or commit;
- target and transport: QEMU, Pi GENET, Pi Wi-Fi, direct TCP, REST, or host-only;
- selected seL4 build and manifest fingerprint;
- workload mode, operation mix, worker range, intensity, duration, random seed,
  target RPS, and maximum in-flight requests;
- gateway bind, session pools, timeouts, cache settings, and auth mode;
- retry policy and whether `buffer-full` is counted as an error;
- population mode and the generated maximum plus requested, discovered, and
  structured READY counts, backend class, and evidence-derived proof class;
- overall and per-operation success, errors, latency, and throughput;
- backpressure counter deltas;
- exact summary, log, target-proof, and comparator artifact paths.

If any field needed to reproduce or interpret a result is missing, label the
result **diagnostic**, not accepted.

## Evidence Lanes

| Lane | Proves | Does not prove |
| --- | --- | --- |
| QEMU REST `simulate` | Gateway plus VM mixed-workload capacity, cardinality limits, bounded refusal, and same-harness regressions. | Pi physical-network or local-seat behavior. |
| Host-model REST `simulate` | Gateway broker, REST projection, report, and large-reference telemetry reliability at the configured synthetic population. | QEMU/Pi execution, target Worker capacity, target scheduling, or hardware transport. |
| QEMU direct TCP or `cohsh` | Console and grammar latency without REST projection. | Gateway, browser, or hardware transport cost. |
| REST `perf` | Sequential-versus-parallel status or telemetry read behavior. | Worker-scale mixed mutation capacity. |
| Pi GENET | Wired target latency and throughput only when paired with fresh current-image runtime, network, raw TCP, and `cohsh` proof. | Wi-Fi capacity or QEMU parity. |
| Pi Wi-Fi | Site-specific CYW43/SDIO behavior and failure modes with paired packet evidence. | Wired capacity or a general production envelope. |
| Driver-runtime counters | Service-turn, deadline, ring, IRQ, and bounded-backpressure attribution. | User-visible throughput without a same-boot workload. |
| Host microbenchmark | Gateway, parser, report, replay, or UI cost outside the VM. | VM or physical-target capacity. |

Lanes may explain one another, but they are not interchangeable.

## Changed Surface to Evidence Lane

Choose lanes from the component that changed, not from the easiest environment
to run. Cross-layer changes require every applicable row.

| Changed surface | Minimum performance lane | Required companion proof |
| --- | --- | --- |
| Console parser, authentication, or `cohsh` transport | QEMU direct TCP or `cohsh`; add the physical transport lane when target code changed | Console grammar fixtures, exact auth mode, and ACK/ERR/END regression |
| Root-task namespace, worker lifecycle, or schedule queue | QEMU REST `simulate` with a fixed manifest, seed, and operation mix | Generated-artifact guard and target-qualified Test Plan |
| `hive-gateway`, REST client, session pool, cache, or broker | Host-model REST `simulate`; add QEMU executable pressure and REST `perf` when the target-backed path or read path changed | Gateway status delta, queue/time-out settings, per-operation errors, and explicit backend/proof class |
| HAL or isolated driver runtime | Driver-runtime counters plus the affected physical Pi lane | Same-image serial, runtime/DMA proof, packet capture when networked, and driver tests |
| GENET transport | Pi GENET only | Fresh wired boot, DHCP/static policy, bidirectional packets, raw TCP, and authenticated `cohsh` |
| CYW43/SDIO transport | Pi Wi-Fi only | Association, host EAPOL, DHCP, ARP/data, DPC/IRQ, raw TCP, `cohsh`, and repeatability evidence |
| Harness, report schema, parser, replay, or visualization | Host microbenchmark or fixture replay plus one unchanged-target control run | Artifact-schema tests and a before/after comparison from identical source data |

A security or authority change is never accepted from a performance result
alone. Run the functional, policy, and generated-contract gates first.

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

The summary contains a `report` object whose `schema` is
`cohesix-benchmark-report/v1`. Legacy top-level fields are compatibility
projections; automation and review decisions must use the versioned object.

| Report field | Contents and interpretation |
| --- | --- |
| `schema` | Exact report contract identifier; reject unknown major versions. |
| `workload` | Mode, scenario, seed, entropy, worker/multi-hive bounds, intensity, base and target RPS, duration/ramp interval, read-size controls, lifecycle/approval state, configured in-flight limit, timeout, role, auth-presence boolean, retry state, and strict-error state. These fields define comparability; secret values are never serialized. |
| `population` | Explicit `host-model` or `executable` mode, generated maximum live tasks when applicable, requested/discovered/structured-READY counts, bounded discovery observations, gateway backend class, and evidence-derived proof class. Executable discovery never expands ids or turns connectivity into proof. |
| `throughput` | Attempted, successful, and failed operations per second over the configured duration. Throughput without reliability is not a capacity result. |
| `latency` | Overall average, minimum, maximum, p50, p90, p95, and p99 seconds. Use `operations` in the parent summary for per-operation latency. |
| `reliability` | Counts, error rate, declared error budget, pass/fail result, and lossless classification of `buffer-full`, other, and unclassified errors. `all_errors_buffer_full` is `null` when no errors occurred; classification never removes an error from the total. Exact error strings remain in the parent `overall` and `operations` objects. |
| `capacity_boundary` | Fixed-versus-ramped worker/intensity shape, configured/effective/observed worker maxima, whether each endpoint was observed, and bounded projections of the first error row and first row strictly over the declared error budget. A worker cap can make the effective endpoint lower than the configured endpoint. |
| `retained_state` | Independent count/success/error/refusal projections for `schedule_write`, `lease_grant`, `lease_preempt`, and `lease_quota`. It identifies bounded `buffer-full` refusals without reclassifying them as success or changing the run verdict. |
| `concurrency` | Configured maximum, observed high-water mark, current in-flight count, and submitted/completed counts. |
| `backpressure` | Gateway-status deltas for waiters/high-water marks, checkouts, pool exhaustion, checkout retries, timeout refusal, control-write retry behavior, and `/proc` cache effectiveness. Zero means no observed delta, not proof that another layer had no pressure. |
| `top_operations_by_p95` | Up to ten operation rows ranked by p95 latency, including count, success, and error totals. |
| `top_operations_by_error_rate` | Up to ten operation rows ranked by error rate, including count, success, and error totals. |
| `visualization` | Canonical series names and recommended chart types; guidance only, not measured data. |

These additive diagnostics do not alter operation selection, weights, random
number consumption, retries, request ordering, strict-error behavior, or exit
criteria. The regression suite locks the stateful control operation names and
weights so a reporting change cannot silently redefine the workload.

A ramp holds its configured Worker and intensity maxima for the final ramp
interval. `configured_endpoint_observed=false` is therefore a failed workload
shape, even when the error budget is otherwise clean; interpolating only over
elapsed wall time and stopping before the maximum is not the declared
comparator.

`perf` writes a `*.perf-summary.json` artifact. Always state that it is a read
microbenchmark and name whether status, telemetry, or both suites ran.

## Running a Mixed REST Benchmark

Use a repository-local virtual environment so the interpreter is isolated and
part of the recorded provenance. The harness itself uses the Python standard
library:

```bash
test -x .venv/bin/python || python3 -m venv .venv
test -x .venv/bin/python
.venv/bin/python scripts/rest_perf_harness.py --help >/dev/null
```

Load real secrets from an approved secret manager into environment variables.
Do not pass them as command arguments or save them in scripts, shell history,
reports, or checked-in environment files:

```bash
test -n "${COH_AUTH_TOKEN:?set the target console secret}"
test -n "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?set the REST mutation token}"
```

### Harness-Managed Host-Model Gateway

Use one exact packaged `hive-gateway`. `--gateway-mock` selects the in-process
NineDoor host model, while `--no-qemu` prevents target launch and target TCP
preflight. The harness starts and stops the gateway, and status must report
`backend_class=host-model` before any synthetic Worker mutation. This lane is a
gateway and harness workload; it is not QEMU or Pi evidence.

```bash
HIVE_GATEWAY_BIN="${HIVE_GATEWAY_BIN:?set the exact packaged gateway}"
test -x "$HIVE_GATEWAY_BIN"

.venv/bin/python scripts/rest_perf_harness.py \
  --mode simulate \
  --population-mode host-model \
  --no-qemu \
  --gateway-mock \
  --gateway-bin "$HIVE_GATEWAY_BIN" \
  --tail-bytes 8192 \
  --workers-min 8 \
  --workers-max 8 \
  --intensity-min 2 \
  --intensity-max 2 \
  --duration-mins 1 \
  --base-rps 0.1 \
  --max-inflight 16 \
  --seed 26 \
  --no-transient-retries \
  --strict-control-errors \
  --error-budget-rate 0.01 \
  --gateway-log out/bench/gateway-managed.log \
  --log-dir out/bench \
  --log-prefix host-model-managed-smoke
```

This is a bounded smoke workload, not an accepted target-capacity result. Check
that the gateway bind is free before the run; the harness fails closed rather
than competing with an existing owner.

Worker telemetry uses one fail-closed `8192`-byte request ceiling, matching the
harness's existing complete structured Worker-state bound. The host-model
`cohesix-worker-observation/v1` record is 381 bytes for `worker-3`; the former
implicit 256-byte ceiling therefore rejected a valid record before pressure.
This ceiling revision adds no retry or truncation and changes no response
bytes, but it is a declared comparator input: do not compare the revised
results directly with runs that used a 256-byte Worker-tail ceiling.

Conditional D also enables `--strict-control-errors`. Every typed bounded
control refusal remains a failed operation and counts against the unchanged 1%
budget; the harness must not use relaxed buffer-full handling for this lane.

### Establishing a target-backed run

First establish an accepted QEMU or physical-target boot and a gateway already
backed by that target. A target gateway reports
`backend_class=console-projection`; never pair it with high-count
`--population-mode host-model`. QEMU executable pressure uses the exact-three
accepted Worker/session flow below. Pi requires a target-neutral fresh-Pi
acceptance path and cannot reuse the QEMU validator or metadata.

```bash
HIVE_GATEWAY_REQUEST_AUTH_TOKEN="$(openssl rand -hex 32)"
export HIVE_GATEWAY_REQUEST_AUTH_TOKEN

scripts/m26e_qemu_pressure.sh \
  --run-dir out/m26e-qemu-pressure

unset HIVE_GATEWAY_REQUEST_AUTH_TOKEN
```

That canonical runner owns the exact accepted QEMU artifact/session/component
inputs and emits fixed-three executable pressure. It is distinct from
Conditional D's 24-to-120 host-model gateway comparator.

### QEMU executable-Worker pressure

Use `executable` only against a live QEMU boot whose gateway projects generated
Worker bounds and imports a matching, same-boot staged component record as
described in [HOST_API.md](HOST_API.md). The harness fails before load unless
the gateway is a connected `console-projection`, the shared validator accepted
the exact current target session, and the three canonical `/shard/<label>/worker/<id>/telemetry`
records are structured READY instances matching that component. It never
expands ids, substitutes `/worker`, or treats reachability as target proof.

The canonical Mac command performs the clean build and runs medium first, then
high against a separate fresh equivalent four-core HVF
`virt,gic-version=3,virtualization=off` boot:

```bash
HIVE_GATEWAY_REQUEST_AUTH_TOKEN="$(openssl rand -hex 32)"
export HIVE_GATEWAY_REQUEST_AUTH_TOKEN

scripts/m26e_qemu_pressure.sh \
  --run-dir out/m26e-qemu-pressure

unset HIVE_GATEWAY_REQUEST_AUTH_TOKEN
```

The orchestrator cleans repository `target/` and `out/`, rebuilds the selected
SMP+MCS seL4 profile, and performs one canonical
`scripts/cohesix-build-run.sh --no-run` artifact build. It hash-binds immutable
collector copies before the critical-duty, medium, and high QEMU processes use
`--launch-existing`; each verifies and launches the same locked elfloader,
kernel, rootserver, system CPIO, GICv3 topology, and build context without
regeneration or repackaging. Only after those QEMU transcripts and pressure
reports are immutable does the runner execute the complete staged QEMU plan.
Final acceptance requires that plan to pass and consumes the frozen collector
copies, so later host or regression builds cannot replace the ELFs, archives,
manifest, topology, or target session that produced the pressure evidence. It
retains the actual QEMU command, pidfile,
flushed UART, three role-specific GDB injection transcripts, GPU fixture status,
cohsh and host-agent transcripts, staged component, and exact target-session
and image/archive manifests. The driver hash always comes from
`out/cohesix/driver-runtimes/cohesix-driver-runtimes.cpio`; the large archive is
embedded in rootserver and is not duplicated into the system CPIO.

The runner derives the TCP console token from the compiler-selected Queen
ticket in `configs/root_task.toml` and checks both generated builds against the
resolved manifest before any QEMU acceptance work. An optional inherited
`COH_AUTH_TOKEN` must match that compiler-owned value; an environment-only
console token is rejected because it cannot change the target. The REST bearer
is a distinct fresh 256-bit host-edge value. Retained-evidence scanning rejects
the complete REST bearer everywhere and credential-bearing console forms such
as an `AUTH` frame or token assignment, while ordinary public source names such
as `bootstrap` and `bootstrap-trace` are not misclassified as leaked secrets.

Before each load it exercises bounded fault/teardown/recreation and the exact
host-ticket-v2 GPU/LoRA receipt matrix through existing control files and host
agent execution. The GPU/model/export-job input is admitted only as
`mode=fixture` under `release-qemu,bootstrap-trace`; it is retained as fixture
evidence and never relabelled provider-live or production. The normal pressure
boot's UART prefix and Worker/service GDB files, together with a clearly
separate same-artifact `-S` critical-duty transcript, produce the staged
component imported by the gateway. The auxiliary critical boot is not labelled
same-boot Worker or pressure evidence. The final component/root/system collector consumes the immutable preflight plus
medium/high reports afterward, avoiding a circular dependency on the record
the pressure run is helping produce.

For both reports, retain `report.population` and require `mode=executable`,
`maximum_live_tasks=3`, `requested=3`, `discovered=3`, `ready=3`,
`backend_class=console-projection`, and `proof_class=qemu`. Re-derive the
numerical maximum from `/v1/meta/bounds` if the selected generated profile
changes; never raise the command merely to preserve an earlier value. A control
write outcome is `admitted`, not accepted or READY. Preserve all timeouts,
bounded refusals, and liveness failures as measured errors; a completed QEMU
launch or connected gateway alone leaves proof class `none`.

Each summary also retains top-level `target_session_sha256` and
`report.executable_state`: exact topology/session hashes; pre/post three-role
identities, READY/control/receipt/completion sequences, SCs and per-slot
compiler-admission object bundles (not a claimed live retype census);
five canonical `/proc` snapshots; bounded lifecycle cycles; live receipt
operations; and exact UART/GDB hashes plus required-marker index. Medium/high
must have distinct intensities, a clean error budget, increasing GPU/LoRA
receipt sequences, and a fresh Heartbeat supervisor generation. Missing target
fault markers, service teardown, fixture status/job files, or immutable
artifact equality fails closed rather than producing executable evidence.

For a focused read-path run:

```bash
.venv/bin/python scripts/rest_perf_harness.py \
  --mode perf \
  --suite all \
  --runs 5 \
  --no-qemu \
  --no-gateway \
  --rest-url "$COH_REST_URL" \
  --log-dir out/bench \
  --log-prefix qemu-read-path
```

### Split Cardinality from Steady-State Performance

The existing harness supports complementary tests that must retain separate
artifacts and verdicts. Do not infer sustained throughput from a cardinality
fill, or worker-scale mutation capacity from a read microbenchmark.

| Method | Harness mode | Valid conclusion | Important boundary |
| --- | --- | --- | --- |
| Retained-state cardinality/refusal | `simulate` with strict control errors | Accepted schedule/lease records, first bounded refusal, and owning operation | The mixed workload fills state indirectly; this is not an isolated or zero-traffic cardinality probe. |
| Bounded fixed-cardinality mixed load | `simulate` with equal worker and intensity minima/maxima | Reliability, latency, and throughput for a finite interval in which no retained-state bound is reached | It is not a long-duration steady-state result once monotonic schedule or preemption state fills. |
| State-neutral read service | `perf` with `status`, `telemetry`, or `all` | Sequential-versus-parallel read latency and gateway counter deltas | It does not measure mixed mutations, target RPS, or worker-admission capacity. |

#### Retained-state cardinality and refusal

Run each repetition from a fresh target state. The retained-state pressure
method uses the following exact workload:

```bash
BENCH_BUNDLE="${BENCH_BUNDLE:?set one matching unpacked release bundle}"

.venv/bin/python scripts/rest_perf_harness.py \
  --mode simulate \
  --population-mode host-model \
  --bundle "$BENCH_BUNDLE" \
  --workers-min 8 \
  --workers-max 1000 \
  --intensity-min 6 \
  --intensity-max 6 \
  --duration-mins 2 \
  --ramp-step-secs 10 \
  --base-rps 0.6 \
  --max-inflight 256 \
  --seed 2501 \
  --no-transient-retries \
  --strict-control-errors \
  --error-budget-rate 0.01 \
  --qemu-log out/bench/split-cardinality-qemu.log \
  --gateway-log out/bench/split-cardinality-gateway.log \
  --log-dir out/bench \
  --log-prefix split-cardinality
```

Crossing the declared budget makes this command return non-zero even when the
intended overload boundary was reached. Accept the artifact as a cardinality
result only when the summary and end marker are complete and the exact errors
are bounded target refusals. Use `report.retained_state` and
`report.capacity_boundary` to report:

- successful and refused counts for `schedule_write`, `lease_grant`,
  `lease_preempt`, and `lease_quota` independently;
- the first `ramp` row whose interval error rate crosses the declared budget,
  using the row's actual workers and RPS rather than configured maxima;
- `worker_cap`, actual maximum worker row, and whether the configured endpoint
  was fixed-point confirmed in a separate run;
- gateway pool, checkout-retry, waiter, and timeout deltas so target
  cardinality is not misclassified as gateway saturation.

`capacity_boundary.first_error_budget_crossing` uses a strict greater-than
comparison of exact interval `err / ops`, not the six-decimal display value;
its bounded row projection includes `exact_err_rate`. A row exactly equal to
the budget is not the first crossing.
`configured_endpoint_observed` describes only this artifact; it is not a claim
that a separate fixed-point run confirmed the endpoint.

The current mixed operation builder always includes unique schedule writes and
lease grant/preempt/quota operations when `/queen` exists. It therefore exposes
retained-state limits probabilistically through the fixed seed and operation
mix; it does not directly assert a pure collection capacity. The configured
Worker and intensity maxima begin no later than the final ramp interval and are
held through that interval; failure to observe that endpoint is non-qualifying.

#### Bounded fixed-cardinality mixed load

Use equal worker minima/maxima and equal intensity minima/maxima. Start every
repetition from equivalent fresh state, keep the seed and operation mix fixed,
and change only one load dimension at a time. Use a separate prefix for every
run.

This method is valid only while all control operations remain successful and
the target's retained collections stay within their generated bounds. Total
lease-grant admissions may exceed the active-lease bound when successful
preemption releases slots, so judge the run from exact operation outcomes and
target state rather than raw cumulative admissions alone. If `buffer-full`
appears, classify that repetition as a cardinality/refusal result from its first
failing step; do not pool it into a steady-state throughput average. Never omit
`--strict-control-errors`, enable retries, reuse dirty target state, or widen a
bound to preserve a throughput claim.

#### State-neutral read service

Use `perf` for a long read-path sample that does not grow schedule or lease
state. Against one already accepted target and gateway:

```bash
test -n "${COH_REST_URL:?set the accepted gateway URL}"

.venv/bin/python scripts/rest_perf_harness.py \
  --mode perf \
  --suite status \
  --runs 100 \
  --no-qemu \
  --no-gateway \
  --rest-url "$COH_REST_URL" \
  --log-dir out/bench \
  --log-prefix split-steady-status-read
```

Use `--suite telemetry --max-workers <count>` only when the accepted target
already exposes the intended worker set. Declare any warm-up exclusion before
comparison, retain every raw timing sample, and compare exact gateway status
deltas. `perf` reports sequential and parallel batch timing; it does not offer
a sustained target-RPS controller.

The current CLI has no operation-family selector, custom operation weights,
read-only `simulate` profile, bounded-ID recycler, schedule consumer, or lease
expiry/reaping mode. Consequently, the existing harness cannot qualify a
long-duration mixed-mutation steady state. Disabling strict errors merely hides
the retained-state boundary and is invalid. Such a claim requires an explicit
profile plus harness tests before it is added to this methodology.

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
Do not substitute a wired boot from another image, another Wi-Fi image, or
ambient traffic for the benchmark boot.

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

## Visualization and Review Package

Build charts from `<prefix>.summary.json`, `<prefix>.ops.csv`, and
`<prefix>.ramp.csv`; retain the generated `<prefix>.ramp.svg` as a quick smoke
view. A review package should contain, at minimum:

1. **Pressure and throughput over time:** target RPS and observed successful and
   failed operations per second by ramp step.
2. **Reliability boundary:** error rate against the declared budget, annotated
   with the first worker/intensity step that crosses it.
3. **Latency by operation:** p50, p95, and p99 for operations with meaningful
   sample counts; show the count beside each series.
4. **Backpressure attribution:** gateway status deltas and observed in-flight
   high water aligned to the same ramp steps.
5. **Comparator:** the same charts for a provenance-compatible baseline with a
   clear indication of missing or non-comparable lanes.

Use seconds or milliseconds consistently and label the unit. Start time-series
axes at zero unless a non-zero origin is explicitly called out. Do not use a
dual axis that makes latency and throughput appear causally linked. Never
silently remove failed attempts, warm-up intervals, overload steps, or empty
worker suites. If telemetry is skipped because the target exposed no workers,
say so in the chart and conclusion rather than plotting a zero result.

The written conclusion must name the accepted envelope or first failing step,
the error and latency contract, the counter evidence identifying the owning
layer, and the exact engineering decision. A chart without the canonical JSON,
workload configuration, and target proof is illustrative only.

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
- Is the artifact a retained-state cardinality test, a bounded fixed-cardinality
  mixed interval, or a state-neutral read test, and does the selected harness
  mode support that conclusion?
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
