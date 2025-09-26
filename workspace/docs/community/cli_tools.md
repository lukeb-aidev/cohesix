// CLASSIFICATION: COMMUNITY
// Filename: cli_tools.md v1.2
// Author: Lukas Bower
// Date Modified: 2029-09-26

# CLI Tools Reference

This document provides a concise reference for all Cohesix command line tools.
Each tool runs in the Plan9 userland unless noted otherwise.  Examples assume
execution from a shell environment with trace logging enabled. For a high level list see [go_tools_overview.md](go_tools_overview.md).

## Index
- [cohcli](#cohcli)
- [cohrun](#cohrun)
- [cohtrace](#cohtrace)
- [cohcc](#cohcc)
- [secure9p-sign](#secure9p-sign)
- [secure9p-onboard](#secure9p-onboard)
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
cohtrace diff
cohtrace cloud
cohtrace push_trace worker01 examples/trace.json
```

The diff command enforces the path rules defined in `/etc/cohtrace_rules.json`
(or the path provided via `COHTRACE_RULES_PATH`). The rules file must specify an
`allowed_roots` array and optional prefix rewrites; the schema is validated on
load. Exit taxonomy for `cohtrace diff`:

* `0` – snapshots match (no drift)
* `30` – drift detected between the two most recent snapshots
* `31` – insufficient snapshots available to compute a diff
* `32` – rule violation or unreadable rule set

Any schema or permission failure within the rule set halts the diff operation
and surfaces exit code `32` for audit automation.

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
coh-9p-helper --listen :5640 --socket /srv/coh9p.sock
```

## gui-orchestrator (Go helper)
Web dashboard for live cluster status. Executed on a Linux development host.
```bash
go run ./go/cmd/gui-orchestrator --port 8888 --bind 0.0.0.0
```

For agent schemas and Codex interaction see
[AGENTS_AND_CLI.md](guides/AGENTS_AND_CLI.md).

## srvctl (Plan9 service helper in Rust)
Registers a service in the Plan9 */srv/services* directory.
```rc
srvctl announce -name demo -version 0.1 /mnt/test
```

## indexserver (Plan9 helper in Rust)
Creates an index of file paths and exposes query/result files under */srv/index*.
```rc
echo "gpu" > /srv/index/query
cat /srv/index/results
```

## devwatcher (Plan9 helper in Rust)
Watches files and logs events under */dev/watch*.
```rc
echo /tmp/foo.txt > /dev/watch/ctl
cat /dev/watch/events
```
## secure9p-sign
Utility to generate a SHA-512 signature for `secure9p.toml` manifests. The
output defaults to `secure9p.sha512` alongside the manifest and includes
metadata headers for audit traceability.

```bash
secure9p-sign --manifest config/secure9p.toml
secure9p-sign --manifest policy/queen.toml --output policy/queen.sha512 --no-header
```

Each invocation records a `secure9p_sign` trace event so validator pipelines can
verify the manifest digest recorded in `/log/trace/net_secure9p.log`.

## secure9p-onboard
Automates SPIFFE-aligned mTLS onboarding for Secure9P clients. Provide the CA
certificate/key pair and desired SPIFFE ID; the command emits a signed client
certificate and private key along with trace entries confirming issuance.

```bash
secure9p-onboard --ca-cert ca.pem --ca-key ca.key \
  --spiffe-id spiffe://cohesix/worker/DroneWorker/worker-01 \
  --out-cert workers/worker-01.cert --out-key workers/worker-01.key
```

Certificates default to a 365-day validity window and use client-auth key usage
constraints. The CLI enforces SPIFFE URI format and logs a
`secure9p_onboard` trace entry for validator replay.

