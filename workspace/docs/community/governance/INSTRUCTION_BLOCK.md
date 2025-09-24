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
Cohesix runs on seL4 with Plan 9 inspired userland in a bare meta QEMU environment.   
QEMU boot sequence: elfloader → CPIO (seL4 → `cohesix_root` = Plan 9 inspired userland).  

## 2 · Hardware & CI Matrix
- Primary: aarch64 systems on bare metal QEMU.
- Secondary: x86_64 via QEMU emulation.

## 3 · Language Boundaries by Layer

| Layer             | Language      | Rationale                           |
|------------------|---------------|-------------------------------------|
| Kernel Patches    | C             | Required by seL4 upstream           |
| Low-Level Drivers | Rust          | Safety + cross-arch + 9P-friendly   |
| Tooling & Testing | Python        | CLI, validator, DSL, glue           |
| Shell Scripts (Bash) | Bash       | Used for build orchestration in the UEFI execution environment (LLVM/LLD, Rust UEFI targets) |

- Plan 9 shell + CLI tools orchestrate all agent workloads and proxy commands to remote CUDA annexes; no POSIX runtime expected within Cohesix roles

## 4 · Core Workflow Rules
1. Atomic, single-step hydration and temp-write+rename for all files.
2. No placeholder code (`todo!()`, `unimplemented!()`); CI rejects stubs.
3. Metadata enforcement: headers in every file, `METADATA.md` & `CHANGELOG.md` synchronization.
4. CI matrix: build & test on aarch64 and x86_64 targets; QEMU boot validation.
5. License compliance: only Apache 2.0, MIT, BSD; SPDX headers required.

## 5 · Testing Requirements
- Unit & integration: `cargo test`, `pytest`.
- Boot & CI Validation: QEMU → elfloader → CPIO (seL4 → `cohesix_root`)
- Fuzzing: 9P protocol + syscall surface.
- Trace Replay: use `/history/` or `SimMount`.

## 6 · Codex Task Format
- **Task Title & ID:** Short label + unique ID.
- **Goal:** What the change achieves.
- **Input:** Files/directories.
- **Output:** Files generated/updated.
- **Checks:** Pass/fail criteria (e.g., `cargo test`, boot success).
- **Notes:** Rationale or security caveats.
