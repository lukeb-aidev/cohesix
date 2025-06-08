// CLASSIFICATION: COMMUNITY
// Filename: CLI_HELP_COHRUN.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-12

# cohrun

Utility for running Cohesix demo scenarios and orchestrator actions.

```bash
Usage: cohrun <command> [options]
```

## Commands
- `physics_demo` – start the Rapier physics showcase
- `test_webcam` – capture a single webcam frame
- `webcam_tilt` – run tilt control with webcam input
- `kiosk_start` – deploy and start kiosk UI bundle
- `kiosk_event --event <evt> [--user <id>]` – log kiosk event
- `orchestrator status` – show registered agents
- `orchestrator assign <role> <worker_id>` – assign role
- `gpu_status` – list GPU-equipped workers
- `gpu_dispatch <task>` – schedule a GPU job
- `goal add <json>` – add a goal definition
- `goal list` – list active goals
- `goal assign <goal_id> <worker_id>` – assign goal
- `trace_replay [--context failover] [--limit N]` – replay traces
- `inject_rule --from <file>` – load validator rule

## Examples
```bash
# Run kiosk demo locally
cohrun kiosk_start

# Assign DroneWorker role to worker02
cohrun orchestrator assign DroneWorker worker02
```
