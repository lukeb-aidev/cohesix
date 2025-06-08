// CLASSIFICATION: COMMUNITY
// Filename: VALIDATION_AND_TESTING.md v1.1
// Author: Lukas Bower
// Date Modified: 2025-07-14

# Validation and Testing

Cohesix uses layered tests and continuous validation to guarantee reliability across all roles and architectures.

## Test Strategy
- **Unit Tests:** `cargo test`, `go test`, and `pytest`
- **Property Tests:** QuickCheck/proptest on kernel and syscall handlers
- **Integration:** boot the OS, mount services, and replay traces
- **Fuzzing:** 9P protocol and syscall mediation with libFuzzer
- **Multi-Arch CI:** run `./test_all_arch.sh` for aarch64 and x86_64
- **Auto-run script:** `scripts/autorun_tests.py` watches for changes and executes tests automatically.
- **Replay Harness:** re-run traces from `/history/` and verify outcomes

## CI Hooks
- `scripts/validate_metadata_sync.py` ensures document headers match `METADATA.md`
- `tools/validate_batch.sh` checks file structure after each checkpoint
- `scripts/collect_boot_logs.sh` uploads logs from Jetson Orin Nano and Raspberry Pi 5

## Batch Testing
`tools/simulate_batch.sh` can create a mock batch. Replay with `tools/replay_batch.sh` to verify recovery. Confirm `CODEX_BATCH: YES` appears in generated metadata.

Adhering to these practices keeps Cohesix robust and ready for demo-critical deployments.
