<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define Cohesix benchmark methodology, evidence qualification, and artifact requirements. -->
<!-- Author: Lukas Bower -->
# Cohesix Benchmarking

Cohesix benchmarks measure a bounded control plane, not an unconstrained
throughput service. A valid result preserves the same tickets, namespace
semantics, audit behavior, backpressure, console grammar, and target ownership
model used in normal operation.

This document owns benchmark methodology and evidence qualification. Target
boot and device proof belongs in [HARDWARE_BRINGUP.md](HARDWARE_BRINGUP.md),
staged acceptance in [TEST_PLAN.md](TEST_PLAN.md), and scope and result history
in [BUILD_PLAN.md](BUILD_PLAN.md). Exact measurements remain with their owning
task and immutable evidence rather than being copied into this reference.

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

The canonical QEMU target has two host execution envelopes. macOS uses HVF with
`virt,gic-version=3,virtualization=off,kernel-irqchip=off` and `cortex-a57`;
Linux AArch64 uses KVM with `virt,gic-version=3,virtualization=off`, the `host`
CPU, and the in-kernel GICv3. Both use QEMU-native HVC PSCI and the same
four-core topology. The macOS `qemu_smp_production` profile generates
24,000,000 Hz; Jetson KVM cannot override its architectural counter and uses
the separately generated `qemu_smp_kvm_production` value, currently 31,250,000
Hz. TCG, `-icount`, and artificial timer variants are diagnostic execution
models and must not be compared as accepted latency or throughput evidence.

Target scheduling values, Worker bounds, console descriptors, and component
identities come from the selected generated build. Benchmark commands must
record them; this document does not duplicate their current values. See
[STATUS.md](STATUS.md) for the current implementation boundary and
[ROLES_AND_SCHEDULING.md](ROLES_AND_SCHEDULING.md) for the scheduling contract.

Exact measured findings belong in immutable benchmark artifacts and their
qualified audit records. A result becomes a public claim only when its source,
image, target, workload, comparator, and required Test Plan state are complete.

## Qualification Rules

Bind compiler-profile changes to a new exact image even when scheduling and
protocol source are unchanged. Release package speed optimization for
`root-task`, `console-network-runtime`, `pi4-driver-runtime` and `smoltcp`
does not change the raw request workload, throughput interval, p95 arithmetic
or target thresholds. Compare fresh target receipts and retain image/page
admission plus the QEMU canary as separate qualification evidence.

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

For the current Pi GENET convergence candidate, the core-1
`3,000 us / 10,000 us` SC and 3,400 us computed response are manifest/admission
facts only. The finite legacy-to-direct DMA/IRQ cutover and larger SC may remove
the observed post-DHCP stall, but neither predicts August 10 latency. A
qualified comparison must record same-boot GENET consumed-time, IRQ/DPC and
ring counters alongside raw TCP and the canonical workload. Report scheduling
exhaustion, packet loss, and p50/p95/p99 separately; do not attribute a result
to the SC move without a same-image before/after lane.

## Canonical Tools

| Tool | Purpose |
| --- | --- |
| `scripts/rest_perf_harness.py --mode simulate` | Mixed REST load, worker cardinality, mutation/read pressure, and QEMU/Pi same-harness runs. |
| `scripts/rest_perf_harness.py --mode perf` | Sequential-versus-parallel status and telemetry read microbenchmarks. |
| `scripts/pi4_compare_driver_models.py` | Compare historical Pi serial-driver logs or reject stale/mismatched target reports before a QEMU/Pi throughput verdict. |
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
| `provenance` | For a qualified target run, the exact target/transport/proof class, source inventory, resolved manifest, staged image, root image, target session, nullable component-acceptance hash (required for QEMU and exactly `null` for Pi performance), runtime, network, performance-qualification, capture-time, and workload hashes. Diagnostic runs keep the same shape with unavailable fields set to `null`; they cannot be upgraded by reachability. |
| `workload` | Mode, scenario, seed, entropy, worker/multi-hive bounds, intensity, base and target RPS, duration/ramp interval, read-size controls, lifecycle/approval state, configured in-flight limit, timeout, role, auth-presence boolean, retry state, and strict-error state. These fields define comparability; secret values are never serialized. |
| `population` | Explicit `host-model` or `executable` mode, generated maximum live tasks when applicable, requested/discovered/structured-READY counts, bounded discovery observations, gateway backend class, and evidence-derived proof class. Executable discovery never expands ids or turns connectivity into proof. Qualified executable pre/post state additionally retains an aggregate `ready_census` binding the full count and generated topology while detailed evidence remains one Heartbeat/GPU/LoRA exemplar. |
| `throughput` | Attempted, successful, and failed operations per second over the configured duration. Throughput without reliability is not a capacity result. |
| `latency` | Overall average, minimum, maximum, p50, p90, p95, and p99 seconds. Use `operations` in the parent summary for per-operation latency. |
| `reliability` | Counts, error rate, declared error budget, pass/fail result, and lossless classification of `buffer-full`, other, and unclassified errors. `all_errors_buffer_full` is `null` when no errors occurred; classification never removes an error from the total. Exact error strings remain in the parent `overall` and `operations` objects. |
| `capacity_boundary` | Fixed-versus-ramped worker/intensity shape, configured/effective/observed worker maxima, whether each endpoint was observed, and bounded projections of the first error row and first row strictly over the declared error budget. A worker cap can make the effective endpoint lower than the configured endpoint. |
| `retained_state` | Independent count/success/error/refusal projections for `schedule_write`, `lease_grant`, `lease_preempt`, and `lease_quota`. `schedule_write` is one logical FIFO producer/consumer lifecycle: enqueue followed by exact-head dequeue under the Queen-owned consumer lock. It identifies bounded `buffer-full` refusals without reclassifying them as success or changing the run verdict. |
| `concurrency` | Configured maximum, observed high-water mark, current in-flight count, and submitted/completed counts. |
| `backpressure` | Gateway-status deltas for waiters/high-water marks, checkouts, pool exhaustion, checkout retries, timeout refusal, control-write retry behavior, and `/proc` cache effectiveness. Zero means no observed delta, not proof that another layer had no pressure. |
| `top_operations_by_p95` | Up to ten operation rows ranked by p95 latency, including count, success, and error totals. |
| `top_operations_by_error_rate` | Up to ten operation rows ranked by error rate, including count, success, and error totals. |
| `visualization` | Canonical series names and recommended chart types; guidance only, not measured data. |

These additive diagnostics do not alter operation selection, weights, retries,
strict-error behavior, or exit criteria. The regression suite locks the
stateful control operation names and weights. The schedule operation issues two
ordered target writes by contract; report counts remain logical operations, not
raw HTTP or Secure9P request counts.

A ramp holds its configured Worker and intensity maxima for the final ramp
interval. `configured_endpoint_observed=false` is therefore a failed workload
shape, even when the error budget is otherwise clean; interpolating only over
elapsed wall time and stopping before the maximum is not the declared
comparator.

`perf` writes a `*.perf-summary.json` artifact. Always state that it is a read
microbenchmark and name whether status, telemetry, or both suites ran.
If executable preflight blocks because the complete structured READY Worker
census is absent, a same-session `perf --suite status|telemetry` run is
transport/read-path diagnostic only. Record the exact endpoint, backend and
proof class, authentication/session continuity, run count, latency,
retries, and timeouts. It cannot be relabelled executable Worker pressure,
mixed mutation, target capacity, Pi acceptance, or QEMU/Pi parity.

For physical Pi read-concurrency diagnostics, the gateway must use
`--worker-runtime-profile pi4-production` so its generated namespace bounds
match the target. Keep `perf --population-mode executable-log` and the actual
Pi transport; structured discovery must validate the requested real READY
Workers before load. With `--suite telemetry --runs 3`, increasing
`--max-workers 16` to `32` increases parallel submissions while keeping the
repeat count fixed. Use a fresh attached session for each level and let the
harness perform its sole READY census: TAIL advances a session cursor even
when its response is empty. With 32 discovered Workers, the census consumes
32 of the generated 256 cursor advances; three sequential plus three parallel
batches consume another 96 or 192, for totals of 128 or 224. Eight repetitions
would instead require 288 or 544 and hit the ticket quota. Preserve that
refusal as quota evidence; do not enlarge the ticket, rewind cursors or hide
session rotation inside a run. Preserve the generated pool limits and report
queue or timeout failures. These are medium/high read-concurrency diagnostics, not the
mixed offered-load profiles below or 256-Worker acceptance.

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
`--population-mode host-model`. QEMU and Pi both require the complete generated
256-Worker population plus exactly one accepted Heartbeat/GPU/LoRA exemplar.
Pi additionally requires a target-neutral fresh-Pi runtime, network, and image
proof chain and cannot reuse QEMU fault/GDB evidence.

```bash
HIVE_GATEWAY_REQUEST_AUTH_TOKEN="$(openssl rand -hex 32)"
export HIVE_GATEWAY_REQUEST_AUTH_TOKEN

scripts/m26e_qemu_pressure.sh \
  --run-dir out/m26e-qemu-pressure

unset HIVE_GATEWAY_REQUEST_AUTH_TOKEN
```

That canonical runner owns the exact accepted QEMU artifact/session/component
inputs and emits generated-population executable pressure. It is distinct from
Conditional D's 24-to-120 host-model gateway comparator.

### QEMU executable-Worker pressure

Use `executable` only against a live QEMU boot whose gateway projects generated
Worker bounds and imports a matching, same-boot staged component record as
described in [HOST_API.md](HOST_API.md). The harness fails before load unless
the gateway is a connected `console-projection`, the shared validator accepted
the exact current target session, and every generated canonical
`/shard/<label>/worker/<id>/telemetry` record is a structured READY instance
matching that component. It never invents ids, substitutes `/worker`, or
treats reachability as target proof.

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

After transferring the exact source and reviewed patch to an AArch64 Linux
host, build the KVM timer profile and replay the same workload under KVM:

```bash
HIVE_GATEWAY_REQUEST_AUTH_TOKEN="$(openssl rand -hex 32)"
export HIVE_GATEWAY_REQUEST_AUTH_TOKEN

scripts/m26e_qemu_pressure.sh \
  --reuse-artifacts \
  --qemu /path/to/qemu-system-aarch64 \
  --sel4-build out/sel4/profile-v2/qemu-smp-kvm-production \
  --run-dir out/m26e-qemu-pressure-linux

unset HIVE_GATEWAY_REQUEST_AUTH_TOKEN
```

The Linux lane verifies the same source commit and patch identity, then binds
its profile-qualified guest bytes to `-cpu host`, the native architectural
counter, and the in-kernel GICv3. Mac and Linux guest hashes are recorded
separately and are not expected to match. Results are comparable when their
generated topology, Worker population, root/service bounds, workload, and
source patch are equivalent; they are not a substitute for the macOS lane's
seL4 profile validation or complete staged release acceptance. A launch record
must match the selected host profile before load.

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
flushed UART, any separately required fault transcripts, GPU fixture status,
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
agent execution. After the host-forwarded TCP port opens, the runner also waits
for the target-emitted `Cohesix console ready` marker before starting the
gateway, then uses the shared bounded readiness probe to require an
authenticated backend and successful root listing; a host socket accept alone
is not guest/backend readiness. The
GPU/model/export-job input is admitted only as
`mode=fixture` under `release-qemu,bootstrap-trace`; it is retained as fixture
evidence and never relabelled provider-live or production. Direct `cohsh` fault
injection, when required by the independent acceptance lane, finishes before
the gateway first attaches, and the gateway remains the sole console owner for
the rest of the normal pressure boot. GDB is never part of the iterative
medium/high benchmark or diagnosis loop. That boot's UART and bounded flight
records, together with any clearly separate same-artifact critical-duty
transcript required for acceptance, produce the staged component. The
gateway starts once with fixed trust-root, future-component, and
current-target-session paths. It projects no executable acceptance while that
component is missing or invalid, then promotes the first fully validated
same-boot PASS component exactly once and keeps that accepted summary
immutable. Pressure begins only after the shared validator confirms the
promoted binding; no gateway restart or console-owner handoff occurs. The
auxiliary critical boot is not labelled same-boot Worker or pressure evidence.
Its collector pauses once at a QEMU-only
post-SMP arm hook and then installs the four duty breakpoints after secondary
core initialization, preventing host-accelerator debug-state resets from being
misreported as missing guest duties. The hook performs no I/O or scheduling
work. The final component/root/system collector consumes the immutable preflight plus
medium/high reports afterward, avoiding a circular dependency on the record
the pressure run is helping produce.

For both reports, retain `report.population` and require `mode=executable`,
with `maximum_live_tasks`, `requested`, `discovered`, and `ready` all equal to
the selected generated population (256 for the current QEMU profile),
`backend_class=console-projection`, and `proof_class=qemu`. Re-derive the
numerical maximum from `/v1/meta/bounds` if the selected generated profile
changes; never raise the command merely to preserve an earlier value. A control
write outcome is `admitted`, not accepted or READY. Preserve all timeouts,
bounded refusals, and liveness failures as measured errors; a completed QEMU
launch or connected gateway alone leaves proof class `none`.

For iterative medium/high performance diagnosis after a separate current-image
correctness baseline, `--population-mode executable-log` may use every
generated real target Worker with UART-bound READY identity instead of running
the independent fault-containment collector. This is
`proof_class=qemu-live-log`, not Conditional B2 acceptance. Keep GDB out of the
load/debug loop and retrieve
`/proc/schedule/qemu-flight` after the run. Correlate its virtual-counter
activation gaps, useful service units, queue-drainage ratio, queue high-water
mark, and exit reasons with the report throughput and p50/p95/p99, gateway
backpressure deltas, MCS timeout/fault scan, and host CPU/RSS samples. The
flight recorder is diagnostic evidence only and cannot promote an otherwise
unqualified image.

Each summary also retains top-level `target_session_sha256` and
`report.executable_state`: exact topology/session hashes; pre/post aggregate
256-Worker READY censuses plus three-role exemplar identities,
READY/control/receipt/completion sequences, executor-lane SCs,
per-instance Reply identities, and per-slot compiler-admission object bundles
(not a claimed live retype census);
five canonical `/proc` snapshots; bounded lifecycle cycles; live receipt
operations; and exact UART plus any separately required fault-evidence hashes
and marker index. Medium/high
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

### Qualified Pi executable pressure and QEMU parity

Run Pi pressure only after the current boot has passed the physical-target
preconditions below and the already-running gateway continuously projects that
same boot's exact generated Worker population. This performance lane does not
consume or claim Pi Worker component acceptance. First, on a fresh controlled
Wi-Fi boot, run
`pi4_gate_proof.sh` with concurrent serial and packet capture and its positive
CYW43 output enabled. Finalize those immutable bytes with the command below.
The finalizer independently revalidates the clean stage graph, exact image,
generated topology, source inventory, Worker ABI, runtime and root archives,
current boot marker, positive Wi-Fi outcomes, and controlled capture before it
publishes the canonical bundle. A build or stage proof cannot create this
positive record.

```bash
PI_CAPTURE_INTERFACE="${PI_CAPTURE_INTERFACE:?set the verified Pi-facing interface}"
PI_SERIAL_DEVICE="${PI_SERIAL_DEVICE:?set the sole Pi serial device}"
PI_WIFI_TARGET_IP="${PI_WIFI_TARGET_IP:?set the serial-reported Wi-Fi IPv4 address}"
COH_REST_URL="${COH_REST_URL:?set the exact already-running gateway base URL}"
PI_WIFI_EVIDENCE_DIR="${PI_WIFI_EVIDENCE_DIR:?set a private existing directory}"
PI_WIFI_SERIAL_LOG="$PI_WIFI_EVIDENCE_DIR/pi4-cyw43-serial.log"
PI_WIFI_NETWORK_CAPTURE="$PI_WIFI_EVIDENCE_DIR/pi4-cyw43-network.pcap"
PI_WIFI_RUNTIME_DMA_PROOF="$PI_WIFI_EVIDENCE_DIR/pi4-cyw43-runtime-proof.env"
PI_WIFI_CYW43_RECORD="$PI_WIFI_EVIDENCE_DIR/pi4-cyw43-coexistence.json"

test -d "$PI_WIFI_EVIDENCE_DIR"
test ! -e "$PI_WIFI_SERIAL_LOG"
test ! -e "$PI_WIFI_NETWORK_CAPTURE"
test ! -e "$PI_WIFI_RUNTIME_DMA_PROOF"
test ! -e "$PI_WIFI_CYW43_RECORD"

scripts/pi4_gate_proof.sh \
  --skip-build \
  --serial-device "$PI_SERIAL_DEVICE" \
  --log "$PI_WIFI_SERIAL_LOG" \
  --require-wifi-ready \
  --require-driver-task-proof \
  --network-interface "$PI_CAPTURE_INTERFACE" \
  --network-capture-out "$PI_WIFI_NETWORK_CAPTURE" \
  --gateway-status-url "$COH_REST_URL" \
  --gateway-target-host "$PI_WIFI_TARGET_IP" \
  --runtime-dma-proof-out "$PI_WIFI_RUNTIME_DMA_PROOF" \
  --cyw43-coexistence-record-out "$PI_WIFI_CYW43_RECORD"
```

Start this active capture before booting the freshly flashed image. It refuses
pre-existing output files, offline/normalize-only pairing, a non-current boot,
or an uncontrolled packet capture. `--skip-build` is valid here only when the
retained clean stage proof is the exact image already flashed and now booting;
otherwise rebuild and reflash before capture.

```bash
test -f "${PI_WIFI_RUNTIME_DMA_PROOF:?set the fresh controlled Wi-Fi runtime proof}"
test -f "${PI_WIFI_CYW43_RECORD:?set the matching gate-produced CYW43 record}"
test -n "${PI_SESSION_DIR:?set a new output directory below out/}"

.venv/bin/python scripts/worker_task_evidence.py emit-pi4-target-session \
  --repo-root "$PWD" \
  --runtime-proof "$PI_WIFI_RUNTIME_DMA_PROOF" \
  --cyw43-coexistence-record "$PI_WIFI_CYW43_RECORD" \
  --max-age-secs 21600 \
  --out-dir "$PI_SESSION_DIR"

PI_TARGET_SESSION="$PI_SESSION_DIR/target-session.json"
PI_CYW43_RECORD="$PI_SESSION_DIR/pi4-cyw43-coexistence.json"
```

The retained bundle also contains the exact Wi-Fi runtime proof, serial log,
network capture, source inventory, and Worker ABI identity under their
canonical sibling names. A later GENET run may use a different physical boot,
but it must use the same staged image and canonical target session. Its live
runtime proof and packet capture must both come from that current GENET boot.
The harness binds the canonical target session and generated topology/Worker
manifest to the live 256-Worker census and three role exemplars instead of
trusting reachability or a caller-authored PASS.
The gateway's `/v1/meta/status` must continuously report normalized configured
backend `target_host="$PI_TARGET_IP"` and `target_port=31337`; the gate seals
that first connection and the harness rejects endpoint or connection drift.

The Pi GENET run must additionally bind the same-boot
`CONSOLE_NETWORK_HANDOFF phase=direct-link-complete` record for the current
nonzero generation, `owner=driver-console-direct`, and
`root_packet_mediation=disabled`. An `IDLE/QUIESCING` retry is permitted before
that terminal but is not performance readiness. Missing selected-lane or
shell/armed/terminal identity, malformed handoff, pair containment, cursor
poison, retained root packet work, or a later driver/console fault rejects the
run. The roughly 200-millisecond
SYN response and 50-millisecond class staircase from the predecessor image are
diagnostic baseline evidence only. The direct path is intended to remove that
structural mediation cost; only fresh same-harness percentiles and throughput
may establish the magnitude, including any claimed 100-times improvement or
QEMU parity.

The current high-profile GENET command is:

```bash
test -n "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?set a fresh gateway request token}"
test -f "${PI_TARGET_SESSION:?set the exact Pi target-session.json}"
test -f "${PI_RUNTIME_DMA_PROOF:?set the same-boot live runtime/DMA proof}"
test -f "${PI_NETWORK_CAPTURE:?set the same-boot controlled packet capture}"
test -f "${PI_CYW43_RECORD:?set the retained positive CYW43 record}"

.venv/bin/python scripts/rest_perf_harness.py \
  --mode simulate \
  --population-mode executable \
  --benchmark-target pi4 \
  --benchmark-transport genet \
  --pi-runtime-dma-proof "$PI_RUNTIME_DMA_PROOF" \
  --pi-network-capture "$PI_NETWORK_CAPTURE" \
  --pi-cyw43-coexistence-record "$PI_CYW43_RECORD" \
  --benchmark-evidence-max-age-secs 21600 \
  --target-session "$PI_TARGET_SESSION" \
  --no-qemu \
  --no-gateway \
  --rest-url "$COH_REST_URL" \
  --tcp-host "$PI_TARGET_IP" \
  --tcp-port 31337 \
  --workers-min 256 \
  --workers-max 256 \
  --intensity-min 8 \
  --intensity-max 8 \
  --duration-mins 2 \
  --base-rps 4 \
  --max-inflight 32 \
  --seed 2608 \
  --no-transient-retries \
  --strict-control-errors \
  --error-budget-rate 0.01 \
  --log-dir out/bench/pi4-genet \
  --log-prefix m26e-pi4-genet-high
```

The harness rejects a count other than the generated maximum, an incomplete
READY census, missing role exemplar, diagnostic/log-only proof, stale or
mutated input, a reboot during the run, a changed session/build graph, or a Pi
stage/image/serial/network mismatch. It re-reads every frozen target input
after load; mutation, replacement, growth, or a changed boot slice fails. A
Wi-Fi run changes only `--benchmark-transport wifi`, the current
runtime/capture paths, and its output directory/prefix. For that transport the
current runtime and capture bytes must equal the retained canonical Wi-Fi
siblings, and the boot must still satisfy fresh CYW43/SDIO, DHCP, raw-TCP,
authenticated-`cohsh`, timer, runtime, and image proof.

Compare the retained canonical HIGH QEMU report with the GENET report only
after declaring the same-harness physical GENET p95 ceiling for this workload:

```bash
test -f "${QEMU_HIGH_REPORT:?set the qualified QEMU HIGH summary}"
test -f "${PI_GENET_REPORT:?set the qualified Pi GENET HIGH summary}"
test -n "${GENET_SAME_HARNESS_P95_MAX_MS:?declare the physical GENET p95 ceiling}"

python3 scripts/pi4_compare_driver_models.py \
  --qemu-report "$QEMU_HIGH_REPORT" \
  --pi-report "$PI_GENET_REPORT" \
  --reference-unix-s "$(date +%s)" \
  --max-age-secs 21600 \
  --min-throughput-ratio 1.0 \
  --genet-max-p95-ms "$GENET_SAME_HARNESS_P95_MAX_MS" \
  --output out/bench/pi4-genet/qemu-pi-parity.json
```

The output path must not already exist. The comparator rejects duplicate-key,
non-finite, stale, internally inconsistent, differently sourced, differently
shaped, or differently populated reports. `PASS` requires Pi GENET successful
operations per second to be at least the QEMU value and both reports to pass
their identical explicit error budget. Comparative error counts,
backpressure, QEMU latency, and the separately declared physical GENET p95
status remain visible but do not change that parity verdict.

An optional qualified Wi-Fi report may be added with `--wifi-report`,
`--wifi-min-ok-ops-per-s`, and `--wifi-max-p95-ms`. Those thresholds must have
been declared for the same mixed workload and metric; do not apply the raw-TCP
request-to-first-payload targets to REST summary latency. Wi-Fi status is
reported separately and never changes the wired QEMU/GENET verdict.

#### Separate conditional Pi Worker-component acceptance

This full-component procedure is not a prerequisite for the performance run or
comparator above, and a performance report cannot claim or replace it. Run it
only after an authorized physical Pi fault/integration procedure has produced
the complete same-boot three-role receipt, fault, teardown, recreation, and
integration matrix. The collector derives its detailed role rows from the
gate-owned serial bytes, compares every live image with the staged Worker
manifest, revalidates the current image/runtime/network graph, and retains
exactly `pi4-network-capture`, `pi4-runtime-dma-proof`, and
`pi4-serial-boot` as raw evidence. It cannot manufacture missing physical
outcomes and therefore remains fail-closed until that prerequisite exists.

```bash
PI_GENERATED_INVENTORY=out/pi4-sd/cohesix-root-task-topology.json
PI_INTEGRATION_DIR="${PI_INTEGRATION_DIR:?set the accepted Pi integration-record directory}"
PI_COMPONENT_DIR="${PI_COMPONENT_DIR:?set a new Pi Worker component output directory}"

test -f "$PI_TARGET_SESSION"
test -f "$PI_GENERATED_INVENTORY"
test -f "$PI_RUNTIME_DMA_PROOF"
test -f "$PI_NETWORK_CAPTURE"
test -d "$PI_INTEGRATION_DIR"
test ! -e "$PI_COMPONENT_DIR"

.venv/bin/python scripts/worker_task_evidence.py collect-pi4-component \
  --target-session "$PI_TARGET_SESSION" \
  --generated-inventory "$PI_GENERATED_INVENTORY" \
  --runtime-proof "$PI_RUNTIME_DMA_PROOF" \
  --network-capture "$PI_NETWORK_CAPTURE" \
  --transport genet \
  --integration-dir "$PI_INTEGRATION_DIR" \
  --max-age-secs 21600 \
  --out-dir "$PI_COMPONENT_DIR"
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

The current mixed operation builder always includes unique schedule lifecycles
and lease grant/preempt/quota operations when `/queen` exists. Each schedule
lifecycle serializes only the Queen-owned producer/consumer edge, enqueues one
unique request, and dequeues that exact FIFO head. The queue must therefore
return to its prior depth; a retained schedule entry is a lifecycle failure,
not benchmark cleanup. Active lease and quota state remain independently
bounded. Completed preemptions use the production fixed-capacity observation
ring: the newest records are retained, while `/proc/lease/summary` counts every
successful preemption cumulatively. Evidence-ring turnover therefore cannot
refuse an otherwise valid lease transition, and the harness must not add a
cleanup operation or retry around it. The configured Worker and intensity maxima begin no later
than the final ramp interval and are held through that interval; failure to
observe that endpoint is non-qualifying.

#### Bounded fixed-cardinality mixed load

Use equal worker minima/maxima and equal intensity minima/maxima. Start every
repetition from equivalent fresh state, keep the seed and operation mix fixed,
and change only one load dimension at a time. Use a separate prefix for every
run.

This method is valid only while all control operations remain successful and
the target's authoritative retained collections stay within their generated bounds. Schedule
depth must return to its pre-run value while `dequeued` advances once per
successful `schedule_write`. Total
lease-grant admissions may exceed the active-lease bound when successful
preemption releases slots, and cumulative preemptions may exceed the bounded
history capacity, so judge the run from exact operation outcomes and target
state rather than raw cumulative admissions alone. If `buffer-full`
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
read-only `simulate` profile, or lease expiry/preemption-evidence drain.
Consequently, the existing harness still cannot qualify a long-duration
mixed-mutation steady state after a lease collection reaches its generated
bound. Disabling strict errors merely hides the retained-state boundary and is
invalid. Such a claim requires an explicit lifecycle plus harness tests before
it is added to this methodology.

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
   local-seat counters from the same run. On QEMU, also compare root-control
   activation gaps, service quantum, and queue-drainage ratio from the bounded
   same-run flight snapshot.
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
