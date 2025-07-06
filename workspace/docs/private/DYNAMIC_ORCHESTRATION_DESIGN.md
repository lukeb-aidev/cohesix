// CLASSIFICATION: PRIVATE
// Filename: DYNAMIC_ORCHESTRATION_DESIGN.md v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-06

# Dynamic Orchestration Design

This document details the next evolution of Cohesix orchestration. The architecture leverages ephemeral circuits, validator-driven adaptability, and optional web-native hooks for remote orchestrators.

## New Modules

Three core modules enable dynamic coordination:

- **orch_agent**  
  Supervises ephemeral mounts, Secure9P sessions, role transitions, and triggers fallback on failures.

- **orch_metrics**  
  Exports runtime metrics from validator traces, hardware counters (CPU, GPU, memory), and ephemeral circuit health.

- **orch_learn**  
  A lightweight reinforcement loop that adjusts orchestration policies based on historical trace outcomes and real-time metrics.

## Ephemeral Bind/Mount Circuits

Workers establish transient bind and mount circuits. When a service requires a namespace, `orch_agent` issues a Secure9P capability, attempts the mount, and validates. Failure triggers exponential backoff retries. After three failed attempts, the node marks the target degraded and escalates to standby remount.

Ephemeral circuits automatically tear down post-task, freeing namespaces and minimizing stale mounts.

## Multi-Queen Negotiation

Taking inspiration from Spanning Tree Protocol, multiple Queens negotiate to elect a temporary root for orchestration. The first Queen advertising capability becomes the root for that specific circuit. Others monitor via validator health checks, deferring unless the root fails. All negotiation uses validator-traced channels, preserving replayability for auditing and learning.

## Integration Points

- **Secure9P**  
  All orchestration RPCs and ephemeral circuit handshakes travel over existing Secure9P. Capability scopes are dynamically issued per role and stored under `/srv/orch`.

- **Plan9 Namespace**  
  Ephemeral orchestration mounts live under `/n/orch`. GPU workloads mount `/srv/cuda` opportunistically; Rapier physics states integrate from `/sim/`.

- **Validator Traces**  
  Every orchestration action emits a validator trace. This ensures all ephemeral circuits can be replayed, analyzed, and optimized by `orch_learn`.

## Fallback Logic & Health Checks

Each node continuously runs health probes via `orch_agent check`. Failed mounts or node outages trigger local remount attempts. Persistent issues demote the role and notify all Queens. Aggregated health is fed into `orch_metrics` and directly shapes `orch_learn` decisions.

## Optional Web-Native Hooks

Where deployment includes HTTP-friendly edge devices or hybrid cloud controllers, Cohesix’s web-native hooks can coordinate ephemeral orchestration via lightweight POST/GET APIs. These hooks mount under `/srv/webhooks` and mirror Secure9P flows, preserving trace-based accountability while enabling broader edge compatibility.

This feature is optional and only integrated when it enhances orchestration reach without compromising Plan9 semantics.

## Implementation Prompt

```
BuildDynamicOrchestration-074
Goal: implement dynamic orchestration modules and validation.

1. Create:
   - src/orch_agent/
   - src/orch_metrics/
   - src/orch_learn/
2. Add rc scripts under /rc/orch/ for ephemeral circuit setup, negotiation, and health probes.
3. Update scripts/cohesix_fetch_build.sh to build these modules and install rc helpers.
4. Ensure ephemeral orchestration is staged into cohesix_boot.elf.
5. Validate with cohtrace and replay through runtime validator.
```
