// CLASSIFICATION: COMMUNITY
// Filename: INSTRUCTION_BLOCK.md v4.0
// Author: Lukas Bower
// Date Modified: 2025-07-11

## 0 · Classification Header Requirement
All canonical documents **must** begin with a classification header as the first non-blank line.

---

## 1 · Canonical Docs
Reference the file list in **METADATA.md**. It is the sole source of truth for document versions, classifications, and batch metadata.

---

## 2 · Architecture Snapshot
- **Kernel:** seL4 L4-micro-kernel (upstream + Cohesix patches)
- **Userland:** Plan 9 services (9P, rc shell, minimal POSIX)
- **Boot target:** <200 ms cold start
- **Security:** seL4 proofs preserved, Plan 9 srv sandbox caps
- **CohRole:** Declared before kernel init, exposed via `/srv/cohrole`
- **Roles:** QueenPrimary, KioskInteractive, DroneWorker, GlassesAgent, SensorRelay, SimulatorTest
- **Physics Core:** Rapier-based `/sim/` for workers
- **GPU Support:** `/srv/cuda` with graceful fallback
- **OSS Policy:** Apache 2.0, MIT, or BSD only

---

## 3 · Reference & Test Hardware
1. **Jetson Orin Nano 8 GB** — primary Worker
2. **Raspberry Pi 5 8 GB** — fallback Worker
3. **AWS EC2 Graviton/x86** — Queen CI
4. **Intel NUC-13 Pro** — optional dev host

---

## 4 · Languages & Roles
| Layer           | Language    | Rationale                            |
|-----------------|-------------|--------------------------------------|
| Kernel patches  | C           | seL4 style, proof continuity         |
| Low-level       | Rust        | Memory-safe, cross-arch              |
| 9P + services   | Go          | CSP model, fast cross-compile        |
| Trace + logic   | Python      | DSL, testing, runtime validator      |
| CUDA models     | C++ / CUDA  | Jetson runtime, Torch/TensorRT deploy|

---

## 5 · Workflow Rules (Batch Edition)
1. **Batch Hydration**
   Files for a batch are written under `/mnt/data/cohesix_active/` and grouped into checkpoints of at most 10 files. Each checkpoint validates structure before continuing.
2. **Atomic Write**
   Write to a temp file then rename once validation passes. Files must be non-empty.
3. **Progressive Hydration with Verification**
   Temporary scaffolds are allowed if clearly marked and hydrated within the same batch. CI verifies that no placeholders remain before merge.
4. **Version Bump & ChangeLog**
   Every canonical file requires headers (`// CLASSIFICATION`, `// Filename vX.Y`, `// Author`, `// Date Modified`) and an entry in CHANGELOG.md.
5. **Watchdog Heartbeat**
   Heartbeat every 5 min. If generation stalls for 30 min, self-heal by replaying from last checkpoint.
6. **Directory Recovery**
   Recreate `/mnt/data/cohesix_active/` if missing or corrupted.
7. **Metadata-Driven Document List**
   `METADATA.md` tracks Filename, Version, Classification, optional `BATCH_SIZE` and `BATCH_ORIGIN` fields.
8. **CI Metadata Synchronization Check**
   `validate_metadata_sync.py` ensures headers match `METADATA.md`.
9. **Directory & File Validation**
   Verify all hydrated files exist and are non-empty for the current batch.
10. **CI Matrix**
    Build on aarch64 and x86_64.
11. **Rapier & CUDA Checks**
    Workers expose `/sim/` and `/srv/cuda` if available; otherwise log and continue.
12. **Upstream Sync**
    Monthly rebase of seL4 & 9front.
13. **OSS License Guard**
    All imports must be MIT/BSD/Apache 2.0, recorded in `OSS_REUSE.md` with SPDX hash.
14. **Documentation Simplification (v3.2)**
    Consolidate related docs and enforce metadata consistency.
15. **Codex-Generated Batches**
    Files may include `// CODEX_BATCH: YES` header lines linking to upstream trace. Milestone commits use tags such as `codex-batch-support-v1`.
16. **Milestone Commits**
    Group checkpoints under a milestone label. Commit after each successful checkpoint.

---

## 6 · Testing & Quality Gates
- **Unit Tests:** `cargo test`, `go test`, `pytest`
- **Multi-Arch CI:** Run on aarch64 and x86_64 via `test_all_arch.sh`
- **Property Testing:** QuickCheck / proptest
- **Integration:** Boot → trace validation
- **Fuzzing:** 9P protocol + syscall mediation
- **Replay:** Snapshot replay from `/history/`
- **Validator:** Live rule checks per syscall
- **Simulator Override:** set `COHROLE=` via bootarg or env

---

## 7 · Risk & Mitigations
| Risk                   | Mitigation                            |
|------------------------|---------------------------------------|
| Ambiguous requirements | Ask early; document before code       |
| Cross-arch failure     | Matrix CI + container builds          |
| Partial hydration loss | Atomic write + watchdog recovery      |
| License contamination  | License guard + reuse registry        |
| Hardware shortages     | Cloud fallback + container builds     |

---

## 8 · Regex Safety & Editing Protocols
- Use anchor tags like `<^…$>`
- Avoid unbounded wildcards
- Normalize encoding and whitespace
- Prefer full-document hydration on major updates
- Tag each section with comment headers

---

## 9 · Document Classification Protocol
Folders:
```
docs/
├── community/   # GitHub-safe
├── private/     # internal
```
Headers:
- `// CLASSIFICATION: COMMUNITY`
- `// CLASSIFICATION: PRIVATE`
Enforced by `validate_classification.py`.

---

## 10 · Collaboration Protocol
- **Task format:** `YYYY-MM-DD`
- **Assistant duties:** hydrate, mirror, version, validate
- **User duties:** set intent, approve merges, clarify specs

