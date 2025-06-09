// CLASSIFICATION: COMMUNITY
// Filename: SECURITY_REVIEW.md v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-13

# Cohesix Security Review Summary

This document records the results of an internal security review focusing on
alignment with seL4 principles and adherence to OWASP development guidelines.

## seL4 Alignment

- **Minimal Trusted Computing Base (TCB):** Cohesix preserves the seL4 microkernel
  proofs and keeps kernel patches minimal to retain verification coverage.
- **Capability Enforcement:** All userland services access resources through
  seL4 capabilities mapped to 9P tokens (`CohCap`). Sandbox caps are enforced for
  Plan 9 `srv` processes.
- **Formal Proof Continuity:** Kernel patches follow the seL4 coding style and are
  revalidated when rebasing upstream.

## OWASP Compliance

- **OWASP Top Ten:** Network-facing components are tested against injection,
  broken authentication, and XSS threats as outlined in `TEST_GUIDE.md`.
- **ASVS Level 1+:** Critical services aim for ASVS Level 1 compliance with a
  roadmap toward Level 2 on edge-critical modules.
- **Dependency Scanning:** CI integrates `cargo audit` and Go module scanning to
  detect vulnerable libraries.
- **Container Scanning:** Tools such as `trivy` run against all container images
  prior to deployment.
- **Secure Coding Practices:** Code reviews enforce input validation, error
  handling, and least privilege.

## Recommendations

1. Schedule quarterly seL4 proof refreshes after major patch sets.
2. Expand fuzz testing for 9P protocol handlers.
3. Document incident response playbooks in `SECURITY_POLICY.md`.

## July 2025 Updates

- Sandbox service now enforces syscalls using `/etc/cohcap.json` and logs
  blocked actions to `/log/sandbox.log` with PID and `cohrole`.
- FFI entry points loaded via `libloading` are wrapped by the validator and
  unknown symbols are rejected.


