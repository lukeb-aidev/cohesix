// CLASSIFICATION: COMMUNITY
// Filename: AGENTS.md v2.2
// Author: Lukas Bower
// Date Modified: 2025-07-22

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
Checks: Validator hooks present. Trace log successfully written.

### Task Title: Check GUI Orchestrator Compliance
Description: Confirm that dev mode disables auth and rate limiting, and that middleware stack is correctly ordered.
Input: go/orchestrator/http/server.go, docs/community/gui_orchestrator.md
Output: log/gui_check.md
Checks: All required middleware are registered in the correct order. Dev mode bypasses auth logic.

## Related Documents
Include references to supporting files to help Codex agents resolve context. Minimum recommended:
docs/community/INSTRUCTION_BLOCK.md
docs/community/END_USER_DEMOS.md
docs/community/COMMERCIAL_PLAN.md

## Execution Notes
Codex is automatically triggered by GitHub Actions.
Agent output is written to log/codex_output.md.
Build fails if any agent task fails or emits a warning.
