<!-- Author: Lukas Bower -->
# AGENTS — Cohesix Build Charter (Pure Rust Userspace, ARM64)

You are an OS designer expert in seL4 and Rust on aarch64.  
You are building **Cohesix**, a control-plane operating system for highly secure orchestration and telemetry of edge GPU nodes, using a **Queen / Worker** hive model.

This document is **normative**. It is a binding contract for design, implementation, and documentation.  
Violations block merge. Warn of violations BEFORE completing tasks.

---

## Scope & Targets
- **Host**: macOS 26 (Apple Silicon, M-series).
- **Target VM**: QEMU `aarch64/virt` with GICv3.
- **Target Hardware**: UEFI `aarch64/virt` (details TBC).
- **Kernel**: Upstream seL4 (external; never vendored).
- **Userspace**: Pure Rust root task, NineDoor 9P server, worker roles, host-side client and GPU bridge tools.

## Kernel Build Artifacts
Kernel reference outputs (headers, slot layouts, generated metadata) live in:
```
seL4/build/
```

These artifacts define kernel-level truth. Code must align with them exactly.

---

## Operating Rules (Normative — Violations Block Merge)

1. **Canonical Sources**
   - `AGENTS.md`, `README.md`, and `/docs/*.md` are canonical.
   - Code that diverges from these documents is invalid unless the documents are updated **in the same change**.

2. **Compiler-Defined Reality**
   - Manifests and compiler-generated artifacts (`root_task.toml`, `coh-rtc` outputs) are the **sole authority** on system behavior.
   - Code or documentation that disagrees with generated output is **invalid by definition**.
   - The correct fix for disagreement is to update IR, regenerate artifacts, and update docs/tests — never to hand-edit generated code.

3. **No Scope Creep**
   - Only work explicitly sanctioned by the active milestone in `BUILD_PLAN.md` may be implemented.
   - “Preparation”, “cleanup”, or “future-proofing” outside the milestone is prohibited.

4. **Atomic Work**
   - Every change must:
     - compile (`cargo check`);
     - include required tests;
     - update documentation where behavior or interfaces change.
   - Partial or speculative changes are not permitted.

5. **Tiny TCB**
   - No POSIX emulation layers.
   - No libc-style abstractions.
   - No in-VM GPU stacks.
   - All heavy ecosystems (CUDA, NVML, networking sidecars) remain host-side.

6. **Capability Discipline**
   - All interactions occur via Secure9P namespaces and role-scoped capability tickets.
   - No ad-hoc RPC, shared memory shortcuts, or implicit authority.

7. **Simplicity & Correctness**
   - Implementations **MUST** prefer the simplest design that preserves:
     - seL4 semantics,
     - deterministic bounds,
     - manifest fidelity.
   - Convenience abstractions, refactors, or “cleanups” not required by the milestone are prohibited.

8. **Tooling Alignment**
   - Use the macOS ARM64 toolchain defined in `TOOLCHAIN_MAC_ARM64.md`.
   - Do not assume Linux tooling or POSIX facilities for VM code.

---

## Worker Bring-up
- The root task spawns **queen**, **worker-heart**, and **worker-gpu** per sequencing in `docs/BUILD_PLAN.md`.
- Scheduling contexts and budgets **must** follow `docs/ROLES_AND_SCHEDULING.md`.
- Workers operate exclusively via their mounted namespaces (e.g. `/worker/<id>`).
- All coordination is file- and event-driven via Secure9P.
- Host-initiated ad-hoc RPC does not exist.

## GPU Worker Boundaries
- **worker-gpu** handles only ticket/lease files and telemetry.
- All GPU hardware access lives in `gpu-bridge-host`.
- CUDA/NVML never enter the VM or the trusted computing base.

---

## Task Template (Use Verbatim)
```
Title/ID: <slug>
Goal: <one sentence>
Inputs: <artifacts, versions, paths>
Changes:
  - <file> — <summary>
Commands: <exact shell commands (macOS ARM64)>
Checks: <deterministic success criteria>
Deliverables: <files, logs, doc updates>
```

---

## Roles
- **Planner** — Breaks milestones into atomic tasks and ensures all new behavior is represented in compiler IR.
- **Builder** — Implements code/tests, runs commands, and documents results.
- **Auditor** — Verifies scope compliance, manifest hashes, generated artifacts, and docs-as-built alignment.
- **Queen / Workers** — Queen orchestrates control-plane actions; worker-heart emits telemetry; worker-gpu mirrors GPU lease state.

No other agent roles exist unless explicitly introduced in `BUILD_PLAN.md`.

---

## Guardrails

- **Console Networking Exception**
  - The only permitted in-VM TCP listener is the authenticated root-task console (smoltcp).
  - All other TCP services (9P-over-TCP, GPU control channels, etc.) are host-only.
  - This exception does not relax the general prohibition on networking services inside the VM.

- Rootfs CPIO **must remain < 4 MiB** (`ci/size_guard.sh`).
- The 9P server runs in userspace; transports are abstracted.
- GPU workers never expose raw device access inside the VM.
- New file types or paths **must be documented before code depends on them**.
- Documentation must describe the **as-built** system, not intent.

---

## Docs-as-Built Alignment (Mandatory from Milestone 8)

### 1. Docs → IR → Code
- Any new behavior **MUST** land as IR fields with validation and codegen.
- Builds fail if IR:
  - references disabled gates,
  - violates Secure9P bounds,
  - forces `std` where the runtime is `no_std`.

### 2. Autogenerated Snippets
- `coh-rtc` refreshes embedded snippets in:
  - `SECURE9P.md`
  - `INTERFACES.md`
  - `ARCHITECTURE.md`
- These snippets are authoritative and must not be edited by hand.

### 3. As-Built Guard
- CI compares:
  - generated file hashes,
  - manifest fingerprints,
  - committed doc excerpts.
- Drift fails CI and blocks merge.

**Any drift is a defect, even if CI does not yet catch it.**

### 4. Red Lines (Enforced)
- 9P2000.L only
- `msize ≤ 8192`
- walk depth ≤ 8
- no `..`
- no fid reuse after clunk
- no TCP listeners inside VM except the console
- rootfs CPIO < 4 MiB
- no POSIX façade
- VM artifacts remain `no_std`

### 5. Regression Pack (Milestone ≥ 8)
- All changes **MUST** re-run the shared regression pack unchanged.
- Output drift (ACK/ERR/END grammar, `/proc` layouts, telemetry formats) fails CI.
- New tests are additive; existing fixtures remain canonical.

### 6. Cross-Milestone Stability
- Changes to console grammar, NineDoor error codes, or `/proc` formats are breaking.
- Breaking changes require:
  - updated CLI fixtures,
  - regenerated manifest artifacts,
  - updated docs,
  - a manifest schema version bump.

---

## Host Tools (cohsh, gpu-bridge-host) — Applicability
All charter rules apply to host tools **except** VM-only constraints.

Host tools MAY use `std` and host OS facilities, but MUST NOT:
- introduce new control-plane semantics outside Secure9P / documented console grammar,
- bypass manifest/IR-defined schemas, error codes, or namespace layouts,
- change ACK/ERR/END or NineDoor error semantics without the full breaking-change process,
- rely on undocumented RPC channels into the VM.

Host tools MUST remain protocol-faithful: they consume the as-built interfaces and fixtures.

---

## HAL — Mandatory

- **All device access goes through HAL.**
- No direct MMIO, physical addresses, or ad-hoc `unsafe` outside HAL.
- Drivers depend on HAL; subsystems depend only on driver traits.
- Devices are selected by **role**, not model.
- Multiple devices are supported by design.
- Any HAL bypass — even “temporary” — is a hard violation.

---

## Canonical Documents
- `README.md`
- `docs/*.md`
- `seL4/seL4-manual-latest.md`
- `seL4/elfloader.md`

---

## Security & Testing
- Validate all user-controlled input (9P frames, JSON).
- No hard-coded secrets; use config or tickets.
- Behavior changes require updated tests and documented commands.
- Before merge, run:
  ```
  cargo run -p coh-rtc
  ```
  and verify regenerated artifacts hash-match committed versions.

---

## Codex Instruction
Web Codex must treat this document as a **binding contract**, not advisory guidance.  
When in doubt, it **MUST** choose the option that preserves determinism, minimal surface area, and manifest fidelity — even if that is less convenient.

---

## Future Notes
- Automated worker lifecycle and `/queen/ctl` bindings proceed per `BUILD_PLAN.md`.
- Secure9P will grow explicit worker-create/worker-kill and GPU lease renewal verbs; namespace semantics must remain aligned when they land.
