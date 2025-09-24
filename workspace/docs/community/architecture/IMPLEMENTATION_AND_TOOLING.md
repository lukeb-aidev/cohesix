// CLASSIFICATION: COMMUNITY
// Filename: IMPLEMENTATION_AND_TOOLING.md v1.2
// Author: Lukas Bower
// Date Modified: 2029-01-26

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
- `tools/validate_batch.sh` verifies document headers at each checkpoint.
- `tools/annotate_batch.py` populates `BATCH_SIZE` and `BATCH_ORIGIN` fields.
- `tools/replay_batch.sh` replays hydration logs after failures.
- `tools/simulate_batch.sh` creates mock batches for testing.
- `tools/perf_log.sh` records build and boot timings on Orin and EC2.
- `tools/trace_diff.py` compares trace snapshots and highlights rule drift, validator regressions, or telemetry mismatches.

These tools ensure repeatable, audited automation while keeping the runtime lean and portable across aarch64 and x86_64.

## Batch Tooling Usage

### `tools/validate_batch.sh`
*Purpose:* Ensure every Markdown or text artifact retains the Cohesix metadata header (`CLASSIFICATION`, `Filename`, `Author`, and `Date Modified`).

*Prerequisites:* `python3`, a writable directory exported via `TMPDIR`, `COHESIX_TRACE_TMP`, or `COHESIX_ENS_TMP`.

*Example:*
```bash
tools/validate_batch.sh workspace/docs/private workspace/docs/community
```
Use `--strict` to fail the job when warnings (for example, mismatched filenames) are detected and `--extensions` to validate non-Markdown formats.

### `tools/annotate_batch.py`
*Purpose:* Populate metadata tables (such as `workspace/docs/community/governance/METADATA.md`) with authoritative `BATCH_SIZE` and `BATCH_ORIGIN` values.

*Example:*
```bash
python3 tools/annotate_batch.py --metadata workspace/docs/community/governance/METADATA.md \
  --origin codex://batch/2029-01-26 docs/community/architecture/IMPLEMENTATION_AND_TOOLING.md \
  docs/community/guides/AGENTS_AND_CLI.md
```
If `--size` is omitted, the script uses the number of entries provided. Updates are written atomically to TMPDIR-aware scratch space before being moved into place.

### `tools/simulate_batch.sh`
*Purpose:* Generate representative documentation, manifests, and hydration logs for dry runs. Each invocation creates a directory tree containing `docs/`, `staging/`, `manifest.json`, `METADATA.md`, and `hydration.log`.

*Example:*
```bash
SIM_DIR=$(mktemp -d)
tools/simulate_batch.sh --size 3 --origin mock://smoke --outdir "$SIM_DIR"
```
The resulting hydration log can be consumed by `tools/replay_batch.sh`, while the generated metadata table can be annotated with `tools/annotate_batch.py` during CI rehearsals.

### `tools/replay_batch.sh`
*Purpose:* Execute the actions recorded in hydration logs. Supported operations include `CWD`, `ENV`, `RUN`, `COPY`, and `SLEEP` entries separated by the pipe (`|`) delimiter.

*Example:*
```bash
tools/replay_batch.sh "$SIM_DIR/hydration.log"
```
Use `--dry-run` to preview actions and `--base-dir` when log paths are relative to a shared directory captured during hydration.

### `tools/perf_log.sh`
*Purpose:* Capture build and boot timings while wrapping existing automation such as `scripts/boot_qemu.sh`. Results are written as JSON alongside stage-specific logs.

*Example:*
```bash
tools/perf_log.sh --build-cmd "ninja -C build" \
  --boot-cmd "scripts/boot_qemu.sh" --tag nightly
```
Each stage log (for example, `nightly_build.log`) records stdout/stderr. The JSON summary captures status, duration in milliseconds, exit codes, and absolute log paths. Supply `--skip-build` or `--skip-boot` to focus on a single phase.

### `tools/trace_diff.py`
*Purpose:* Compare validator snapshots stored under `/history/snapshots` (or a custom directory) and highlight additions, removals, or content differences.

*Example:*
```bash
python3 tools/trace_diff.py baseline_20240101 baseline_20240115 \
  --snapshots-dir /history/snapshots --output /tmp/trace_diff.txt
```
Set `--fail-on-diff` to fail CI when deviations are detected. The generated summary includes unified diffs for textual files and binary-change markers when raw bytes differ.

## Integrated Smoke Coverage

`scripts/run-smoke-tests.sh` now executes `tools/simulate_batch.sh`, validates the generated documents, replays the hydration log, and runs a focused pytest suite (`tests/test_batch_tools.py`). Shell linting for the batch helpers is enforced via `shellcheck` during the same smoke pass.
