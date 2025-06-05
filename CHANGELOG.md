// CLASSIFICATION: COMMUNITY
// Filename: CHANGELOG.md v0.4
// Date Modified: 2025-06-04
// Author: Lukas Bower

# TODO: Draft content for CHANGELOG.md.

// CLASSIFICATION: COMMUNITY
// Filename: CHANGELOG.md v0.2
// Date Modified: 2025-05-27
// Author: Lukas Bower

# Changelog for Cohesix

All notable changes to this project are documented in this file.  
Version bumps correspond to updates of canonical documents and major subsystem scaffolding.

## [v0.2] - 2025-05-27
### Changed
- **BATCH_PLAN.md**: version bumped to v0.2; added OS, tooling, Codex, testing, and documentation batch entries; updated status legend icons.  
- **BUILD_PLAN.md**: created detailed build plan with Docker multi-arch, BusyBox, reproducibility, and CI integration.  
- **docs/security/**: fleshed out `SECURITY_POLICY.md` and `THREAT_MODEL.md` with full policies and threat modeling.  
- **tests/**: fully implemented integration tests for IR, passes, CLI, Codex artifacts, and library entry points.  
- **src/**: hydrated all core modules (`ir`, `pass_framework`, `passes`, `codegen`, `cli`, `utils`, `dependencies.rs`) with real implementations and stubs.  
- **.github/workflows/**: stubbed `ci.yml` and `codex.yml`.  
- **.gitignore**: added common ignores for artifacts and helper files.  

### Added
- **CHANGELOG.md** initial content and versioning.  
- **.github/workflows**: CI and Codex workflow stubs.  
- **scripts/**: stubbed build and helper scripts (`build-busybox.sh`, `deploy-ci.sh`, `heartbeat-check.sh`, `package-manager-stub.sh`, `run-smoke-tests.sh`).  
- **cli/cohcli.py**: placeholder for Python CLI orchestrator.

## [v0.4] - 2025-06-04
### Added
- **AGENTS.md**: introduced `batch` field and negative test cases for each agent.
### Changed
- Bumped AGENTS schema example and documentation version numbers.

## [v0.3] - 2025-06-01
### Added
- **HAL**: architecture stubs `src/hal/arm64`, `src/hal/x86_64`, and facade `src/hal/mod.rs`.
- **Bootloader**: argument parser, early‑init, and module wiring (`bootloader/{args,init,mod.rs}`); secure‑boot measurement helper (`boot/measure.rs`).
- **C shim**: `c/sel4/shim/boot_trampoline.c` + header, with Makefile `c-shims` target.
- **Go workspace**: `go/` tree with `cmd/coh-9p-helper`, `internal/tooling`, `go.mod`, and `go.work`.
- **Scripts**: updated `build-busybox.sh`, `deploy-ci.sh`, `heartbeat-check.sh`, `package-manager.sh`, `run-smoke-tests.sh`; new scaffold scripts for Go & C.
- **Makefile**: v0.2 top‑level build targets (`all`, `go-build`, `c-shims`, `help`).
- **Utilities**: `utils/format.rs` (human-readable bytes, middle‑truncate) and `utils/helpers.rs` (hex dump, sleep_ms).
- **Worker**: new `worker/cli.rs`; upgraded `worker/args.rs` and `worker/mod.rs`.

### Changed
- Version bumps and hydration across core docs and code to ensure single classification headers.
- Removed duplicate legacy code blocks causing rust‑analyzer syntax errors.

### Removed
- Duplicate stubs and outdated headers in HAL, Worker, and C shim sources.

## [v0.1] - 2025-05-24
### Added
- Initial scaffolding for canonical documents per `INSTRUCTION_BLOCK.md` v3.4.  
- Stub files for compiler, OS, tooling, and tests generated via `scripts/create_stubs.sh`.  
- `README.md`, `METADATA.md`, `INSTRUCTION_BLOCK.md`, and core docs versioned and initialized.  
- Initial automation scripts stubbed.  

---

> _For full history prior to v0.1 refer to archived batches in `/canvas/archive/`._