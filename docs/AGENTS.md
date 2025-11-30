<!-- Author: Lukas Bower -->
# Agent Guidelines

## Roles
- **Queen (hive orchestrator)** — Exactly one per hive. The Queen manages many workers through `/queen/ctl` and Secure9P, creating, configuring, and revoking worker instances while holding the authoritative view of the hive namespace.
- **Workers** — Many per hive across worker-heart, worker-gpu, and future worker types, each constrained to their role-specific mounts and tickets.

## Tooling Alignment
- `cohsh` is the intended entry point for both human operators and automated agents. Any GUI or host-side tooling, including the planned WASM hive dashboard, should reuse the `cohsh` protocol rather than introducing new RPC surfaces.
