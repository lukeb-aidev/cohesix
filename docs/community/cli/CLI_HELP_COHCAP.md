// CLASSIFICATION: COMMUNITY
// Filename: CLI_HELP_COHCAP.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-15


# cohcap
*(Applies to: KioskInteractive, DroneWorker, SimulatorTest)*

Placeholder capability management CLI used for demos.

```bash
Usage: cohcap <command> [options]
```
See CLI README.md for full role-by-command mapping.

## Commands
- `list` – show available capabilities
- `grant <cap> --to <worker>` – grant a capability
- `revoke <cap> --from <worker>` – revoke a capability

## Examples
```bash
# List caps registered on worker03
cohcap list --worker worker03
```
