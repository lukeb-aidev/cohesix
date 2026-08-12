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

Milestone 26e additionally requires a full same-harness comparison because it
changes target scheduling, root-service IPC, and Worker execution. Its current
lane is QEMU-first on four-core AArch64 `virt` with GICv3. Pi 4 measurements
remain a later independent hardware lane and cannot be copied from, predicted
by, or replaced with QEMU results.

The current `m26e-qemu-root-exclusive-predispatch-candidate-v23` repair is
performance-relevant even though it changes no REST endpoint, workload, retry
policy, or report schema. It preserves v21's separate successor-before-work
Timer and Nic Network visits and the following NETDIAG-only visit, so the
featured sequence remains Timer -> Nic -> DeferredDiagnostic -> Timer and the
NIC remains serviced once per three featured Network visits. V23 makes compact
predispatch completion success-sensitive. A physical-tail reconciliation or
prompt-tail queue attempt returns exclusively when it clears its predicate; if
the bounded attempt leaves the predicate pending, exactly one compact Operator
unit may run before return without advancing the ordinary phase. Ready reboot
remains an exclusive pre-phase return. Runtime and Network never share a turn
with this housekeeping, while an Operator fallback may advance only its private
subcursor. Do not infer parity from green compiler or protocol tests.
First pass fresh QEMU authentication and standard/timeout injection,
then the focused direct base `.coh` batch, then Hive Gateway REST core/parity
plus the Python smoke. After those functional gates, rerun TEST_PLAN
Conditional D with retries disabled for the canonical `telemetry-1mb`,
`telemetry-10mb`, `telemetry-100mb`, and `telemetry-1gb` scenarios. Preserve
the unchanged harness inputs and summary schema so the result remains
comparable; report any error-budget or latency regression rather than masking
it with retry or timeout changes.

The paired child candidate is now
`m26e-qemu-console-bounded-stack-steps-candidate-v6`. It replaces the private
composite smoltcp poll with one ingress attempt, one egress pass, and Session
on separate child replenishments. It changes no benchmark request, REST
endpoint, retry policy, timeout, or report schema. The direct `.coh`, Hive
Gateway REST, and no-retry telemetry benchmarks therefore remain unchanged but
withheld until fresh v23/root-fault-V6/child-V6 authentication and fault injection pass.

### Milestone 26e QEMU-first evidence contract

The 26e benchmark artifacts are inputs to target evidence; they are not proof
merely because a benchmark process or QEMU instance started. Retain exact bytes
and publish their lowercase SHA-256 and byte length in the relevant
`raw_evidence` list:

| Evidence layer | Required explicit benchmark/target inputs |
| --- | --- |
| Worker component | Direct target transcript covering the complete Heartbeat, GPU receipt, LoRA receipt, saturation, timeout, fault, teardown, revocation, fresh-generation, operator-liveness, and driver-liveness outcome matrix; the matching live role-required integration records; medium- and high-pressure REST summaries when they exercise Worker/MCS behavior. |
| Root TCB | Compiler-generated admitted-maximum topology/inventory, the separate constructed-actual critical-duty census and live Worker/service attachment receipts, fault-containment transcript, operator-liveness transcript, and pressure/latency summaries. |
| Full system | Exact accepted component/root digests, four-core admission totals, no-classic-scheduler proof, normal/overload/timeout/fault/recovery results, protocol regression, same-harness performance, and the target-qualified CYW43 coexistence-record binding. |

The component record exposes each Worker's attach/fault badges, assigned core,
scheduling-context budget and period, and full compiler-budget object inventory
as typed fields. The component emitter checks those fields against
`coh-rtc`'s canonical `root_task_topology.json`, including the role's exact
temporal task and per-slot object bundle. The root record exposes the generated
maximum-role inventory as an admitted budget, not an allocation census. Its
projected admitted-maximum copy must match compiler truth exactly across TCB,
CNode, VSpace, page-table, ASID, frame, endpoint, notification,
standard/timeout fault-cap, Reply, scheduling-context, CSpace-slot, and
untyped-byte totals. Independently, `ROOT_CRITICAL_OBJECTS
scope=constructed-actual` and the live service/Worker markers bind what was
actually constructed, attached, faulted, and torn down; those observations are
checked against their own bounds and are never required to equal the maximum
budget. The full-system run input names the component and root digests
explicitly. All three records carry complete sorted outcome matrices
and hash-only raw-artifact descriptors. The emitters also bind their exact
observation, compiler-topology/inventory, and target-session input bytes, so an
edited observation invalidates downstream evidence. The compiler topology hash
is recomputed from deterministic compact JSON and the maximum inventory is
re-derived before either component or root promotion.

QEMU qualification must retain both medium and high pressure runs with retries
disabled, strict control-error accounting enabled, bounded in-flight work, and
separate requested/discovered/READY Worker population counts. Record latency,
throughput, every error class, console/operator liveness, per-core admission,
budget exhaustion, timeout attribution, fault recovery, and post-teardown
object/capability counts. A numerical improvement does not excuse a missing
receipt, semantic error, liveness loss, inventory drift, or leaked authority.
Conversely, an explicit bounded refusal remains an error for the stated error
budget and must not be retried or reclassified away.

Until those direct observations exist, the QEMU 26e evidence state is
unaccepted. The staged Test Plan may pass its ordinary QEMU regression tier
without emitting a 26e component/root/system PASS. Runtime release promotion
remains impossible until an independent fresh-Pi graph supplies the matching
three accepted records and positive hardware coexistence evidence.

## Current Qualified State

| Evidence | Qualification |
| --- | --- |
| Current QEMU Test Plan | Stages 01-05 pass under `out/test-plan/m26d-repository-gates-qemu`. This qualifies the current QEMU regression environment, not a mixed-load numerical baseline. |
| Focused seL4 15 QEMU read microbenchmarks | The retained M26d provenance ledger records status-suite runs and their exact artifacts. Telemetry was skipped because the selected run exposed no discoverable worker entries. These results qualify only the measured status read path. See [M26D_SEL4_15_PROVENANCE.md](audit/M26D_SEL4_15_PROVENANCE.md). |
| seL4 16 refresh | Fresh v16 source, five-profile, direct external Pi-source, root-task target-check, and linked QEMU `--no-run` validation passed on 2026-07-23. The exact `a94903514f72...` Pi image now supplies the transport-specific hardware comparison below, but it does not qualify the later source candidate. The v15 results above remain a historical comparator and must not be relabeled as v16 Pi evidence. |
| Historical Pi 4 wired GENET | M26c retains a coherent GENET Stage 01-05, runtime/DMA, DHCP, raw TCP, and authenticated `cohsh` proof chain. It is accepted historical target readiness, not current-tree throughput proof. See [M26C_AS_BUILT_BLOCKERS.md](audit/M26C_AS_BUILT_BLOCKERS.md). |
| Current Pi 4 source candidate | Hardware-free and offline source checks do not qualify live performance. The candidate after exact image `a94903514f72...` has not been measured on Pi hardware; it still requires an exact rebuild/readback, repeated first-pair Wi-Fi boots, TCP/`cohsh`, pressure, pcap, and unchanged GENET control. |
| Exact `a949` Pi 4 Wi-Fi | Five powered warm reboots passed Gate 8a-8h on their first pair, but the power-off boot consumed recovery after first-pair failure and was correctly rejected. The measured TCP flow was clean, while latency and the no-retry pressure result materially failed the accepted envelope. This is diagnostic hardware evidence, not cold-repeatability or performance qualification. |
| Exact `a949` Pi 4 GENET control | The same-image wired control completed 8,983 of 8,983 pressure operations with zero errors and approximately 1.2-ms request-to-first-payload latency. It qualifies the common TCP/console/REST/gateway stack as the control for this comparison, not the later source candidate or Wi-Fi capacity. |

### Exact `a949` Pi 4 Wi-Fi/GENET Comparison (2026-07-30)

This group measures source commit
`190786fac51acf9a84b8db0650e316a3b48347ba` in exact read-back image
`a94903514f72b5f2ac26a7565a4cf89951abaf945b318200468e48fa0135e51b`.
The later source candidate is not this image and has no measured performance
result. The figures below therefore remain bound to `a949`; they are a
transport comparison and defect baseline, not evidence that the candidate has
reached its target.

| Evidence | Pi Wi-Fi / CYW43 | Pi wired / GENET control |
| --- | --- | --- |
| Boot frontier | The power-off boot lost its first-pair owner, consumed the sole recovery allowance, and was correctly rejected as `supervisor-in-attempt-recovery-used`. Five subsequent powered warm reboots were 5/5 first-pair Gate 8a-8h successes with zero recovery. This does not close the cold failure. | One wired/DHCP boot supplied the unchanged common-stack control. |
| First-connection `.coh` | `boot_v0.coh` 2.524 s; `tcp_basic.coh` 2.633 s; `smp_parity.coh` 3.074 s. | The same scripts completed in 0.597 s, 0.079 s, and 0.019 s. |
| Link setup and latency | Ping averaged approximately 160.7 ms; SYN-to-SYN/ACK was 202-213 ms. | DHCP DORA completed in 8.133 ms; ping averaged 0.440 ms; SYN-to-SYN/ACK was 0.351 ms. |
| Request to first payload | Mean 314.653 ms, p95 407.430 ms, maximum 572.289 ms; approximately 2.9 sequential requests/s. | Mean 1.232 ms, p95 1.464 ms; measured goodput 165.164 kbit/s. |
| No-retry mid/high pressure | 309 of 824 operations succeeded; 515 timed out. | 8,983 of 8,983 operations succeeded with zero errors. |
| Paired-capture TCP quality | The persistent flow was clean: no sequence gap, out-of-order segment, reset, zero window, SACK, or SYN retry. That clean transport does not excuse the application timeouts or latency failure. | One retransmission accompanied an approximately 252-ms `CAT` stall, with no reset, reconnect, sustained loss, or material flow defect. |

Artifacts:

- Wi-Fi: `out/bench/pi4-wifi-a949-20260729T212254Z`.
- GENET: `out/bench/pi4-genet-a949-20260729T214633Z`.

GENET used the same smoltcp console, REST harness, and hive gateway and therefore
isolates the material delay to the CYW43 linked-runtime/HAL cadence rather than
the common TCP/application stack. The earlier proposed tenfold Wi-Fi floor of
request-to-first-payload p95 at or below 40 ms and at least 29 successful
sequential requests/s is retained as a non-blocking optimization target. The
aggressive stretch target remains p95 at or below 10 ms and at least 100
successful sequential requests/s. Milestone 26b closure instead uses the
operator-approved usable, repeatable, no-semantic-loss envelope recorded in the
Build Plan, with honest recovered-retransmission and RF-exclusion accounting
and unchanged GENET control. Source and model results alone cannot satisfy that
hardware gate.

### M26d QEMU Gateway Cache/Coalescing Revalidation (2026-07-16)

This diagnostic ledger covers Milestone 26d
`m26d-benchmark-revalidation-and-tuning` and the reopened Milestone 26b
`m26b-rest-normalized-parity` guarantee that cache/coalescing remains a
host-only projection with no write bypass. The candidate source commit is
`eed905d0edfe1e65f4dc83af652626a055cfcb4e`.

The retained 8-to-1000-worker baseline and final runs use the same mixed
workload: intensity 6, 120 seconds, seed-controlled ramp, target range
28.8-to-3600 requests/second, maximum 256 in flight, transient retries off,
strict control errors on, and a 1% error budget.

| Metric | Baseline | Final aligned | Interpretation |
| --- | ---: | ---: | --- |
| Successful / attempted operations | 96,458 / 104,023 | 100,437 / 108,316 | More successful work completed under the same two-minute workload. |
| Average latency | 45.7 ms | 22.6 ms | Improved; diagnostic QEMU REST result only. |
| p95 latency | 135.8 ms | 41.6 ms | Improved without retry masking. |
| p99 latency | 2.407 s | 248.4 ms | Improved; the tail remains workload- and QEMU-specific. |
| Telemetry waiter high-water | 139 | 47 | Reduced gateway contention. |
| Gateway pool exhaustion / timeout rejection | 0 / 0 | 0 / 0 | No gateway pool or timeout failure was observed. |
| Gateway read-cache hit rate (`proc_cache_*` counters) | 83.95% | 81.06% | Both runs exercised the cache; hit rate is descriptive, not a verdict. |
| Overall strict-control error rate | 7.272% | 7.274% | **FAIL** against the 1% budget in both runs. |

Artifacts:

- Baseline: `out/bench/m26d-high-pressure-baseline_20260715T115501Z.summary.json`.
- Final aligned: `out/bench/m26d-high-pressure-final-aligned_20260715T124018Z.summary.json`.
- Fresh bounded smoke using the candidate source: `out/bench/hive-cache-simulate_20260715T212758Z.summary.json` (`8` workers, intensity `2`, `96/96` operations successful, no retry, no gateway pool exhaustion or timeout rejection).
- Fresh affected-read microbenchmark: `out/bench/hive-cache-perf-status_20260715T213118Z.perf-summary.json` (`5` status runs, `108` cache hits, `12` misses, no gateway pool exhaustion or timeout rejection, measured sequential/parallel ratio `267.62x`). The first cold sequential sample dominates the average, so this ratio is not a general capacity claim.
- Companion gates: `out/test-plan/m26d-gateway-cache-20260715` Stages 01-04 and `out/audit/gate/20260715T212429Z` due diligence.

All 27 REST read operation kinds in the final aligned stress artifact completed
90,305 attempts with zero errors. The overall stress run is nevertheless not
an accepted capacity envelope: all 7,879 errors are explicit bounded VM
schedule/lease `buffer-full` refusals (5,367 lease and 2,512 schedule). That
upstream bounded-pressure debt blocks an 8-to-1000-worker acceptance claim but
does not justify hiding retries, widening queues, or treating cache reads as
failed. This ledger is QEMU gateway evidence only; it is not Pi 4, GENET,
CYW43/SDIO, Wi-Fi, or repeated-boot proof.

### M26d GICv3 High-Pressure Safe-Tuning Revalidation (2026-07-16)

This follow-up is authorized by Milestone 26d
`m26d-benchmark-revalidation-and-tuning`. It compares two baseline and two
final two-minute runs against byte-identical seL4/root-task images. Only the
host `hive-gateway` binary differs. The selected external seL4 15 build is
`/Users/lukasbower/seL4/SMP_build_gic3_smp4_v15`; its generated configuration,
device tree, and every retained QEMU log identify GICv3 and four CPUs. The QEMU
machine is `virt,gic-version=3,virtualization=on,kernel-irqchip=off` under TCG.

Both lanes use the same logical and request workload: workers ramp from 8 to
1000, intensity is fixed at 6, duration is 120 seconds, ramp steps are 10
seconds, seed is 2501, target load spans 28.8 to 3600 requests/second, and the
configured in-flight limit is 256. Transient retries are off, strict control
errors are on, the error budget remains 1%, the console is authenticated as
queen, and REST mutation authentication is enabled on loopback. Default
gateway pool, response-timeout, and control-write retry-window settings are
unchanged.

| Four-minute aggregate | Baseline | Final | Change |
| --- | ---: | ---: | ---: |
| Attempted operations | 221,719 | 224,348 | +1.19% |
| Successful operations | 205,428 | 210,937 | +2.68% |
| Errors | 16,291 | 13,411 | -17.68% |
| Strict error rate | 7.3476% | 5.9778% | -1.370 percentage points |
| Attempted throughput | 923.83/s | 934.78/s | +1.19% |
| Successful throughput | 855.95/s | 878.90/s | +2.68% |
| Average latency | 22.596 ms | 12.466 ms | -44.8% |
| p50 latency | 0.218 ms | 0.214 ms | -1.8% |
| p95 latency | 40.371 ms | 24.170 ms | -40.1% |
| p99 latency | 508.460 ms | 104.989 ms | -79.4% |
| Maximum latency | 2.939 s | 2.258 s | -23.2% |
| Gateway cache hit rate | 81.15% | 90.29% | +9.14 percentage points |
| Gateway cache misses | 31,230 | 16,293 | -47.8% |
| Gateway control + telemetry checkouts | 63,619 | 58,280 | -8.4% |
| Gateway pool exhaustion / checkout retry / timeout rejection | 0 / 0 / 0 | 0 / 0 / 0 | Unchanged |

Counts are exact pooled totals. Aggregate percentile rows are count-weighted
estimates from the two canonical summaries per lane because the harness does
not retain raw samples. Both final repetitions independently improve p95 and
p99. The meaningful residual read regression is `/proc/lease/summary` (p95
22.467 ms to 49.458 ms): successful quota mutations must invalidate that
count-bearing view. Quota-write latency is not directly comparable because the
baseline mostly measured incorrect fast collateral refusals while the final
lane executes every quota write at the target.

The four bounded changes are:

- expose the exact telemetry segment ID already present in the successful
  target `ECHO` acknowledgement, while retaining a validated `/latest` read for
  compatibility and the canonical comparator workload;
- isolate the 250 ms cached control refusal by operation so a failed lease
  grant or preempt cannot suppress unrelated renew or quota writes;
- invalidate only read-cache views an accepted write can change, including
  lease-operation-specific summary, active, and preemption paths; and
- share immutable cache-fill results with `Arc<[String]>`, copying into caller
  responses only after releasing cache locks.

The canonical harness still performs one segment-create `ECHO` plus one
`/latest` `CAT`. It prefers a valid receipt when another creator advances
`latest`, and uses the validated `CAT` result only as a compatibility fallback.
An intermediate receipt-only diagnostic changed the raw request workload and
exposed rapid four-segment eviction churn; it is intentionally excluded from
the qualified comparison. No segment bound, eviction rule, queue, timeout,
retry budget, error classification, authority check, or ACK/ERR/END behavior
was relaxed.

Artifacts and identity:

- Baseline summaries:
  `out/bench/m26d-high-pressure-gicv3-baseline_20260715T220607Z.summary.json`
  and
  `out/bench/m26d-high-pressure-gicv3-baseline-rep2_20260715T221458Z.summary.json`.
- Final summaries:
  `out/bench/m26d-high-pressure-gicv3-final-current_20260715T225703Z.summary.json`
  and
  `out/bench/m26d-high-pressure-gicv3-final-current-rep2_20260715T225923Z.summary.json`.
- Final QEMU logs:
  `out/bench/m26d-high-pressure-gicv3-final-current-qemu.log` and
  `out/bench/m26d-high-pressure-gicv3-final-current-rep2-qemu.log`.
- Baseline gateway SHA-256:
  `25b01a9ea2c27d6b748eaf16b6119ae8bd146f4bf85cad5863cb254c0ed3d19d`;
  final gateway SHA-256:
  `20ed7c0257137e2096654fe62085b1a95526dd3814aa6772ce93b777f5cf42f9`.
- Final REST harness SHA-256:
  `527d856d00375d052c59a7e45be73a36d20507d4c6ccd485795c280f06839cca`.
- Shared target hashes: elfloader
  `18073401eebcc754961ebd6163861b812ed2b4b56ddcdfc94a93aafc677e0bcc`,
  kernel
  `26877c4fdf79a6186ee7bfd54128dd6b81cf2b41ec160a232608c405b02478a8`,
  rootserver
  `60546dd33d4a7f2bdcaffdce7161c8f9c56563c098783cc1d225bb6637a32519`,
  and 2,542,080-byte rootfs CPIO
  `275cc3a5a5de327170c42de99e9eac2c6812c27884e291a03f0cb4fbcf4b8101`.
  The runtime manifest reports
  `376f09a49cdb37c07ae8ef007d4d4c715df4b4f949d4d6c1546002108d495599`.
- Final status-read companion:
  `out/bench/m26d-gicv3-final-read-status_20260715T223511Z.perf-summary.json`
  (5 runs, 108 cache hits, 12 misses, no pool exhaustion, checkout retry, or
  timeout rejection). Its first cold sequential sample dominates the reported
  31.11x sequential/parallel ratio, so that ratio is not a general capacity
  claim.
- Current-tree companion gates: QEMU Stages 01-05 under
  `out/test-plan/m26d-gicv3-safe-optimizations-20260716` and due diligence under
  `out/audit/gate/20260715T225244Z`.

This result is safely better than the paired GICv3 baseline but remains
**diagnostic, not an accepted 1000-worker envelope**. All four runs fail the
unchanged 1% budget. The final 13,411 errors are explicit bounded target
schedule, lease-grant, and lease-preemption capacity refusals; quota has zero
errors, telemetry has zero errors, and no failure was hidden or retried. This
QEMU result also provides no current Pi 4, GENET, CYW43/SDIO, Wi-Fi, or
repeated-boot proof.

### M26d Async Cache-Hit Fast-Path Revalidation (2026-07-16)

This tuning pass is also authorized by Milestone 26d
`m26d-benchmark-revalidation-and-tuning`. For validated cacheable `LS` and
`CAT` requests, `hive-gateway` now probes the existing read cache before
dispatching blocking work. A hit copies the immutable cached lines after
releasing the cache lock. A miss, expired entry, in-flight fill, or contended
lock follows the existing blocking/coalescing path, which remains the sole
place that accounts a miss and performs target I/O. Path and `max_bytes`
validation still occurs before the probe, and successful `CAT` responses use
the same byte-bound response builder in both paths.

The strongest changed-layer comparison is a same-QEMU-boot, gateway-restart
A/B using 100 `status` runs per binary. Excluding the first cold sample,
sequential latency improved from 1.578 ms to 1.469 ms (-6.90%) and parallel
latency improved from 1.604 ms to 1.577 ms (-1.68%). Both lanes recorded
exactly 2,388 cache hits, 12 misses, and 12 telemetry checkouts, with zero pool
exhaustion, checkout retry, or timeout rejection. The retained summaries are:

- pre-fast-path control:
  `out/bench/m26d-gicv3-cache-hit-control-status-100_20260716T022400Z.perf-summary.json`;
- cache-hit fast path:
  `out/bench/m26d-gicv3-cache-hit-fastpath-status-100_20260716T022432Z.perf-summary.json`.

Three full high-pressure repetitions retained the unchanged 8-to-1000-worker
workload described above. Together they attempted 334,725 operations, completed
315,004 successfully, and returned 19,721 explicit bounded errors (5.8917%).
Their count-weighted average latency was 9.612 ms and estimated p50 was
0.201 ms. Against the preceding two-run final aggregate, those values improve
by 22.9% and 6.0%, respectively, while attempted and successful throughput are
effectively flat (-0.53% and -0.44%). Estimated p95 and p99 were mixed at
29.177 ms and 176.221 ms (+20.7% and +67.8%), so this pass makes no broad tail
latency or capacity-envelope claim. Two interleaved pre-fast-path controls also
show substantial QEMU/telemetry tail variance. No candidate or control run
observed gateway pool exhaustion, checkout retry, or timeout rejection.

Full-pressure artifacts are:

- candidate repetitions:
  `out/bench/m26d-high-pressure-gicv3-cache-hit-fastpath_20260716T020540Z.summary.json`,
  `out/bench/m26d-high-pressure-gicv3-cache-hit-fastpath-rep2_20260716T020807Z.summary.json`,
  and
  `out/bench/m26d-high-pressure-gicv3-cache-hit-fastpath-rep3_20260716T021541Z.summary.json`;
- interleaved controls:
  `out/bench/m26d-high-pressure-gicv3-cache-hit-control_20260716T021312Z.summary.json`
  and
  `out/bench/m26d-high-pressure-gicv3-cache-hit-control-rep2_20260716T022051Z.summary.json`.

The pre-fast-path gateway SHA-256 is
`20ed7c0257137e2096654fe62085b1a95526dd3814aa6772ce93b777f5cf42f9`;
the candidate gateway SHA-256 is
`9020a2213bf21216b201f4445311f9954d25f66c4d367b9ad96f0b1241bc918c`.
The REST harness and target hashes are byte-identical to the preceding GICv3
ledger. No cache TTL or capacity, queue, timeout, retry policy, authority check,
error classification, API field, or ACK/ERR/END behavior changed. The tuning is
retained for its reproducible hot-read benefit, not as permission to weaken the
unchanged 1% budget or bounded target refusal behavior.

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
| `hive-gateway`, REST client, session pool, cache, or broker | QEMU REST `simulate` plus REST `perf` for affected read paths | Gateway status delta, queue/time-out settings, and per-operation errors |
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

### Harness-Managed QEMU and Gateway

Use one unpacked, internally matching release bundle. The harness starts its
QEMU launcher and `hive-gateway`, validates console authentication, runs the
workload, writes the artifacts, and stops the child processes. Do not mix a
bundle launcher with host tools or manifests from another build.

```bash
BENCH_BUNDLE="${BENCH_BUNDLE:?set one matching unpacked release bundle}"
test -x "$BENCH_BUNDLE/qemu/run.sh"
test -x "$BENCH_BUNDLE/bin/hive-gateway"

.venv/bin/python scripts/rest_perf_harness.py \
  --mode simulate \
  --population-mode host-model \
  --bundle "$BENCH_BUNDLE" \
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
  --qemu-log out/bench/qemu-managed.log \
  --gateway-log out/bench/gateway-managed.log \
  --log-dir out/bench \
  --log-prefix qemu-managed-smoke
```

This is a bounded harness smoke workload, not an accepted capacity target.
Check that neither console port nor gateway bind is already owned before the
run; the harness fails closed rather than competing with an existing owner.

### Existing Accepted Target

First establish an accepted QEMU or physical-target boot and a gateway already
backed by that target. `--no-qemu --no-gateway` tells the harness not to launch
or replace either owner:

```bash
test -n "${COH_REST_URL:?set the accepted gateway URL}"

.venv/bin/python scripts/rest_perf_harness.py \
  --mode simulate \
  --population-mode host-model \
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

### Milestone 26e QEMU executable-Worker pressure

Use `executable` only against a live QEMU boot whose gateway projects generated
Worker bounds and imports a matching, same-boot staged component record as
described in [HOST_API.md](HOST_API.md). The harness fails before load unless
the gateway is a connected `console-projection`, the shared validator accepted
the exact current target session, and the three canonical `/shard/<label>/worker/<id>/telemetry`
records are structured READY instances matching that component. It never
expands ids, substitutes `/worker`, or treats reachability as target proof.

The canonical Mac command performs the clean build and runs medium first, then
high against a separate fresh equivalent four-core `virt,gic-version=3` boot:

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
changes; never raise the command to preserve a historical value. A control
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
  --log-prefix candidate-read-path
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

Run each repetition from a fresh target state. The retained M26d pressure
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
mix; it does not directly assert a pure collection capacity. A linear ramp also
need not execute its configured final worker value before time expires.

#### Bounded fixed-cardinality mixed load

Use equal worker minima/maxima and equal intensity minima/maxima, as in the
`candidate-fixed100-i4` example above. Start every repetition from equivalent
fresh state, keep the seed and operation mix fixed, and change only one load
dimension at a time. Use separate prefixes for every candidate.

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
