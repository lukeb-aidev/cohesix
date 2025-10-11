<!--
Name: AGENTS.md
Purpose: Web Codex orchestration charter for rebuilding Cohesix with Queen/Worker roles & GPU node roadmap.
Version: 0.2
Author: Lukas Bower
Date: 2025-10-10
-->

# AGENTS — Cohesix Charter (Pure Rust Userspace, ARM64, Roles-Aware)

## Scope
- Host: **macOS 26 on Apple Silicon (M4)**. Target: **QEMU aarch64/virt (GICv3)**.
- Kernel: **seL4 external** (no vendoring).
- Userspace: **pure Rust** root task + 9P server (**NineDoor**) + workers.
- Control model: **Queen/Worker** with **Roles** and capability tickets.

## Non‑Goals (v0–v1)
- No BusyBox/POSIX userspace. No TCP inside TCB. No GPU on v0 VM.
- CUDA/NVML support lives **outside TCB**; integrate later as remote workers.

## Web Codex Task Contract (use verbatim)
**Title/ID**, **Goal**, **Inputs**, **Changes**, **Commands (Mac ARM64)**, **Checks**, **Deliverables**.

### Acceptance Gates (anti‑drift)
- Implement **only** APIs described in `INTERFACES.md` and `ROLES_AND_SCHEDULING.md` for the current milestone.
- For any new file type/path, update the relevant doc or **reject the diff**.
- Keep CPIO under 4MB. Do not add TCP servers to TCB.

## Roles in Development
- Prefer **role-labeled crates** and tests (see `ROLES_AND_SCHEDULING.md`).
- Workers must access the system **only** through 9P files mounted for their role.


**Security note:** 9P policy and layering are defined in `SECURE9P.md`. Follow it for codec/core reuse and to keep TLS outside the VM.
