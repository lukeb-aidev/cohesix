<!-- Author: Lukas Bower -->
# Cohesix

Cohesix is a pure Rust userspace stack running atop upstream seL4 on `aarch64/virt (GICv3)` under QEMU. Userspace ships as a static rootfs CPIO containing the root task, NineDoor 9P server, workers, and host-facing tools; all control flows through Secure9P. Operators interact via two consoles: the always-on PL011 root console and the remote TCP NineDoor console consumed by `cohsh`.

## Getting Started
- Build and launch via `scripts/cohesix-build-run.sh`, pointing at your seL4 build and desired output directory; the script stages host tools alongside the VM image and enables the TCP console when `--transport tcp` is passed.
- Terminal 1: run the build script to start QEMU with `-serial mon:stdio` for the PL011 root console and TCP forwarding for the NineDoor console.
- Terminal 2: from `out/cohesix/host-tools/`, connect with `./cohsh --transport tcp --tcp-port <port>` to reach the TCP console; `cohsh` runs on the host only and mirrors the root console verbs.

## Architecture
Cohesix exposes a minimal control plane over Secure9P: the root task owns initial capabilities and schedulers, NineDoor presents the synthetic namespace, and all role-specific actions are file-driven under `/queen`, `/worker/<id>`, `/log`, and `/gpu/<id>`. Local operators rely on the PL011 console for bring-up, while remote operators attach through the TCP NineDoor console without entering the VM. The stack keeps CUDA/NVML and other heavy dependencies outside the TCB and host VM.

## Components
- **root-task** — seL4 bootstrapper configuring capabilities, timers, and the cooperative event pump; publishes the root console and hands initial caps to NineDoor.
- **nine-door** — Secure9P server exposing `/proc`, `/queen`, `/worker`, `/log`, and (host-fed) `/gpu` namespaces with role-aware mount tables.
- **worker-heart** — Minimal worker emitting heartbeat telemetry into `/worker/<id>/telemetry` and reading boot/log views per its ticket.
- **worker-gpu** — VM-resident stub consuming GPU lease/ticket files and telemetry hooks; it never touches hardware, deferring to host bridge nodes.
- **cohsh** — Host-only CLI that connects to the TCP NineDoor console, attaches with role/ticket pairs, and mirrors root console commands for operators.
- **gpu-bridge-host** — Host-side process that discovers or mocks GPUs, enforces leases, and mirrors `/gpu/<id>/` nodes into the VM via Secure9P transport adapters.
- **secure9p-wire** — Codec/transport crate providing bounded 9P framing for NineDoor and host tools, including the TCP adapter (host-only).

## Status
- Milestones 0–4: repository scaffolding, seL4 boot/timer/IPC bring-up, Secure9P namespace, and bind/mount semantics are implemented per `docs/BUILD_PLAN.md`.
- Milestones 5–6: hardening, fuzz/integration coverage, and GPU role/bridge scaffolding are in place; worker-gpu remains namespace-only pending host bridge wiring.
- Milestone 7a–7c: cooperative event pump, authenticated dual consoles (PL011 + TCP), and namespace-aligned docs are live; future milestones extend worker lifecycle automation and GPU lease renewals.

## References
See `docs/ARCHITECTURE.md`, `docs/USERLAND_AND_CLI.md`, `docs/SECURE9P.md`, `docs/ROLES_AND_SCHEDULING.md`, `docs/GPU_NODES.md`, and `docs/BUILD_PLAN.md` for detailed design, interfaces, and milestone tracking.
