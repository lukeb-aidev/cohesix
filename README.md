<!-- Author: Lukas Bower -->
# Cohesix
## Why Cohesix?
I spent the early part of my career in film and braodcast technology, working with brilliant and creative people pushing the bounds of hardware and software. Some of that work involved custom OS development, and I have been fascinated by this ever since.

Cohesix is a Research OS project, where I am exploring ideas I have been pondering for a long time - which I can now explore with the support of AI agents in my limited free time. Writing an OS is the Mount Everest of technical challenges (which is part of the fun), but Cohesix also has some specific research goals:

- Validate the feasibility of a secure control plane to provide centralised and highly secure orchestration of edge GPU nodes
- Explore a Queen/Worker paradigm for one-to-many orchestration and telemetry
- Design a hive protocol based on Plan 9's 9P that manages the entire hive as a unit, underpinned by a client (called 'cohsh') that runs on Linux/Unix
- Prove out a unified UI concept, leveraging 'cohsh', that provides a single hive-wide WASM interface on Linux/Unix
- Integrate seamlessly with existing GPU edge ecosystems, such as NVidia CUDA on Linux/Jetson

## What is Cohesix?
Cohesix is a pure Rust userspace stack running atop upstream seL4 on `aarch64/virt (GICv3)`. Currently Userspace ships as a static rootfs CPIO containing the root task, NineDoor 9P server, workers, and Linux host-facing tools; all control flows through Secure9P. Operators interact via two consoles: the always-on PL011 root console and the remote TCP NineDoor console consumed by `cohsh`.

Cohesix is designed for physical ARM64 hardware booted via UEFI as the primary deployment environment. Today’s reference setup runs on QEMU `aarch64/virt` for bring-up, CI, and testing, and QEMU behaviour is expected to mirror the eventual UEFI board profile.

Cohesix targets a number of [use cases](docs/USE_CASES.md) focused on edge management.

Cohesix is NOT intended to replace general purpose operating systems. Developers using Cohesix should focus on its design goals of secure orchestration and telementry. Choesix deliberately avoid POSIX and Linux libraries to keep its surface area small and highly secure - Cohesix developers should embrace this design principle.

## Getting Started
- Build and launch via `scripts/cohesix-build-run.sh`, pointing at your seL4 build and desired output directory; the script stages host tools alongside the VM image and enables the TCP console when `--transport tcp` is passed.
- Terminal 1: run the build script to start QEMU with `-serial mon:stdio` for the PL011 root console and TCP forwarding for the NineDoor console.
- Terminal 2: from `out/cohesix/host-tools/`, connect with `./cohsh --transport tcp --tcp-port <port>` to reach the TCP console; `cohsh` runs on the host only and mirrors the root console verbs.

## Architecture
Cohesix is structured as a hive: one Queen process orchestrates multiple worker roles (worker-heart, worker-gpu, and future variants) over a shared Secure9P namespace. Cohsh is the command surface for this hive, used by human operators and automation alike. Cohesix exposes a minimal control plane over Secure9P: the root task owns initial capabilities and schedulers, NineDoor presents the synthetic namespace, and all role-specific actions are file-driven under `/queen`, `/worker/<id>`, `/log`, and `/gpu/<id>`. Local operators rely on the PL011 console for bring-up, while remote operators attach through the TCP NineDoor console without entering the VM. The stack keeps CUDA/NVML and other heavy dependencies outside the TCB and host VM.

## Components
- **root-task** — seL4 bootstrapper configuring capabilities, timers, and the cooperative event pump; publishes the root console and hands initial caps to NineDoor to underpin the hive-wide namespace shared by the Queen and its workers.
- **nine-door** — Secure9P server exposing `/proc`, `/queen`, `/worker`, `/log`, and (host-fed) `/gpu` namespaces with role-aware mount tables, forming the shared hive namespace.
- **worker-heart** — Minimal worker emitting heartbeat telemetry into `/worker/<id>/telemetry` and reading boot/log views per its ticket; a worker role scheduled and managed by the Queen.
- **worker-gpu** — VM-resident stub consuming GPU lease/ticket files and telemetry hooks; it never touches hardware, deferring to host bridge nodes; another worker role under Queen control.
- **cohsh** — Host-only CLI that connects to the TCP NineDoor console, attaches with role/ticket pairs, and mirrors root console commands for operators; it is the canonical shell for the hive, and planned GUI clients are expected to speak the same protocol.
- **gpu-bridge-host** — Host-side process that discovers or mocks GPUs, enforces leases, and mirrors `/gpu/<id>/` nodes into the VM via Secure9P transport adapters.
- **secure9p-wire** — Codec/transport crate providing bounded 9P framing for NineDoor and host tools, including the TCP adapter (host-only).
- **Future tooling** — Planned host-side WASM “hive dashboard” that reuses the cohsh protocol and adds no in-VM services.

## Status
- Milestones 0–4: repository scaffolding, seL4 boot/timer/IPC bring-up, Secure9P namespace, and bind/mount semantics are implemented per `docs/BUILD_PLAN.md`.
- Milestones 5–6: hardening, fuzz/integration coverage, and GPU role/bridge scaffolding are in place; worker-gpu remains namespace-only pending host bridge wiring.
- Milestone 7a–7c: cooperative event pump, authenticated dual consoles (PL011 + TCP), and namespace-aligned docs are live; future milestones extend worker lifecycle automation and GPU lease renewals.

## References
See `docs/ARCHITECTURE.md`, `docs/USERLAND_AND_CLI.md`, `docs/SECURE9P.md`, `docs/ROLES_AND_SCHEDULING.md`, `docs/GPU_NODES.md`, and `docs/BUILD_PLAN.md` for detailed design, interfaces, and milestone tracking.
