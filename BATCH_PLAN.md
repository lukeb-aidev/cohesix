// CLASSIFICATION: COMMUNITY
// Filename: BATCH_PLAN.md v0.1
// Date Modified: 2025-05-24

## Cohesix Batch Plan

This document tracks all high-level work phases for the Cohesix project.

### 1 · Coh_CC Compiler Batches
All compiler-related batches for building and scaffolding the Coh_CC toolchain.
- **Batches 1–4:** IR design, pass framework, example passes, tests — ✅ Complete
- **Batches 5–11:** Optimization passes (nop removal, dead code, const folding), SSA renaming — ✅ Complete
- **Batches 12–17:** SSA phi insertion, IR validation, codegen interface, backends, CLI — ✅ Complete
- **Batch 18:** WASM & C backends scaffold — ✅ Complete
- **Batch 19–24:** Codegen dispatcher, CLI args & integration, example IR input — ✅ Complete
- **Batch 25:** Example IR module stub — ✅ Complete
- **Next:** Any remaining compiler tooling, testing, and regression harnesses

### 2 · Cohesix OS Batches
Operating system and runtime environment scaffolding
- **Batch O1:** seL4 boot hydration & CohRole init — ◯ Queued
- **Batch O2:** Plan 9 namespace & rc shell — ◯ Queued
- **Batch O3:** Sandbox caps & security proofs — ◯ Queued
- **Batch O4:** Physics Core (/sim/) integration — ◯ Queued
- **Batch O5:** GPU support service & /srv/cuda — ◯ Queued
- **Batch O6:** Driver model & hardware abstraction — ◯ Queued
- **Batch O7:** Full OS image validation & CI integration — ◯ Queued

### 3 · Tooling Batches
Common CLI and system utilities adaptation
- **Batch T1:** BusyBox coreutils integration — ◯ Queued
- **Batch T2:** SSH & networking tools — ◯ Queued
- **Batch T3:** Manual pages & help system (`man`, `help`) — ◯ Queued
- **Batch T4:** Logging & last/finger utilities — ◯ Queued
- **Batch T5:** Package manager stub & installation scripts — ◯ Queued
- **Batch T6:** Monitoring & healthcheck services — ◯ Queued
- **Batch T7:** Distributed build tooling (ssh-driven CI) — ◯ Queued

### 4 · Codex Enablement Batches
Preparation for AI-driven code generation / agent workflows
- **Batch C1:** Stub specifications & version pinning — ◯ Queued
- **Batch C2:** CI smoke tests & README_Codex.md — ◯ Queued
- **Batch C3:** API adapter & driver integration — ◯ Queued
- **Batch C4:** Agent instructions & example tasks — ◯ Queued

---

*Statuses*: ✅ = Complete, ◯ = Queued / Pending
