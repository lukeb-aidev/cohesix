// CLASSIFICATION: COMMUNITY
// Filename: ALPHA_VALIDATION_ISSUES.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-13

# Alpha Release Validation Issues

This document captures unresolved issues discovered during the final sanity and resilience validation. These items block declaring `v1.0-alpha` until resolved.

- `cohcc` binary missing from build artifacts. CLI docs mention the compiler, but no executable is produced.
- `cohcli` utility not present or installed; `cohcli --version` fails.
- Agent lifecycle tests pass but CLI coverage and hardware boot traces not available.
- Documentation mismatches: CLI manpages describe commands not implemented.
- TODO markers remain in `src/cohcc/ir/mod.rs`, violating "No Stubs" policy.
- Boot and hardware validation logs absent for Jetson and Pi targets.

