// CLASSIFICATION: COMMUNITY
// Filename: CONTRIBUTING.md v1.0
// Date Modified: 2025-06-09
// Author: Lukas Bower

# Contributing to Cohesix

Thank you for considering a contribution to the Cohesix project!

Cohesix is a security-first, performance-critical, and rigorously engineered operating system built atop seL4, Plan 9, and Rapier physics. We welcome contributions that uphold the integrity, quality, and long-term maintainability of the project.

## 📌 Project Principles

Every contribution must align with these pillars:

- **Security by Design**: All logic must default to safe, capability-scoped behavior.
- **Performance Matters**: Fast boot, efficient execution, and real-time determinism are priorities.
- **Hardware-Aware**: NVIDIA CUDA, Jetson/Orin targets, and 9P integration are first-class concerns.
- **Robust Engineering**: No placeholders, no stubs, no broken windows. Ship only what works.
- **Quality > Quantity**: Every line of code must demonstrate care, clarity, and purpose.

## ✅ How to Contribute

### 1. Fork and Branch

- Fork the repository.
- Create a feature branch (`feat/your-feature-name`) or fix branch (`fix/your-fix-name`).

### 2. Follow the Coding Standards

- **Rust**: Use `rustfmt`, follow Cohesix kernel/driver conventions.
- **Go**: Idiomatic Go with CSP in mind.
- **Python**: Typed where applicable, documented, no unbounded wildcards.
- **C**: Match seL4 upstream conventions.
- Include `// CLASSIFICATION:` and version headers in all canonical files.
- No `TODO` or `unimplemented!()` in committed code. CI will fail.

### 3. Commit Messages

- Use concise and descriptive messages.
- Reference issues when relevant (`Fixes #42`, `Closes #103`).
- Begin with `feat:`, `fix:`, `refactor:`, or `docs:`.

### 4. Submit a Pull Request

- Open a PR to the `main` branch.
- Ensure all CI tests pass (multi-arch, fuzz, validator, trace replay).
- Expect a review by maintainers and the expert panel if touching core logic.

## 🛡 Security and Licensing

- All code must comply with the Cohesix OSS policy: MIT, BSD, or Apache 2.0 only.
- Include SPDX license headers where appropriate.
- Avoid GPL-derived content unless pre-cleared and sandboxed.
- Any security-relevant code changes will undergo additional scrutiny.
- All CUDA and 9P modules must pass role-based validation and must not expose unsafe defaults.

## 🧪 Testing Expectations

- Include or update unit tests for all new logic.
- Use deterministic simulations where applicable (`/sim/`, `cohfuzz`, etc.).
- Validate GPU runtime and 9P handler behavior using real-device or emulated test cases.
- Include trace files or examples if modifying agent behavior or validation logic.
- Add to `VALIDATION_SUMMARY.md` when contributing core validation rules.

## 🧠 Final Checklist

Before you submit a pull request:

- [ ] Code is readable, minimal, and well-commented.
- [ ] You added or updated tests.
- [ ] It builds cleanly on both `aarch64` and `x86_64`.
- [ ] You ran the validation suite and reviewed logs.
- [ ] All stubs and placeholders are removed.
- [ ] Documentation (e.g., CLI help, man pages) is updated.

---

Thank you for helping build a clean, principled, and world-class system.
