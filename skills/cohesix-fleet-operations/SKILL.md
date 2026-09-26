---
name: cohesix-fleet-operations
description: Build a trustworthy read-only view across Cohesix hives and coordinate one scoped cross-hive action. Use for fleet pressure, leases and federation review, not for changing a whole fleet from one inferred approval.
license: Apache-2.0
---
<!-- Author: Lukas Bower -->
<!-- Purpose: Preserve per-hive authority and freshness while combining fleet observations and actions. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Operate across hives

Start with the same-version [fleet recipe](../../docs/OPERATOR_RECIPES.md#read-a-small-fleet),
[host tools](../../docs/HOST_TOOLS.md) and
[authority contract](../../docs/M27A_AUTHORITY.md). Select the actual named
hives and a matching macOS or Linux client installation. Either host can read
the selected hives; their QEMU, Pi and native provider profiles still determine
which actions are available. Use `coh fleet --help` and read-only
`status`, `lease-summary` and `pressure` before any mutation. A missing hive,
stale timestamp or failed read is **unknown**, not healthy or idle.
Check the client version and each hive's selected profile separately. If one
host or provider is incompatible, keep that row unknown and offer the matching
host bundle or a supported provider configuration for that hive; do not copy
another hive's authority or capability into it.

## Produce a useful fleet view

For each hive, record target image/profile and connection, observation time
and freshness, Worker readiness, scheduler pressure, active leases, last
relevant job and strongest blocker. Keep QEMU, physical Pi and retained
replay observations in their own evidence classes. Compare pressure using
each hive's published bounds; do not add quotas as if they were one global
pool. Host-side fan-in may improve visibility, but it does not transfer
authority from one Queen to another.

If the user asks for a cross-hive action, name the source and destination,
which hive admits each step, the exact delegated subject/ticket, limit,
expiry and fallback. Preview the route and check each hive's current state.
Submit only the specifically authorised leg. Preserve source and destination
IDs and receipts separately; a relay ACK cannot certify a downstream native
effect or exactly-once execution. After a disconnect, inspect the original
identity at both ends before considering recovery. Do not use a broad shared
credential to fill a missing local grant.

Return a fleet table with one row per hive, source and timestamp, pressure,
readiness, active work and uncertainty. For an action, add the route, both
identities, local admission and native result, independently verified outcome
and unresolved leg. Use the [evidence skill](../cohesix-evidence/SKILL.md)
to compare before/after packs or create a federation incident case. Factory,
logistics and other sector applications still require their own live provider
and safety evidence; playbook names alone are design patterns.
