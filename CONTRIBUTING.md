# Contributing to Cohesix

Thank you for your interest in contributing to **Cohesix**.

Cohesix is a deliberately constrained system with a strong emphasis on **security, determinism, and auditability**. Contributions are welcome, but they must align closely with the project’s architecture, scope, and operating rules.

This document explains how to contribute effectively—and how to avoid common pitfalls.

---

## 1. Canonical Sources (Required Reading)

The following files define the authoritative behavior and constraints of the system:

- `AGENTS.md` — Build charter and non-negotiable rules
- `docs/ARCHITECTURE.md`
- `docs/BUILD_PLAN.md`
- `docs/SECURE9P.md`
- `docs/INTERFACES.md`
- `docs/USERLAND_AND_CLI.md`
- `docs/ROLES_AND_SCHEDULING.md`

These documents are **normative**.

If a change causes code and documentation to diverge, the change is considered incomplete. Any behavioral change must update documentation and regenerated artifacts in the same pull request.

---

## 2. Project Scope and Philosophy

Cohesix is:
- a **control-plane operating system**
- built on **upstream seL4**
- with a **pure Rust userspace**
- exposing a **file-shaped control plane** via Secure9P
- designed to keep the **trusted computing base (TCB) as small as possible**

Cohesix is **not**:
- a general-purpose operating system
- a Linux or POSIX environment
- an in-VM networking platform
- a place to embed CUDA, ML runtimes, or large ecosystems

Design decisions strongly favor clarity, bounded behavior, and explicit authority over flexibility or convenience.

---

## 3. What Contributions Are Welcome

We are happy to review contributions that include:

- Bug fixes with deterministic reproduction steps
- Security hardening and validation improvements
- Test coverage (unit, integration, regression)
- Documentation corrections that reflect *as-built* behavior
- Features explicitly scoped to an active milestone in `BUILD_PLAN.md`
- Host-side tools (CLI, UI, bridges) that do not expand the VM TCB
- Performance or memory improvements with clear measurements

---

## 4. Common Reasons Contributions Are Rejected

To save time on both sides, please be aware that we generally do **not** accept:

- Refactors without a concrete correctness or safety justification
- POSIX abstractions or libc usage inside the VM
- Dynamic loading or background daemons in the VM
- New in-VM TCP services or ad-hoc networking
- CUDA, NVML, or ML runtimes inside the VM
- RPC or control paths that bypass Secure9P / NineDoor
- Features not tied to an active milestone
- Changes without tests or regression guards
- Documentation describing intent rather than actual behavior

These constraints are intentional and central to the project.

---

## 5. Contribution Workflow

### Step 1 — Confirm scope

Before starting work, ensure that:

- The change is permitted by `AGENTS.md`
- It fits within an active milestone in `BUILD_PLAN.md`
- The user-visible outcome can be stated clearly
- Any new attack surface is understood
- Regression tests can be added

If these conditions are not met, the change is unlikely to be accepted.

---

### Step 2 — Prepare the change

A complete contribution typically includes:

1. **Code**
   - Rust-only userspace in the VM (`no_std` where applicable)
   - Explicit bounds on memory and work
   - Deterministic behavior (no hidden retries or unbounded loops)

2. **Documentation**
   - Updates to affected files under `/docs`
   - Documentation must reflect the *current built system*

3. **Tests**
   - Unit tests for parsers and state machines
   - Integration tests (QEMU where appropriate)
   - CLI or `.coh` regression scripts for user-visible behavior

4. **Generated artifacts**
   - Re-run `coh-rtc`
   - Commit regenerated outputs
   - Ensure hashes match

---

### Step 3 — Validate locally

At a minimum, contributors should run:

```bash
cargo check
cargo test
cargo run -p coh-rtc
```

Depending on the change, additional validation may be required:
- Console changes → `.coh` regression scripts
- Secure9P changes → negative tests or fuzzing
- Networking changes → QEMU reproduction and logs
- Manifest changes → regeneration and hash verification

---

## 6. Pull Request Expectations

Please include the following in your pull request description:

Goal:
Current milestone:
User-visible change:
Attack surface impact:
Determinism considerations:
Regression coverage:

Pull requests are easiest to review when they are:
- narrowly scoped
- focused on a single root cause
- supported by tests and documentation

---

## 7. Style and Design Constraints

### VM / kernel side
- Upstream seL4 only
- Pure Rust userspace
- No POSIX or libc layers
- No unbounded allocation
- Secure9P is the only control plane

### Host side
- May use `std` and host OS facilities
- Must remain protocol-faithful
- Must not introduce undocumented semantics

### Logging and observability
- Prefer structured, single-line audit logs
- Avoid excessive log volume
- Use counters instead of repetitive prints where possible

---

## 8. Security Model

Cohesix assumes:
- untrusted or unreliable networks
- potentially compromised hosts
- hostile input by default

As a result:
- All input must be validated
- Authority must be explicit and revocable
- Actions must be auditable
- Failure modes must be deterministic

Changes should preserve or strengthen these properties.

---

## 9. Proposing Larger Changes

For non-trivial or architectural proposals:

1. Open an issue or discussion
2. Reference relevant documentation
3. Explain:
   - what invariant needs to change
   - why the current design is insufficient
   - how the TCB is affected
4. Propose documentation changes first

Large changes without prior discussion are unlikely to be merged.

---

## 10. Final Notes

Cohesix is intentionally opinionated and narrowly scoped.  
The goal is a system that is **small, auditable, and predictable**.

If that aligns with how you like to work, we’re glad you’re here.