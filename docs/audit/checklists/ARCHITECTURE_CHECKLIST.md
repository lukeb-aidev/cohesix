<!-- Author: Lukas Bower -->
<!-- Purpose: Checklist for architecture and charter conformance with evidence capture and independent review. -->
<!-- Copyright 2026 Lukas Bower -->

# Architecture Conformance Checklist

## Run Metadata
- Audit date:
- Commit SHA:
- Auditor:
- Independent reviewer:

## Architecture Checks
- [ ] Trust-boundary and affected control families documented in `docs/audit/CONTROL_TRACEABILITY.md`.
- [ ] Capability discipline preserved: no implicit authority transfer, no ad-hoc RPC bypass.
- [ ] Secure9P constraints enforced (`msize <= 8192`, walk depth <= 8, no `..`, fid lifecycle discipline).
- [ ] No unauthorized in-VM TCP listener beyond approved console exception.
- [ ] HAL boundary preserved: no direct MMIO/unsafe access outside HAL-owned layers.
- [ ] Queen/worker lifecycle semantics align with `docs/ROLES_AND_SCHEDULING.md`.
- [ ] Namespace layout and control paths align with `docs/INTERFACES.md`.
- [ ] Generated manifest and docs snippets align with code behavior (`scripts/check-generated.sh`).
- [ ] Unsafe-code deltas reviewed with explicit safety justification and reviewer sign-off.

## Evidence References
- Architecture evidence paths:
- Command logs:
- Related finding IDs:

## Sign-off
- Auditor decision: `PASS` | `FAIL`
- Independent reviewer decision: `PASS` | `FAIL`
- Decision date:
- Notes:
