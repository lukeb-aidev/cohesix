// CLASSIFICATION: COMMUNITY
// Filename: CLI_HELP_COHTRACE.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-15

# cohtrace

*(Applies to: QueenPrimary, DroneWorker, SimulatorTest)*

Trace inspection and federation utilities.

```bash
Usage: cohtrace <command> [options]
```

See CLI README.md for full role-by-command mapping and related tools.

## Role–Command Matrix

| Command             | Applies To         |
|---------------------|--------------------|
| list                | QueenPrimary       |
| push_trace          | DroneWorker        |
| kiosk_ping          | SimulatorTest      |
| trust_check         | QueenPrimary       |
| view_snapshot       | QueenPrimary       |

## Commands
- `list` – list connected workers
- `push_trace <worker_id> <path>` – send a simulation trace to the queen
- `kiosk_ping` – simulate kiosk ping event
- `trust_check` – view worker trust levels
- `view_snapshot <worker_id>` – show stored world snapshot

## Examples
```bash
# View workers known to the queen
cohtrace list

# Push a physics trace
cohtrace push_trace worker01 examples/trace_example_physics.json
```
