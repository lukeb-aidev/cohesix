# Cohesix 0.1.0-alpha1 (2026-01-23)

Status: First public alpha release.

## Highlights
- Secure9P control plane with Queen/Worker roles and file-shaped interfaces.
- Deterministic replay artifacts for CLI and SwarmUI demos.
- SwarmUI dual-mode UX: replay (default) and live read-only observation.
- Full host tool suite included in the bundle.
- Prebuilt VM image, QEMU runner, and Quickstart guide.

## Components included
- Host tools: cohsh, swarmui, cas-tool, gpu-bridge-host, host-sidecar-bridge.
- VM image: elfloader, kernel, rootserver, CPIO, manifest.
- Traces: canonical trace and hive snapshot with hashes.
- Documentation: architecture, interfaces, roles, and host tools.

## Security and trust model
- No POSIX layer; small TCB by design.
- No in-VM GPU access; GPU stacks remain host-side.
- Single authenticated TCP console listener in-VM.

## Compatibility
- Host: macOS 26 (Apple Silicon) or Ubuntu 24 aarch64.
- Target VM: QEMU aarch64/virt (GICv3).
- Kernel: upstream seL4 (external build).

## Known limitations (alpha)
- QEMU is the reference environment; UEFI hardware targets are future milestones.
- Live UI is read-only; all control remains CLI-driven.

## Assets
- releases/Cohesix-0.1-Alpha-MacOS.tar.gz
- releases/Cohesix-0.1-Alpha-linux.tar.gz
