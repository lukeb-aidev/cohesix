<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Tailored NIST SP 800-53 (LOW) assessment pack for Cohesix with evidence pointers. -->
<!-- Author: Lukas Bower -->
# Cohesix NIST SP 800-53 (LOW) Tailored Assessment Pack

## Scope statement
Cohesix is a seL4-based control-plane operating system for orchestrating and monitoring edge GPU nodes. It is not a web application and does not run HTTP or in-VM TLS stacks. Control-plane access is via Secure9P namespaces and the authenticated TCP console; host tools (cohsh, coh, gpu-bridge-host) run outside the VM.

## System boundary (text diagram description)
- In-VM boundary: seL4 kernel (upstream), root-task event pump, NineDoor Secure9P server, queen/worker roles, synthetic namespaces, and console transport (serial + TCP console).
- Host boundary: cohsh/coh CLI, gpu-bridge-host, host storage, and host network boundary (VPN/overlay/TLS) that fronts TCP console access.
- External dependencies: upstream seL4 kernel and host GPU tooling boundary (NVML/CUDA stay host-side).

## Control inheritance
Inherited controls explicitly rely on:
- Upstream seL4 kernel isolation and capability enforcement.
- Host network boundary (VPN/overlay/TLS) for transport confidentiality and integrity.
- Host GPU tooling boundary for device access and telemetry outside the VM.

## Tailoring approach
- Baseline: NIST SP 800-53 LOW.
- Families assessed for Cohesix: AC, AU, CM, IA, SC, SI.
- Each control is marked as Implemented, Inherited, Planned, or NA.
- Mapping uses evidence pointers to repo-local docs, code, and tests. This is an evidence-based mapping, not a full compliance claim.

## Assessment methodology
- Primary registry: docs/nist/controls.toml (machine-checkable).
- Evidence guard: tools/security-nist validates status, evidence coverage, and repo-local refs.
- Smoke evidence: tests/security/nist_evidence_smoke.sh asserts non-negotiable invariants are documented.

## Control summary (tailored)
| ID | Status | Summary | Evidence pointers |
|---|---|---|---|
| AC-1 | Implemented | Access control policy defined in Secure9P and role docs. | docs/SECURE9P.md; docs/ROLES_AND_SCHEDULING.md; tests/security/nist_evidence_smoke.sh |
| AC-2 | NA | No local accounts; access uses role-scoped tickets. | - |
| AC-3 | Implemented | Access enforced by Secure9P policy and role mounts. | docs/SECURE9P.md; docs/ROLES_AND_SCHEDULING.md; tests/security/nist_evidence_smoke.sh |
| AC-5 | Implemented | Queen and worker duties are separated by roles/namespaces. | docs/ROLES_AND_SCHEDULING.md; docs/INTERFACES.md; tests/security/nist_evidence_smoke.sh |
| AC-6 | Implemented | Least privilege via role-scoped mounts and tickets. | docs/ROLES_AND_SCHEDULING.md; docs/SECURE9P.md; tests/security/nist_evidence_smoke.sh |
| AC-7 | Implemented | Auth failures are rate-limited with cooldown. | docs/SECURITY.md; tests/security/nist_evidence_smoke.sh |
| AC-17 | Implemented | Remote access limited to authenticated TCP console. | docs/SECURITY.md; docs/USERLAND_AND_CLI.md; tests/fixtures/transcripts/boot_v0/tcp.txt |
| AU-2 | Implemented | Security events emit audit lines to /log/queen.log. | docs/SECURITY.md; docs/INTERFACES.md; tests/security/nist_evidence_smoke.sh |
| AU-3 | Implemented | Audit records include reason tags and bounded details. | docs/SECURITY.md; docs/INTERFACES.md; tests/security/nist_evidence_smoke.sh |
| AU-6 | Planned | Formal audit review workflow is not yet defined. | - |
| AU-12 | Implemented | Audit generation is built into event pump/control paths. | docs/SECURITY.md; docs/INTERFACES.md; tests/security/nist_evidence_smoke.sh |
| CM-2 | Implemented | Baseline configuration in manifest; generated outputs verified. | configs/root_task.toml; docs/ARCHITECTURE.md; scripts/check-generated.sh |
| CM-3 | Planned | Formal change control process not defined. | - |
| CM-6 | Implemented | Security settings (bounds/quotas) are manifest-defined. | configs/root_task.toml; docs/SECURE9P.md; tests/security/nist_evidence_smoke.sh |
| CM-7 | Implemented | Minimal in-VM services; console-only TCP listener. | AGENTS.md; docs/SECURITY.md; tests/security/nist_evidence_smoke.sh |
| IA-2 | Implemented | Attach requires role and tickets for workers; claims verified. | docs/INTERFACES.md; docs/USERLAND_AND_CLI.md; tests/security/nist_evidence_smoke.sh |
| IA-5 | Planned | Ticket rotation/lifecycle policies not formalized. | - |
| IA-8 | NA | No public/unauthenticated user access. | - |
| SC-5 | Implemented | DoS protections via bounds and rate limiting. | docs/SECURE9P.md; docs/SECURITY.md; tests/security/nist_evidence_smoke.sh |
| SC-7 | Inherited | Boundary protection via host network boundary and seL4. | docs/SECURITY.md |
| SC-8 | Inherited | Transport confidentiality/integrity provided by host VPN/TLS. | docs/SECURITY_NIST_800_53.md |
| SC-12 | Planned | Key establishment/management outside VM not formalized here. | - |
| SC-28 | NA | No persistent storage at rest inside VM. | - |
| SI-2 | Planned | Formal flaw remediation process pending. | - |
| SI-3 | NA | No code download or arbitrary execution surfaces in VM. | - |
| SI-7 | Planned | Additional integrity verification beyond CAS signatures not scoped. | - |

## Assumptions / Non-goals
- TLS/VPN termination occurs outside the VM at the host boundary.
- Secure9P and the authenticated console are the only control-plane surfaces in the VM.
- No in-VM web stack, URL fetch, or arbitrary code execution is provided.
- The VM has no persistent storage; at-rest controls apply to host-side storage.

## Registry and report
- Authoritative control registry: docs/nist/controls.toml
- Machine-checkable report: docs/nist/REPORT.md (generated by security-nist)
