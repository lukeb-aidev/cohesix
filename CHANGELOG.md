// CLASSIFICATION: COMMUNITY
// Filename: CHANGELOG.md v0.41
// Date Modified: 2025-07-21
## [v0.71] - 2025-07-21
### Added
- HTTP route and CLI docs for GUI orchestrator.
### Changed
- README clarified usage and example output.

## [v0.72] - 2025-07-21
### Changed
- Refactored GUI orchestrator modules and added dev-mode logic.

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
// Date Modified: 2025-07-11
// Author: Lukas Bower
## [v0.43] - 2025-07-11
### Added
- Agent introspection namespace `/srv/agent_meta` with runtime files.
- World state summary `/srv/world_state/world.json`.
- Python and Go `agent_sdk` for runtime context.
- CLI `cohrun goal_*` and trust zone commands.
- `cohtrace trust_check` lists current trust zones.
// Filename: CHANGELOG.md v0.28
// Date Modified: 2025-06-08
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

