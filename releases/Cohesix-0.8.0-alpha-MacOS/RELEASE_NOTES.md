<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Release notes for Cohesix 0.8.0-alpha. -->
<!-- Author: Lukas Bower -->
# Cohesix 0.8.0-alpha Release Notes

Date: 2026-02-14

## Highlights
- Milestone 25b closure: secure-scale gateway hardening and due-diligence closure evidence completed.
- Milestone 25c delivery: world-class Python orchestration SDK with typed control APIs, host integrations, and 1k-fleet playbooks.
- Post-25a carry-forward improvements: stronger Python test-plan gates, REST auth alignment in Python clients, and benchmark evidence packaged in-repo.

## Milestone 25b - Secure Scale Gateway
- Request-level auth is enforced for mutating REST paths (`Authorization: Bearer` or `x-cohesix-auth`) while preserving single-console authority.
- Console token hygiene and auth-log redaction are tightened across root-task and host tooling paths.
- Due-diligence closure evidence is published with baseline gate run artifacts:
  - `out/audit/gate/20260214T044955Z`
  - `docs/audit/AUDIT_REPORT_2026-02-14.md`
  - `docs/audit/findings.csv` (P0/P1 blockers closed)
- Deterministic REST harness and 1k-worker readiness reporting are integrated into benchmark workflows.

## Milestone 25c - Python Orchestration SDK
- Added typed orchestration APIs for approvals, schedule, lease, and export control-file workflows.
- Added host integration adapters for `systemd`, Docker, Kubernetes, NVML/NVIDIA, and PEFT runtime probes.
- Added nine built-in high-impact playbooks for Mac, Jetson, and mixed fleets, with `cohesix-playbook` CLI support.
- Expanded Python tests for orchestration, integrations, playbooks, and parity workflows.
- Improved REST Python interoperability:
  - `RestBackend` now sends gateway request-auth headers.
  - Env fallback supports `HIVE_GATEWAY_REQUEST_AUTH_TOKEN`, `COHSH_REST_AUTH_TOKEN`, and `COH_REST_AUTH_TOKEN`.

## Additional Enhancements Since 25a
- Test-plan flow now includes broader Python coverage:
  - Stage 02 runs the full `tools/cohesix-py/tests` suite and playbook CLI dry-run smoke.
  - Stage 04 adds a Python REST smoke (`LS /`, `CAT /log/queen.log`) against gateway mode.
- Release bundle packaging includes enhanced Python SDK files, tests, examples, and `docs/PYTHON_SUPPORT.md`.
- Benchmark artifacts referenced by docs are now vendored under `docs/bench/` with updated links from `docs/BENCHMARKS.md`.

## Bundled tools
- `cohsh`, `coh`, `swarmui`, `cas-tool`, `gpu-bridge-host`, `host-sidecar-bridge`, `hive-gateway`
- Python SDK under `python/cohesix-py`
- QEMU run script under `qemu/run.sh`

## Notes
- Linux host tools for this release are produced on the Ubuntu `t4g` builder role and packaged into the Linux bundle.
- GPU hardware access remains host-side only; the VM never touches CUDA/NVML directly.
