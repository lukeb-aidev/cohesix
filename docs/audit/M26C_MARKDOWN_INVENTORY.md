<!-- Author: Lukas Bower -->
<!-- Purpose: Summarize the Milestone 26c tracked-Markdown inventory and dispositions. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Markdown Inventory

This report is derived from `docs/audit/M26C_MARKDOWN_INVENTORY.csv`.

## Summary

- tracked Markdown files: 204
- append-only audit evidence: 4
- external reference mirror: 2
- generated report: 1
- generated snippet: 18
- human-edited canonical source: 44
- live audit register: 24
- release snapshot: 72
- vendored reference: 39

## Disposition Rules

- `append-only audit evidence`: append only with dated evidence; do not rewrite for style
- `external reference mirror`: inventory only; update through accepted seL4 reference refresh
- `generated report`: update only through NIST report generator
- `generated snippet`: update only through coh-rtc or source generator
- `human-edited canonical source`: edit directly with generated/as-built evidence
- `live audit register`: update only with matching evidence command
- `release snapshot`: update only through release-cut flow
- `vendored reference`: inventory only; update through upstream vendor import

## Inventory

| Path | Disposition | Owner | Update rule |
| --- | --- | --- | --- |
| `AGENTS.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `CONTRIBUTING.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `README.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `apps/gpu-bridge-host/README.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `apps/nine-door/README.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `apps/root-task/README.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `apps/worker-gpu/README.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `crates/boot/uefi-elfloader-shim/README.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `crates/cohesix-ticket/README.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `crates/secure9p-codec/README.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/API_GUIDELINES.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/ARCHITECTURE.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/AWS_AMI.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/BENCHMARKS.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/BOOT_REFERENCE.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/BUILD_PLAN.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/CODING_GUIDELINES.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/DRIVERS.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/FAILOVER.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/FAILURE_MODES.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/GPU_NODES.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/HARDWARE_BRINGUP.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/HOST_API.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/HOST_TOOLS.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/INTERFACES.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/NETWORK_CONFIG.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/OPERATOR_RECIPES.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/OPERATOR_WALKTHROUGH.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/PYTHON_SUPPORT.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/QUICKSTART.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/QUICKSTART_ALPHA.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/REPO_LAYOUT.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/ROLES_AND_SCHEDULING.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/SECURE9P.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/SECURITY.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/SECURITY_NIST_800_53.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/TEST_PLAN.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/TOOLCHAIN_MAC_ARM64.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/USERLAND_AND_CLI.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/USE_CASES.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/WORKER_TICKETS.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `docs/audit/AUDIT_REPORT_2026-02-12.md` | append-only audit evidence | audit-owner | append only with dated evidence; do not rewrite for style |
| `docs/audit/AUDIT_REPORT_2026-02-13.md` | append-only audit evidence | audit-owner | append only with dated evidence; do not rewrite for style |
| `docs/audit/AUDIT_REPORT_2026-02-14.md` | append-only audit evidence | audit-owner | append only with dated evidence; do not rewrite for style |
| `docs/audit/BLOCKERS.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/CONTROL_TRACEABILITY.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/DUE_DILIGENCE_PLAN.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/EXCEPTIONS.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/M26B_COMPLETION_EVIDENCE.md` | append-only audit evidence | audit-owner | append only with dated evidence; do not rewrite for style |
| `docs/audit/M26C_AGENT_HANDOFFS.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/M26C_AGENT_RUNNER_HANDOFF.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/M26C_AI_FINGERPRINT_AUDIT.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/M26C_AS_BUILT_BLOCKERS.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/M26C_DOCS_AS_BUILT_AUDIT.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/M26C_DOC_DRIFT_LEDGER.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/M26C_MARKDOWN_INVENTORY.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/M26C_MERMAID_GITHUB_RENDER_AUDIT.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/M26C_NINEDOOR_PARITY_MATRIX.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/M26C_POST_BEHAVIOR_BASELINE.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/M26C_REFACTOR_MAP.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/M26C_REFACTOR_OWNERSHIP.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/M26C_RUNTIME_BOUNDARY_AUDIT.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/M26C_SIMPLICITY_SCORECARD.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/M26C_TARGET_RUNNER_BASELINE.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/M26D_SEL4_15_PROVENANCE.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/PI4_ROOT_OWNED_DRIVER_INVENTORY.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/checklists/ARCHITECTURE_CHECKLIST.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/audit/checklists/SECURITY_CHECKLIST.md` | live audit register | audit-owner | update only with matching evidence command |
| `docs/nist/REPORT.md` | generated report | generator-owner | update only through NIST report generator |
| `docs/snippets/cas_interfaces.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/cas_security.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/coh_doctor_checks.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/coh_policy.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/cohesix_py_defaults.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/cohsh_client.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/cohsh_grammar.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/cohsh_policy.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/cohsh_ticket_policy.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/gpu_breadcrumbs.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/latency_metrics.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/observability_interfaces.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/observability_security.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/root_task_manifest.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/swarmui_defaults.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/telemetry_cbor_schema.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/ticket_quotas.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `docs/snippets/trace_policy.md` | generated snippet | generator-owner | update only through coh-rtc or source generator |
| `releases/Cohesix-0.8.0-alpha-MacOS/QUICKSTART.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-MacOS/README.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-MacOS/RELEASE_NOTES.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-MacOS/docs/ARCHITECTURE.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-MacOS/docs/GPU_NODES.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-MacOS/docs/HOST_TOOLS.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-MacOS/docs/INTERFACES.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-MacOS/docs/NETWORK_CONFIG.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-MacOS/docs/PYTHON_SUPPORT.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-MacOS/docs/ROLES_AND_SCHEDULING.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-MacOS/docs/SECURE9P.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-MacOS/docs/SECURITY.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-MacOS/docs/USERLAND_AND_CLI.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-MacOS/docs/USE_CASES.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-MacOS/docs/WORKER_TICKETS.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-MacOS/python/cohesix-py/README.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-linux/QUICKSTART.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-linux/README.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-linux/RELEASE_NOTES.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-linux/docs/ARCHITECTURE.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-linux/docs/GPU_NODES.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-linux/docs/HOST_TOOLS.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-linux/docs/INTERFACES.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-linux/docs/NETWORK_CONFIG.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-linux/docs/PYTHON_SUPPORT.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-linux/docs/ROLES_AND_SCHEDULING.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-linux/docs/SECURE9P.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-linux/docs/SECURITY.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-linux/docs/USERLAND_AND_CLI.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-linux/docs/USE_CASES.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-linux/docs/WORKER_TICKETS.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.8.0-alpha-linux/python/cohesix-py/README.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-MacOS/QUICKSTART.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-MacOS/README.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-MacOS/RELEASE_NOTES.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-MacOS/docs/ARCHITECTURE.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-MacOS/docs/GPU_NODES.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-MacOS/docs/HOST_TOOLS.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-MacOS/docs/INTERFACES.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-MacOS/docs/NETWORK_CONFIG.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-MacOS/docs/PYTHON_SUPPORT.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-MacOS/docs/ROLES_AND_SCHEDULING.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-MacOS/docs/SECURE9P.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-MacOS/docs/SECURITY.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-MacOS/docs/USERLAND_AND_CLI.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-MacOS/docs/USE_CASES.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-MacOS/docs/WORKER_TICKETS.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-MacOS/python/cohesix-py/README.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-linux/QUICKSTART.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-linux/README.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-linux/RELEASE_NOTES.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-linux/docs/ARCHITECTURE.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-linux/docs/GPU_NODES.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-linux/docs/HOST_TOOLS.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-linux/docs/INTERFACES.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-linux/docs/NETWORK_CONFIG.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-linux/docs/PYTHON_SUPPORT.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-linux/docs/ROLES_AND_SCHEDULING.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-linux/docs/SECURE9P.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-linux/docs/SECURITY.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-linux/docs/USERLAND_AND_CLI.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-linux/docs/USE_CASES.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-linux/docs/WORKER_TICKETS.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/Cohesix-0.9.0-beta-linux/python/cohesix-py/README.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/RELEASE_NOTES-0.1.0-alpha1.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/RELEASE_NOTES-0.2.0-alpha2.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/RELEASE_NOTES-0.3.0-alpha.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/RELEASE_NOTES-0.4.0-alpha.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/RELEASE_NOTES-0.6.0-alpha.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/RELEASE_NOTES-0.7.0-alpha.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/RELEASE_NOTES-0.8.0-alpha.md` | release snapshot | release-owner | update only through release-cut flow |
| `releases/RELEASE_NOTES-0.9.0-beta.md` | release snapshot | release-owner | update only through release-cut flow |
| `seL4/elfloader.md` | external reference mirror | kernel-reference-owner | inventory only; update through accepted seL4 reference refresh |
| `seL4/seL4-manual-latest.md` | external reference mirror | kernel-reference-owner | inventory only; update through accepted seL4 reference refresh |
| `tests/integration/README.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `third_party/raspberry-pi-firmware/v1.50/Readme.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/raspberry-pi-firmware/v1.50/firmware/cyw43455-linux-capture/README.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/3rdparty/everest/README.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/3rdparty/p256-m/README.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/3rdparty/p256-m/p256-m/README.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/BRANCHES.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/BUGS.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/CONTRIBUTING.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/ChangeLog.d/00README.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/README.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/SECURITY.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/SUPPORT.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/configs/ext/README.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/3.0-migration-guide.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/architecture/alternative-implementations.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/architecture/mbed-crypto-storage-specification.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/architecture/psa-crypto-implementation-structure.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/architecture/psa-migration/md-cipher-dispatch.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/architecture/psa-migration/psa-legacy-bridges.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/architecture/psa-migration/psa-limitations.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/architecture/psa-migration/strategy.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/architecture/psa-migration/testing.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/architecture/psa-shared-memory.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/architecture/psa-storage-resilience.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/architecture/psa-thread-safety/psa-thread-safety.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/architecture/tls13-support.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/driver-only-builds.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/proposed/psa-conditional-inclusion-c.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/proposed/psa-driver-developer-guide.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/proposed/psa-driver-integration-guide.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/proposed/psa-driver-interface.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/proposed/psa-driver-wrappers-codegen-migration-guide.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/psa-driver-example-and-guide.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/psa-transition.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/tls13-early-data.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/docs/use-psa-crypto.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/programs/README.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/programs/fuzz/README.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `third_party/u-boot/lib/mbedtls/external/mbedtls/tests/git-scripts/README.md` | vendored reference | external-reference-owner | inventory only; update through upstream vendor import |
| `tools/cohesix-py/README.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
| `tools/host-bootpd/README.md` | human-edited canonical source | docs-owner | edit directly with generated/as-built evidence |
