// CLASSIFICATION: COMMUNITY
// Filename: boot_log_analysis_plan.md v0.2
// Author: Lukas Bower
// Date Modified: 2030-08-09

# Boot Log Analysis and Remediation Plan

## Task Title & ID
- **Task**: AGENT:BOOT_LOG_PLAN
- **Epic/Feature Alignment**: E5-F13 (Boot Performance Instrumentation)

## Observations from Boot Log
1. **Rootserver build configuration**
   - Log indicates the rootserver payload was stripped, yet residual debug sections remain (`Rootserver still contains debug sections; install aarch64 binutils ... or enable the Cohesix release profile override`).
   - Payload size is **346,807,160 bytes**, which is significantly larger than expected for a release artifact.

2. **Interrupt controller configuration**
   - Kernel reports `Warning: Could not infer GIC interrupt target ID, assuming 0.` implying the GIC Redistributor or affinity routing information is absent or malformed in the device tree blob (DTB).

3. **IOPT level absence**
   - Userland reports `IOPT levels: 0`, suggesting stage-2 translation tables are not configured; this is typical for bare-metal boot but should be confirmed against platform expectations to avoid missing MMU stages for devices requiring passthrough.

4. **CPIO payload sourcing**
   - QEMU boot uses an external CPIO image (`/boot/cohesix.cpio`), skipping embedded elfloader payload. This is acceptable but requires validation that CI reproduces the same path.

## Proposed Remediation Plan

### 1. Harden Rootserver Release Build
- Enforce the release profile by enabling the Cohesix release override in `workspace/Cargo.toml` or associated profile configuration.
- Add CI validation to run `aarch64-linux-gnu-strip --strip-debug` on the rootserver artifact and fail when debug sections remain.
- Document dependency on `aarch64-linux-gnu-binutils` within `requirements.txt` or developer setup scripts to prevent tooling drift.
- Track payload size in CI (e.g., `stat --format=%s`) and set thresholds to catch regressions beyond release expectations.

### 2. Correct GIC Target Identification
- Inspect `boot/kernel.dts` (or source DT) to verify `interrupt-controller` nodes include `#redistributor-regions` and CPU affinity properties required by seL4 on `virt`.
- Ensure PSCI and GICv3 configuration is explicitly defined (`compatible = "arm,gic-v3"`, `redistributor-stride`, `reg` entries) so the kernel can compute target IDs without defaulting to zero.
- Add regression test (e.g., QEMU boot check script) asserting absence of the warning to catch future regressions.

### 3. Validate IOPT Expectations
- Confirm with seL4 configuration whether zero IOPT levels are expected for the target; if not, enable the required stage-2 translation tables via Kconfig or board support patches.
- Add unit tests or integration checks in `tests/` ensuring critical devices map through the correct I/O page tables.

### 4. Align CPIO Workflow Across Environments
- Update build documentation to emphasize external CPIO usage and provide a deterministic path for CI.
- Create a smoke test in CI that boots QEMU with both embedded and external payloads to ensure parity.

## Next Steps
1. Draft detailed tasks for each remediation item and link them to PI-2029.3 boot telemetry objectives.
2. Update developer setup documentation with new tooling requirements and CI checks.
3. Execute CI pipeline with new validations and gather boot telemetry confirming the warnings are resolved and payload sizes remain within thresholds.

## Evidence Hooks
- Store future boot log captures under `/log/boot/elf_checks/` with trace IDs per SAFe audit requirements.

## Implementation Summary
- `cohesix_fetch_build.sh` now rebuilds `kernel.dtb` via `dtc`, enforces `aarch64-linux-gnu-strip --strip-debug`, and fails when payloads exceed the 1 MiB release cap.
- `ci/rootserver_release_check.sh` executes the strip/readelf validation before QEMU boots; `ci/qemu_boot_check.sh` rejects GIC target warnings and confirms expected IOPT levels from `platform_gen.h`.
- Documentation (`README.md`) lists `aarch64-linux-gnu-binutils` and `dtc` as mandatory host dependencies so CI tooling remains reproducible.

