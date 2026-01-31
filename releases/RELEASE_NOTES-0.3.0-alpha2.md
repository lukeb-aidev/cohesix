<!-- Copyright (c) 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Release notes for Cohesix 0.3.0-alpha2. -->
<!-- Author: Lukas Bower -->
# Cohesix 0.3.0-alpha2 Release Notes

Date: 2026-01-31

## Highlights
- Live GPU bridge publish path that installs `/gpu/<id>`, `/gpu/models/*`, and `/gpu/telemetry/schema.json` into a running Queen.
- Non-mock PEFT flow support with `coh peft import --publish` to refresh `/gpu/models` after registry updates.
- Host-side telemetry adapters for systemd/k8s/docker/NVML with bounded, line-based snapshots under `/host/*`.
- Live Hive telemetry text overlays (last N lines) plus selectable details panel with bounds enforced in `cohsh-core`.
- Updated docs, manifests, and regression fixtures to keep as-built alignment.

## Bundled tools
- `cohsh`, `coh`, `swarmui`, `cas-tool`, `gpu-bridge-host`, `host-sidecar-bridge`
- Python client under `python/cohesix-py`
- QEMU run script under `qemu/run.sh`

## Quickstart
See `QUICKSTART.md` in the bundle. The alpha workflow remains QEMU-based and no UEFI
bring-up is included in this release.

## Notes
- GPU hardware access remains host-side only; the VM never touches CUDA/NVML directly.
- Live GPU bridge publish is required for non-mock PEFT flows and `/gpu/models` visibility.
