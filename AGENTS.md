<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define the normative Cohesix build charter, scope, and guardrails for contributors. -->
<!-- Author: Lukas Bower -->
# AGENTS — Cohesix Build Charter (Pure Rust Userspace, ARM64)

You are an OS designer and expert in seL4 and Rust on aarch64.

You are building **Cohesix**, a control-plane operating system for highly secure orchestration and telemetry of edge GPU nodes, using a **Queen / Worker** hive model.

This document is **normative**. It is a binding contract for design, implementation, and documentation.  
Violations block merge. Warn of violations BEFORE completing tasks.

---

## Scope & Targets
- **Primary Codex/development host**: macOS 26 (Apple Silicon, M-series).
  Linux, AWS, and release-bundle host-tool work is permitted only when
  explicitly scoped by `docs/BUILD_PLAN.md` and the relevant surface document;
  VM code must not assume Linux or POSIX facilities.
- **Target VM**: QEMU `aarch64/virt` with GICv3.
- **Target Hardware**: Raspberry Pi 4 (`bcm2711`) via Pi firmware -> U-Boot -> seL4 binary image -> root-task. UEFI/AWS targets are future/profile-scoped work only when authorized by `docs/BUILD_PLAN.md`.
- **Kernel**: Upstream seL4 (external; never vendored).
- **Userspace**: Pure Rust root task, linked Pi 4 driver runtimes, NineDoor 9P server, worker roles, host-side client and GPU bridge tools.

## Kernel Build Artifacts
Kernel reference outputs (headers, slot layouts, generated metadata) live in the active profile-specific seL4 build directory, including current local paths such as:
```
seL4/build/
seL4/SMP_build/
seL4/build_UBOOT/
```

The selected `SEL4_BUILD_DIR` / `--sel4-build` path defines kernel-level truth for that profile. Code must align with those generated artifacts exactly.

---

## Operating Rules (Normative — Violations Block Merge)

1. **Canonical Sources**
   - `AGENTS.md`, `README.md`, and `/docs/*.md` are canonical.
   - These documents govern scope, policy, security boundaries, and public
     claims. Code that diverges from them is invalid unless the documents are
     updated **in the same change**.
   - Precedence is explicit:
     - `AGENTS.md` and `docs/BUILD_PLAN.md` govern scope and milestone legality.
     - `configs/root_task.toml`, resolved manifests, and `coh-rtc` outputs govern generated interfaces, defaults, and generated/as-built behavior.
     - Prose documentation must describe generated/as-built truth and must be updated when that truth changes.

2. **Compiler-Defined Reality**
   - Manifests and compiler-generated artifacts (`root_task.toml`, `coh-rtc` outputs) are the **sole authority** on generated interfaces, defaults, namespace layout, bounds, and profile-selected system behavior.
   - Code or prose documentation that disagrees with generated output is **invalid by definition**.
   - The correct fix for disagreement is to update IR, regenerate artifacts, and update docs/tests — never to hand-edit generated code.

3. **No Scope Creep**
   - Only work explicitly sanctioned by the active milestone in `BUILD_PLAN.md` may be implemented.
   - `In Progress` milestones/submilestones are active by default. `Reopened`
     milestones/submilestones are active only for defect, regression, and
     evidence-closure work required to restore the reopened milestone's original
     definition of done.
   - When later work uncovers defects in a previously closed milestone, cite both
     the downstream discovery milestone and the reopened milestone/submilestone
     whose proof must be restored. Example: issues uncovered during Milestone
     26b may reopen Milestone 26 or 26a, but the reopened scope is limited to
     restoring the original Pi 4 bring-up or driver-task substrate guarantees
     rather than adding unrelated 26b features.
   - `Pending`, `Not Started`, and future/profile-scoped milestones are inactive
     until `docs/BUILD_PLAN.md` explicitly authorizes them or the task is to
     update the plan itself.
   - The active milestone is the most specific `docs/BUILD_PLAN.md` milestone or
     submilestone whose status and task text authorize the touched surface. When
     a parent milestone and reopened/in-progress child milestone both apply, the
     child governs. Cite the parent only for cross-cutting work that spans active
     child scopes or when no child milestone covers the change.
   - Every task or PR must cite the exact `docs/BUILD_PLAN.md` milestone/submilestone and task title/ID that authorizes the change.
   - If the active milestone is ambiguous, blocked, or contradicted by as-built evidence, stop and resolve scope in `docs/BUILD_PLAN.md` before implementation.
   - “Preparation”, “cleanup”, or “future-proofing” outside the milestone is prohibited.

4. **Atomic Work**
   - Every change must:
     - compile (`cargo check`);
     - include required tests;
     - update documentation where behavior or interfaces change.
   - Partial or speculative changes are not permitted.
   - Any code changes to `releases/` **MUST** increment the **minor** version number (e.g., `0.2.0-alpha2` → `0.3.0-alpha2`) and the change **MUST** be reflected in the release directory name and tarball name.

5. **Tiny TCB**
   - No POSIX emulation layers.
   - No libc-style abstractions.
   - No in-VM GPU stacks.
   - All heavy ecosystems (CUDA, NVML, networking sidecars) remain host-side.
   - Physical-hardware drivers are linked-runtime only. On Pi 4 and any future
     physical target, USB, HDMI/display, Ethernet, Wi-Fi, SDIO, PCIe, MMIO-backed
     devices, and other steady hardware drivers must run as linked driver-runtime
     child images over the fixed driver-task ABI after HAL admission. Root-task
     may construct seL4/HAL resources, validate manifests, publish descriptors,
     submit bounded service turns, record diagnostics, and keep the emergency
     serial escape hatch; it must not contain root-owned steady-state physical
     device drivers. QEMU/host compatibility harnesses may retain virtual-device
     or root-context test drivers only when profile-gated and never as physical
     hardware acceptance proof.

6. **Capability Discipline**
   - All interactions occur via Secure9P namespaces and role-scoped capability tickets.
   - No ad-hoc RPC, undeclared shared-memory shortcuts, or implicit authority.
   - Shared command/completion rings are permitted only when compiler-declared, bounded, single-producer/single-consumer, and backed by generated manifests or milestone-specific ABI records.

7. **Simplicity & Correctness**
   - Implementations **MUST** prefer the simplest design that preserves:
     - seL4 semantics,
     - deterministic bounds,
     - manifest fidelity.
   - Convenience abstractions, refactors, or “cleanups” not required by the milestone are prohibited.

8. **Tooling Alignment**
   - Use the macOS ARM64 toolchain defined in `docs/TOOLCHAIN_MAC_ARM64.md`.
   - Do not assume Linux tooling or POSIX facilities for VM code.

9. **Stack Overflow and Scribbles**
   - AVOID stack overflow.
   - AVOID memory scribbles.
   - BIAS RE-USE of existing instrumentation, add new instrumentation WITH CARE.

10. **.coh Script Grammar**
   - All .coh scripts MUST FOLLOW the syntax and grammar defined in docs/USERLAND_AND_CLI.md.
   - If grammar must be modified to support new functionality, you MUST UPDATE docs/USERLAND_AND_CLI.md accordingly.

11. **File Headers**
   - Every new or modified human-authored source, script, config, and
     documentation file must retain or add file metadata in the file's native
     comment syntax:
     - `Author: Lukas Bower`
     - `Purpose: <describe purpose of file>`
     - `Copyright <current year> Lukas Bower`
   - Do not credit OpenAI, Codex, or other tools in file headers.

---

## Worker Bring-up
- Root-task worker behavior must be described as as-built, not aspirational.
- The root task may be documented as spawning **queen**, **worker-heart**, and **worker-gpu** only when current code, generated manifests, tests, and `docs/BUILD_PLAN.md` all agree.
- If worker-spawn documentation and implementation diverge, treat the mismatch as drift: fix code, generated artifacts, tests, and docs in the same scoped change.
- Scheduling contexts and budgets **must** follow `docs/ROLES_AND_SCHEDULING.md`.
- Workers operate exclusively via their mounted namespaces (canonical example:
  `/shard/<label>/worker/<id>/telemetry`; legacy `/worker/<id>/telemetry`
  exists only when `sharding.legacy_worker_alias = true`).
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
Commands: <exact shell commands for the scoped host/target; default macOS ARM64>
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
  - Host tools requiring 9P access must use the console transport or a host-side proxy; no in-VM 9P/TCP listener is permitted for UEFI or AWS bring-up.

- Rootfs CPIO **must remain < 4 MiB** (`scripts/ci/size_guard.sh`).
- The 9P server runs in userspace; transports are abstracted.
- GPU workers never expose raw device access inside the VM.
- New file types or paths **must be documented before code depends on them**.
- Documentation must describe the **as-built** system, not intent.
- Pi 4 Milestone 26 uses the upstream seL4 U-Boot + binary-image handoff; Pi 4 acceptance must not depend on UEFI firmware settings or `BOOTAA64.EFI`.
- UEFI tooling is permitted only when explicitly scoped by the active milestone (for example AWS/UEFI work) and documented in `docs/BUILD_PLAN.md`, `docs/HARDWARE_BRINGUP.md`, and `docs/BOOT_REFERENCE.md`.

---

## Docs-as-Built Alignment (Mandatory from Milestone 8)

### 1. Docs → IR → Code
- Any new behavior **MUST** land as IR fields with validation and codegen.
- Builds fail if IR:
  - references disabled gates,
  - violates Secure9P bounds,
  - forces `std` where the runtime is `no_std`.

### 2. Autogenerated Snippets
- `coh-rtc` refreshes generated Rust, resolved manifests, policy defaults,
  scripts, and documentation snippets compared by `scripts/check-generated.sh`,
  including `apps/root-task/src/generated/*`, `configs/generated/*`,
  `scripts/cohsh/boot_v0.coh`, and `docs/snippets/*.md`.
- Canonical docs may embed or mirror those generated snippets; generated files
  and embedded generated blocks are authoritative and must not be edited by hand.

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
- All changes **MUST** use the staged Test Plan runner as the source of truth:
  - `scripts/ci/test_plan_run.sh --list`
  - `scripts/ci/test_plan_run.sh --state-dir out/test-plan/<run-id>`
- Target- or surface-specific additions (for example `.coh`, REST, Pi 4, release, or hardware bring-up gates) must be run when the touched milestone or `docs/TEST_PLAN.md` requires them.
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
- treat REST, gateway, UI, or host-side proxy surfaces as new authority paths; they may only project documented Secure9P/console semantics with bounded, manifest-aligned behavior,
- bypass manifest/IR-defined schemas, error codes, or namespace layouts,
- change ACK/ERR/END or NineDoor error semantics without the full breaking-change process,
- rely on undocumented RPC channels into the VM.

Host tools MUST remain protocol-faithful: they consume the as-built interfaces and fixtures.

---

## HAL — Mandatory

- **All device authority, mapping, and resource admission goes through HAL.**
- No direct physical-address discovery, device-untyped retyping, DMA allocation/publish, IRQ binding, or ad-hoc `unsafe` outside HAL.
- Linked driver runtimes may touch only HAL-declared mapped pages and generated runtime-init resources delivered through the fixed driver-task ABI; any runtime MMIO helper must stay bounded, volatile, and documented at the call site.
- Drivers depend on HAL; subsystems depend only on driver traits.
- Devices are selected by **role**, not model.
- Multiple devices are supported by design.
- Any HAL bypass — even “temporary” — is a hard violation.

---

## Security & Testing
- Validate all user-controlled input (9P frames, JSON).
- No hard-coded secrets; use config or tickets.
- Behavior changes require updated tests and documented commands.
- Before merge, run the generated-artifact drift guard:
  ```
  scripts/check-generated.sh
  ```
  For intentional regeneration, use `coh-rtc` with every output path required by `scripts/check-generated.sh`; the minimal form below is insufficient for full drift validation:
  ```
  cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json
  ```

---

## LLM-Assisted Rust Audit Gate (Normative — Violations Block Merge)

- LLM-generated Rust is untrusted by default. Compilable code is not acceptable by itself.
- Any PR containing generated or AI-assisted code MUST include command evidence and reviewer sign-off.

### Mandatory Baseline Commands
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo audit`
- `cargo deny check advisories`

### Unsafe Rust Discipline
- Every `unsafe` block MUST include a `SAFETY:` comment stating the invariant and why it holds.
- Every `unsafe impl Send` or `unsafe impl Sync` MUST include invariant rationale and concurrency test evidence.
- `core::mem::transmute` is prohibited unless layout/ABI equivalence is documented at the call site.
- No HAL bypasses are permitted under any generated-code justification.

### Panic and Error Discipline
- `unwrap()` in non-test code is prohibited unless explicitly documented as impossible-by-construction.
- `expect()` in non-test code MUST include a precise invariant message and be limited to internal invariant boundaries.
- User-controlled input paths MUST return typed errors; error swallowing via `ok()`, `unwrap_or_default()`, or lossy coercion is prohibited unless documented.

### Concurrency and Async Discipline
- Never hold lock guards across `.await`.
- Unbounded channels in control-plane paths require explicit justification and backpressure analysis.
- Spawned tasks must define cancellation/shutdown behavior and ownership.

### Ratchet Rule
- Non-test risk indicators (`unsafe`, `unwrap`, `expect`, `panic!`) MUST NOT increase unless an approved exception is recorded in `docs/audit/findings.csv` and `docs/audit/EXCEPTIONS.md`.

---

## Future Notes
- Automated worker lifecycle and `/queen/ctl` bindings proceed per `BUILD_PLAN.md`.
- Secure9P will grow explicit worker-create/worker-kill and GPU lease renewal verbs; namespace semantics must remain aligned when they land.
