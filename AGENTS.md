<!-- Author: Lukas Bower -->
# AGENTS — Cohesix Rebuild Charter (Pure Rust Userspace, ARM64)

## Scope & Targets
- **Host**: macOS 26 on Apple Silicon (M4).
- **Target VM**: QEMU `aarch64/virt` with GICv3.
- **Kernel**: Upstream seL4 (external; never vendored).
- **Userspace**: Pure Rust root task, NineDoor 9P server, worker suites, and host-side GPU bridge tools.

## Kernel Build Artefacts
- Reference kernel build outputs (headers, slot layouts, generated metadata) reside in `seL4/build/`.

## Operating Rules
1. **Single Source of Truth** — This `AGENTS.md` plus `/docs/*.md` constitute canonical guidance. Update them alongside code.
1a. **Compiler Alignment** — All manifests and generated artefacts (`root_task.toml`, `coh-rtc` outputs) define the system state. Code or docs that diverge from compiler output are invalid; regenerate manifests instead of editing generated code.
2. **No Scope Creep** — Implement only items sanctioned by the active milestone in `BUILD_PLAN.md`.
3. **Atomic Work** — Each task must land with compiling code (`cargo check`) and updated tests/docs. Keep commits focused.
4. **Tiny TCB** — No POSIX emulation layers, or TCP servers inside the VM. GPU integration stays outside the VM. The code footprint must be self-contained and secure. Sidecars run outside the seL4 VM whenever possible; only lightweight control stubs (e.g., LoRa schedulers) may execute inside the VM under manifest quotas.
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
- **Planner** — Breaks milestones into atomic tasks using the template above and ensures each new capability or provider field is represented in the compiler IR.
- **Builder** — Implements code/tests, runs commands, documents results.
- **Auditor** — Reviews diffs for scope compliance, verifies generated artefacts match manifest fingerprints, and updates docs.

## Guardrails
- Keep rootfs CPIO under 4 MiB (see `ci/size_guard.sh`).
- 9P server runs in userspace; transports abstracted (no direct TCP in VM).
- GPU workers run on host/edge nodes; VM only sees mirrored files.
- Document new file types/paths before committing code that depends on them.
- Changes to `/docs/*.md` must reflect the as-built state produced by the compiler (snippets, schemas, `/proc` layouts).

## Canonical Documents
docs/*.md
seL4/seL4-manual-latest.md
seL4/elfloader.md

## seL4
A full seL4 build tree can be found in:
seL4

## Security & Testing Expectations
- Validate all user-controlled input (9P frames, JSON commands).
- No hardcoded secrets; use config or tickets.
- Update or add tests whenever behaviour changes; list executed commands in PR summaries.
- Run `cargo run -p coh-rtc` and verify regenerated artefacts hash-match committed versions before merge.

