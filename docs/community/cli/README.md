// CLASSIFICATION: COMMUNITY
// Filename: README.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-06-15

# Cohesix CLI Overview

This index provides quick links to CLI documentation and explains how the tools integrate with Codex agents and role definitions.

## Available CLI Help Pages

- **cohcli** – general node and agent control ([CLI_HELP_COHCLI.md](CLI_HELP_COHCLI.md))
- **cohrun** – demo orchestration and scenario runner ([CLI_HELP_COHRUN.md](CLI_HELP_COHRUN.md))
- **cohcc** – compiler front-end for Cohesix IR ([CLI_HELP_COHCC.md](CLI_HELP_COHCC.md))
- **cohcap** – capability manager for demos ([CLI_HELP_COHCAP.md](CLI_HELP_COHCAP.md))
- **cohtrace** – trace management and federation utilities ([CLI_HELP_COHTRACE.md](CLI_HELP_COHTRACE.md))

Each help page contains usage examples and command summaries.

## Agent and CLI Interaction

`AGENTS_AND_CLI.md` defines Codex agent schemas and links them to CLI commands. The high-level guide in [cli.md](cli.md) explains common patterns, such as `cohcli agent start` or `cohrun physics_demo`.


## Roles and CLI Tools

Role assignments described in [../governance/ROLE_POLICY.md](../governance/ROLE_POLICY.md) determine which CLI actions are permitted. Queens enforce these policies when commands are issued over the network or via local agents.

## CLI Matrix by Role

| Role             | Primary CLI Tools                   |
|------------------|--------------------------------------|
| QueenPrimary     | cohcli, cohrun, cohtrace             |
| DroneWorker      | cohrun, cohtrace, cohcc              |
| KioskInteractive | cohcap, cohrun                       |
| SimulatorTest    | cohrun, cohtrace, cohcc              |

For a full list of role-bound commands, see each individual CLI_HELP_*.md file.

