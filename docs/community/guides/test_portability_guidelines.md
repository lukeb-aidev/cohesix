// CLASSIFICATION: COMMUNITY
// Filename: test_portability_guidelines.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-26

# Test Portability Review

This document summarizes filesystem and environment sensitive operations found in the Rust test suite. Skipping or failing to handle permission errors can cause CI failures on EC2 or other restricted environments.

## Observed Patterns

- **File::open**: `tests/test_capabilities.rs` demonstrates direct file opening with panic on success when permission should be denied.
- **fs::metadata** usages appear in several tests including `9p_validator_hook.rs`, `world_model_sync.rs`, `test_cohcc_output.rs`, `test_capabilities.rs`, and the `codex` test set.
- **fs::read_dir** is called with `unwrap()` in `runtime_safety.rs`.
- Several tests create or use paths under `/dev`, `/srv`, or `/proc` which may not be writable in cloud CI runners.
- `unwrap()` or `expect()` is commonly used after these operations, leading to hard failures on permission denied.

## Recommendations

1. **Permission Checks Before File Access**
   ```rust
   if fs::metadata(path).is_err() {
       println!("\u{1F512} Skipping test: {path} not accessible");
       return;
   }
   ```
   Wrap file opens, directory reads, or metadata calls in checks and skip tests gracefully when they fail due to permissions.
2. **Replace `unwrap()`/`expect()`**
   Use `?` to propagate errors inside `Result`-returning tests, or match on `io::ErrorKind::PermissionDenied` to emit a skip message.
3. **Avoid Hard-Coded Device Paths**
   Tests referencing `/dev`, `/proc`, or `/srv` should first attempt `fs::create_dir_all` and skip on failure. Prefer temporary directories when possible.
4. **Environment Awareness**
   Check for `CI` or similar environment variables when tests require network or privileged operations (e.g., `cohesix_netd.rs` already does this). Use `TMPDIR` or `tempdir()` for scratch space.
5. **Cross‑Platform Portability**
   Ensure tests do not assume root privileges or the presence of `/dev/nvidia0` or similar devices. Provide fallbacks and emit informative skip messages for EC2, GitHub Actions, or containers.

By guarding filesystem calls with runtime checks and avoiding unconditional `unwrap()`/`expect()`, the test suite will be resilient across diverse execution environments.
