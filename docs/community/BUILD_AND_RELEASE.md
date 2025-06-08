// CLASSIFICATION: COMMUNITY
// Filename: BUILD_AND_RELEASE.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-07-12

# Build and Release Plan

This guide covers core build steps and the criteria for a v1.0-alpha release.

## Build Overview
1. Install the Rust (≥1.76), Go (≥1.21), and Python (≥3.10) toolchains.
2. Run `cargo build --release` and `./test_all_arch.sh` for cross-arch tests.
3. Use Docker Buildx for reproducible multi-arch images.
4. Compile BusyBox and Plan 9 services for aarch64.
5. Assemble the OS image via `scripts/assemble_image.sh`.

## Release Milestones
- **Compiler:** IR operations, optimization passes, and codegen validated with ≥80% test coverage.
- **OS Runtime:** seL4 boots with `/srv/cohrole`; physics and GPU services run sample workloads.
- **Tooling:** BusyBox, SSH, man pages, and logging utilities operational.
- **AI/Codex:** Agents in `AGENTS_AND_CLI.md` validated and audit logs captured.

All criteria must pass on aarch64 and x86_64 before tagging a release.
