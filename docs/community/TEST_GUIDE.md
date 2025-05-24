// CLASSIFICATION: COMMUNITY
// Filename: TEST_GUIDE.md v1.2
// Date Modified: 2025-05-26
// Author: Lukas Bower

# Testing Guide

A rigorous, multi‐tiered testing strategy ensures each component of Cohesix is rock solid. Tests are grouped by subsystem and type, enforced via CI on ARM and x86 platforms.

---

## 1. Bootloader Tests

### 1.1 Unit Tests
- Validate configuration parsing for bootloader images.  
- Mock device trees to ensure correct memory layout computation.

### 1.2 Smoke Tests
- Flash minimal boot image to reference SBC (Jetson Orin Nano) and verify kernel load stage begins.  
- Check serial console output for expected boot messages.

### 1.3 Regression Tests
- Maintain a suite of known-good board initialization tests; fail if any regression detected.  
- Automate via QEMU emulation with saved snapshots.

---

## 2. Kernel Tests (seL4)

### 2.1 Unit & Property Tests
- Run `seL4` proof validator on modified patches.  
- QuickCheck/Proptest on syscall mediation layer.

### 2.2 Integration & API Tests
- Launch guest processes; verify IPC isolation and capability enforcement.  
- Test edge cases: invalid syscalls, boundary buffer sizes.

### 2.3 Fuzzing
- Fuzz 9P protocol handler and syscall interface using libFuzzer.  
- Coverage target ≥ 90% in protocol state machine.

---

## 3. Core OS Tests (Plan 9 userland)

### 3.1 Unit Tests
- Shell built-ins (`rc`): test parser, built-in commands, environment variable handling.  
- 9P server mounts: mock clients to validate read/write semantics.

### 3.2 End‐to‐End Smoke Tests
- Boot image → run `rc` scripts → mount `/srv/cuda` and `/sim/` → invoke sample workloads.  
- Validate correct teardown and resource cleanup.

### 3.3 Security & Regression Tests
- Perform sandbox escape attempts; ensure seL4 caps block unauthorized access.  
- Regression harness on filesystem operations and namespace remounting.

---

## 4. OSS Dependencies Tests

### 4.1 License & Compliance Checks
- Automated audit of `DEPENDENCIES.md` vs. SPDX headers in source.  
- Fail build on prohibited licenses (GPL/AGPL).

### 4.2 Functional Tests
- BusyBox coreutils: validate common commands (`ls`, `cp`, `grep`) against Linux reference outputs.  
- Verify `ssh` login/logout flows via BusyBox `sshd`.

---

## 5. Compiler Tests (Coh_CC)

### 5.1 Unit Tests
- IR constructors and accessors: ensure correct AST structure.  
- Pass transformations: test NOP elimination, dead-code, const-folding on synthetic IR.

### 5.2 Integration & End‐to‐End Tests
- Compile example IR modules through `cohcc` → generate WASM/C → compile and run outputs.  
- Verify results match expected output for arithmetic, branching, and function calls.

### 5.3 Regression & Performance Tests
- Track compilation time and generated code size; alert on significant regressions.  
- Maintain performance baselines for pass execution times.

---

## 6. Tooling Tests

### 6.1 CLI & Argument Parsing
- Test `--help`, required/optional flags, error messages for invalid inputs.  
- Validate integration with `clap` and exit codes.

### 6.2 CI & Automation
- Smoke tests for `cohcli` commands (`codex run`, `hydrate_docs`).  
- Validate log output in `codex_logs/` and correct filename conventions.

### 6.3 Distributed & Remote Build
- Simulate remote build via SSH; verify artifact transfer and exit status.  
- Healthcheck service endpoint returns OK within 100 ms.

---

## 7. Additional Testing Practices

- **Auditing:** All new tests must be reviewed by at least one peer; document in PR.  
- **Coverage:** Enforce 80%+ coverage across Rust and Go codebases.  
- **Test Data Management:** Version‐control representative IR modules, fixture scripts, and sandbox images.  
- **CI Enforcement:** GitHub Actions must run all tests on PR; failures block merges.

---

> _Strict adherence to this guide guarantees Cohesix remains robust, secure, and performant at every development stage._

---

## 8. Security Standards & OWASP Compliance

To ensure Cohesix meets modern security benchmarks, we incorporate OWASP standards across OS services and tooling:

- **Threat Modeling:** Perform threat analysis for each subsystem; document in `docs/security/THREAT_MODEL.md`.
- **OWASP Top Ten:** Regularly test network-facing components (SSH, gRPC endpoints, 9P mounts) against Top Ten risks (Injection, Broken Auth, XSS, etc.).
- **Application Security Verification Standard (ASVS):** Validate critical services at ASVS Level 1; aim for Level 2 on EDGE-critical modules.
- **Dependency Scanning:** Integrate SCA tools (e.g., `cargo audit`, `go list -m`) into CI to detect CVEs in OSS dependencies.
- **Container & Image Scanning:** Apply tools like `Trivy` or `Clair` to any containerized images (e.g., Codex agents) before deployment.
- **Secure Coding Practices:** Enforce secure patterns (input sanitization, least privilege, proper error handling) as part of code reviews.
- **Security Logging:** Ensure all security events (authentication failures, permission denials) are logged and audited via 9P logs.