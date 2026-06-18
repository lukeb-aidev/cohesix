<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define the as-built Cohesix role, ticket, namespace, and scheduling-control model. -->
<!-- Author: Lukas Bower -->
# Roles & Scheduling Policy

This document summarizes the as-built role model and scheduling-control surfaces. Generated manifests and snippets remain authoritative for exact limits, paths, and gates; see
[docs/snippets/root_task_manifest.md](snippets/root_task_manifest.md),
[docs/snippets/observability_interfaces.md](snippets/observability_interfaces.md),
[SECURE9P.md](SECURE9P.md), and [USERLAND_AND_CLI.md](USERLAND_AND_CLI.md).

## Roles

| Role | Purpose | Namespace view |
| --- | --- | --- |
| **Queen** | Hive-wide orchestration through `cohsh` and host tools: lifecycle control, mounts/binds, logs, GPU leases, policy, audit, and replay when enabled. | Full tree: `/`, `/queen`, `/log`, `/proc`, `/shard/*/worker/*`, legacy `/worker/*` when enabled, plus manifest-gated `/gpu`, `/host`, `/policy`, `/actions`, `/audit`, `/replay`, `/updates`, and `/models`. |
| **WorkerHeartbeat** | Minimal heartbeat worker for attach, telemetry, and lifecycle proof. | `/proc/boot`, `/proc/lifecycle/*`, `/shard/<label>/worker/<id>/telemetry`, `/log/queen.log` read-only; legacy `/worker/<id>/telemetry` when enabled. |
| **WorkerGpu** | GPU lease-state and telemetry worker for host-published GPU nodes. | WorkerHeartbeat view plus `/gpu/<id>/*` when GPU nodes are present. CUDA/NVML remain host-side. |
| **WorkerBus** | Field-bus worker for manifest/sidecar-provided bus adapters. | WorkerHeartbeat view plus `/bus/<adapter>/*` when MODBUS/DNP3 sidecars are enabled. |
| **WorkerLora** | LoRa worker for manifest/sidecar-provided LoRa adapters. | WorkerHeartbeat view plus `/lora/<adapter>/*` when LoRa sidecars are enabled. |

Exactly one Queen role owns hive-wide orchestration. Multiple worker instances may exist across the worker roles above. Queen tickets are optional in current attach flows; worker roles require a ticket with a subject identity.

## Worker Namespaces

The generated manifest enables sharded worker namespaces:

- `sharding.enabled = true`
- `sharding.shard_bits = 8`
- `sharding.legacy_worker_alias = true`
- canonical telemetry path: `/shard/<label>/worker/<id>/telemetry`
- legacy telemetry alias: `/worker/<id>/telemetry`

`label` is derived from the top `shard_bits` of `sha256(worker_id)[0]` and formatted as two hex digits. The checked-in manifest currently enables both canonical sharded paths and legacy `/worker` aliases; new role and namespace documentation should prefer the canonical sharded path while acknowledging generated host/UI defaults that still consume `/worker` aliases.

When the legacy alias is disabled, manifests must not reference `/worker/*` in mounts or policy rules. `coh-rtc` validates the required Secure9P walk depth for the selected sharding mode.

## Tickets and Attach

Tickets use the `cohesix-ticket` format: a role, optional subject identity, budget, optional mounts, issue timestamp, optional UI scopes, and per-ticket quotas, MACed with a role secret. Host tooling can mint tickets from role secrets in the selected config; root-task and NineDoor register the generated role secrets and validate presented tickets during attach.

Attach rules:

- Queen may attach without a ticket; if a Queen ticket is supplied, it is validated.
- Worker roles require a valid ticket and a subject identity.
- Ticket subject mismatch, expiration, invalid MAC, unsupported role, or missing worker ticket fails attach explicitly.
- Valid tickets configure the session role, namespace view, budget state, and any ticket-scoped quotas.

Default budgets are role-aware: Queen is unbounded, WorkerGpu uses the GPU default, and WorkerHeartbeat, WorkerBus, and WorkerLora use the heartbeat default. Budget fields are `ticks`, `ops`, and `ttl_s`; NineDoor tracks remaining ticks/ops and ticket TTL at session level.

## Control Surfaces

Role orchestration is file-oriented. Host tools and Queen sessions append bounded commands to manifest-defined files; there is no ad-hoc RPC path.

Primary control files:

- `/queen/ctl`
- `/queen/lifecycle/ctl`
- `/queen/schedule/ctl`
- `/queen/lease/ctl`
- `/queen/export/ctl`
- `/policy/ctl`

Primary observability files:

- `/proc/schedule/summary`
- `/proc/schedule/queue`
- `/proc/lease/summary`
- `/proc/lease/active`
- `/proc/lease/preemptions`
- `/proc/pressure/policy`
- `/policy/rules`
- `/policy/preflight/*`
- `/actions/queue`

These paths are manifest-gated and size-bounded. The checked-in manifest enables policy and actions, but audit and replay are disabled by default unless their generated gates are enabled.

## Scheduling Model

As built, Cohesix exposes a bounded schedule-control queue rather than a full general-purpose scheduler contract:

- `/queen/schedule/ctl` accepts append-only JSONL schedule entries such as `id`, `role`, `priority`, `ticks`, and `budget_ms`.
- `/proc/schedule/summary` reports queue totals and generated queue capacity.
- `/proc/schedule/queue` reports queued entries in the generated text format.

This is control-plane scheduling state, not a claim of kernel-level priority-band scheduling or end-to-end worker CPU isolation. seL4 scheduling contexts, affinity, worker service buckets, and physical-target throughput claims remain profile-qualified and must be backed by the relevant generated manifests and target evidence.

`cohsh` and other host tools run outside the target. QEMU/VM sessions use the authenticated TCP console path; Pi 4 hardware uses the U-Boot profile family and platform serial/TCP-console paths where enabled. UEFI/AWS behavior is profile-scoped work only where the build plan admits it.

## Revocation and Closure

Revocation is enforced at the session/control-surface level today:

1. NineDoor detects ticket TTL expiry, operation budget exhaustion, tick budget exhaustion, Queen kill commands, or policy denial.
2. The affected session is marked closed or revoked with an explicit reason.
3. Further operations fail through the existing Secure9P/console error path.
4. Relevant events are surfaced through logs, audit/policy surfaces when enabled, and generated observability nodes.

Target-specific seL4 resource teardown belongs to root-task and is profile-qualified. Do not document TCB, scheduling-context, or cap deallocation as complete for a role unless the current code, generated manifests, and target evidence prove it.

## Validation

Use the staged Test Plan and generated-artifact checks as the source of truth for this document:

- `scripts/check-generated.sh`
- `scripts/ci/test_plan_run.sh --list`
- `scripts/ci/test_plan_run.sh --state-dir out/test-plan/<run-id>`

Focused tests that currently cover this surface include NineDoor schedule queue tests, control-plane transcript tests, ticket-mint tests, policy/audit tests, and worker attach tests. Add target-specific checks when changing role semantics, ticket fields, namespace paths, schedule-control formats, or generated manifest gates.
