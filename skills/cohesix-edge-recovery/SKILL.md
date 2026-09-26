---
name: cohesix-edge-recovery
description: Diagnose a degraded edge hive and carry out one authorised service recovery with before-and-after evidence. Use for heartbeat, scheduler or host-service incidents, not for automatic safety control or hardware repair.
license: Apache-2.0
---
<!-- Author: Lukas Bower -->
<!-- Purpose: Turn an edge-health alert into a scoped diagnosis, recovery decision and observed result. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Recover a degraded edge service

Use the selected installation and the operator's target identity. Start with
the read-only [inspection skill](../cohesix-inspect/SKILL.md) and the
[degradation recipe](../../docs/OPERATOR_RECIPES.md#inspect-sel4-and-mcs-state).
Keep the target's Queen/Worker state separate from the host service or model
that reported trouble. A heartbeat says the control path answered; it does
not prove an application is healthy.

## Locate the first failed boundary

Collect a bounded before-state: target image/profile, heartbeat freshness,
Worker readiness and lifecycle, scheduler/lease pressure, last relevant
ticket and deadletter, host-provider freshness, and the affected service's
native status. Classify the symptom as an unreachable target, stale Worker,
unavailable provider, resource pressure, refused action, failed native effect,
or uncertain result. Preserve the strongest known blocker. Missing data stays
unknown rather than being filled by an older capture or a model's diagnosis.

If a classifier or agent suggests a fix, make it name the evidence and a
specific allowlisted action. Compare that action with the site's fail-safe
rules, maintenance window and current scope. For a service restart, identify
the exact unit/container, expected healthy signal, maximum disruption and
rollback or manual fallback before requesting the ticket. A Kubernetes or
domain-specific action is available only when that provider is actually
configured and selected. Use the [agent delegation skill](../cohesix-agent-delegation/SKILL.md)
for MCP/A2A callers.

Choose the provider for the service's **actual host**. On macOS, an enrolled
`launchd` service can use selected `launchd.start`, `launchd.stop`,
`launchd.restart` or `launchd.status-check` actions with native account
permission; follow the
[macOS provider contract](../../docs/MACOS_PROVIDERS.md#launchd-service-lifecycle).
On Linux, use the selected `systemd` unit action, or a Docker/Kubernetes action
only where that provider is configured; follow the
[host-ticket recipe](../../docs/OPERATOR_RECIPES.md#host-tickets-and-federation).
The controller may be a different supported Mac or Linux host. Do not infer a
service action from the controller OS or substitute one provider for another.
Check the live provider catalogue, service enrollment and native permission
before suggesting a ticket. If they do not match the service host, explain
which piece is missing and offer the platform's enrollment or permission path;
leave the service unchanged until a supported action is selected.

## Act once and observe

With existing authority, submit the narrow host-ticket action through the
installed CLI, REST or agent client. Keep its original ticket/idempotency ID.
An ACK is admission, not recovery. Follow ticket status and deadletter,
native service state, fresh application health, and target heartbeat after
the action. If the response is lost or the provider restarts, reconcile the
original ID before considering another effect. Cancellation and rollback
require their own current authority. Do not turn a diagnostic read into a
write by starting a second gateway, changing networking or bypassing a refusal.

Give the operator a short incident account: before-state and freshness,
diagnosis with uncertainty, action and scope, original ID, native observation,
after-state, evidence references and remaining blocker. The
[evidence skill](../cohesix-evidence/SKILL.md) helps package the case. For a
camera, robot, traffic or production line, site owners must separately prove
application safety and physical fail-safe behavior; a successful Cohesix
ticket alone cannot do that.
