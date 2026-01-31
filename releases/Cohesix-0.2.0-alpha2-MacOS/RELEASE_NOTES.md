<!-- Copyright (c) 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Release notes for Cohesix 0.2.0-alpha2. -->
<!-- Author: Lukas Bower -->
# Cohesix 0.2.0-alpha2 Release Notes

Date: 2026-01-27

## Highlights
- `coh` host bridge CLI for mount, GPU lease/status, telemetry pull, run, and PEFT flows.
- Telemetry ingest with OS-named segments (`cohsh telemetry push` + `coh telemetry pull`).
- Lifecycle controls and `/proc` cut signals (`cohsh lifecycle`, `/proc/lifecycle/*`, `/proc/root/*`).
- SwarmUI Live Hive view with embedded console panel; header alignment unified.
- `coh doctor` for deterministic host checks and mock-mode onboarding.
- `cohesix` Python client (filesystem + TCP backends) with bounded, inspectable examples.
- Updated release bundle packaging: docs, python client, and quickstart artifacts included.

## Bundled tools
- `cohsh`, `coh`, `swarmui`, `cas-tool`, `gpu-bridge-host`, `host-sidecar-bridge`
- Python client under `python/cohesix-py`
- QEMU run script under `qemu/run.sh`

## Quickstart
See `QUICKSTART.md` in the bundle. The alpha workflow remains QEMU-based and no UEFI
bring-up is included in this release.

## Notes
- SwarmUI on headless Linux requires `xvfb-run`.
- GPU access remains host-side only; the VM never touches CUDA/NVML directly.
