// CLASSIFICATION: COMMUNITY
// Filename: TEST_REFACTOR_GUIDE.md v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-31

# Plan9 Test Refactor Guide

The following tests remain but assume POSIX semantics or Linux paths. They
should be updated for the Plan9 + Secure9P environment:

- `tests/codex/cli_tools_test.rs` – uses `std::os::unix::fs::PermissionsExt` to
  inspect executable bits. Replace with Plan9 metadata checks via 9P.
- `tests/codex/dummy_test.rs` – also relies on Unix permission bits when
  verifying `cohcli.py` is executable. Use Plan9-friendly permission queries.
- `tests/test_compile_trace.rs` – sets directory permissions using
  `PermissionsExt` and assumes Unix-style modes. Adapt to 9P attribute updates.
- `tests/test_cohcc_output.rs` – writes a binary and sets permissions via
  `PermissionsExt`. Use Plan9 ACL or 9P walk to mark executables.
- `tests/test_qemu_boot.rs` – hardcodes `/usr/bin/qemu-system-x86_64`.
  Parameterize the QEMU path via environment variable for cross-platform runs.
- `tests/contracts.rs` – determines privilege using `libc::geteuid`. Replace
  with role or capability checks that work on Plan9.
- `tests/test_syscall_queue.rs` – uses `libc::geteuid` to verify root access.
  Update to Plan9 role-based permissions.
- `tests/test_namespace_semantics.rs` – checks `/srv` writability using
  `geteuid`. Rework to rely on Secure9P role policy instead.

These tests compile today but will fail or be skipped on Plan9 until refactored.
