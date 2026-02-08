<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Release notes for Cohesix 0.6.0-alpha. -->
<!-- Author: Lukas Bower -->
# Cohesix 0.6.0-alpha Release Notes

Date: 2026-02-08

## Highlights
- Refresh of generated manifests, policies, and documentation aligned to current coh-rtc outputs.
- SwarmUI Live Hive UI updates alongside console/telemetry presentation refinements.
- Release bundle refresh with updated host tools and SMP/QEMU defaults.

## Carry-Forward From 0.5.0-alpha
- SMP build flow aligned to the seL4 SMP artifacts with PSCI `smc` DTB generation for QEMU bring-up on macOS/TCG.
- `selftest_smp.coh` is bundled and served via `/proc/tests/selftest_smp.coh`, enabling `cohsh test --mode smp` on fresh SMP boots.
- `cohsh --transport qemu` uses the 4-core SMP defaults and honors `COHESIX_QEMU_SMP*` overrides.
- macOS host tools default to FUSE disabled; Linux remains FUSE-enabled by default. macOS FUSE is opt-in via `--features fuse` with MacFUSE installed.
- QEMU bundle `qemu/run.sh` supports `COHESIX_QEMU_VIRT` and `COHESIX_QEMU_MACHINE_EXTRA` overrides to handle hosts without EL2 or in-kernel GICv2 support.
- Documentation updates across BUILD_PLAN, TEST_PLAN, HOST_TOOLS, USERLAND_AND_CLI, QUICKSTART, SECURITY, and Python support to reflect SMP scope and host tool behavior.

## Bundled tools
- `cohsh`, `coh`, `swarmui`, `cas-tool`, `gpu-bridge-host`, `host-sidecar-bridge`, `hive-gateway`
- Python client under `python/cohesix-py`
- QEMU run script under `qemu/run.sh`

## Quickstart
See `QUICKSTART.md` in the bundle. The alpha workflow remains QEMU-based and no UEFI
bring-up is included in this release.

## Notes
- Cohesix 0.6.0-alpha supersedes 0.5.0-alpha; the 0.5.0-alpha bundles should not be used.
- GPU hardware access remains host-side only; the VM never touches CUDA/NVML directly.
- Live GPU bridge publish is required for non-mock PEFT flows and `/gpu/models` visibility.
