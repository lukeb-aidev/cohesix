// CLASSIFICATION: COMMUNITY
// Filename: INSTRUCTION_BLOCK.md v3.6
// Author: Lukas Bower
// Date Modified: 2025-07-11

## 0 · Mandatory Headers & Source Metadata
Every source file must begin with:
- A `CLASSIFICATION` header (`// CLASSIFICATION: COMMUNITY` or `// CLASSIFICATION: PRIVATE` for code; `#` for scripts/config).
- `Filename: <name> vX.Y`, `Author: Lukas Bower`, `Date Modified: YYYY-MM-DD`.
Register each file in `METADATA.md` and record versions in `CHANGELOG.md`.

Always refer to `METADATA.md` for version numbers, classification, and last-modified dates. Do not hardcode metadata in multiple places.

## 1 · Architecture & Boot Flow
Cohesix runs on seL4 with Plan 9 userland in a pure UEFI environment. Source layers: C (kernel patches), Rust (low-level drivers & root task), Go (services), Python (tools), C++/CUDA, Bash.  
Boot sequence: UEFI → seL4 → `cohesix_root` (Rust) → Plan 9 userland.  
Link `cohesix_root` statically against `libsel4.a` and validate ELF entry points.

## 2 · Hardware & CI Matrix
- Primary: x86_64 UEFI systems with NVIDIA GPU (CUDA & QEMU tests).
- Secondary: aarch64 UEFI targets via QEMU emulation.

## 3 · Language Boundaries by Layer

| Layer             | Language      | Rationale                           |
|------------------|---------------|-------------------------------------|
| Kernel Patches    | C             | Required by seL4 upstream           |
| Low-Level Drivers | Rust          | Safety + cross-arch + 9P-friendly   |
| Userland/Services | Go            | CSP-style concurrency               |
| Tooling & Testing | Python        | CLI, validator, DSL, glue           |
| CUDA Models       | C++ / CUDA    | Jetson inference & deployment       |
| Shell Scripts (Bash) | Bash       | Used for build orchestration in the UEFI execution environment (LLVM/LLD, Rust UEFI targets) |

- Plan 9 shell + CLI tools orchestrate all agent and CUDA workloads; no POSIX runtime expected

## 4 · Core Workflow Rules
1. Atomic, single-step hydration and temp-write+rename for all files.
2. No placeholder code (`todo!()`, `unimplemented!()`); CI rejects stubs.
3. Metadata enforcement: headers in every file, `METADATA.md` & `CHANGELOG.md` synchronization.
4. CI matrix: build & test on x86_64 + aarch64 targets; QEMU boot validation.
5. License compliance: only Apache 2.0, MIT, BSD; SPDX headers required.

## 5 · Testing Requirements
- Unit & integration: `cargo test`, `go test`, `pytest`.
- Boot & CI Validation: QEMU UEFI → seL4 → Cohesix_root → Plan9 shell.
- Fuzzing: 9P protocol + syscall surface.
- Trace Replay: use `/history/` or `SimMount`.

## 6 · Codex Task Format
- **Task Title & ID:** Short label + unique ID.
- **Goal:** What the change achieves.
- **Input:** Files/directories.
- **Output:** Files generated/updated.
- **Checks:** Pass/fail criteria (e.g., `cargo test`, boot success).
- **Notes:** Rationale or security caveats.
