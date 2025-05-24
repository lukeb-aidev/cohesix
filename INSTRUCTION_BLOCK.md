// CLASSIFICATION: COMMUNITY
// Filename: INSTRUCTION_BLOCK.md v3.5
// Date Modified: 2025-05-24

## 0 · Classification Header Requirement

All canonical documents **must** begin with a classification header as the first non-blank line, for example:

```cpp
// CLASSIFICATION: COMMUNITY
```

or

```cpp
// CLASSIFICATION: PRIVATE
```

This ensures every file clearly states its intended audience before any other content.

---

## 1 · Canonical Docs

You must always refer to the canonical documents listed in **METADATA.md** for Cohesix project context.

*(Archived docs live under `/canvas/archive/`; do not reference here.)*

---

## 2 · Architecture Snapshot

- **Kernel:** seL4 L4-micro-kernel (vanilla upstream + Cohesix patches)
- **Userland:** Plan 9 services (9P namespace, rc shell, minimal POSIX)
- **Boot target:** <200 ms cold start on reference SBC
- **Security:** seL4 proofs preserved; additional sandbox caps for Plan 9 srv processes
- **CohRole:** Declared before kernel init; immutable; exposed via `/srv/cohrole`
- **Roles:** QueenPrimary, KioskInteractive, DroneWorker, GlassesAgent, SensorRelay, SimulatorTest
- **Physics Core:** Rapier-based `/sim/` for workers (unless exempted)
- **GPU Support:** Workers with CUDA expose `/srv/cuda`; fallbacks log + disable cleanly
- **OSS Policy:** All physics/CUDA components must use Apache 2.0, MIT, or BSD licenses

---

## 3 · Reference & Test Hardware

Ranked by Impact ÷ Cost:
1. **Jetson Orin Nano 8 GB** — primary Worker; CUDA tests
2. **Raspberry Pi 5 8 GB** — fallback Worker; fast boot
3. **AWS EC2 Graviton/x86** — Queen role + orchestration CI
4. **Intel NUC-13 Pro** — retired from Queen, optional dev host

---

## 4 · Languages & Roles

| Layer           | Language    | Rationale                                  |
|-----------------|-------------|--------------------------------------------|
| Kernel patches  | C           | seL4 style, proof continuity               |
| Low-level       | Rust        | Memory-safe, cross-arch, 9P-friendly       |
| 9P + services   | Go          | CSP model, fast cross-compile              |
| Trace + logic   | Python      | DSL, testing, runtime validator            |
| CUDA models     | C++ / CUDA  | Jetson runtime, Torch/TensorRT deploy      |

---

## 5 · Workflow Rules (Bulletproof Edition)

1. **Single-Step Hydration**  
   All files written to `/mnt/data/cohesix_active/` in the same step as generated.

2. **Atomic Write**  
   Write to a temp file, then rename. Valid only if file > 0 B and structurally complete.

3. **No Stubs Left Behind**  
   CI fails on:
   - Empty `fn`/`impl` blocks
   - `unimplemented!()`, `todo!()`, or placeholder comments

4. **Version Bump & ChangeLog**  
   Every canonical file must have:
   - `// CLASSIFICATION:` header
   - `// Filename vX.Y` header
   - `// Author: Lukas Bower` header
   - `// Date Modified` header
   - Entry in `CHANGELOG.md`

5. **Watchdog Heartbeat**  
   Every 5 min; restart stalled steps after 30 min.

6. **Directory Recovery**  
   Recreate `/mnt/data/cohesix_active/` if missing or corrupted.

7. **Metadata-Driven Document List**  
   The canonical list of documents—including filename, version, last‐modified date, and classification—is maintained **solely** in `METADATA.md`. Do **not** duplicate or hard-code this list elsewhere; always reference `METADATA.md` as the single source of truth.

8. **CI Metadata Synchronization Check**  
   On every push or merge, CI must run `validate_metadata_sync.py` to ensure:
   - Every canonical document listed in `METADATA.md` has matching `// Filename vX.Y` and `// CLASSIFICATION:` headers inside the file.
   - No entries are missing, and no version or classification mismatches exist.
   - Build fails if any discrepancy is detected.

9. **Directory & File Validation**  
   Ensure all hydrated files from the current batch are present and non-empty.

10. **CI Matrix**  
    All modules must build on aarch64 (Jetson/Pi) and x86_64 (AWS/dev).

11. **Rapier & CUDA Checks**  
    - Workers must expose `/sim/` and `/srv/cuda` if supported  
    - Log + fallback (no panic) if absent  
    - Pin Rapier crate versions across builds  
    - No GPL-derived CUDA content (see `OSS_REUSE.md`)

12. **Upstream Sync**  
    Monthly rebase of seL4 & 9front.

13. **OSS License Guard**  
   All imports must:
   - Use MIT/BSD/Apache 2.0
   - Be recorded in `OSS_REUSE.md`
   - Include SPDX header + hash

14. **Documentation Simplification (v3.2)**  
    - Merge related technical docs into consolidated guides (e.g., `IMPLEMENTATION_GUIDE.md`)  
    - CI check enforces metadata & classification consistency

---

## 6 · Testing & Quality Gates

- **Unit Tests:** `cargo test`, `go test`, `pytest`  
- **Multi-Arch CI:** Build & run on aarch64 and x86_64  
- **Property Testing:** QuickCheck / proptest  
- **Integration:** Boot → trace validation  
- **Fuzzing:** 9P protocol + syscall mediation  
- **Replay:** Snapshot replay from `/history/` or SimMount  
- **Validator:** Live rule checks per syscall  
- **Simulator Override:** Set `COHROLE=` via bootarg or env

---

## 7 · Risk & Mitigations

| Risk                         | Mitigation                                              |
|------------------------------|---------------------------------------------------------|
| Ambiguous requirements       | Rule #10 — ask early, document before code              |
| Cross-arch failure           | Matrix CI + container builds                            |
| Partial hydration loss       | Atomic write + watchdog recovery                        |
| License contamination        | License guard + SPDX + reuse registry                   |
| Hardware shortages           | Amazon AU fallback + container builds                   |

---

## 8 · Regex Safety & Editing Protocols

All regex edits must:  
- Use anchor tags like `<^…$>`  
- Avoid unbounded wildcards  
- Normalize encoding and whitespace  
- Prefer full-document hydration on major updates  
- Tag each section with comment headers

---

## 9 · Document Classification Protocol

**Folder Structure:**  
```text
docs/
├── community/                   ← GitHub-safe
│   ├── AGENTS.md                ← Agent spec
│   ├── examples/                ← Codex & integration examples
│   └── …                        ← other community docs
├── private/                     ← Strategy, IP, investor docs
```

**Classification Headers:**  
- `// CLASSIFICATION: COMMUNITY` — safe to publish  
- `// CLASSIFICATION: PRIVATE` — internal only  

Enforced at review and by `validate_classification.py`.

---

## 10 · Collaboration Protocol

- **Task format:** `YYYY-MM-DD`  
- **Assistant duties:** hydrate, mirror, version, validate  
- **User duties:** set intent, approve merges, clarify specs  
