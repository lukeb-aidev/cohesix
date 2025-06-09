// CLASSIFICATION: COMMUNITY
// Filename: gui_orchestrator.md v1.2
// Author: Lukas Bower
// Date Modified: 2025-07-20

# Web GUI Orchestrator

This document outlines the architecture for a community-facing dashboard that
exposes live cluster state over a browser connection. The GUI is now provided
by a Go service using the chi router with JSON APIs.

## Overview

The orchestrator queries the `/srv` namespace and the worker registry at
`/srv/agents/active.json` to display agent status, role assignments,
federation peers, and boot logs. Static content under `gui/` or `static/` is
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
