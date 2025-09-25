// CLASSIFICATION: COMMUNITY
// Filename: GUI_ORCHESTRATOR.md v1.3
// Author: Lukas Bower
// Date Modified: 2029-02-15

# Web GUI Orchestrator

This document outlines the architecture for a community-facing dashboard that
exposes live cluster state over a browser connection. The GUI is now provided
by a Go service using the chi router with JSON APIs.

## Overview

The orchestrator now queries the gRPC control plane
(`cohesix.orchestrator.OrchestratorService`) via the `GetClusterState`
RPC to display agent status, role assignments, federation peers, and
boot logs. Legacy filesystem mirrors such as `/srv/agents/active.json`
are read only when the gRPC endpoint is unreachable. The GUI is secured
by default and exposes only authenticated and rate-limited endpoints
unless explicitly run in developer mode. Static content under `gui/` or
`static/` is served directly over HTTP. WebSocket support remains
available for live updates.

## Features

- Live agent table with migration controls
- Federation status showing connected Queens
- Boot attestation logs with TPM results
- Role manifest viewer and editing helpers
- REST-style endpoints `/api/status` and `/api/control`
- `/api/metrics` exposes Prometheus counters
- Developer mode via `--dev` with live reload and verbose logs
- Access logging via `--log-file`
- CLI helpers through `cohrun orchestrator`

Run the service with `go run ./go/cmd/gui-orchestrator --port 8888 --bind
127.0.0.1`. The frontend communicates using JSON over WebSocket or plain HTTP,
bridging to 9P filesystem calls. Static assets reside under `gui/` and can be
served directly by Plan 9's webfs or the embedded server.

---

### HTTP Routes

| Method | Path | Request | Response |
|-------|------|---------|----------|
| GET | `/api/status` | none | `{"uptime":string,"status":string,"role":string,"queen_id":string,"workers":int,"generated_at":int,"timeout_seconds":int,"worker_statuses":WorkerStatus[]}` |
| POST | `/api/control` | `{ "command": string, "worker_id"?: string, "role"?: string, "trust_level"?: string, "agent_id"?: string, "require_gpu"?: bool }` | `{ "status": "ack" }` |

`WorkerStatus` mirrors the gRPC shape:

```json
{
  "worker_id": "worker-a",
  "role": "DroneWorker",
  "status": "ready",
  "ip": "10.0.0.10",
  "trust": "green",
  "boot_ts": 1700000000,
  "last_seen": 1700000300,
  "capabilities": ["cuda"],
  "gpu": {
    "perf_watt": 12.5,
    "mem_total": 1024,
    "mem_free": 512,
    "last_temp": 50,
    "gpu_capacity": 100,
    "current_load": 80,
    "latency_score": 3
  }
}
```

Supported `command` values:

- `assign-role` — requires `worker_id` and `role`; forwards to `AssignRole`.
- `update-trust` — requires `worker_id` and `trust_level`; forwards to `UpdateTrust`.
- `schedule` — requires `agent_id` and optional `require_gpu`; forwards to `RequestSchedule`.

Other commands yield HTTP `502` with an explanatory error.
| GET | `/static/*` | none | static file contents |

### CLI Options

| Flag | Description | Default |
|------|-------------|---------|
| `--port` | Listen port | `8888` |
| `--bind` | Bind address | `127.0.0.1` |
| `--static-dir` | Directory for static files | `static` |
| `--dev` | Development mode (disables auth and enables verbose reloads) | `false` |
| `--log-file` | Access log path | `/log/gui_access.log` |

Run with example:

```bash
go run ./go/cmd/gui-orchestrator --port 8080 --bind 0.0.0.0 --static-dir gui/static --dev
```

### Authentication and Rate Limiting

Basic HTTP auth is enforced whenever the GUI runs outside of developer mode. Credentials are loaded from `/srv/orch_user.json`, which now accepts the following shape:

```json
{
  "user": "admin",
  "pass": "secret",
  "roles": ["QueenPrimary", "RegionalQueen"],
  "tls_cert": "/srv/certs/gui.pem",
  "tls_key": "/srv/certs/gui.key",
  "client_ca": "/srv/certs/orchestrator-ca.pem"
}
```

Only roles listed in the `roles` array may be targeted by the `assign-role` command. Requests that attempt to assign an unauthorized role are rejected with HTTP `403 Forbidden` before the underlying gRPC call executes. When `tls_cert`/`tls_key` are provided, the HTTP server terminates TLS with a minimum of TLS 1.3. Supplying `client_ca` enables optional mTLS, requiring dashboard callers to present a certificate signed by the orchestrator CA.

Each client is limited to 60 control requests per minute by default using an in-memory token bucket. The `/api/metrics` endpoint exposes `control_limit_per_minute`, the active burst size, available tokens, and total allowed/denied counts so dashboards and observability tooling can track throttling behaviour. Excess requests continue to return HTTP `429 Too Many Requests`.

Future enhancements may include JWT-based session tokens layered atop the existing mTLS and basic-auth foundations.
