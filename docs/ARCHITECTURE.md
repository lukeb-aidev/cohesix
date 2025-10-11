<!-- Author: Lukas Bower -->
# Cohesix Architecture Overview

## 1. System Boundaries
- **Kernel**: Upstream seL4 for `aarch64/virt (GICv3)`; treated as an external dependency that provides the capability system, scheduling primitives, and IRQ/timer services.
- **Userspace**: Entirely Rust, delivered as a CPIO rootfs containing the root task and all services.
- **Host Tooling**: macOS 26 (Apple Silicon M4) developer workstation running QEMU for validation, plus auxiliary host workers (e.g., GPU bridge) that communicate with the VM over 9P or serial transports.

## 2. High-Level Boot Flow
1. **seL4 Bootstraps** using the external elfloader and enters the Cohesix root task entry point.
2. **Root Task Initialisation**
   - Configures serial logging and prints the boot banner.
   - Establishes a periodic timer and registers IRQ handlers.
   - Creates the capability space for initial services, including the 9P endpoint and worker slots.
3. **Service Bring-up**
   - Spawns the **NineDoor** 9P server task and hands it the root capability set.
   - Registers static providers that expose `/proc`, `/queen`, `/log`, and the worker namespace.
4. **Operational State**
   - Queen and worker processes attach through NineDoor, exchanging capability tickets that encode their role and budgets.
   - The queen drives orchestration by appending JSON commands to `/queen/ctl`.
   - Telemetry and logs are streamed through append-only files in `/worker/<id>/telemetry` and `/log/queen.log`.

## 3. Component Responsibilities
### Root Task (crate: `root-task`)
- Owns seL4 initial caps, configures memory, and manages scheduling budgets.
- Provides a minimal RPC surface to NineDoor for spawning/killing tasks and for timer events.
- Enforces budget expiry (ticks, ops, ttl) and revokes capabilities on violation.

### NineDoor 9P Server (crate: `nine-door`)
- Implements the Secure9P codec/core stack and publishes the synthetic namespace.
- Delegates permission checks to a role-aware `AccessPolicy` using capability tickets minted by the root task.
- Tracks per-session state (fid tables, msize) and ensures append-only semantics on log/telemetry nodes.

### Workers (crate family: `worker-*`)
- Spawned by queen commands; each worker receives a ticket describing its role and budget.
- Communicate exclusively through their mounted NineDoor namespace—no raw IPC between workers.
- Heartbeat workers emit periodic telemetry; future GPU workers coordinate with host GPU bridges.

### Host GPU Bridge (future, crate: `gpu-bridge`)
- Runs **outside** the VM, using NVML/CUDA to manage real hardware.
- Mirrors GPU control surfaces into the VM via a 9P transport adapter (`secure9p-transport::Tcp` on the host side only).
- Maintains lease agreements and enforces memory/stream quotas independent of the VM.

## 4. Namespaces & Mount Tables
- Each session is mounted according to role:
  - **Queen**: `/`, `/queen`, `/proc`, `/log`, `/worker/*`, `/gpu/* (future)`.
  - **WorkerHeartbeat**: `/proc/boot`, `/worker/self/telemetry`, `/log/queen.log (read-only)`.
  - **WorkerGpu (future)**: Worker heartbeat view + `/gpu/<lease>/*` nodes.
- `bind` and `mount` operations are implemented via per-session mount tables maintained by NineDoor. Operations are scoped to a single path (no union mounts) and require queen privileges.

## 5. Capability & Role Model
- **Ticket**: 32-byte capability minted by the root task, bound to `{role, budget, mounts}`.
- **Session**: Contains ticket, negotiated `msize`, fid allocator, and mount table.
- NineDoor verifies every `walk`/`open`/`write` call against the ticket role and append/read mode before delegating to the provider.

## 6. Data Flow Highlights
- **Queen Control**: Append JSON commands to `/queen/ctl`; NineDoor forwards valid commands to root-task orchestration APIs.
- **Telemetry**: Workers append newline-delimited status records to `/worker/<id>/telemetry`. NineDoor enforces append-only semantics by ignoring offsets.
- **Logging**: Root task and queen append to `/log/queen.log`; workers read logs read-only for situational awareness.
- **GPU Integration (future)**: Host bridge exposes GPU metadata/control/job/status nodes; WorkerGpu instances mediate job submission and read back status via NineDoor.

## 7. Reliability & Security Considerations
- Minimal trusted computing base: no POSIX layers, no TCP servers inside the VM, no dynamic loading.
- All inter-process communication is file-based via 9P; no shared memory between workers.
- Timer and watchdog infrastructure ensures runaway workers are revoked cleanly.
- NineDoor core is `no_std + alloc` capable, allowing potential reuse in bare-metal contexts.

## 8. Roadmap Dependencies
- **Milestone alignment**: Architecture is realised incrementally per `BUILD_PLAN.md` milestones.
- **Documentation as Source of Truth**: Changes to components or interfaces must be reflected here to avoid drift.
