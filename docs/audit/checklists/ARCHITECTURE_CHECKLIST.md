<!-- Author: Lukas Bower -->
<!-- Purpose: Checklist for architecture and charter conformance with evidence capture and independent review. -->
<!-- Copyright 2026 Lukas Bower -->

# Architecture Conformance Checklist

## Stage 05 closure review (2026-09-13)

- [x] Independent automated review verifies DD26 typed TLS storage, accessor
  ownership and the exact `8c050a0aaf7b8d0d074e9fb12fc91ef45ae60b39` Pi ELF layout.
- [x] Preserve focused review/test results for the critical-TCB IPC repair and
  ordinary target lifecycle; do not equate them with missing fault-path proof.
- [x] Independently verify fresh DD27 image integrity and DD29 firmware/USB evidence;
  keyboard absence is explicitly human-attested by Lukas Bower.
- [ ] Complete DD28 and DD30 machine evidence and the remaining release criteria.

Reviewer and evidence: [current audit report](../AUDIT_REPORT_2026-09-13.md).
Current overall decision remains **FAIL**. DD26 closure grants no physical
fault attribution, final-image boot acceptance or human merge sign-off.
The following February checklist is historical.

## Run Metadata
- Audit date: `2026-02-14`
- Commit SHA: `22cd5017d060c3439b6f7fc4f70717f329134803`
- Auditor: `automation-agent`
- Independent reviewer: `TBD`
- Evidence root: `out/audit/gate/20260214T044955Z`

## Architecture Checks
- [x] Trust-boundary and affected control families documented in `docs/audit/CONTROL_TRACEABILITY.md`.
- [x] Capability discipline preserved: no implicit authority transfer, no ad-hoc RPC bypass.
- [x] Secure9P constraints enforced (`msize <= 8192`, walk depth <= 8, no `..`, fid lifecycle discipline).
- [x] No unauthorized in-VM TCP listener beyond approved console exception.
- [x] HAL boundary preserved: no direct MMIO, physical-address/DMA publication, IRQ binding, or unsafe device access outside HAL-owned layers.
- [x] Queen/worker lifecycle semantics align with `docs/ROLES_AND_SCHEDULING.md`.
- [x] Namespace layout and control paths align with `docs/INTERFACES.md`.
- [x] Generated manifest and docs snippets align with code behavior (`scripts/check-generated.sh`).
- [x] Unsafe-code deltas reviewed for touched files in this remediation set.

## Evidence References
- Architecture evidence paths:
  - `out/audit/gate/20260214T044955Z/regression-batch.log`
  - `out/audit/gate/20260214T044955Z/generated-artifacts.log`
  - `out/audit/gate/20260214T044955Z/cargo-check-workspace.log`
  - `docs/audit/CONTROL_TRACEABILITY.md`
- Command logs:
  - `scripts/ci/due_diligence_gate.sh`
  - `scripts/check-generated.sh`
  - `scripts/cohsh/run_regression_batch.sh`
- Related finding IDs:
  - `DD-2026-0009` (closed)

## Sign-off
- Auditor decision: `PASS`
- Independent reviewer decision: `TBD`
- Decision date: `2026-02-14`
- Notes: `Architecture-related due-diligence blockers are closed in this run; independent reviewer confirmation remains pending.`
