// CLASSIFICATION: COMMUNITY
// Filename: TEST_PLAN.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

# Test Plan for Codex Batch Hydration

This plan covers validation of Codex-driven large batches. All tests must pass on both aarch64 and x86_64.

## 1. Batch Hydration Simulation
- Run `tools/simulate_batch.sh` to create a 15-file batch.
- Force a crash after file 7 to test checkpoint recovery.
- Replay the hydration log with `tools/replay_batch.sh` and verify all files hydrate correctly.

## 2. CI Integration
- Execute `validate_metadata_sync.py` after replay.
- Run `test_all_arch.sh` ensuring cross-arch success.
- Confirm that files stamped with `CODEX_BATCH: YES` appear in logs.

## 3. Regression
- Ensure previously generated batches remain reproducible by replaying historical logs from `/history/`.

