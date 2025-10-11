<!--
Name: AGENTS.md
Purpose: Web Codex orchestration charter for rebuilding Cohesix (Plan9-inspired OS on seL4) in pure Rust userspace.
Version: 0.1
Author: Lukas Bower
Date: 2025-10-10
-->

# AGENTS — Cohesix Rebuild Charter (Pure Rust Userspace, ARM64)

## Scope
- Target: **Apple Silicon (M4) host**, QEMU **aarch64/virt (GICv3)**.
- Kernel: **seL4 upstream, treated as external** (no vendoring).
- Userspace: **pure Rust** (root task, services, 9P server, queen/worker).
- Goal: Deterministic **boot + timer + IPC**, then **9P namespaces** and **queen/worker**.

## Web Codex Operating Rules
1. **Single Source of Truth**: This document + `BUILD_PLAN.md` + `ARCHITECTURE.md` and `TOOLCHAIN_MAC_ARM64.md`.
2. **No Scope Creep**: Only implement items explicitly listed in the current milestone's “Deliverables & Checks”.
3. **Atomic Commits**: Each task results in compiling code (`cargo check`) and updated docs.
4. **Zero vendoring**: seL4 remains external. Do *not* copy third-party code into this repo.
5. **Keep the TCB tiny**: No BusyBox, no POSIX shims, no TCP inside TCB.

## Web Codex Task Format (use verbatim)
**Title/ID:** Concise slug (e.g., `root-task-timer-v0`)  
**Goal:** One-sentence outcome.  
**Inputs:** Files/paths + versions.  
**Changes:** Bullet list of files to create/modify.  
**Commands:** Exact shell commands to run (Mac ARM64).  
**Checks:** Deterministic success conditions (builds/tests run, strings in output).  
**Deliverables:** Files to commit and short summary.

Example:
```
Title/ID: root-task-pingpong-v0
Goal: Root task boots and ping-pongs one IPC message with a worker task.
Inputs: external seL4 build (qemu-arm-virt), paths: out/elfloader, out/kernel.elf, out/rootfs.cpio
Changes:
  - apps/root-task/src/main.rs: add timer + ping API
  - apps/worker-heart/src/lib.rs: add handle_ping()
Commands:
  - cargo check -q
  - ./scripts/qemu-run.sh out/elfloader out/kernel.elf out/rootfs.cpio
Checks:
  - Serial contains: "PING 1", "PONG 1"
Deliverables:
  - Diff of edited files
  - Updated ARCHITECTURE.md section "IPC"
```

## Agent Roles
- **Planner**: Expands milestone into atomic tasks using the format above.
- **Builder**: Writes code, runs commands, ensures checks pass.
- **Auditor**: Verifies diff, rejects scope creep, updates docs.

## Guardrails
- ARM64-only (aarch64 QEMU virt).  
- Minimal CPIO (< 4 MB).  
- No GPU/Jetson/containers.  
- 9P server is **userland** and capability-aware.  
- Queen/Worker APIs are append/read files via 9P only.
