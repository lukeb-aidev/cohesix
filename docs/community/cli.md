// CLASSIFICATION: COMMUNITY
// Filename: cli.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-05

# Cohesix CLI Guide

This guide summarises key commands for `cohcli` and `cohup`.

## cohup patch
Apply a live patch to a running node.
```
cohup patch <target> <binary>
```

## agent migrate
Move an agent to another node.
```
cohcli agent migrate <agent_id> --to <node>
```

## sim run BalanceBot
Run the BalanceBot physics simulation.
```
cohcli sim run BalanceBot
```
