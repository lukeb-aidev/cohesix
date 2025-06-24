// CLASSIFICATION: COMMUNITY
// Filename: CLI.md v0.5
// Author: Lukas Bower
// Date Modified: 2026-07-27

# Cohesix CLI Guide

This guide summarises key commands for `cohcli` and `cohup`.

## CLI by Role Reference

| Role             | Commands Relevant To Role                  |
|------------------|--------------------------------------------|
| QueenPrimary     | cohup join, cohup list-peers, cohcli agent migrate |
| KioskInteractive | cohrun kiosk_start, cohrun kiosk_event     |
| DroneWorker      | cohcli agent migrate                       |
| SimulatorTest    | cohtrace kiosk_ping                        |

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

See also: [Detailed CLI help](CLI_HELP_COHCLI.md).

For a complete index and role-by-tool breakdown, see: cli/README.md

## coh-9p-helper
TCP-to-Unix-socket proxy for basic 9P testing.
```
coh-9p-helper --listen :5640 --socket /path/to.sock
```
If `--socket` is omitted, the proxy uses the `COH9P_SOCKET` environment variable,
falling back to `filepath.Join(os.TempDir(), "coh9p.sock")`.
