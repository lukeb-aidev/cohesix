// CLASSIFICATION: PRIVATE
// Filename: DYNAMIC_ORCHESTRATION_DESIGN.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-06

# Dynamic Orchestration Design

This document outlines the planned orchestration stack for Cohesix. Three new modules provide adaptive coordination across heterogeneous nodes:

- **orch_agent** – runtime agent supervising ephemeral mounts, Secure9P sessions and role transitions.
- **orch_metrics** – metrics exporter summarizing validator traces and hardware counters.
- **orch_learn** – reinforcement layer that tunes mount strategies using historical metrics.

## Ephemeral Bind/Mount Circuits

Each worker maintains short lived bind and mount circuits. When a service requests a new namespace, `orch_agent` issues a Secure9P capability grant then attempts the mount. Failed mounts trigger a retry with exponential backoff. After three failures, the node marks the target as degraded and schedules handoff.

## Multi‑Queen Negotiation

Inspired by the Spanning Tree Protocol, Queens elect a temporary root when establishing cross‑site mounts. The first Queen to advertise a capability becomes the root for that circuit. Others defer unless the root fails health checks. Negotiation messages travel over the validator trace channel and are persisted for replay.

## Integration Details

- **Secure9P** – all orchestration RPCs use the existing Secure9P transport. Capabilities are scoped per role and stored in `/srv/orch`.
- **Plan9 Namespace** – orchestration bind points live under `/n/orch`. Workers mount CUDA services from `/srv/cuda` when available and gracefully disable GPU acceleration otherwise.
- **CUDA srv + Rapier** – physics nodes expose `/srv/cuda`; Rapier state lives under `/sim/`. Metrics are exported to `orch_metrics` and included in validator traces.
- **Validator Traces** – every negotiation step emits a trace record so failures can be replayed and verified.

## Fallback Logic and Health Checks

Nodes run periodic health probes via `orch_agent check`. If a mount or node fails, the agent
tries to remount on a standby. Persistent failures result in role demotion and notification to all Queens. Health reports aggregate into `orch_metrics` and feed back into `orch_learn`.

## Implementation Prompt

```
BuildDynamicOrchestration-074
Goal: implement dynamic orchestration modules and validation.

1. Create directories:
   - src/orch_agent/
   - src/orch_metrics/
   - src/orch_learn/
2. Add rc scripts to `/rc/orch/` for mount negotiation and health probes.
3. Update build scripts to compile the new modules and install the rc helpers.
4. Validate using `cohtrace` and replay through the runtime validator.
```
