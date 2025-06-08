// CLASSIFICATION: COMMUNITY
// Filename: cli.md v0.3
// Author: Lukas Bower
// Date Modified: 2025-07-12

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

## cohrun kiosk_start
Deploy the latest kiosk UI bundle locally.
```
cohrun kiosk_start
```

## cohrun kiosk_event
Log a kiosk interaction event.
```
cohrun kiosk_event --event card_insert --user X123
```

## cohtrace kiosk_ping
Emit a ping event for federation testing.
```
cohtrace kiosk_ping
```
