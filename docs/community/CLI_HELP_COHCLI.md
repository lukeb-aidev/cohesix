// CLASSIFICATION: COMMUNITY
// Filename: CLI_HELP_COHCLI.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-12

# cohcli

Command-line interface for interacting with Cohesix services and workers.

```bash
Usage: cohcli <command> [options]
```

## Commands
- `status [--verbose]` – show local node status
- `boot <role>` – bootstrap a node with the specified role
- `trace [--filter X]` – display recent trace entries
- `trace-violations` – print runtime violation log
- `replay-trace <path>` – replay a trace file
- `dispatch-slm --target <worker> --model <slm>` – deploy an SLM
- `agent start <id> --role <role>` – start an agent
- `agent pause <id>` – pause an agent
- `agent migrate <id> --to <node>` – migrate agent to node
- `sim run <scenario>` – execute a simulation
- `federation connect <peer>` – connect to another queen
- `federation list` – list federation peers

## Examples
```bash
# Deploy a kiosk model to worker04
cohcli dispatch-slm --target worker04 --model kiosk_v1

# Run BalanceBot demo simulation
cohcli sim run BalanceBot
```
