// CLASSIFICATION: COMMUNITY
// Filename: CHANGELOG.md v0.9
// Date Modified: 2025-06-17
// Author: Lukas Bower

## [v0.10] - 2025-06-17
### Removed
- Removed obsolete entries like `.DS_Store` from `filetree.txt`.
### Changed
- Regenerated `filetree.txt` based on tracked files only.

## [v0.9] - 2025-06-16
### Added
- **README**: document running Go tests via `make go-test` (workspace in `go/`).

## [v0.8] - 2025-06-15
### Added
- **BATCH_PLAN.md**: added cloud-native hooks deliverable for Queen roles; version bumped to v0.5.
- **METADATA.md**: updated entry for BATCH_PLAN.md to v0.5.

# Changelog for Cohesix

## [v0.8] - 2025-06-11
### Changed
- **BATCH_PLAN.md**: expanded batch entries with bullet-level deliverables and demo heading; version bumped to v0.5.
- **METADATA.md**: version bumped to v2.1 with updated BATCH_PLAN entry.

## [v0.7] - 2025-06-10
### Added
- **BATCH_PLAN.md**: new section for Demo Integration Batches; version bumped to v0.4.
- **METADATA.md**: updated to v2.0 with revised BATCH_PLAN entry.

## [v0.6] - 2025-06-05
### Added
- **END_USER_DEMOS.md**: documented showcase scenarios for Queen–Worker demos.
- **METADATA.md**: version bumped to v1.9 with new entry for END_USER_DEMOS.md.

## [v0.5] - 2025-06-05
### Fixed
- `METADATA.md` entry for `BATCH_PLAN.md` now reflects version `v0.3`.

## [v0.4] - 2025-06-02
### Added
- **cohesix-9p**: Integrated `p9` crate and added `parse_version_message` helper.
- **Dependencies**: Documented `p9` crate in `DEPENDENCIES.md` and `OSS_REUSE.md`.
- **Tests**: Added unit test for version message parsing.

## [v0.4] - 2025-06-05
### Changed
- **BATCH_PLAN.md**: added agent references and watchdog note; version bumped to v0.3.

// CLASSIFICATION: COMMUNITY
// Filename: CHANGELOG.md v0.2
// Date Modified: 2025-05-27
// Author: Lukas Bower


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