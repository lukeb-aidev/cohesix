// CLASSIFICATION: COMMUNITY
// Filename: cli.md v0.2
// Author: Lukas Bower
// Date Modified: 2025-06-07

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

## cohup join
Join a queen federation.
```
cohup join --peer queenB
```

## cohup list-peers
Display known queen peers.
```
cohup list-peers
```
