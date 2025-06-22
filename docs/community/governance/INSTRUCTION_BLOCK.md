// CLASSIFICATION: COMMUNITY
// Filename: INSTRUCTION_BLOCK.md v3.6
// Author: Lukas Bower
// Date Modified: 2025-06-15

## 0 · Classification Header — Mandatory

Every canonical **source** file must begin with a `CLASSIFICATION` header formatted for the file type:
- Rust/C/Go: `// CLASSIFICATION: ...`
- Bash scripts: `# CLASSIFICATION: ...`
- YAML/TOML: `# CLASSIFICATION: ...`
- Markdown and plaintext: may include the header anywhere, or omit it entirely (manual validation allowed).

Valid classification values are:
- COMMUNITY
- PRIVATE

The header must also indlude the following, in the correct format for the file type:
- Filename: INSTRUCTION_BLOCK.md v3.6
- Author: Lukas Bower
- Date Modified: [today's date]

Source files must place the header at the top of the file. Other formats may defer this placement.

---

## 1 · Canonical Source of Truth

Always refer to `METADATA.md` for:
- File version numbers
- Classification type
- Last-modified date

Never hardcode document lists or versions elsewhere.  
Documents archived in `/canvas/archive/` are read-only and excluded from Codex workflows.

---

## 2 · Architecture Summary (Codex-Referencable)

- Kernel: seL4 microkernel with Cohesix-specific patches
- Userland: Plan 9 (9P namespace, rc shell, minimal POSIX)
- Boot Target: ≤200 ms cold start (Jetson Orin Nano)
- Boot Flow: GRUB → seL4 → Cohesix root task (replaces prior UEFI path)
- Security: seL4 proofs enforced; Plan 9 srv sandboxed
- Role Exposure: Immutable `CohRole`, visible at `/srv/cohrole`  
- Roles: QueenPrimary, KioskInteractive, DroneWorker, GlassesAgent, SensorRelay, SimulatorTest  
- Physics: `/sim/` (Rapier) active on Worker nodes  
- GPU: `/srv/cuda` for CUDA-enabled agents; fallback must log gracefully  
- Licensing: Only Apache 2.0, MIT, or BSD allowed (see `LICENSES_AND_REUSE.md`)

---

## 3 · Hardware + CI Matrix

Target Hardware:
1. Jetson Orin Nano (8 GB) – Primary worker with CUDA
2. Raspberry Pi 5 (8 GB) – Fast boot fallback
3. AWS EC2 Graviton/x86 – Queen orchestration
4. Intel NUC – Optional fallback/dev

Codex must auto-detect supported hardware and pass tests accordingly.

---

## 4 · Language Boundaries by Layer

| Layer             | Language      | Rationale                           |
|------------------|---------------|-------------------------------------|
| Kernel Patches    | C             | Required by seL4 upstream           |
| Low-Level Drivers | Rust          | Safety + cross-arch + 9P-friendly   |
| Userland/Services | Go            | CSP-style concurrency               |
| Tooling & Testing | Python        | CLI, validator, DSL, glue           |
| CUDA Models       | C++ / CUDA    | Jetson inference & deployment       |
| Shell Scripts (Bash) | Bash       | Used for build orchestration (e.g., GRUB → seL4 → Cohesix ISO), follows POSIX/Bash best practices |

---

## 5 · Bulletproof Workflow Rules

1. Single-Step Hydration  
   Every file must be hydrated to `/mnt/data/cohesix_active/` in the *same execution frame* as creation.  
   Hydration and file creation must occur together — batching or deferred writes are not allowed.

2. Atomic Write Only  
   Use temp-write + rename. Hydration is valid only if:
   - File > 0 B  
   - Contains valid headers  
   - Structurally complete

3. No Placeholder Code  
   CI fails on:
   - Empty `fn` or `impl` blocks  
   - `unimplemented!()`, `todo!()`, or stub comments

4. Mandatory Headers
   Every canonical **source** file must include:
   - A correctly formatted `CLASSIFICATION:` line for the file type (`//` for source, `#` for shell/config)
   - `// Filename vX.Y`
   - `// Author: Lukas Bower`
   - `// Date Modified: YYYY-MM-DD`
   - Registration in `METADATA.md`
   - Entry in `CHANGELOG.md`

   Shell scripts, YAML/TOML config files, and other plain-text assets may adjust header syntax to match their format. Manual validation is acceptable when CI time is constrained.

5. Watchdog Heartbeat (Live)  
   Codex must:
   - Check progress every 5 min  
   - Auto-restart stalled tasks at 30 min  
   - Log recovery attempts

6. Directory Auto-Recovery  
   Recreate `/mnt/data/cohesix_active/` if missing, wiped, or corrupt.

7. Metadata Enforcement  
   CI must validate:
   - Every file in `METADATA.md` exists, is non-empty, and correctly versioned  
   - No missing headers or version mismatches

8. No Phantom Docs  
   If a document isn’t in `METADATA.md`, it doesn’t exist.

9. Build Must Pass CI Matrix  
   All Cohesix components must build and test successfully on both target architectures:
   - `aarch64` (Jetson Orin Nano, Raspberry Pi 5)
   - `x86_64` (AWS EC2, Intel NUC fallback)

   Required tests:
   - Kernel build (seL4 with Cohesix patches)
   - Userland and shell init
   - Python and Go runtime verification
   - CLI and compiler tools (`cohesix`, `cohcc`)
   - CUDA + Rapier support if applicable
   - ISO image boot in QEMU to working shell

   CI must gracefully skip unavailable targets (e.g., QEMU missing) but must log the skip.

10. Physics + CUDA Checks  
    - `/sim/` required if Rapier enabled  
    - `/srv/cuda` must expose valid CUDA workload  
    - Log + disable gracefully if unsupported  
    - No GPL libraries allowed in CUDA stack

11. Upstream Sync Policy  
    Rebase monthly from:
    - seL4 master  
    - 9front Plan9

12. OSS License Compliance  
    All imported code must:
    - Be Apache 2.0, MIT, or BSD  
    - Include SPDX license header  
    - Be logged in `LICENSES_AND_REUSE.md`

13. Documentation Consolidation Guard  
    Related technical docs must be merged (e.g., TOOLING_PLAN.md → IMPLEMENTATION_GUIDE.md).  
    CI must reject duplication or drift.

14. TMPDIR + Writable Path Enforcement  
    - All code must write to paths under `$TMPDIR`, `$COHESIX_ENS_TMP`, or `$COHESIX_TRACE_TMP` if applicable.  
    - Hardcoded paths like `/tmp/foo.log` or `/var/...` are forbidden unless explicitly marked writable in container or CI.  
    - Codex must enforce writable path compliance in test and runtime artifacts.

---

## 6 · Testing Requirements

- Unit Testing: `cargo test`, `go test`, `pytest`  
- Property Testing: Rust `proptest`, Haskell-style QuickCheck  
- Boot & CI Validation: Full boot traces on Jetson/Pi  
- Fuzzing: 9P protocol + syscall surface  
- Trace Replay: Valid snapshots from `/history/` or `SimMount`  
- Validator: Every syscall checked live by the runtime validator  
- Role Override: Simulate using `COHROLE=` env/bootarg

- Ensemble agents must test under `$COHESIX_ENS_TMP`, and validate safe cleanup afterward.
- QEMU boot scripts must support the GRUB → seL4 → Cohesix boot flow and gracefully skip if `qemu-system-x86_64` is missing or not installed.

---

## 7 · Risk Matrix

| Risk                     | Mitigation                                         |
|--------------------------|----------------------------------------------------|
| Fuzzy Requirements       | Rule #13: Clarify in METADATA.md + inline comments |
| Incomplete Hydration     | Atomic write + size checks + watchdog recovery     |
| Build Breakage on Arch   | CI matrix + fallback builds                        |
| OSS License Violation    | SPDX headers + `LICENSES_AND_REUSE.md` audit       |
| Faulty Stubs             | CI blocklist of all placeholder patterns           |
| Memory Errors            | Rust for all unsafe or low-level logic             |
| Clock Drift / Timeout    | Watchdog + tick-based trace validation             |

---

## 8 · Regex and Edit Protocol

Regex edits must:
- Use anchor markers (`<^...$>`)  
- Avoid greedy wildcards  
- Normalize whitespace and encoding  
- Prefer full-document rewrites for structural changes  
- Include tagged changelogs

---

## 9 · Folder Layout + Classification

docs/  
├── community/                   # Safe for GitHub  
│   ├── AGENTS_AND_CLI.md  
│   ├── DEMO_SCENARIOS.md  
│   ├── IMPLEMENTATION_GUIDE.md  
│   └── ...  
├── private/                     # Strategic / IP / Internal  
│   ├── RETROSPECTIVES.md  
│   ├── COMMERCIAL_PLAN.md  
│   └── ...

Classification headers must exactly match:
- `// CLASSIFICATION: COMMUNITY`
- `// CLASSIFICATION: PRIVATE`

Source code should place this line first. Non-code files may reposition or omit
the header, with verification performed manually or during deferred checks.
`validate_classification.py` can be run on demand.

---

## 10 · Collaboration and Codex Protocol

- Task format: `YYYY-MM-DD`  
- Codex Responsibilities: Hydrate, validate, crosslink, archive
  - Classification header checks may be deferred to manual review when CI time is constrained
- Human Responsibilities: Set intent, resolve ambiguity, approve merges
- Codex Sanity Check: Before running, Codex must ensure:
  - Internet access works if required  
  - All dependencies are available  
  - No hydration or permission errors are present  
  - It can read + write to `/mnt/data/cohesix_active/` cleanly
  - If `qemu-system-x86_64` is not present, Codex must log and skip boot tests instead of failing the entire pipeline.
