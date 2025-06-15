// CLASSIFICATION: COMMUNITY
// Filename: VALIDATION_AND_TESTING.md v1.5
// Author: Lukas Bower
// Date Modified: 2025-07-23

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
  - `Ensemble Agent Tests:` create temp directories using `COHESIX_ENS_TMP` or system temp paths; validate cleanup and path safety.

## CI Hooks
- `scripts/validate_metadata_sync.py` ensures document headers match `METADATA.md`
- `tools/validate_batch.sh` checks file structure after each checkpoint
- `scripts/collect_boot_logs.sh` uploads logs from Jetson Orin Nano and Raspberry Pi 5

## Batch Testing
`tools/simulate_batch.sh` can create a mock batch. Replay with `tools/replay_batch.sh` to verify recovery. Confirm `CODEX_BATCH: YES` appears in generated metadata.

  - `test_boot_efi` now includes a check for QEMU presence and creates `out/` and `tmp/` directories dynamically to avoid runtime errors.
  - If QEMU is missing, the boot test logs a warning and exits with status 0 so CI marks the step as skipped.

Adhering to these practices keeps Cohesix robust and ready for demo-critical deployments.

## Alpha Validation Issues
- `cohcc` binary missing from build artifacts; CLI docs reference a non-existent executable.
- `cohcli` utility not present or installed—`cohcli --version` fails.
- Agent lifecycle tests run, but CLI coverage and hardware boot traces are unavailable.
- Documentation mismatches: man pages mention commands not implemented.
- All TODO markers have been removed from `src/cohcc/ir/mod.rs`, satisfying the no-stub policy.

- FIXME markers remain in `src/cohcc/ir/mod.rs`, violating the no-stub policy.
- Boot and hardware validation logs missing for Jetson and Pi targets.
  - Ensemble agent tests previously failed due to hardcoded temp paths; now fixed via env-based temp directory configuration.
  - Boot script `test_boot_efi` failed without QEMU installed—validation updated to check for `qemu-system-x86_64`.

## Batch Hydration Test Plan
1. Run `tools/simulate_batch.sh` to create a 15-file batch and force a crash after file 7.
2. Replay the hydration log with `tools/replay_batch.sh` and verify all files hydrate correctly.
3. Execute `validate_metadata_sync.py` after replay.
4. Run `test_all_arch.sh` to ensure cross-arch success.
5. Confirm `CODEX_BATCH: YES` appears in logs and prior batches replay identically from `/history/`.
