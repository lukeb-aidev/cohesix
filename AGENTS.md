// CLASSIFICATION: COMMUNITY
// Filename: AGENTS.md v3.1
// Author: Lukas Bower
// Date Modified: 2025-06-28

# Cohesix Codex Agents

This file defines agent tasks for Cohesix. These automated agents support the build, test, and deployment pipeline, enforcing architectural principles, secure OS constraints, and cross-role compliance.

---

## 📌 Project Context

Cohesix is a secure OS platform built on seL4 + Plan9 principles with:
- Role-based trust zones (QueenPrimary, DroneWorker, KioskInteractive, etc.)
- Secure9P for policy and capability enforcement
- Namespace and validator control
- CUDA + Rapier physics modules
- CLI tools in Rust, Go, Python
- The custom compiler `cohcc`
- Pure UEFI execution environment using LLVM/LLD (no Linux syscalls, no `/proc` or `/sys`)
- Rust cross-targets: `x86_64-unknown-uefi`, `aarch64-unknown-uefi`
- Pre-installed tools: `rustc`, `cargo`, `llvm`, `lld`, `clang`, `python3` (with `flake8`, `mypy`, `black`), `go`, `gcc`
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

### Task Title: QEMU Boot ISO Sanity (AGENT:BOOT_ISO_SANITY)
- **Goal:** Validate `make_iso.sh` produces a bootable ISO that starts validator + shell.
- **Input:** tools/make_iso.sh, tests/test_bootflow.py
- **Output:** log/iso_boot_check.md
- **Checks:** ISO mounts, boots via QEMU, shell launches as QueenPrimary.
- Example log:  
  `✅ ISO booted via QEMU. Validator active. Shell running.`

### Task Title: Complete Userland Tool Staging (AGENT:USERLAND_TOOLS)
- **Goal:** Ensure ISO contains cohesix-shell, CLI tools, cohcc, cohtrace, mandoc, BusyBox.
- **Input:** tools/make_iso.sh, /out/iso/
- **Output:** log/userland_tool_check.md
- **Checks:** All binaries staged under /usr/bin or /bin. Shell responds to CLI commands.

### Task Title: Secure9P + Role Policy Audit (AGENT:ROLE_POLICY_CHECK)
- **Goal:** Confirm runtime role + validator matches ROLE_POLICY.md and secure9p.toml.
- **Input:** docs/community/governance/ROLE_POLICY.md, config/secure9p.toml, /srv/cohrole
- **Output:** log/secure9p_policy_check.md
- **Checks:** Roles aligned, Secure9P validated.

### Task Title: Watchdog Heartbeat + Recovery (AGENT:WATCHDOG_RECOVERY)
- **Goal:** Check watchdog heartbeats ≤ 5 min, document restarts.
- **Input:** log/watchdog/
- **Output:** log/watchdog_check.md
- **Checks:** No stale tasks; recovery attempts logged.

---

## 🔍 Supporting Documents

- `docs/community/governance/INSTRUCTION_BLOCK.md` — canonical build + hydration rules
- `docs/community/governance/ROLE_POLICY.md` — trust zones, Secure9P role definitions
- `docs/community/planning/DEMO_SCENARIOS.md` — validator + namespace scenario references
- `docs/private/COMMERCIAL_PLAN.md` — milestones linked to agent enforcement

---

## ⚙️ Execution & Environment Notes

- Agents run under GitHub Actions workflows (x86_64 and aarch64 runners with CUDA fallback) and local CI.
- All builds target pure UEFI binaries using LLVM/LLD.
- Output always written to TMPDIR, COHESIX_TRACE_TMP, or COHESIX_ENS_TMP.
- Any agent failing its check fails the entire build, with logs captured for review.
- No absolute system paths, no persistent background tasks.

---

## ✨ Goal of This Agent System

To ensure every build of Cohesix:
- Boots cleanly via QEMU into a validator-protected shell
- Includes the full userland toolchain (CLI, cohcc, BusyBox, mandoc)
- Enforces role trust + Secure9P policy
- Logs watchdog + validator output for audits
- Aligns 100% with INSTRUCTION_BLOCK.md and the evolving architecture.

✅ With these agents, each build is provably secure, fully testable, and production-grade.