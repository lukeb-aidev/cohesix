// CLASSIFICATION: COMMUNITY
// Filename: RELEASE_AND_BATCH_PLAN.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-06-20

# Release and Batch Plan

This document merges the release criteria and the rolling batch plan used for Cohesix development.

## Release Milestones
1. **Compiler** – IR core, optimization passes, codegen via `cohcc`. ≥80 % test coverage across both architectures.
2. **OS Runtime** – seL4 boot with CohRole init, Plan 9 services, `/sim/` and `/srv/cuda` validated by CI.
3. **Tooling** – BusyBox utilities, SSH, `man`, package stub, monitoring tools.
4. **AI/Codex** – Agents defined in `AGENTS.md`; smoke tests run via `pytest tests/codex/`.

Success requires passing all tests on aarch64 and x86_64 using `test_all_arch.sh`.

## Batch Overview
Batches are grouped by component and executed in order. Examples:
- **C1–C6** – Compiler and Codex enablement
- **O1–O9** – OS boot, services, and cloud hooks
- **T1–T8** – CLI and tooling
- **D1–D6** – Demo integration
- **X1–X2** – Testing infrastructure

Each batch defines deliverables, dependencies, and CI commands. Checkpoints occur every 10 files with logs stored in `codex_logs/`. See `BATCH_PLAN.md` for the full table of tasks.

Upcoming work includes aligning METADATA versions, updating CHANGELOG entries, and running matrix CI after major document updates.
