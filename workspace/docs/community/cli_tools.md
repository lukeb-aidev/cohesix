// CLASSIFICATION: COMMUNITY
// Filename: cli_tools.md v1.1
// Author: Lukas Bower
// Date Modified: 2027-02-01

# CLI Tools Reference

This document provides a concise reference for all Cohesix command line tools.
Each tool runs in the Plan9 userland unless noted otherwise.  Examples assume
execution from a shell environment with trace logging enabled.

## Index
- [cohcli](#cohcli)
- [cohrun](#cohrun)
- [cohtrace](#cohtrace)
- [cohcc](#cohcc)
- [cohcap](#cohcap)
- [coh-9p-helper](#coh-9p-helper-go-helper)
- [gui-orchestrator](#gui-orchestrator-go-helper)

All tools record invocations to `/log/trace/` for validator replay.  Detailed
manual pages live under [docs/man/](man/MANIFEST.md).

## cohcli
Main interface for node status and agent management.
```bash
cohcli status --verbose
cohcli agent migrate <id> --to worker02
```

## cohrun
Scenario launcher and orchestrator helper.
```bash
cohrun kiosk_start
cohrun physics_demo
```

## cohtrace
Trace inspection and federation utilities.
```bash
cohtrace list
cohtrace push_trace worker01 examples/trace.json
```

## cohcc
Compiler front end for Cohesix IR.
```bash
cohcc --input demo.ir --output demo.c --target aarch64
```

## cohcap
Capability manager for demonstrations.
```bash
cohcap list
cohcap grant camera --to worker03
```

## coh-9p-helper (Go helper)
A TCP-to-Unix-socket proxy for 9P testing. Runs on Linux hosts during build or
simulation.
```bash
coh-9p-helper --listen :5640 --socket /tmp/coh9p.sock
```

## gui-orchestrator (Go helper)
Web dashboard for live cluster status. Executed on a Linux development host.
```bash
go run ./go/cmd/gui-orchestrator --port 8888 --bind 0.0.0.0
```

For agent schemas and Codex interaction see
[AGENTS_AND_CLI.md](guides/AGENTS_AND_CLI.md).

## srvctl (Plan9 service helper)
Registers a service in the Plan9 */srv/services* directory.
```rc
srvctl announce -name demo -version 0.1 /mnt/test
```

## indexserver (Plan9 helper)
Creates an index of file paths and exposes query/result files under */srv/index*.
```rc
echo "gpu" > /srv/index/query
cat /srv/index/results
```

## devwatcher (Plan9 helper)
Watches files and logs events under */dev/watch*.
```rc
echo /tmp/foo.txt > /dev/watch/ctl
cat /dev/watch/events
```
