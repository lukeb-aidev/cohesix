// CLASSIFICATION: COMMUNITY
// Filename: AGENTS.md v3.2
// Author: Lukas Bower
// Date Modified: 2025-06-29

# Cohesix Codex Agents

This file defines agent tasks for Cohesix. These automated agents support the build, test, and deployment pipeline, enforcing architectural principles, secure OS constraints, and cross-role compliance.

---

## 📌 Project Context

Cohesix is a secure OS platform built on seL4 + Plan9 principles with:
- Role-based trust zones (QueenPrimary, DroneWorker, KioskInteractive, etc.)
- Secure9P for policy and capability enforcement
- Namespace and validator control
- CUDA + Rapier physics modules (CUDA processing handled only via dedicated Linux microservers; no CUDA in Plan9 roles)
- CLI tools in Rust, Go, Python
- The custom compiler `cohcc`
- Pure Plan9 style namespace bindings, avoiding any POSIX/Linux syscall dependencies
- Pre-installed tools: `rustc`, `cargo`, `llvm`, `lld`, `clang`, `python3` (with `flake8`, `mypy`, `black`), `go`, `gcc`
- Validates ELF entry points and seL4 symbol presence for rootserver (using nm, readelf, objdump), following COHESIX_AARCH64_BUILD.md, COHESIX_ROOT_ELF_DIAG.md, and INSTRUCTION_BLOCK.md.
Agents run in CI (GitHub Actions) and locally, ensuring consistency, security, and sandbox guarantees across all roles and deployments, with CUDA processing handled by dedicated Linux microservers where applicable.

---

## 🚀 Agent Task Format

Each agent task specifies:

- **Task Title & ID:** Short label plus a unique ID for log filtering
- **Goal:** What it guarantees
- **Input:** Files/directories to scan
- **Output:** Log files written
- **Checks:** Explicit pass/fail conditions

Agents always respect TMPDIR, COHESIX_TRACE_TMP, or COHESIX_ENS_TMP — never hardcoded /tmp or /dev/shm.

* Pull Requests must NEVER include binary files. This is prohibited.

---

## ✅ Example Agent Tasks

### Task Title: Kernel Hook Verification (AGENT:KERNEL_TRACE)
- **Goal:** Ensure kernel + namespace modules include boot + validator trace hooks.
- **Input:** src/kernel/, src/namespace/
- **Output:** log/kernel_trace_check.md
- **Checks:** Trace hooks and validator calls present on boot.
- Example log:  
  `✅ Validator hook found in src/kernel/init.rs`

---

## 🔍 Supporting Documents

- `docs/community/governance/INSTRUCTION_BLOCK.md` — canonical build + hydration rules
- `docs/community/governance/ROLE_POLICY.md` — trust zones, Secure9P role definitions
- `docs/community/planning/DEMO_SCENARIOS.md` — validator + namespace scenario references
- `docs/private/COHESIX_AARCH64_BUILD.md` — cross-build and linking requirements for Plan9 ELF

---

## ⚙️ Execution & Environment Notes

- Agents run under GitHub Actions workflows (x86_64 and aarch64 runners with CUDA fallback) and local CI.
- All builds use LLVM/LLD for linking.
- Cross-builds enforce `cargo build --target cohesix_aarch64.json` with explicit `-C linker=lld` to guarantee Plan9 no-syscall ELF.
- Removing or stubbing features to pass tests is explicitly prohibited. Tests must validate full, uncut functionality.
- Output always written to TMPDIR, COHESIX_TRACE_TMP, or COHESIX_ENS_TMP.
- Any agent failing its check fails the entire build, with logs captured for review.
- No absolute system paths, no persistent background tasks.
- ELF inspections leverage OpenAI's documented best practices for Codex Agent.md, ensuring object file + image correctness beyond normal CI.
- QEMU executions must use the `virt` platform (`-M virt -cpu cortex-a57 -m 1024`) with elfloader CPIO images and console output on `-serial mon:stdio`.
- All agent checks and validations tie back explicitly to INSTRUCTION_BLOCK.md, COHESIX_AARCH64_BUILD.md to ensure canonical compliance.
- Pull requests must NEVER include biary files.

---

## ✨ Goal of This Agent System

To ensure every build of Cohesix:
- Boots cleanly via QEMU into a Plan9-style validator-protected shell with fully mounted namespaces, no POSIX mounts, demonstrating isolated roles and Secure9P policy enforcement
- Verifies rootserver ELF with `readelf -h`, `readelf -S`, `nm` to ensure non-zero _start entry and linked seL4 syscalls (e.g. seL4_Send, seL4_Recv).
- Executes QEMU on `virt` platform (`-M virt -cpu cortex-a57 -m 1024`) using elfloader CPIO, console via `-serial mon:stdio`.
- Includes the full userland toolchain (CLI, cohcc, BusyBox, mandoc)
- Enforces role trust + Secure9P policy
- Logs watchdog + validator output for audits
- Aligns 100% with INSTRUCTION_BLOCK.md, COHESIX_AARCH64_BUILD.md, and COHESIX_ROOT_ELF_DIAG.md and the evolving architecture.

✅ With these agents, each build is provably secure, fully testable, and production-grade.