// CLASSIFICATION: COMMUNITY
// Filename: CHANGELOG.md v0.92
// Date Modified: 2026-02-11
[2025-06-15] Docs Consolidation Pass v1.0
• Merged duplicate security files (THREAT_MODEL.md, Q_DAY.md)
• Consolidated OSS reuse files into LICENSES_AND_REUSE.md
• Unified role documents into ROLE_POLICY.md
• Created CLI and Agent index at cli/README.md
• Normalized headers and metadata across all affected documentation

## [v0.66] - 2025-08-28
### Added
- `rc` init now parses `/etc/init.conf` using `toml` and logs init_mode and services.
- Cargo.toml v0.13 enables the `toml` crate for userland config.

## [vNext] - 2025-08-02
### Temp path fixes and QEMU script guard
- cohesix_fetch_build.sh header updated and registered in METADATA
- QEMU boot log handling runs in background in cohesix_fetch_build.sh
- Ensemble tests use writable temp directory via COHESIX_ENS_TMP
- SharedMemory, BaseAgent use TMPDIR and respect override vars
- QEMU boot script checks for qemu-system-x86_64, sets TMPDIR, ensures writable paths
- test_boot_efi.sh verifies bootx64.efi and FAT directory before launch; logs /out manifest on failure
- cohesix_fetch_build.sh v0.4 stages kernel ELF and boot files in out/
- cohesix_fetch_build.sh clones via SSH without credentials
- src/lib/init.rs placeholder module to satisfy module resolution
- sensors.rs respects COHESIX_TELEMETRY_PATH for tests
- test_agent_lifecycle uses temp directories only
- boot_trace_rule tests use tempfile::tempdir to avoid PermissionDenied errors
### Added
- FAT partition mount under `minimal_uefi` with `/bin/init.efi` bootstrap
- Makefile builds `init-efi` target and copies binary to FAT directory
- `fs::open_bin` API for loading binaries from the FAT root
- full_fetch_and_build.sh v0.1 builds userland EFI binaries into out/bin
- Cargo.toml v0.9 removes unsupported `feature` key in target deps and
  gates getrandom for UEFI builds
- Cargo.toml v0.10 adds `minimal_uefi` feature and gates async crates
- secure_9p_server.rs updated to async entry under `tokio::main`
- Fixed x86_64-unknown-uefi build by gating getrandom and entropy sources
- test_boot_efi.sh v0.13 logs to `logs/` and emits test summaries
- test_all_arch.sh v1.1, run-smoke-tests.sh v0.4 now output logs and summaries
- make_iso.sh script creates bootable ISO under out/cohesix.iso

## [v0.91] - 2025-08-26
### Added
- Kernel `proc_mgr` module providing a minimal userspace process model.
- Unit tests exercising `spawn` and `list` functions.

## [v0.92] - 2025-08-27
### Added
- `userland_bootstrap` module with static dispatch table for built-in user programs.
- `dispatch_user` spawns and executes named entries via `proc_mgr`.
- Boot sequence invokes `dispatch_user("init")` to run the demo init program.

## [v0.93] - 2025-08-27
### Added
- Kernel config reader `load_config` for non-UEFI builds.
- Kernel main logs `/etc/init.conf` when present.

## [v0.94] - 2025-08-27

### Added
- Sample `etc/init.conf` configuration added to repository.
- `full_fetch_and_build.sh` populates `out/etc` with `init.conf` if missing.

## [v0.95] - 2025-08-29
### Changed
- `full_fetch_and_build.sh` calls `make_iso.sh` and checks QEMU ISO boot config.
- `test_boot_efi.sh` boots from `out/cohesix.iso` and validates kernel launch.

## [v0.96] - 2025-08-30
### Added
- Role-specific YAML configs under `configs/roles/`
- `make_iso.sh` and `full_fetch_and_build.sh` copy role configs to `/etc/roles`
- `init` binary loads `/etc/roles/<ROLE>.yaml` with fallback to `default.yaml`
- `src/runtime::role_config` module with unit tests

## [v0.97] - 2025-09-02
### Changed
- Removed legacy UEFI FAT boot logic and staging of `bootx64.efi`.
- QEMU commands now boot strictly from `out/cohesix.iso`.
- `cohesix_fetch_build.sh` builds the ISO and runs QEMU using it.

## [v0.98] - 2025-09-02
### Changed
- Normalized ISO layout in `out/` preserving kernel, bin, roles, and config files.
- `make_iso.sh` copies the structured `out/` tree and sets FAT root.
- `init.efi` reads `/etc/cohesix/config.yaml` with role fallback under `/roles/`.

## [v0.99] - 2025-09-03
### Changed
- `full_fetch_and_build.sh` builds the workspace with `--no-default-features` and
  runs library tests only.
- Runtime role configuration base directory fixed to `/roles`.

## [v0.90] - 2025-08-16
### Changed
- Added `uefi` feature flag and cfg guards for crates using getrandom
- Moved rand, sha2, serde_json and ring under conditional dependencies
- Renamed tools/cli `cohrun` binary to `cohrun_cli` to avoid collisions
- Makefile targets updated accordingly
### [2025-08-16] Misc fixes
- Cargo.toml edition bumped to 2024
- Policy memory shared path respects COHESIX_POLICY_TMP for tests
- cohesix_fetch_build.sh removes -daemonize and validates bootx64.efi
- Makefile fmt target warns if formatters missing
- policy_restore_test writes within tempdir
- Cargo.toml v0.5 adds kernel binary target
- Added src/kernel/main.rs minimal entry for ELF boot
 - cohesix_fetch_build.sh v0.6 builds kernel with cargo and validates artifact
 - src/kernel/main.rs v0.2 gated by `kernel_bin` feature
- cohesix_fetch_build.sh v0.5 builds kernel ELF using kernel_bin feature
- Cargo.toml v0.6 keeps kernel_bin feature gating for tests
- rc init displays ASCII Cohesix wordmark with bee icon
- cohesix_fetch_build.sh v0.7 builds bootx64.efi, isolates QEMU monitor, and validates boot log
- Boot test exits with code 0 when QEMU is missing so CI shows the step as skipped
- policy_memory.rs v0.3 adds fallback to local persist path for tests
- Makefile v0.18 skips gnu-efi header checks and tolerates missing cargo tools
- Makefile v0.19 consolidates duplicate qemu rules and enforces OVMF firmware
- Converted remaining TODO comments to descriptive FIXME notes
- `generate_c` now outputs basic arithmetic operations in C
- Bootloader telemetry writes /state/boot_success; watchdog logs to /state/boot_error
- Validator paths now configurable via environment variables; CLI rule merge test added
- `cohtrace` CLI now supports `--verify-trace` and `compare` commands for trace validation
- CUDA runtime now exposes stub /srv/cuda when unavailable
- Added InteractiveAIBooth role with Secure9P namespace and optional CUDA support
- GitHub workflow indentation corrected for OSS dependency audit
- Validator uses `COHESIX_TRACE_TMP` for boot trace lookups; tests use
  `std::env::temp_dir` and clear expects to avoid permission errors.
- secure9p sandbox uses Path to parse agent IDs; tests added for trailing
  slashes and invalid namespaces
- Secure9P build fixes with rustls 0.23, policy clone, path validation, and
  handshake cleanup
- Cargo.toml v0.3 adds optional rustls and tokio-rustls dependencies to fix
  secure9p feature build and clippy
- `Capability` alias exposed in `cap_fid.rs` for consistent imports
- Gpu telemetry includes temperature and utilization via nvml-wrapper
- Fixed Python import paths and enabled pytest discovery across all tests
- Policy engine exposes `new` and `allow` API; tests updated
- sandbox.rs enforce function exported for reuse; unit tests now compile
- Added cuda_test.rs validating runtime CUDA presence
- 9P server enforces per-session sandbox policies with validator logging
- Fixed stray imports and missing closures in `secure_9p_server.rs`
- Added `AuthHandler` trait and `NullAuth` implementation
- New `secure9p` feature flag toggles TLS-backed file server
- Obsolete `src/secure9p` removed; `src/p9/secure` moved to `src/secure9p`
- Secure9P server added with TLS support, per-agent namespaces, capability checks, and JSON trace logging
- Secure9P server log path now uses `COHESIX_LOG_DIR` with temp dir fallback; handshake test verifies log
- CMake CUDA keep directory respects TMPDIR
- Python CLI tools use COHESIX_LOG environment variable; tests redirect logs to tmp paths
- Rust tests use tempfile for temporary files instead of hardcoded /tmp
- QueenWatchdog respects `COHESIX_QUEEN_DIR`; failover test uses temp path and
  explicit expect message
- /proc/nsmap exposes per-role namespace maps
- NsWatchService validates hotplugged mounts
- SandboxService logs namespace violations via validator
- `cohesix-9p` tests use `std::env::temp_dir()` for server root
- Added ns_hotplug.rs integration test
- Makefile bootloader target links with lld-link
- CLI scripts now use wrapper binaries in `bin/` so classification headers remain the first line
- `config/secure9p.toml` header repositioned and version updated to v0.2
- Added integration test `test_qemu_boot.rs` to verify BOOT_OK in qemu_serial.log
- New `scripts/check-qemu-deps.sh` verifies QEMU and gnu-efi packages before boot tests; README documents usage
- Thread spawn closure in `tls_handshake` test now closes correctly to avoid unclosed delimiters

- cohesix-9p/Cargo.toml edition set to 2021 to support async features
- Fixed missing closing braces in `tls_handshake` test

## [v0.89] - 2025-07-25
### Changed
- Secure9P tests start consolidated server with `start_secure_9p_server` using temporary TLS and policy files.
- Validator hook and namespace resolution validated.
- Header cleanup for `cli/cohcli.py` and boot logging scripts
- `test_boot_efi` wrapper added; Makefile updated accordingly
- BootMustSucceed rule verifies /trace/boot_trace.json at startup
- Makefile now includes fmt/lint/check targets and platform flags
- Added end-to-end traceflow test and CLI argument validation
- Added 9P read/write, CUDA presence, and namespace rule tests
- Added `docs/QUICKSTART.md` quick-start guide
- Added `BOOT_KERNEL_FLOW.md` diagram explaining boot through CLI
- README begins with a vision paragraph summarizing why Cohesix matters
- CONTRIBUTING includes local setup, testing, and Codex instructions
- Makefile adds `qemu` and `qemu-check` targets for serial-log boot testing
- Boot trampoline writes `BOOT_OK` or `BOOT_FAIL:<reason>` to console and `/state/boot_success`
- Added guidance to `docs/community/archive/examples_README.md`
- Added `test_qemu_boot.rs` verifying QEMU boot log for `BOOT_OK` and CUDA setup
- `VALIDATION_SUMMARY.md` now includes classification headers and is tracked in `METADATA.md`.
- Added classification headers to metadata.json, Cargo.toml, cohesix-9p/Cargo.toml, and justfile
## [v0.88] - 2025-07-22
- `qemu-check` now fails if `BOOT_FAIL` appears in `qemu_serial.log`
### Fixed
- `make qemu` and `make qemu-check` run QEMU with serial logging and grep for BOOT_OK
- Bootloader writes BOOT_OK or BOOT_FAIL to /dev/console and /state/boot_success
- Makefile adds `qemu` and `qemu-check` targets for serial-log boot testing
- Rust ensemble agent tests write to a safe temporary directory.
- QEMU launch scripts ensure `$HOME/cohesix/out` and `TMPDIR` exist.

## [v0.85] - 2025-07-22
### Added
- Makefile supports GCC and Clang toolchains for UEFI builds.
- Linker selection now follows compiler choice.
- `test_boot_efi.sh` prints the toolchain used.

## [v0.86] - 2025-07-22
### Changed
- Bootloader and kernel targets now place all intermediate files in `out/`.
- `make print-env` shows selected toolchain and compiler version.
- Clang builds use `ld.lld` with Windows COFF target.

## [v0.87] - 2025-07-22
### Fixed
- Makefile detects gnu-efi headers and falls back to available arch.
- Clang builds pass `-fuse-ld=lld` automatically.
- `test_boot_efi.sh` logs include paths and stores `make -n` output.

## [v0.84] - 2025-07-22
### Fixed
- Bootloader logs kernel launch progress and errors.
- Kernel stub moved to `src/kernel/main.c`.
- `test_boot_efi.sh` validates EFI loader messages.
- Makefile now uses linker scripts for UEFI binaries.
## [v0.82] - 2025-07-22
### Added
- Kernel UEFI stub built via new `make kernel` target.
- `test_boot_efi.sh` now builds the stub and uses system OVMF firmware.
### Fixed
- Corrected QEMU BIOS path in boot test.
## [v0.83] - 2025-07-22
### Added
- Bootloader prints "Starting Cohesix EFI loader" on launch.
- `test_boot_efi.sh` checks for QEMU and logs to `qemu_debug.log`.
### Changed
- `Makefile` includes `testboot` target to invoke the EFI boot test.
## [v0.79] - 2025-07-22
### Added
- UEFI bootloader prototype and link script.

## [v0.80] - 2025-07-22
### Added
- Makefile target to build BOOTX64.EFI
- `linker.ld` script and `.cargo/config.toml` for UEFI
- `test_boot_efi.sh` for QEMU boot validation

## [v0.81] - 2025-07-22
### Fixed
- Bootloader now links against GNU-EFI libraries and prints kernel load status.
- QEMU test updated to expect new log messages and use OVMF_CODE_4M.fd.

## [v0.78] - 2025-07-22
### Fixed
- Ensemble agent test handles missing config file gracefully.

## [v0.77] - 2025-07-22
### Fixed
- Added path existence checks in `validator_violation` test to skip when `/log` is missing.

## [v0.76] - 2025-07-22
### Added
- Moved binary logic into library modules with unit tests.
- Python sanity test scaffold.
- Updated CMake to build src/c placeholder and detect CUDA.

## [v0.75] - 2025-07-22
### Added
- Basic CMake build with native hello library and unit tests for scripts.

## [v0.74] - 2025-07-22
### Changed
- Updated METADATA table and canonical doc headers for accuracy.

## [v0.75] - 2025-07-22
### Fixed
- Tests use temporary directories for 9P server permission isolation.
- 9P server reads `COHROLE_PATH` env var when provided.

## [v0.73] - 2025-07-22
### Added
- GitHub Actions workflow `codex.yml` to run Codex tasks.
- Simplified `AGENTS.md` task definitions for Codex automation.
- Added placeholder `log/codex_output.md` for agent output.
- Bootloader trace hooks emit validator events during initialization.

## [v0.72] - 2025-06-08
### Added
- Consolidated CHANGELOG, metadata.json, and OPEN_SOURCE_DEPENDENCIES.md into PROJECT_MANIFEST.md.

## [v0.72] - 2025-06-09
### Added
- Canonical kiosk loop integrates sensor proxy and validator with trace logging.

## [v0.72] - 2025-07-21
### Added
- Rust CLI implementations for `cohrun`, `cohbuild`, `cohtrace`, and `cohcap`.
- Makefile aliases for the new CLI tools.
- `cohtrace` now appends run summaries to `VALIDATION_SUMMARY.md`.

## [v0.71] - 2025-07-21
### Added
- HTTP route and CLI docs for GUI orchestrator.
### Changed
- README clarified usage and example output.

## [v0.72] - 2025-07-22
### Changed
- CUDA artifacts isolated under `tests/gpu_demos`.
- Canonical naming enforced for Shared Learning Module files.
### Added
- Optional CUDA, Rapier and BusyBox features.
- Platform-specific boot targets in Makefile.
- Boot timing logs written to `/log/boot_time.log`.
- Capability enforcement via `/srv/cohrole` and kernel trace module.

## [v0.69] - 2025-07-20
### Added
- Go GUI orchestrator with chi router and API endpoints.

## [v0.71] - 2025-07-20
### Added
- Expanded GUI orchestrator HTTP tests with failure cases and logging checks.

## [v0.70] - 2025-07-20
### Changed
- Updated `gui_orchestrator.md` with chi router and API usage notes.

## [v0.71] - 2025-07-20
### Added
- `/api/metrics` endpoint with request and session counters.
- `--dev` flag enabling verbose logs and static live reload.

## [v0.71] - 2025-07-21
### Changed
- Hardened GUI orchestrator with panic recovery, optional basic auth, rate limiting, and safer logging.

## [v0.68] - 2025-06-09
### Changed
- Hardened kernel capability checks and syscall validation.
- Added CUDA build script and GPU detection logic.

## [v0.67] - 2025-06-09
### Added
- BusyBox test script `test_cohbox.sh` verifying static build and approved applets.
- CI step builds Cohox via `make -f third_party/busybox/Makefile.coh` and runs the new test.

## [v0.66] - 2025-06-09
### Added
- `cohshell.sh` wrapper for Cohesix BusyBox. Users may symlink it to `/bin/sh` for minimal rootfs setups.

## [v0.65] - 2025-06-09
### Added
- Cross-compilation flags `--target` and `--sysroot` for `cohcc`.
- Rust build wrapper with static linking enforcement and logging.
- JSON IR schema loader with versioning and log output.
- Structured logging macros and CI reproducibility test.

## [v0.64] - 2025-06-09
### Added
- Backend registry with trait-based dispatch and input type detection.
- Static flag enforcement and build hashing for `cohcc`.

## [v0.63] - 2025-06-09
### Added
- Zig backend integration for `cohcc` with static build enforcement.
- `test_cohcc_build_c.sh` verifies static output.
- Offline mandoc build via coh_cc and new cohman wrapper.
### Changed
- Man page updated with backend options and logging paths.

## [v0.62] - 2025-06-09
### Added
- Initial `coh_cc` compiler stub replacing placeholder script.
- Man page updated and basic integration test added.
- Documented danger flags and exit codes in `cohcap(1)` and `cohtrace(1)`.
- Added failure-case JSON examples.
- Integrated `cohesix-9p` server with upstream `ninep` crate.
- Added adapter helpers and basic 9P integration tests.

## [v0.61] - 2025-07-15
### Added
- Mutex-protected state and structured logging for Go agent SDK.
- Graceful shutdown and fault-injection tests covering cancellations and timeouts.
### Changed
- Unified logging for CLI tools with `cohlog` and safe shell execution.
- Added argparse subparsers and new flags to `cohcli`.

## [v0.61] - 2025-06-08
### Added
- CI matrix expanded to x86_64/aarch64 with CUDA and DroneWorker role.
- Test steps sandboxed via bubblewrap to restrict writes.
- Grype vulnerability scanning and SHA256 artifact logging.
- OSS dependency workflow now generates an SBOM via syft.
- Validator error tests for corrupted SLM manifests and trace files.
- Boot integration checks for session log and CLI role output.
- 9P validator hook tests for capability, timeout, and replay errors.
- Python validator rules engine with structured dispatch and timeouts.
- Kiosk loop watchdog with clean shutdown.
- Agent SDK version tag and migration tests.
### Changed
- Boot trampoline validates SHA256 CRC before jumping.
- bootloader defaults to DroneWorker role and logs init.

## [v0.61] - 2025-08-18
### Changed
- Python validator CLI hardened with strict rule validation and logging options.
- Added `--output` flag to save results.
- Validator now exits with non-zero status on failure.

## [v0.60] - 2025-07-15
### Changed
- Cleaned METADATA duplicate entries and normalized license files.
- Bumped versions for `OPEN_SOURCE_DEPENDENCIES.md` and `LICENSE_MATRIX.md`.

## [v0.59] - 2025-07-14
### Added
- Restored `INSTRUCTION_BLOCK.md` with v3.6 workflow rules.
- Updated `METADATA.md` to v3.3.

## [v0.58] - 2025-07-14
### Added
- Persistent telemetry logs and timestamp helpers
- `cohcli --version` option and updated man page

## [v0.57] - 2025-07-14
### Added
- 9P server permissions and tests
- Device hotplug documentation and sync checks
- CLI input validation tests
- Session logging with timestamps
### Fixed
- Cargo.lock duplication issues

## [v0.56] - 2025-06-08
### Added
- Deterministic simbridge harness with snapshot resume.
- Nightly CI job for cross-platform determinism.

## [v0.54] - 2025-07-13
### Added
- BusyBox build now includes `finger`, `last`, `free`, `top`, `df`, and `who` utilities.
- Session logging to `/log/session.log` tracks logins and commands.
- Prototype `cohpkg` package manager with manifest-driven installs.
- `UTILS_README.md` and new man page `cohpkg.1` document available tools.

## [v0.55] - 2025-07-13
### Fixed
- Restored `AGENTS.md` and updated METADATA for test compatibility.
### Added
- Runtime sandbox now enforces capabilities from `/etc/cohcap.json` and logs
  blocked syscalls with role and PID.
- CUDA FFI loader wraps symbols with runtime validator checks.
- CUDA runtime now executes real kernel and logs to /log/gpu_runtime.log
- Telemetry schema expanded with exec_time_ns and fallback_reason
- Added cust crate dependency


## [v0.53] - 2025-07-13
### Removed
- AGENTS.md, END_USER_DEMOS.md, IMPLEMENTATION_GUIDE.md, TOOLING_PLAN.md,
  RELEASE_CRITERIA.md, BATCH_PLAN.md, GPU_SWARM.md, WEBCAM_TILT.md,
  KIOSK_FEDERATION.md
### Moved
- INSTRUCTION_BLOCK.md -> archive/INSTRUCTION_BLOCK.md
- examples/README.md -> archive/examples_README.md
### Changed
- Updated METADATA.md to v3.0 after cleanup.
### Added
- Expanded BusyBox command set and Python CLI wrappers.
- New manpage generation script and CLI regression tests.
// Author: Lukas Bower
## [v0.52] - 2025-06-08
### Added
- Consolidated mission, tooling, demo, and release docs.
- New versions: MISSION_AND_ARCHITECTURE.md, AGENTS_AND_CLI.md, DEMO_SCENARIOS.md, RELEASE_AND_BATCH_PLAN.md, IMPLEMENTATION_AND_TOOLING.md, VALIDATION_AND_TESTING.md.

## [v0.51] - 2025-07-12
## [v0.51] - 2025-06-08
### Changed
- Removed prebuilt mandoc binaries and added build script.
### Added
- Initial manpages for Cohesix CLI tools and BusyBox utilities.
- CI hardware validation collects Jetson and Pi boot logs.
- Example trace replay executed in CI.
- VALIDATION_AND_TESTING.md updated to v0

## [v0.50] - 2025-07-12
### Added
- CLI help docs for cohcli, cohrun, cohtrace, cohcc, and cohcap.
- Example JSON templates in examples/ directory.
- New guides: AGENTS_AND_CLI, DEMO_SCENARIOS, VALIDATION_AND_TESTING.

## [v0.49] - 2025-06-08
### Changed
- `cohcc` CLI now infers backend from output path.

## [v0.49] - 2025-07-12
### Added
- Kiosk federation via `cohrun kiosk_start` and `cohrun kiosk_event`.
- `cohtrace kiosk_ping` appends trace events.
- `python/kiosk_loop.py` simulates kiosk inputs.

## [v0.48] - 2025-07-12
### Added
- Real-time sensor feedback via `sensor_proxy.py` and `normalizer.py`.
- Live rule injection CLI `cohrun inject_rule` and validator `--live` mode.
- Demo script `demo_sensor_feedback.sh` and validator test.

## [v0.47] - 2025-07-12
### Added
- `watchdogd` background daemon monitoring heartbeat, tasks, and trace loop.
- Edge-only fallback coordinator promoting a Worker when the Queen disappears.
- Role memory persistence with CLI `cohrun trace_replay`.
- Demo script `demo_edge_failover.sh` and regression test for failover traces.

## [v0.46] - 2025-07-12
### Added
- OSS audit toolchain and demo SBOM artefacts.

## [v0.45] - 2025-06-08
### Added
- Federation handshake supports role inheritance and trust zone mapping.
- Snapshot writer serializes worker state under `/history/snapshots/`.
- `cohrun federate_with` and `cohtrace view_snapshot` CLI commands.
### Fixed
- Resolved merge marker in `METADATA.md` and updated headers.

## [v0.43] - 2025-07-11
### Changed
- Verified batch statuses against repository; all modules compile and tests pass.
## [v0.42] - 2025-07-11
### Changed
- Updated BATCH_PLAN.md to v0.6 with current batch statuses.

// Filename: CHANGELOG.md v0.27
// Date Modified: 2025-06-15
// Author: Lukas Bower
## [v0.43] - 2025-07-11
### Added
- Agent introspection namespace `/srv/agent_meta` with runtime files.
- World state summary `/srv/world_state/world.json`.
- Python and Go `agent_sdk` for runtime context.
- CLI `cohrun goal_*` and trust zone commands.
- `cohtrace trust_check` lists current trust zones.
// Filename: CHANGELOG.md v0.28
// Date Modified: 2025-06-15
// Author: Lukas Bower
## [v0.44] - 2025-06-08
### Added
- COMMERCIAL_PLAN.md updated to v1.5 referencing EY network.
- TECHNICAL_RISK.md v1.1 with EY partner mitigation.
- METADATA.md and CHANGELOG updated.

## [v0.43] - 2025-07-12
### Added
- COMMERCIAL_PLAN.md updated to v1.4 with expert panel and benchmarking sections.
- New TECHNICAL_RISK.md documenting six mitigations.
- BATCH_PLAN.md bumped to v0.6 with next-step tasks.
- METADATA.md synchronized for new versions.
## [v0.41] - 2025-06-08
### Added
- Cross-target `--target` option for `cohcc` CLI.
- POSIX translation helpers and tests.
- Build plan updated for architecture flag.

## [v0.40] - 2025-06-06
### Added
- Webcam and GPU info services registered under `/srv`.
- `cohrun physics_demo` CLI and Rapier demo logging to `/trace/last_sim.json`.
- `cohtrace list` demo CLI for viewing joined workers.
- Boot hooks create role services and write to `/trace/boot.log`.

## [v0.41] - 2025-06-08
### Added
- Real webcam capture with `/srv/webcam/frame.jpg`.
- `cohrun test_webcam` and `cohrun webcam_tilt` commands.
- Webcam tilt simulation logs to `/trace/last_sim.json`.
- Queen validator writes reports under `/trace/reports/`.

## [v0.42] - 2025-06-08
### Added
- Security review summary under `SECURITY_REVIEW.md`.
- Cross-worker orchestrator registry written to `/srv/agents/active.json`.
- CLI commands `cohrun orchestrator status|assign` and `cohrun kiosk_start`.
- GPU swarm registry `/srv/gpu_registry.json` with `gpu_status` and `gpu_dispatch`.
- `cohtrace kiosk_ping` command for kiosk federation demo.

## [v0.39] - 2025-06-08
### Changed
- Fixed Makefile tab indentation to enable successful builds

## [v0.38] - 2025-06-08
### Added
- Codex-Driven Mega Batches with autonomous multi-arch hydration
- GPT model version stamped via `CODEX_BATCH: YES`

## [v0.37] - 2025-06-08
### Added
- UpgradeManager module for atomic upgrades
- AES-GCM SLM decryptor with CLI hooks
- EnsembleAgent trait and shared memory
- CLI commands: upgrade, rollback, list-models, decrypt-model, verify-model, agent-ensemble-status

## [v0.36] - 2025-06-08
### Added
- Agent introspection API and CLI command `agent-introspect`
- 9P validator bridge logging violations
- QueenWatchdog for mesh reconfiguration with CLI `elect-queen` and `assume-role`

## [v0.35] - 2025-06-08
### Added
- World model snapshot structs and sync daemon
- Policy memory persistence utilities
- Vision overlay CLI hooks

## [v0.34] - 2025-06-08
### Added
- SLM registry and dispatch CLI
- Trace validation stream to validator socket
- Queen failover test scripts

## [v0.31] - 2025-06-08
### Changed
- Completed bootloader initialization logic and secure boot checks
- Wrapped CUDA runtime loading in safe abstraction
- Fixed shell script header syntax
- Documented BusyBox and seL4 sandboxing in OSS_REUSE.md

## [v0.32] - 2025-06-08
### Added
- Queen federation manager and CLI commands for connect/disconnect
- Basic distributed orchestrator policies and status export
- Agent migration helper module and demo federation script

## [v0.33] - 2025-06-08
### Added
- GPU swarm scheduler with perf/watt weighting
- Webcam inference module and CLI `run-inference`
- Kiosk federation demo scripts


## [v0.29] - 2025-06-08
### Added
- Documented cobra Go dependency under Apache-2.0

## [v0.30] - 2025-06-07
### Added
- `test_all_arch.sh` script for running Rust, Go, and Python tests across architectures.
- Documented usage in README and BUILD_PLAN.

## [v1.0] - 2025-06-07
### Added
- Federation keyring, handshake, and migration modules
- TPM secure boot attestation with hash verification
- GUI orchestrator stub and federation CLI
- Physics CUDA test harness

## [v0.28] - 2025-06-08
### Added
- Failover manager with automatic Queen promotion
- Live patch infrastructure and `cohup patch` CLI
- Trace consensus module and physics adapter
- Adaptive agent policies with SelfTuningStabilizer
- README and TEST_GUIDE updated with Go testing instructions; Makefile v0.5

## [v0.27] - 2025-06-08
### Added
- Join acknowledgement with worker directories under `/srv/worker/<id>`
- `cohagent` CLI for start/pause/migrate commands
- Runtime syscall validator writing `/srv/violations/<agent>.json`

## [v0.23] - 2025-06-07
### Added
- Distributed swarm registry and agent migration helpers.
- Worker hotplug detection and cluster election.
- Test contracts for runtime safety and failure audits.

## [v0.24] - 2025-06-07
### Added
- Service mesh TTL and remote mounting
- Node health tracking and election improvements
- Fuzz regression runner and CI role harness
- Additional safety and contract tests

## [v0.25] - 2025-06-08
### Added
- Multi-Queen coordination logs and rotation
- Worker hotplug mounting and service sync
- Agent migration shutdown callback
- Distributed trace hash comparison
- CI role runner multi-role support and fuzz regression tracking updates
### Fixed
- Absolute `/srv` paths in tests to resolve lifecycle failures

## [v0.26] - 2025-06-07
### Added
- Distributed orchestration layer with worker join protocol
- Agent snapshot writer and migration CLI
- Queen federation beacons and secure links
- Agent runtime records agent_table.json
- `cohrole` CLI utility shows current runtime role.
- Bootloader logs role and cmdline to `/srv/boot.log` and exposes `/srv/cohrole`.

## [v0.22] - 2025-06-07
### Added
- Basic capability check map in `src/seL4/syscall.rs` enforcing
  path-based permissions for open and exec operations.

## [v0.21] - 2025-06-07
### Added
- Trace fuzzer and scenario compiler tools under `tools/`
- Scenario runner executing compiled scenarios

## [v0.20] - 2025-06-07
### Added
- Integrated Rapier physics and CUDA runtime with service traces
- Expanded 9P multiplexer, seL4 syscall guard and BusyBox shell
- Added OSS dependency table and integration tests


## [v0.19] - 2025-06-07
### Added
- Plan 9 namespace tree with bind flags and persistence
- Syscall wrapper module and service registry
- rc init parser and new tests

## [v0.18] - 2025-06-07
### Added
- Agent runtime with tracing and service registration.
- Queen orchestrator managing worker nodes.
- Trace recorder with replay support.
- Physical sensor mock model and scenario engine.
- `test_agent_lifecycle.rs` covering agent lifecycle.

## [v0.17] - 2025-06-07
### Changed
- Fixed duplicate sections in `init/worker.rs` and cleaned namespace loader.
- Replaced unsafe mount table in `kernel/fs/plan9.rs` with `Mutex`.
- Added `test_plan9_mount.rs` to validate mount capacity.

## [v0.16] - 2025-06-07
### Added
- **bootloader.c**: seL4 root task detecting CohRole and launching role init.
- **cloud hooks**: dynamic agent fetch via `/srv/cloudinit`.
- **init modules**: worker, kiosk and sensor roles with service registration.
- **boottrace.py** script and Python bootflow test.

## [v0.14] - 2025-06-06
### Added
- **plan9_ns.rs**: namespace builder parsing boot args and exposing `/srv/bootns`.
- **seL4/syscall.rs**: stub Plan 9 syscall glue layer.
- **init/queen.rs**: queen root task loads boot namespace and logs to `/dev/log`.
- **cohesix-9p**: in-memory FS supports `/srv/cohrole` and dynamic service registration.
- **test_nsbuilder.rs**: unit tests for namespace builder.

## [v0.13] - 2025-06-06
### Fixed
- **send-heartbeat.sh**: log function now outputs provided message; header bumped to v0.2.

## [v0.14] - 2025-06-06
### Added
- **telemetry/router.rs**: implemented `TelemetryRouter` trait with CPU and thermal metrics routing via 9P.
- **sandbox/queue.rs** and **sandbox/dispatcher.rs**: syscall queueing and dispatch logic with role checks.
- **cohesix_types.rs**: shared `Syscall` and `RoleManifest` definitions.
- **tests/test_syscall_queue.rs**: validates queue ordering and policy enforcement.

## [v0.15] - 2025-06-07
### Added
- **cuda/runtime.rs**: dynamic CUDA initialization and `GpuTaskExecutor` trait.
- **sim/rapier_bridge.rs**: multithreaded Rapier wrapper exposing `/sim` files.
- **p9/multiplexer.rs**: basic service registration and routing logic.
- **shell/busybox_runner.rs**: spawn BusyBox shell with kernel fallback.
- **tests/test_gpu_and_sim.rs**: validates GPU kernel launch and sim state.

## [v0.16] - 2025-06-07
### Added
- **plan9/namespace.rs**: dynamic namespace loader and applier.
- **p9/multiplexer.rs**: async request handling with `handle_async`.
- **init/worker.rs**: worker root task service mounts.
- **cuda/runtime.rs**: kernel loading and launch API.
- **tests/test_cuda_exec.rs** and **tests/test_integration_boot.py**.
- **scripts/cohtrace.py**: syscall trace stub.
- **runtime/service_registry.rs**: global service registration with role filtering.
- **sandbox/chain.rs**: executes sandboxed syscall chains.
- **telemetry/loop.rs** and **telemetry/mod.rs**: telemetry sync loop and module.
- **shell/busybox_runner.rs**: interactive sandbox shell runner.
- **tests/test_service_registry.rs**: validates service registry logic.
### Changed
- **cuda/runtime.rs**, **sim/rapier_bridge.rs**, **init/queen.rs**, **worker/mod.rs**: register services on startup.

## [v0.12] - 2025-06-05
### Added
- **verify-macos-setup.sh**: helper script validating Homebrew, Xcode tools,
  Python 3.10+, git and running `validate_metadata_sync.py`.
- **README_Codex.md**: macOS verification instructions; version bumped to v1.4.

## [v0.11] - 2025-06-05
### Changed
- **ci.yml**: updated rust toolchain step to v1.0.7 using environment files and installed rustfmt/clippy.

## [v0.10] - 2025-06-05
### Removed
- Removed obsolete entries like `.DS_Store` from `filetree.txt`.
### Changed
- Regenerated `filetree.txt` based on tracked files only.

## [v0.9] - 2025-06-05
### Added
- **README**: document running Go tests via `make go-test` (workspace in `go/`).

## [v0.10] - 2025-06-08
### Fixed
- **scripts/run-smoke-tests.sh**: removed stray prompt text and ensured newline at EOF; passes `shellcheck`.

## [v0.8] - 2025-06-08
### Added
- **BATCH_PLAN.md**: added cloud-native hooks deliverable for Queen roles; version bumped to v0.5.
- **METADATA.md**: updated entry for BATCH_PLAN.md to v0.5.

# Changelog for Cohesix

## [v0.8] - 2025-06-08
### Changed
- **BATCH_PLAN.md**: expanded batch entries with bullet-level deliverables and demo heading; version bumped to v0.5.
- **METADATA.md**: version bumped to v2.1 with updated BATCH_PLAN entry.

## [v0.7] - 2025-06-08
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

## [v0.54] - 2025-07-13
### Added
- ALPHA_VALIDATION_ISSUES.md documenting release blockers.

## [v0.55] - 2025-07-13
### Added
- `cohesix_netd` network daemon with TCP 9P, discovery and HTTP fallback.
- Documentation `NETWORKING.md`.
### Changed
- Updated METADATA to include `NETWORKING.md`.
- Document clarified broadcast fallback behavior.

## [v0.55] - 2025-07-13
### Added
- CUDA runtime now executes real kernel and logs to /log/gpu_runtime.log
- Telemetry schema expanded with exec_time_ns and fallback_reason
- Added cust crate dependency

## [v0.56] - 2025-07-13
### Changed
- CUDA executor tracks last execution time and fallback reason
- Validator logs on unsafe kernel launch

## [v0.55] - 2025-06-08
### Added
- cohdevd service with inotify hotplug and sandbox validation.
- Webcam capture telemetry logging and dummy fallback.
- Real sensor input with optional mock injection.
- Tests for device attach/detach, validator logging, and webcam fallback.

## [v0.56] - 2025-06-08
### Added
- `autorun_tests.py` script for automatic test execution on file changes.


## [v0.62] - 2025-08-18
### Added
- Restored CI workflow with Rust, Python, Go and shell checks
- Updated VALIDATION_SUMMARY.md with build results

## [v0.63] - 2025-08-27
### Added
- Kernel user API exposing `sys_log` and `sys_exit` via function pointers
- Process table tracks exited state and exit codes
- Test userland binary `logdemo` prints two lines then exits with code 42

## [v0.64] - 2025-08-30
### Added
- init.efi loads /etc/init.cfg and validates required keys

## [v0.100] - 2025-09-04
### Changed
- `.github/workflows/ci.yml` v1.4 adds cross-target matrix for ISO and Linux builds.
- CI disables CUDA by default via `RUSTFLAGS=--cfg no_cuda`.
- UEFI job verifies kernel and role configs are present and uploads artifacts.

## [v0.101] - 2025-09-05
### Added
- init binary target for UEFI builds.

## [v0.102] - 2025-09-05
### Added
- Default system config at `setup/config.yaml` copied into ISO builds.
- Role configs under `setup/roles/` with QueenPrimary, KioskInteractive, DroneWorker,
  GlassesAgent, SensorRelay and SimulatorTest defaults.
- `full_fetch_and_build.sh` v0.7 verifies these configs and copies them to `$ISO_ROOT`.

## [v0.103] - 2025-09-06
### Fixed
- `RoleConfig::load_active` now honors `ROLE_CONFIG_DIR` allowing tests to
  supply temporary role directories.

## [v0.104] - 2025-09-07
### Added
- scripts/make_iso.sh creates the bootable ISO using xorriso.
- full_fetch_and_build.sh invokes the new script after building.

## [v0.105] - 2025-09-08
### Added
- Bootloader verifies `kernel.elf` against `kernel.sha1` before launch.
- Mismatched hashes or missing kernels write to `boot.log` on the EFI volume.
- `sha1.c` implementation added to bootloader build.

## [v0.106] - 2025-09-09
### Added
- `validator.py` can validate trace files via `--input` and `--format`.
- JSON schema checks ensure trace integrity before parsing.
- `requirements.txt` now includes `jsonschema`.

## [v0.107] - 2025-09-10
### Changed
- `test_boot_efi.sh` now checks for `bootx64.efi` and preserves QEMU logs under
  `logs/qemu_boot.log`. Failed boots print the last 20 log lines and exit with
  status 1.
- `cohesix_fetch_build.sh` uses the same log location and verifies `bootx64.efi`
  after ISO creation.

## [v0.108] - 2025-09-11
### Changed
- ci.yml v1.5 consolidates build steps and prints per-language validation summary.

## [v0.109] - 2025-09-12
### Added
- `tools/make_iso.sh` assembles a complete Cohesix ISO including configs, CLI tools and man pages.

## [v0.110] - 2025-09-13
### Changed
- `cohesix_fetch_build.sh` builds `init.efi`, copies kernel and init into `out/`,
  invokes `scripts/make_iso.sh`, and validates OVMF firmware before running QEMU.

## [v0.111] - 2025-09-14
### Changed
- Consolidated BusyBox build scripts into `build_busybox.sh` with cross-arch
  support and installer options. Old `build-busybox.sh` removed.

## [v0.112] - 2025-09-15
### Fixed
- `cohesix_fetch_build.sh` restores the init EFI build using the correct
  target triple and writes all build output to `~/cohesix_build.log` for
  troubleshooting.

## [v0.113] - 2025-09-16
### Fixed
- Hardened `open_denied_logs_violation` test to avoid PermissionDenied panics
  by checking log and config paths before use.

## [v0.114] - 2025-09-17
### Added
- Quiet console output in `cohesix_fetch_build.sh` with full log saved to
  `~/cohesix_build.log`. High-level step summaries are printed, and failures
  show the last 40 log lines for troubleshooting.

## [v0.115] - 2025-09-18
### Fixed
- `cohesix_fetch_build.sh` verifies all components build before running
  `make_iso.sh`, stages configuration and role files, and validates the ISO
  output size and contents.

## [v0.116] - 2025-09-19
### Added
- `cohesix_fetch_build.sh` stages Rust, Go, and Python tools into `out/bin`
  and prints a QEMU boot hint.
- `scripts/make_iso.sh` copies CLI wrappers, Python modules, and runtime
  binaries into `out/iso_root`.
- `make_iso.sh` now delegates to `scripts/make_iso.sh`.

## [v0.117] - 2025-09-20
### Added
- Improved build logging with structured summaries in `cohesix_fetch_build.sh`.
- Errors and test failures are extracted to `~/cohesix_logs/summary_errors.log`
  and `summary_test_failures.log` with phase markers.

## [v0.118] - 2025-09-21
### Fixed
- `cohesix_fetch_build.sh` ensures `out/bin` exists before copying `cohcc` and validates the binary path.

## [v0.119] - 2025-09-21
### Changed
- `make kernel` now builds the Rust kernel and produces `BOOTX64.EFI` in `out/`.
- ISO creation scripts expect `BOOTX64.EFI` instead of `kernel.efi`.
- Top-level `make build` and `make all` invoke the kernel build automatically.

## [v0.120] - 2025-09-21
### Added
- CI boot verification script `ci/qemu_boot_check.sh`.
- Workflow `ci.yml` runs this script after building the project.

## [v0.121] - 2025-09-22
### Changed
- `secure9p` feature now pulls in `tokio` as an optional dependency.
- Cargo manifest marks `tokio` optional to avoid unnecessary compile overhead.

## [v0.122] - 2025-09-23
### Added
- `userland/miniroot` with basic `echo`, `help`, and `ls` utilities.
- `tools/make_iso.sh` copies miniroot into the ISO and validates presence.
- `rc/init.rs` binds `/miniroot` to `/` when present for interactive tests.

## [v0.123] - 2025-09-23
### Added
- Shell loop entry via `sh_loop::run` invoked during userland bootstrap.
### Changed
- `userland_bootstrap.rs` now initializes runtime and rc before starting the shell.

## [v0.124] - 2025-09-24
### Added
- `cohcc` command available within the interactive shell.
- Example source file at `/usr/src/example.coh` for quick testing.
- Test `test_cohcc_shell.rs` validates shell compilation pathway.

## [v0.125] - 2025-09-25
### Changed
- `cohcc` shell command supports `-o` for output redirection.
- `cohcc::compile()` now returns binary bytes for file output.
### Added
- Test `test_cohcc_output.rs` verifies binary creation.

## [v0.126] - 2025-09-26
### Added
- Runtime loader `load_and_run` for `.out` binaries.
- `run` command available in the interactive shell.
- Tests `test_loader.rs` and `test_run_shell.rs` cover loader usage.

## [v0.127] - 2025-09-27
### Changed
- `cohesix_fetch_build.sh` builds with `--features secure9p`.
- `scripts/make_iso.sh` now stages `userland/miniroot`, `/usr/src/example.coh`, and creates writable `/tmp`.
- Wrapper `make_iso.sh` bumped for consistency.
## [v0.128] - 2025-06-18
### Fixed
- Added `tokio` dependency with full features.
- Resolved `UnboundedReceiver` type inference in `p9/multiplexer.rs`.
- Removed unused imports in `coh_cc/mod.rs` and `kernel/userland_bootstrap.rs`.

## [v0.129] - 2025-11-30
### Fixed
- `cohesix_fetch_build.sh` now validates `$TARGET` and installs missing targets
  via `rustup`. The script falls back to `aarch64-unknown-linux-gnu` when the
  variable is unset.

## [v0.130] - 2025-11-30
### Added
- `cohesix_fetch_build.sh` verifies the C toolchain by compiling and running a
  dummy program, ensuring `gcc` and linker functionality before building C
  components.

## [v0.131] - 2025-12-01
### Fixed
- `cohesix_fetch_build.sh` now checks the target triple for UEFI support and
  skips EFI builds when incompatible. Kernel and init EFI artifacts are verified
  to be non-empty.

## [v0.132] - 2025-12-02
### Changed
- `scripts/make_iso.sh` uses EFI binaries from `out/bin/`, builds `/boot/efi` hierarchy,
  and detects architecture via `$TARGET`.
- Gracefully skips ISO creation when `xorriso`, `grub-mkrescue`, or `mtools` are missing.
- Root wrapper `make_iso.sh` bumped to v0.6.

## [v0.133] - 2025-12-03
### Fixed
- `cohesix_fetch_build.sh` prompts for architecture or defaults to x86_64 when
  non-interactive, exporting `COHESIX_TARGET` for all build steps.

## [v0.134] - 2025-12-04
### Fixed
- `cohesix_fetch_build.sh` builds `cohcc` with the target triple and copies the
  release binary from `target/${COHESIX_TARGET}/release/` to `out/bin/`. The
  script now fails early if the binary is missing.

## [v0.135] - 2025-12-06
### Fixed
- `cohesix_fetch_build.sh` ensures `out/etc/cohesix/` exists before copying
  `config/config.yaml` and stops with a clear error if the file is missing.

## [v0.136] - 2025-12-07
### Fixed
- Boot prerequisite checks for kernel and init binaries before ISO creation.
- `scripts/make_iso.sh` now stages kernel and init from `out/boot/` and copies the bootloader.
- Auto-generates a fallback `config.yaml` if none is provided.
- Version bumps for `cohesix_fetch_build.sh`, `make_iso.sh`, and `scripts/make_iso.sh`.

## [v0.137] - 2025-12-08
### Fixed
- `test_cohcc_output.rs` now writes example sources to a temporary directory and marks the output binary executable to avoid permission errors on restricted systems. The helper `coh_cc::compile` no longer unwraps temp paths.

## [v0.138] - 2025-12-09
### Fixed
- `test_compile_trace.rs` writes all artifacts to a temporary directory and sets execution permissions explicitly. Toolchain path checks now honor the `COHESIX_TOOLCHAIN_ROOT` environment variable, allowing tests to run in sandboxed environments.

## [v0.139] - 2025-12-10
### Fixed
- CUDA tests check GPU availability and permissions.
- cohesix_fetch_build.sh exports COH_PLATFORM and COH_GPU.
- mypy and flake8 warnings resolved.

## [v0.140] - 2025-12-12
### Fixed
- `cuda_kernel_result_file` test now uses `tempfile` paths and skips when CUDA or
  permissions are unavailable.
- Added `coh_check_gpu_runtime` helper for runtime GPU validation.

## [v0.141] - 2025-12-15
### Fixed
- `bind_overlay_order` test now uses a temporary `/srv` directory when possible
  and skips if permissions are insufficient. Errors include the current UID for
  easier diagnostics.
## [v0.142] - 2025-12-17
### Fixed
- ISO build script now uses bash and validates output paths.
- QEMU boot test logs command and ensures ISO exists before boot.

## [v0.143] - 2025-12-18
### Fixed
- make_iso.sh verifies BOOTX64.EFI presence and lists staged files.
- cohesix_fetch_build.sh validates kernel and init outputs before ISO build.
- Makefile qemu target fails early if BOOTX64.EFI or ISO missing.
- test_qemu_boot.rs prints build commands and logs when artifacts are absent.

## [v0.144] - 2025-12-19
### Added
- scripts/validate_iso_build.sh for standalone ISO checks.
### Changed
- scripts/make_iso.sh validates each copied file and logs expected paths.
- cohesix_fetch_build.sh chains EFI → ISO → boot tests with diagnostic checks.
- tests/test_qemu_boot.rs verifies artifacts before boot and dumps QEMU log tail.

## [v0.145] - 2025-12-20
### Fixed
- test_qemu_boot.rs prints qemu command, retries on failure, and checks log presence.
- scripts/make_iso.sh exits with errors when ISO tools are missing.

## [v0.146] - 2025-12-21
### Changed
- scripts/make_iso.sh logs mkisofs stderr on failure and verifies ISO readability.
- cohesix_fetch_build.sh validates ISO artifacts and logs sha256 checksums.
- tests/test_qemu_boot.rs fully validates QEMU execution and dumps log tails on error.

## [v0.147] - 2025-12-22
### Added
- scripts/debug_qemu_boot.sh provides preboot diagnostics and QEMU dry-run output.
### Changed
- scripts/make_iso.sh validates mountability via isoinfo.
- tests/test_qemu_boot.rs uses debug_qemu_boot.sh and logs boot traces.

## [v0.148] - 2025-12-23
### Changed
- scripts/debug_qemu_boot.sh enforces bash interpreter and signals DEBUG_BOOT_READY.
- tests/test_qemu_boot.rs invokes the script via bash and checks readiness.
- Added unit test ensuring debug_qemu_boot.sh is executable.

## [v0.149] - 2025-12-24
### Changed
- cohesix_fetch_build.sh builds ISO before running tests and logs key milestones.
- scripts/make_iso.sh fails loudly when BOOTX64.EFI or config.yaml are missing.
- tests/test_qemu_boot.rs assert presence of ISO and EFI before running QEMU.
### Added
- tests/test_boot_build_chain.rs verifies build script log markers.


## [v0.150] - 2025-12-26
### Added
- scripts/build_sel4_kernel.sh builds the seL4 kernel for QEMU pc99.

## [v0.151] - 2025-06-20
### Added
- Add static seL4 root task build (cohesix_root.elf)

## [v0.152] - 2025-12-27
### Added
- Rust cohesix_root static ELF build script
- src/root/main.rs entry point for seL4 root task

## [v0.153] - 2025-12-28
### Added
- scripts/make_grub_iso.sh builds GRUB ISO with seL4 kernel and Cohesix root task

## [v0.154] - 2025-12-29
### Changed
- Added scripts/boot_qemu.sh launching GRUB ISO via QEMU.
- tests/test_qemu_boot.rs now looks for COHESIX_BOOT_OK and captures serial logs.

## [v0.155] - 2025-12-30
### Changed
- cohesix_fetch_build.sh stages full root filesystem under out/stage and invokes make_grub_iso.sh
- make_grub_iso.sh builds ISO from staged directory and prints summary counts

## [v0.156] - 2025-12-31
### Added
- Minimal seL4 entry stub at src/bootstrap/sel4_entry.rs logging COHESIX_BOOT_OK and launching shell

## [v0.157] - 2025-12-31
### Added
- src/util/debug_log.rs providing seL4 DebugPutChar logger and debug! macro.
- src/bootstrap/sel4_entry.rs now logs boot messages using debug!.

## [v0.158] - 2025-12-31
### Changed
- src/bootstrap/sel4_entry.rs panic handler gated behind `std` feature.
- Cargo.toml adds `std` and `sel4` features; `sel4_entry` excluded from tests.
- src/util/debug_log.rs now falls back to stderr logging when `std` is enabled.
- cohesix_fetch_build.sh now clones via HTTPS instead of SSH.
- scripts/make_grub_iso.sh now builds missing artifacts and stages files before
  calling grub-mkrescue.
- build_sel4_kernel.sh now falls back to system ninja when tools-provided ninja
  is unavailable.

## [v0.159] - 2025-12-31
### Changed
- scripts/make_grub_iso.sh accepts `COHROLE` or CLI argument to set `CohRole`
  and injects the value into `grub.cfg`.

## [v0.160] - 2026-01-02
### Changed
- scripts/make_grub_iso.sh now ensures bin and roles directories exist before scanning
  and defaults summary counts to zero if absent.

## [v0.161] - 2026-01-05
### Changed
- cohesix_fetch_build.sh builds seL4 kernel and root ELF via helper scripts.
- EFI build logic is disabled by default and enabled with `--with-efi`.

## [v0.162] - 2026-01-06
### Changed
- `make_iso.sh` now calls `scripts/make_grub_iso.sh`.

## [v0.163] - 2026-01-07
### Changed
- `build_sel4_kernel.sh` auto-detects architecture and removes manual selection.
- `cohesix_fetch_build.sh` selects Rust target based on `uname -m` with no prompts.

## [v0.164] - 2026-01-08
### Fixed
- seL4 kernel build works on x86_64 and aarch64 with automatic toolchain checks.
- `fetch_sel4.sh` validates host architecture and compiler availability.
- Missing PyYAML dependency added to requirements and installed during build.

## [v0.165] - 2026-01-09
### Fixed
- `build_sel4_kernel.sh` sets required CMake options and writes defaults to
  `settings.cmake`.
- Ninja now builds `kernel.elf` target on both architectures.

## [v0.166] - 2026-01-10
### Fixed
- `build_sel4_kernel.sh` forces `KernelWordSize` in CMake cache and adds
  classification headers. Compatible with Jetson Orin Nano (aarch64).

## [v0.167] - 2026-01-20
### Added
- `make_grub_iso.sh` builds `init.efi`, generates fallback `config.yaml`, and
  validates ISO boot via QEMU. Cleans staging on failure.
- Root wrapper `make_iso.sh` bumped for consistency.


## [v0.168] - 2026-01-20
### Added
- Minimal `/bin/init` launcher and placeholder `/bin/rc` script for boot.
- ISO staging directory moved to `out/iso`.
- Build scripts copy userland binaries into the ISO image.

## [v0.169] - 2026-01-25
### Added
- Python CLI cohbuild orchestrates kernel, userland, and ISO build steps.
- cohrun can boot the latest ISO via QEMU when --iso is provided.
- cohtrace gains "capture" subcommand to save boot or command traces.
- cohcap now includes "show" to list capabilities for all workers.

## [v0.170] - 2026-01-26
### Added
- Static BusyBox build via `scripts/build_busybox.sh` installs to `out/bin/busybox`.
- ISO generation scripts link BusyBox applets (`sh`, `ls`, `cp`, `mv`, `echo`, `mount`, `cat`, `ps`, `kill`).
- BusyBox included in GRUB-based images and validation checks updated.

## [v0.171] - 2026-01-27
### Added
- Plan 9-style CLI tools `srv`, `mount`, `import`, and `exportfs` for basic namespace operations.
- Secure9P handshake now logs via `cohtrace` with stubbed certificate validation.

## [v0.172] - 2026-01-27
### Added
- `requirements.txt` now includes `jinja2==3.1.6` for templating support.

## [v0.173] - 2026-01-27
### Added
- `bootstrap_sel4_tools.sh` installs Python dependencies for seL4 build scripts.

## [v0.174] - 2026-01-28
### Fixed
- `build_sel4_kernel.sh` now checks for `gcc-aarch64-linux-gnu` when building `imx8mm_evk` regardless of host architecture and selects `aarch64-linux-gnu-gcc` when available.

## [v0.175] - 2026-01-29
### Changed
- `build_sel4_kernel.sh` installs `ninja-build` if Ninja is missing and re-exports `CMAKE_MAKE_PROGRAM` after installation.

## [v0.176] - 2026-02-01
### Changed
- `build_sel4_kernel.sh` creates `settings.cmake` with default `KernelWordSize` and `KernelSel4Arch` when missing.

## [v0.177] - 2026-02-01
### Changed
- `build_sel4_kernel.sh` now calls `bootstrap_sel4_tools.sh` to auto-install Python modules.

## [v0.178] - 2026-02-02
### Fixed
- `bootstrap_sel4_tools.sh` detects virtualenv and omits `--user` when installing Python dependencies to avoid pip errors.

## [v0.179] - 2026-02-03
### Added
- `build_root_elf.sh` enables `rapier` and optional `cuda` features when building `cohesix_root.elf`.

## [v0.180] - 2026-02-04
### Added
- `build_mandoc.sh` invoked from `cohesix_fetch_build.sh` to compile a static `mandoc` binary.
- All manual pages from `docs/man` and the `man` wrapper are bundled into the ISO.
- `scripts/make_grub_iso.sh` stages `man` files and binaries during ISO creation.

## [v0.181] - 2026-02-05
### Updated
- Community docs now reference GRUB and Multiboot2 boot flow.
- `BUILD_PLAN.md` notes `cohesix_fetch_build.sh` and root ELF generation.
- Renamed lower-case filenames (`cli.md`, `gui_orchestrator.md`, `README_Codex.md`, `examples_README.md`) to uppercase.
- Removed deprecated `NETWORKING.md` and updated license notes for QEMU.

## [v0.182] - 2026-02-11
### Added
- Refined demo scenarios; added "The Bee Learns" as flagship demo; removed weaker entries.

## [v0.183] - 2026-02-11
### Added
- Runnable implementations for all demo scenarios in `DEMO_SCENARIOS.md`.
- ISO build now bundles demo launchers and assets.
- Test scripts validate each demo under `tests/demos/`.

## [v0.184] - 2026-02-12
### Added
- `setup_build_env.sh` installs build dependencies and Python modules.
### Changed
- `bootstrap_sel4_tools.sh` now clones missing repositories and checks
  `settings.cmake` permissions.
- `build_sel4_kernel.sh` sources the new environment script, defines
  explicit CMake variables, and falls back to `Unix Makefiles` when
  Ninja is unavailable.

## [v0.185] - 2026-02-13
### Fixed
- sel4 fetch script handles existing directories.
- ARM build installs gcc-aarch64-linux-gnu.
- CUDA apt key uses /etc/apt/keyrings.

## [v0.186] - 2026-02-14
### Changed
- `build_sel4_kernel.sh` auto-detects host architecture, selects the matching
  toolchain, cleans the build directory, and builds with Ninja.

## [v0.187] - 2026-02-15
### Fixed
- Kernel build now uses `host_arch` or `COH_ARCH` to select the toolchain
  automatically without prompts.
- Toolchain scripts are executed via `bash` to avoid sourcing issues.

## [v0.188] - 2026-02-15
### Added
- ISO build now validates man pages with `mandoc` and installs them to `/usr/share/man`.
- `/srv/cuda` and `/sim` directories are guaranteed in the ISO for GPU and physics support.

## [v0.189] - 2026-02-16
### Changed
- `build.rs` now respects the `COH_GPU` environment variable and only attempts to
  compile PTX with `nvcc` when `COH_GPU=1`. Otherwise it uses the prebuilt PTX
  with a single warning message.
- `setup_build_env.sh` uses numeric Ubuntu identifiers for CUDA repo URLs and maps `amd64` to `x86_64`.


