<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Release notes for Cohesix 0.9.0-beta. -->
<!-- Author: Lukas Bower -->
# Cohesix 0.9.0-beta Release Notes

Date: 2026-02-18

## Highlights
- Milestone 25d stabilization is in: REST request-auth parity is aligned across host tools, and REST mount behavior is hardened for real operator workflows.
- Milestone 25e is shipped: deterministic evidence packs, timeline correlation, and CI/SIEM integration kits are available in-tree and in bundle docs/examples.
- Milestone 25f is shipped: `hive-gateway` broker reliability work plus explicit no-retry large-telemetry gates (`1 MB`, `10 MB`, `100 MB`, `1 GB`) are now part of release readiness.
- Milestone 25g is shipped: host control tickets are now first-class (`/host/tickets/*`) with a dedicated `host-ticket-agent` and bounded executors for GPU/PEFT, systemd, Docker, and K8s coexistence intents.

## Scope Since 0.8.0-alpha
This beta includes all updates landed after tag `v0.8.0-alpha`, including Milestones 25d, 25e, 25f, and 25g, plus staged test-plan and due-diligence closure work required for beta quality gates.

## Milestone 25d - REST Request-Auth Parity and Mount Correctness
- REST request-auth handling is consistent across REST-capable host tools, with canonical env fallbacks and deterministic behavior under `hive-gateway`.
- `coh mount --rest-url` correctness is tightened for operator use:
  - bounded readable `/proc` and `/log` projections,
  - append-only telemetry path behavior preserved,
  - mount exclusivity and safety semantics kept deterministic.
- Host/operator docs and test-plan coverage were updated to reflect as-built REST parity behavior.

## Milestone 25e - Evidence Packs and Integration Kits
- `coh evidence pack` and `coh evidence timeline` workflows are in place with deterministic artifact layout.
- Evidence pipelines include stronger cross-surface correlation (manifest bounds, policy snapshots, logs/proc/audit where present).
- Python evidence support and integration examples are added for CI and SIEM export workflows.
- Cohesix docs now include adoption-ready operator guidance for audit-first environments.

## Milestone 25f - Gateway Broker and Large Telemetry Reliability Gate
- `hive-gateway` request handling is brokerized with bounded queueing and explicit backpressure signaling.
- Large telemetry scenarios are first-class in the harness:
  - `telemetry-1mb`
  - `telemetry-10mb`
  - `telemetry-100mb`
  - `telemetry-1gb`
- No-retry, fast-ramp reliability gating (`error_budget_rate=0.01`) is now part of mandatory beta validation.
- Harness and docs were updated so failures are visible and actionable (no hidden retry masking).

## Milestone 25g - Host Control Tickets and Orchestration Plane Expansion
- New bounded ticket namespace under `/host/tickets/*`:
  - `/host/tickets/spec`
  - `/host/tickets/status`
  - `/host/tickets/deadletter`
  - snapshot views for deterministic reads.
- New `host-ticket-agent` host tool:
  - tails ticket specs,
  - enforces idempotency via `id + idempotency_key`,
  - appends deterministic lifecycle receipts.
- Implemented high-value adapter classes:
  - GPU lease actions (`gpu.lease.grant|renew|release`)
  - PEFT lifecycle (`peft.import|activate|rollback`)
  - systemd remediation (`start|stop|restart|status-check`)
  - Docker remediation (`restart|stop|status-check`)
  - K8s coexistence intents (`k8s.cordon|drain|lease.sync`)
- Evidence/timeline tooling now correlates ticket spec/status/deadletter using the same idempotency keying model.
- Python orchestration support includes typed host-ticket requests and K8s RBAC-to-ticket translation helpers.

## Validation Summary for Beta
- Staged test-plan PASS run:
  - `out/test-plan/beta-verify-20260218-115016`
  - `stage_01.done` through `stage_05.done`
  - no incomplete markers.
- Due-diligence gate PASS:
  - `out/audit/gate/20260218T010655Z`.
- Large telemetry reliability gates PASS (`--no-retries --fast-ramp --error-budget-rate 0.01`):
  - local summaries under `logs/rest_bench_20260217T*.summary.json`,
  - G5g summaries under `/home/ubuntu/cohesix-m25f/logs/rest_bench_20260218T*.summary.json`.

## Bundled Tools
- `cohsh`
- `coh`
- `swarmui`
- `cas-tool`
- `gpu-bridge-host`
- `host-sidecar-bridge`
- `host-ticket-agent`
- `hive-gateway`
- Python SDK under `python/cohesix-py`
- QEMU launcher under `qemu/run.sh`

## Platform and Build Notes
- Linux host tools for this release are built on the Ubuntu AWS `t4g.small` builder role and synced into the Linux bundle.
- GPU, CUDA, NVML, and PEFT remain host-side by design; no in-VM GPU stack is introduced.
- Single-console architecture is preserved: `hive-gateway` is the multiplexed console holder for REST multi-client workflows.
