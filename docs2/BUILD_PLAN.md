# Cohesix Build Plan (ARM64, Pure Rust) — v0/v1

Host: macOS 26 (M4) • Target: QEMU aarch64/virt (GICv3) • Kernel: seL4 (external)

## M0 — Skeleton & Toolchain (1–2 days)
- Workspace with crates: `root-task`, `nine-door`, `worker-heart`
- QEMU runner for aarch64/virt; CPIO size guard; toolchain script
**Checks:** `cargo check`, QEMU present

## M1 — Boot + Timer + IPC (3–5 days)
- Root banner + periodic tick, spawn second task, single ping/pong
**Checks:** serial shows tick, PING/PONG

## M2 — NineDoor Minimal 9P (1–2 weeks)
- 9P2000.L codec, fid table, synthetic FS: `/proc/boot`, `/log/queen.log`, `/queen/ctl`, `/worker/<id>/telemetry`
**Checks:** read `/proc/boot`, append `/queen/ctl`

## M3 — Queen/Worker MVP with Roles (1 week)
- Implement **Roles** (Queen, Worker:Heartbeat) and role-scoped mounts
- `/queen/ctl` JSON: `{"spawn":"heartbeat","ticks":100}`, `{"kill":"<id>"}`
**Checks:** spawn/telemetry/kill; role isolation enforced

## M4 — Bind/Mount Namespaces (1 week)
- Per-process mount tables; `bind` and `mount` (single path)
**Checks:** remap visible to caller only

## M5 — Hardening & Tests (ongoing)
- Unit tests (codec, fid lifecycle, providers); negative/fuzz for decoder

## M6 — GPU Worker Integration (out-of-VM, optional) (2–3 weeks)
- Define **GPU Worker Role** + remote worker protocol (host tool or edge node)
- Host-side **CUDA probe** via NVML; 9P‑exposed control files: `/gpu/<id>/info`, `/gpu/<id>/ctl`
- Simple job types: vector add, matmul; streams and mem limits as **leases**
**Checks:** spawn remote GPU worker from queen; collect telemetry lines; enforce lease expiry

> CUDA/NVML stays **outside** the seL4 VM; the VM sees only 9P files reflecting GPU leases and job state.
