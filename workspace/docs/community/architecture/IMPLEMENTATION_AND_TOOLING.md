// CLASSIFICATION: COMMUNITY
// Filename: IMPLEMENTATION_AND_TOOLING.md v1.1
// Author: Lukas Bower
// Date Modified: 2026-12-31

# Implementation and Tooling

This document summarizes the Cohesix runtime architecture and the tooling used to support large Codex batches.

## Runtime Overview
- **Kernel:** seL4 with Cohesix patches, booted by a minimal loader
- **Startup:** <200 ms to reach Plan 9 userland; telemetry logged during init
- **Roles:** QueenPrimary orchestrates workers such as DroneWorker and KioskInteractive
- **Services:** `/srv/` hosts modular services including `cuda`, `telemetry`, `sandbox`, `trace`, and `agent`
- **Namespace:** Workers overlay the Queen’s namespace using 9P mounts
- **Validator:** embedded rule engine intercepts syscalls and records traces
- **Trace Snapshots:** Captured trace state is saved to `/history/snapshots/` for validator and CI replay

## Tooling Highlights
- `tools/validate_batch.sh` verifies document headers at each checkpoint
- `tools/annotate_batch.py` populates `BATCH_SIZE` and `BATCH_ORIGIN` fields
- `tools/replay_batch.sh` replays hydration logs after failures
- `tools/simulate_batch.sh` creates mock batches for testing
- `tools/perf_log.sh` records build and boot timings on Orin and EC2
- `tools/trace_diff.py` compares trace snapshots and highlights rule drift, validator regressions, or telemetry mismatches

These tools ensure repeatable, audited automation while keeping the runtime lean and portable across aarch64 and x86_64.
