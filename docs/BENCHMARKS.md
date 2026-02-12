<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document Cohesix hive-gateway worker-scaling benchmark status and evidence for v0.7.0-alpha. -->
<!-- Author: Lukas Bower -->
# Cohesix Benchmarks

## Hive-Gateway Worker Scaling (v0.7.0-alpha)

### Executive Summary
- Objective: identify where control-plane performance degrades as worker count scales through `hive-gateway` on a real VM path (not `--mock`).
- Validation gate passed before load tests: QEMU boot, TCP reachability on `127.0.0.1:31337`, gateway auth, and `LS /`.
- Buffer limits were increased and regenerated through `coh-rtc` before final scaling runs.
- The original apparent `64`-worker ceiling was a harness measurement artifact (`/worker` listing saturation), not a hard Cohesix capacity limit.
- After harness worker tracking fixes, Cohesix reached and sustained **100 workers** in a fixed-100 validation run.
- Under a more aggressive activity profile (same topology and concurrency controls, higher intensity), control-plane timeout onset moved earlier.

### Benchmark Methodology

#### 1) Research Question
What is the maximum worker count Cohesix v0.7.0-alpha can sustain through `hive-gateway` before control-plane behavior degrades, and does higher activity surface failures earlier?

#### 2) Testbed and System Under Test
- Host: macOS ARM64.
- Target: QEMU `aarch64/virt`.
- Release bundle: `out/bench-cap100-buf256/bundle`.
- Definitive QEMU argument source: `releases/Cohesix-0.7.0-alpha-MacOS/qemu/run.sh`.
- QEMU SMP topology used: `4,cores=4,threads=1,sockets=1`.
- Harness: `scripts/rest_perf_harness.py` in `--mode simulate`.
- Gateway transport: authenticated TCP console path via `hive-gateway` (`127.0.0.1:31337`).

#### 3) Preflight Gates (Required Before Benchmark)
Each benchmark run is considered valid only if preflight proves:
- VM boot reached userspace/root-task.
- TCP console listener is reachable on `127.0.0.1:31337`.
- Gateway reaches authenticated Queen session.
- Filesystem request path is operational (`LS /` returns `OK`).

Preflight artifacts:
- `logs/preflight_final_result_20260212T021300Z.txt`
- `logs/qemu_final_preflight_20260212T021300Z.log`
- `logs/gateway_final_preflight_20260212T021300Z.log`

#### 4) Workload Model (`scripts/rest_perf_harness.py`)
- Worker ramp: linear from `workers-min` to `workers-max` over `duration-mins`, updated every `ramp-step-secs`.
- Request rate equation:
  - `rps = base_rps * intensity * active_workers`
- Concurrency cap:
  - `max_inflight` bounds concurrent in-flight REST operations.
- Operation mix:
  - Reads: `LS`, `CAT`, telemetry tails.
  - Control writes: schedule writes, lease grant/preempt/quota.
  - Policy and telemetry writes when available in the namespace.
- Worker identities:
  - Harness keeps synthetic worker IDs when `/worker` listing saturates, avoiding false worker-ceiling conclusions.

#### 5) Degradation Definitions
- Soft degradation: first control-plane `buffer-full` quota pressure event during ramp.
- Hard degradation: first `timed out` error in control-plane operations.
- Run failure: uncaught exception terminating the run before normal completion.

#### 6) Fairness and Comparison Rules
- All A/B comparisons keep these fixed unless explicitly changed:
  - same release bundle
  - same QEMU SMP topology
  - same `workers=8..100`, `duration=3m`, `ramp-step=10s`
  - same `base-rps=1.0`, `max-inflight=64`, `tail-bytes=32768`
- Aggressive test changes only average activity (`intensity=6` vs `intensity=4` baseline).

### Buffer Changes Applied
| Scope | Setting | Before | After |
| --- | --- | --- | --- |
| `configs/root_task.toml` | `control_plane.schedule.queue_max_entries` | `128` | `256` |
| `configs/root_task.toml` | `control_plane.lease.active_max_entries` | `128` | `256` |
| `configs/root_task.toml` | `control_plane.lease.preemptions_max_entries` | `256` | `256` (kept at max) |
| `configs/root_task.toml` | `ecosystem.policy.queue_max_entries` | `64` | `256` |
| `tools/coh-rtc/src/ir.rs` | `MAX_POLICY_QUEUE_ENTRIES` | `64` | `256` |

Notes:
- `ecosystem.policy.queue_max_bytes` remains `8192` because it is bounded by `secure9p.msize`.
- `secure9p.msize` remains `8192` (charter red-line preserved).

### Run Matrix

#### Run A: Unpatched Harness (False 64 Cap)
Purpose:
- Establish initial 8..100 ramp behavior before harness worker tracking fix.

Profile:
- `--workers-min 8 --workers-max 100 --intensity-min 4 --intensity-max 4 --duration-mins 3`

Evidence:
- Harness: `logs/rest_bench_buf256_live_w8_100_20260212T020035Z.log`
- QEMU: `logs/qemu_buf256_live_w8_100_20260212T020035Z.log`
- Gateway: `logs/gateway_buf256_live_w8_100_20260212T020035Z.log`

Result:
- `worker capacity reached at 64 (buffer full)` (false cap due `/worker` listing saturation)
- `overall ops=17937 ok=17542 err=395 avg=0.176s p95=0.006s`

#### Run B: Patched Harness Ramp (8..100, Baseline Activity)
Patch impact:
- `scripts/rest_perf_harness.py` continues worker tracking past `/worker` listing saturation.

Profile:
- `--workers-min 8 --workers-max 100 --intensity-min 4 --intensity-max 4 --duration-mins 3`

Evidence:
- Harness: `logs/rest_bench_final_w8_100_20260212T021309Z.log`
- QEMU: `logs/qemu_final_live_w8_100_20260212T021309Z.log`
- Gateway: `logs/gateway_final_live_w8_100_20260212T021309Z.log`

Result:
- Reached `95` workers during ramp with no new hard worker-cap event.
- `overall ops=16817 ok=16424 err=393 avg=0.157s p95=0.006s`
- Degradation signals under this profile appeared in control-plane writes near top-end:
  - `schedule_write: ops=436 ok=391 err=45 avg=3.908s p95=10.363s`
  - `lease_preempt: ops=351 ok=260 err=91 avg=2.544s p95=10.191s`

#### Run C: Fixed 100 Worker Confirmation
Purpose:
- Validate that worker count can actually reach and hold 100 once harness artifact is removed.

Profile:
- `--workers-min 100 --workers-max 100 --intensity-min 1 --intensity-max 1 --duration-mins 1 --no-cleanup`

Evidence:
- Harness: `logs/rest_bench_final_fixed100_20260212T021653Z.log`
- QEMU: `logs/qemu_final_fixed100_20260212T021653Z.log`
- Gateway: `logs/gateway_final_fixed100_20260212T021653Z.log`

Result:
- Confirmed worker creation through `worker-100`.
- Sustained `workers=100` through the 1-minute run.
- `overall ops=4236 ok=4097 err=139 avg=0.002s p95=0.004s`

#### Run D: Aggressive Activity A/B (8..100, Same Configuration, Higher Intensity)
Purpose:
- Test whether higher average activity surfaces control-plane timeouts earlier.

Profile:
- Same as Run B except `--intensity-min 6 --intensity-max 6`.

Evidence:
- Harness: `logs/rest_bench_final_w8_100_int6_20260212T023045Z.log`
- QEMU: `logs/qemu_final_live_w8_100_int6_20260212T023045Z.log`
- Gateway: `logs/gateway_final_live_w8_100_int6_20260212T023045Z.log`

Result:
- Reached `86` workers (`rps=516.0`) before timeout storm.
- First timeout events appeared at `workers=86`.
- Run aborted with unhandled `TimeoutError` before `[simulate] summary`.

### Results Table (Normalized View)
| Run | Workers target | Intensity | Max workers reached | First `buffer-full` | First `timed out` | Summary emitted |
| --- | --- | --- | --- | --- | --- | --- |
| A (unpatched) | 8..100 | 4 | 64 | 64 | none | yes |
| B (patched baseline) | 8..100 | 4 | 95 | 69 | 95 | yes |
| C (fixed-100 confirm) | 100..100 | 1 | 100 | 100 | none | yes |
| D (aggressive) | 8..100 | 6 | 86 | 64 | 86 | no |

### A/B Comparison: Baseline vs Aggressive (Runs B vs D)
| Metric | Run B (`intensity=4`) | Run D (`intensity=6`) | Delta |
| --- | --- | --- | --- |
| Max workers reached in ramp | 95 | 86 | -9 workers |
| First `buffer-full` onset | 69 workers | 64 workers | -5 workers |
| First timeout onset | 95 workers | 86 workers | -9 workers |
| Timeout onset RPS | `95*4*1.0 = 380` | `86*6*1.0 = 516` | +136 RPS |
| Run completion | summary emitted | aborted pre-summary | less stable |

Interpretation:
- Higher activity causes earlier worker-count failure onset in control-plane terms (both soft and hard thresholds shift left).
- Even with earlier onset by worker count, the aggressive run still drives higher absolute request pressure at failure (516 RPS vs 380 RPS).

### Graph 1: Worker Ramp Achieved (Runs B, C, D)
```text
workers
100 |                                   C:████████████████████
 95 |                      B:███████████████████
 90 |                      B:██████████████████
 86 |                      D:█████████████████
 80 |                      B,D:███████████████
 70 |                      B,D:█████████████
 60 |                      B,D:███████████
 50 |                      B,D:█████████
 40 |                      B,D:███████
 30 |                      B,D:█████
 20 |                      B,D:███
 10 |                      B,D:█
    +---------------------------------------------------------
       B=patched baseline ramp, D=aggressive ramp, C=fixed-100 confirmation
```

### Graph 2: Degradation Onset Thresholds (Runs B vs D)
```text
              60    65    70    75    80    85    90    95   100 workers
Run B (i4):        B------------------------------T
Run D (i6):   B-------------------------T

B = first buffer-full
T = first timed out
```

### Evidence Index
- Preflight:
  - `logs/preflight_final_result_20260212T021300Z.txt`
  - `logs/qemu_final_preflight_20260212T021300Z.log`
  - `logs/gateway_final_preflight_20260212T021300Z.log`
- Run A:
  - `logs/rest_bench_buf256_live_w8_100_20260212T020035Z.log`
  - `logs/qemu_buf256_live_w8_100_20260212T020035Z.log`
  - `logs/gateway_buf256_live_w8_100_20260212T020035Z.log`
- Run B:
  - `logs/rest_bench_final_w8_100_20260212T021309Z.log`
  - `logs/qemu_final_live_w8_100_20260212T021309Z.log`
  - `logs/gateway_final_live_w8_100_20260212T021309Z.log`
- Run C:
  - `logs/rest_bench_final_fixed100_20260212T021653Z.log`
  - `logs/qemu_final_fixed100_20260212T021653Z.log`
  - `logs/gateway_final_fixed100_20260212T021653Z.log`
- Run D:
  - `logs/rest_bench_final_w8_100_int6_20260212T023045Z.log`
  - `logs/qemu_final_live_w8_100_int6_20260212T023045Z.log`
  - `logs/gateway_final_live_w8_100_int6_20260212T023045Z.log`

### Reproducibility Commands
```bash
# Baseline ramp (Run B)
python3 scripts/rest_perf_harness.py \
  --mode simulate \
  --bundle out/bench-cap100-buf256/bundle \
  --qemu-smp '4,cores=4,threads=1,sockets=1' \
  --workers-min 8 --workers-max 100 \
  --intensity-min 4 --intensity-max 4 \
  --duration-mins 3 --ramp-step-secs 10 \
  --base-rps 1.0 --max-inflight 64 --tail-bytes 32768 \
  --qemu-log logs/qemu_final_live_w8_100_$(date -u +%Y%m%dT%H%M%SZ).log \
  --gateway-log logs/gateway_final_live_w8_100_$(date -u +%Y%m%dT%H%M%SZ).log \
  --log-prefix rest_bench_final_w8_100

# Aggressive activity A/B (Run D)
python3 scripts/rest_perf_harness.py \
  --mode simulate \
  --bundle out/bench-cap100-buf256/bundle \
  --qemu-smp '4,cores=4,threads=1,sockets=1' \
  --workers-min 8 --workers-max 100 \
  --intensity-min 6 --intensity-max 6 \
  --duration-mins 3 --ramp-step-secs 10 \
  --base-rps 1.0 --max-inflight 64 --tail-bytes 32768 \
  --qemu-log logs/qemu_final_live_w8_100_int6_$(date -u +%Y%m%dT%H%M%SZ).log \
  --gateway-log logs/gateway_final_live_w8_100_int6_$(date -u +%Y%m%dT%H%M%SZ).log \
  --log-prefix rest_bench_final_w8_100_int6
```

### Threats to Validity
- Stochastic workload: seed was not pinned in these runs (`--seed` unset), so micro-level op sequences vary.
- Single-trial comparison per profile: stronger confidence requires repeated runs with fixed seeds.
- Run D terminated pre-summary: full per-op latency distributions for that run were not emitted.
- Transport retries and policy-denied events are part of realistic behavior, but they can mask isolated subsystem bottlenecks.

### Conclusion
- A real benchmark was completed against the live VM TCP/auth path.
- The previous apparent `64` worker cap was a harness artifact, not the true system ceiling.
- Maximum demonstrated worker count is **100 workers sustained** (Run C).
- In ramp tests, timeout onset occurred at:
  - `95` workers for baseline intensity (`4`)
  - `86` workers for aggressive intensity (`6`)
- Increased average activity causes control-plane timeout behavior to surface earlier by worker count.
