<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Release notes for Cohesix 0.7.0-alpha. -->
<!-- Author: Lukas Bower -->
# Cohesix 0.7.0-alpha Release Notes

Date: 2026-02-11

## Highlights
- Manifest-driven SMP affinity defaults enabled by default, with authority core 0, ninedoor core 1, provider cores 2-3, and worker cores 2-3.
- Root task affinity wiring now applies these defaults to the Queen/NineDoor and worker spawn paths, with guard logging to validate core placement.
- REST performance harness hardened (lease and policy tracking, buffer-full handling, larger log tails), enabling long-run REST stress without flaky failures.
- SwarmUI REST parallelisation improved: the gateway can run host-local while tunneling only the remote console port (`31337`), reducing UI latency and keeping REST clients multiplexed behind a single console session.

## Bundled tools
- `cohsh`, `coh`, `swarmui`, `cas-tool`, `gpu-bridge-host`, `host-sidecar-bridge`, `hive-gateway`
- Python client under `python/cohesix-py`
- QEMU run script under `qemu/run.sh`

## Notes
- SMP parity remains gated by the single-core DTB flow; multi-core SMP validation and selftest coverage are complete.
- GPU hardware access remains host-side only; the VM never touches CUDA/NVML directly.
- SwarmUI, `cohsh`, and host publishers should use REST (`hive-gateway`) in parallel mode; direct TCP console attachment remains single-client.
