// CLASSIFICATION: PRIVATE
// Filename: PHYSICAL_AGENT_POLICY.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-31

# Physical Agent Policy

Internal guidelines for deploying physical agents.

## Deployment Requirements

- All physical deployments must follow platform-specific safety and initialization checklists.
- Logs from startup sequences must be stored in `/srv/physics/startup/` and accessible for audit.
- Emergency stop and override mechanisms must be tested prior to agent activation.

## Trace Integration

- Every physical actuation (e.g. motor start, camera engage, sensor init) must emit a trace to `/log/trace/physical_<ts>.log`
- Snapshots of agent posture, applied forces, and sensor state must be recorded to `/history/snapshots/`
- CI harness will validate presence and continuity of trace logs for all physical agents

## Validator Hooks

- Physical agents are subject to the same rule enforcement system as virtual agents
- Traces are inspected for compliance with movement bounds, startup sequences, and override triggers
- Policy violations are raised via `/log/validation/physics_alerts.log`
