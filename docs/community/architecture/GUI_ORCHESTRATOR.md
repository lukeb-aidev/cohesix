// CLASSIFICATION: COMMUNITY
// Filename: GUI_ORCHESTRATOR.md v1.3
// Author: Lukas Bower
// Date Modified: 2026-02-05

# Web GUI Orchestrator

This document outlines the architecture for a community-facing dashboard that
exposes live cluster state over a browser connection. The GUI is now provided
by a Go service using the chi router with JSON APIs.

## Overview

The orchestrator queries the `/srv` namespace and the worker registry at
`/srv/agents/active.json` to display agent status, role assignments,
federation peers, and boot logs. The GUI is secured by default and exposes only authenticated and rate-limited endpoints unless explicitly run in developer mode. Static content under `gui/` or `static/` is
served directly over HTTP. WebSocket support remains available for live
updates.

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
| GET | `/api/status` | none | `{"uptime":string,"status":"ok","role":string,"workers":int}` |
| POST | `/api/control` | `{ "command": string }` | `{ "status": "ack" }` |
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

Basic HTTP auth is enabled unless `--dev` is supplied. Credentials are loaded from `/srv/orch_user.json` containing `{"user":"admin","pass":"secret"}`. Each client is limited to 60 requests per minute via an in-memory token bucket. Excess requests return HTTP `429 Too Many Requests`.

Future enhancements may include JWT-based session tokens and TLS termination behind a reverse proxy.
