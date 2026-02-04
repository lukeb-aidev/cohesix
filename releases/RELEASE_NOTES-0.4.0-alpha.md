<!-- Copyright (c) 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Release notes for Cohesix 0.4.0-alpha. -->
<!-- Author: Lukas Bower -->
# Cohesix 0.4.0-alpha Release Notes

Date: 2026-02-03

## Highlights
- Authoritative scheduling/lease/export/policy control grammar with `/proc` observability for queue/lease state.
- Host-side REST gateway (`hive-gateway`) projecting the console/file semantics over HTTP.
- Test plan and demo runbooks updated for policy-gated approvals and deterministic selftests.
- Documentation refresh for Quickstart and operator workflows aligned to the as-built system.

## Bundled tools
- `cohsh`, `coh`, `swarmui`, `cas-tool`, `gpu-bridge-host`, `host-sidecar-bridge`, `hive-gateway`
- Python client under `python/cohesix-py`
- QEMU run script under `qemu/run.sh`

## Quickstart
See `QUICKSTART.md` in the bundle. The alpha workflow remains QEMU-based and no UEFI
bring-up is included in this release.

## Notes
- GPU hardware access remains host-side only; the VM never touches CUDA/NVML directly.
- Live GPU bridge publish is required for non-mock PEFT flows and `/gpu/models` visibility.
