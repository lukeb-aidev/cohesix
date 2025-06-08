// CLASSIFICATION: COMMUNITY
// Filename: VALIDATION_AND_TESTING.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-07-12

# Validation and Testing

Cohesix uses a multi‑tiered testing strategy enforced via CI on ARM and x86 platforms.

## Test Levels
- **Bootloader:** unit and smoke tests using QEMU snapshots.
- **Kernel (seL4):** property tests and fuzzing for syscalls and 9P handlers.
- **Core OS:** unit tests for `rc` shell and 9P servers; end‑to‑end boot images.
- **Compiler:** IR passes and codegen tested with regression harnesses.
- **Tooling:** CLI parsing, remote build simulation, and Go unit tests.

CI scripts run `cargo test`, `go test`, `pytest`, and demo scripts via `test_all_arch.sh`.

## Watchdog Policy
A 15‑minute watchdog monitors hydration batches. Heartbeats every 5 min keep tasks alive. On timeout, the agent restarts, validates metadata, and resumes from the last checkpoint. Logs are retained for 30 days.

## Security Review Summary
- Minimal seL4 patches preserve formal proofs.
- Capabilities map to 9P tokens (`CohCap`).
- OWASP Top Ten checks run in CI with container scanning and `cargo audit`.

## Batch Testing Plan
`tools/simulate_batch.sh` creates a 15‑file batch, intentionally crashes after file 7, and replays logs with `tools/replay_batch.sh`. After replay, run `validate_metadata_sync.py` and `test_all_arch.sh` to ensure consistency.

## Trace Consensus
Peer Queens exchange trace segments and store them under `/srv/trace/consensus/`. Divergence raises a `ConsensusError` for manual review.
