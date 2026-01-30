# NIST 800-53 LOW Control Registry Report (Generated)

Source: docs/nist/controls.toml

| ID | Family | Status | Title | Evidence refs |
|---|---|---|---|---|
| AC-1 | AC | Implemented | Access Control Policy and Procedures | docs/SECURE9P.md, docs/ROLES_AND_SCHEDULING.md, tests/security/nist_evidence_smoke.sh |
| AC-2 | AC | NA | Account Management | - |
| AC-3 | AC | Implemented | Access Enforcement | docs/SECURE9P.md, docs/ROLES_AND_SCHEDULING.md, tests/security/nist_evidence_smoke.sh |
| AC-5 | AC | Implemented | Separation of Duties | docs/ROLES_AND_SCHEDULING.md, docs/INTERFACES.md, tests/security/nist_evidence_smoke.sh |
| AC-6 | AC | Implemented | Least Privilege | docs/ROLES_AND_SCHEDULING.md, docs/SECURE9P.md, tests/security/nist_evidence_smoke.sh |
| AC-7 | AC | Implemented | Unsuccessful Logon Attempts | docs/SECURITY.md, tests/security/nist_evidence_smoke.sh |
| AC-17 | AC | Implemented | Remote Access | docs/SECURITY.md, docs/USERLAND_AND_CLI.md, tests/fixtures/transcripts/boot_v0/tcp.txt |
| AU-2 | AU | Implemented | Audit Events | docs/SECURITY.md, docs/INTERFACES.md, tests/security/nist_evidence_smoke.sh |
| AU-3 | AU | Implemented | Content of Audit Records | docs/SECURITY.md, docs/INTERFACES.md, tests/security/nist_evidence_smoke.sh |
| AU-6 | AU | Planned | Audit Review, Analysis, and Reporting | - |
| AU-12 | AU | Implemented | Audit Generation | docs/SECURITY.md, docs/INTERFACES.md, tests/security/nist_evidence_smoke.sh |
| CM-2 | CM | Implemented | Baseline Configuration | configs/root_task.toml, docs/ARCHITECTURE.md, scripts/check-generated.sh |
| CM-3 | CM | Planned | Configuration Change Control | - |
| CM-6 | CM | Implemented | Configuration Settings | configs/root_task.toml, docs/SECURE9P.md, tests/security/nist_evidence_smoke.sh |
| CM-7 | CM | Implemented | Least Functionality | AGENTS.md, docs/SECURITY.md, tests/security/nist_evidence_smoke.sh |
| IA-2 | IA | Implemented | Identification and Authentication (Organizational Users) | docs/INTERFACES.md, docs/USERLAND_AND_CLI.md, tests/security/nist_evidence_smoke.sh |
| IA-5 | IA | Planned | Authenticator Management | - |
| IA-8 | IA | NA | Identification and Authentication (Non-Organizational Users) | - |
| SC-5 | SC | Implemented | Denial of Service Protection | docs/SECURE9P.md, docs/SECURITY.md, tests/security/nist_evidence_smoke.sh |
| SC-7 | SC | Inherited | Boundary Protection | docs/SECURITY.md |
| SC-8 | SC | Inherited | Transmission Confidentiality and Integrity | docs/SECURITY_NIST_800_53.md |
| SC-12 | SC | Planned | Cryptographic Key Establishment and Management | - |
| SC-28 | SC | NA | Protection of Information at Rest | - |
| SI-2 | SI | Planned | Flaw Remediation | - |
| SI-3 | SI | NA | Malicious Code Protection | - |
| SI-7 | SI | Planned | Software, Firmware, and Information Integrity | - |
