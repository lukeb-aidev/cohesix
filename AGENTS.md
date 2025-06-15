// CLASSIFICATION: COMMUNITY
// Filename: AGENTS.md v2.2
// Author: Lukas Bower
// Date Modified: 2025-06-15

# Codex Agent Tasks
This file contains Codex-executable tasks for the Cohesix system.

## Task Format
Each task must include the following fields:
Task Title
Description: What Codex should do
Input: Source files or directories
Output: Output paths or logs
Checks: How Codex should verify success

## Example Tasks

### Task Title: Validate Kernel Hooks
Description: Check that kernel and namespace source files include boot and trace validation hooks.
Input: src/kernel/, src/namespace/
Output: log/kernel_trace_results.md
Checks: Validator hooks present. Trace log written to TMPDIR-respecting path. No hardcoded system paths used.
### Task Title: Verify QEMU Boot Script Robustness
Description: Ensure `test_boot_efi.sh` checks for QEMU availability, sets TMPDIR, and creates required writable directories.
Input: test/test_boot_efi.sh
Output: log/qemu_script_check.md
Checks: QEMU presence verified. TMPDIR is initialized. No hardcoded paths. Script exits cleanly if QEMU is missing.

### Task Title: Check Temp Path Compliance
Description: Ensure that all Rust tests and runtime modules respect TMPDIR, COHESIX_TRACE_TMP, or COHESIX_ENS_TMP where appropriate.
Input: tests/, src/
Output: log/temp_path_check.md
Checks: All temporary paths use environment variables or OS tempdir. No /tmp or /dev/shm hardcoding.

### Task Title: Check GUI Orchestrator Compliance
Description: Confirm that dev mode disables auth and rate limiting, and that middleware stack is correctly ordered.
Input: go/orchestrator/http/server.go, docs/community/gui_orchestrator.md
Output: log/gui_check.md
Checks: All required middleware are registered in the correct order. Dev mode bypasses auth logic.

### Task Title: Validate Trace Snapshot Emission
Description: Confirm that CLI and runtime operations emit trace snapshots to the expected location under COHESIX_TRACE_TMP or TMPDIR.
Input: src/, cli/, tools/
Output: log/trace_snapshot_check.md
Checks: Snapshot files emitted. Trace logs present. Paths respect environment constraints.

## Related Documents
Include references to supporting files to help Codex agents resolve context. Minimum recommended:
docs/community/INSTRUCTION_BLOCK.md
docs/community/END_USER_DEMOS.md
docs/community/COMMERCIAL_PLAN.md

## Execution Notes
Codex is automatically triggered by GitHub Actions.
Agent output is written to log/codex_output.md.
Build fails if any agent task fails or emits a warning.

Codex executes in a restricted environment. All agent tasks must:
- Avoid network fetches unless explicitly permitted
- Use TMPDIR-respecting writable paths
- Avoid absolute paths or root-only directories
- Avoid spawning background threads or processes that persist after task completion
