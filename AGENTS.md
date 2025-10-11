<!-- Author: Lukas Bower -->
# AGENTS — Cohesix Rebuild Charter (Pure Rust Userspace, ARM64)

## Scope & Targets
- **Host**: macOS 26 on Apple Silicon (M4).
- **Target VM**: QEMU `aarch64/virt` with GICv3.
- **Kernel**: Upstream seL4 (external; never vendored).
- **Userspace**: Pure Rust root task, NineDoor 9P server, worker suites, and host-side GPU bridge tools.

## Operating Rules
1. **Single Source of Truth** — This `AGENTS.md` plus `/docs/*.md` constitute canonical guidance. Update them alongside code.
2. **No Scope Creep** — Implement only items sanctioned by the active milestone in `BUILD_PLAN.md`.
3. **Atomic Work** — Each task must land with compiling code (`cargo check`) and updated tests/docs. Keep commits focused.
4. **Tiny TCB** — No BusyBox, POSIX emulation layers, or TCP servers inside the VM. GPU integration stays outside the VM.
5. **Capability Discipline** — Interactions occur through 9P namespaces using role-scoped capability tickets.
6. **Tooling Alignment** — Use the macOS ARM64 toolchain in `TOOLCHAIN_MAC_ARM64.md`. Do not assume Linux-specific tooling in VM code.

## Task Template (use verbatim in planning docs)
```
Title/ID: <slug>
Goal: <one sentence>
Inputs: <artefacts, versions, paths>
Changes:
  - <file> — <summary>
Commands: <exact shell commands (macOS ARM64)>
Checks: <deterministic success criteria>
Deliverables: <files, logs, doc updates>
```

## Roles
- **Planner** — Breaks milestones into atomic tasks using the template above.
- **Builder** — Implements code/tests, runs commands, documents results.
- **Auditor** — Reviews diffs for scope compliance and updates docs.

## Guardrails
- Keep rootfs CPIO under 4 MiB (see `ci/size_guard.sh`).
- 9P server runs in userspace; transports abstracted (no direct TCP in VM).
- GPU workers run on host/edge nodes; VM only sees mirrored files.
- Document new file types/paths before committing code that depends on them.

## Security & Testing Expectations
- Validate all user-controlled input (9P frames, JSON commands).
- No hardcoded secrets; use config or tickets.
- Update or add tests whenever behaviour changes; list executed commands in PR summaries.
