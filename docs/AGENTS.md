<!-- Author: Lukas Bower -->
# Agent Guidelines

## Scope & Targets
- Long-term deployment target: UEFI-booted ARM64 hardware running upstream seL4 with Cohesix providing the userspace stack.
- Current development/CI harness: QEMU `aarch64/virt` mirroring the hardware profile; any VM-specific packaging is expected to match physical UEFI behaviour.

## Roles
- **Queen (hive orchestrator)** — Exactly one per hive. The Queen manages many workers through `/queen/ctl` and Secure9P, creating, configuring, and revoking worker instances while holding the authoritative view of the hive namespace.
- **Workers** — Many per hive across worker-heart, worker-gpu, and future worker types, each constrained to their role-specific mounts and tickets.

## Tooling Alignment
- `cohsh` is the intended entry point for both human operators and automated agents. Any GUI or host-side tooling, including the planned WASM hive dashboard, should reuse the `cohsh` protocol rather than introducing new RPC surfaces.
