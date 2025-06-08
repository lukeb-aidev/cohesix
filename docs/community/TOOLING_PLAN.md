// CLASSIFICATION: COMMUNITY
// Filename: TOOLING_PLAN.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

# Tooling Plan for Batch Support

This document describes helper scripts and tools enabling Codex-driven mega batches.

## 1. Batch Validation Scripts
- `tools/validate_batch.sh` — checks file headers and structural integrity for each checkpoint.
- `tools/annotate_batch.py` — adds `BATCH_SIZE` and `BATCH_ORIGIN` to document headers and updates METADATA.md.

## 2. Hydration Replay
- `tools/replay_batch.sh` — replays hydration logs if a batch is interrupted.
- `tools/simulate_batch.sh` — creates mock batches for testing purposes.

## 3. Performance Logging
- `tools/perf_log.sh` collects compile time, test runtime, and boot duration metrics on Jetson Orin Nano and AWS EC2.

