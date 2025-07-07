# CLASSIFICATION: COMMUNITY
# Filename: go_inventory_report.md v0.1
# Author: Codex Bot
# Date Modified: 2027-08-30

# Go Tooling Inventory

This report catalogs all Go packages and main binaries under `go/`.

## Summary

| Tool/Package | Directory | Brief Purpose |
|--------------|-----------|---------------|
| `coh-9p-helper` | `go/cmd/coh-9p-helper` | TCP to Unix socket proxy for 9P services |
| `devwatcher` | `go/cmd/devwatcher` | Watches paths listed in `/dev/watch/ctl` and logs events |
| `gui-orchestrator` | `go/cmd/gui-orchestrator` | Launches the HTTP orchestrator GUI server |
| `indexserver` | `go/cmd/indexserver` | Builds a filename index and serves query results via `/srv/index` |
| `physics-server` | `go/cmd/physics-server` | Processes physics job JSON files and writes simulation results |
| `srvctl` | `go/cmd/srvctl` | Announces services under `/srv/services` |
| `orchestrator` | `go/orchestrator` | Library wrapping the HTTP server for orchestration |
| `agent_sdk` | `go/agent_sdk` | Agent context helper for reading world and role metadata |
| `9p` | `go/9p` | Simple in-memory 9P multiplexer used in tests |
| `internal/tooling` | `go/internal/tooling` | Minimal Cobra CLI scaffold |

Detailed descriptions follow.

## coh-9p-helper
- **Directory:** `go/cmd/coh-9p-helper`
- **Purpose:** Minimal proxy forwarding 9P traffic from a TCP listener to a Unix domain socket. Intended as a stub for integration tests.
- **CLI Flags:** `--listen` (default `:5640`), `--socket` (path to Unix socket). If not specified, uses the `COH9P_SOCKET` environment variable or temp path.
- **Dependencies:** Standard library packages (`flag`, `net`, `io`, `log`, `os`).
- **Config Files:** None.
- **Output:** Logs connection information to stdout.

## devwatcher
- **Directory:** `go/cmd/devwatcher`
- **Purpose:** Monitors file system changes for any paths listed in `/dev/watch/ctl`. Writes event lines to `/dev/watch/events`.
- **Dependencies:** `github.com/fsnotify/fsnotify` for watch events.
- **Config Files:** Reads `/dev/watch/ctl` for a list of directories/files to watch.
- **Output:** Writes notifications to `/dev/watch/events`.

## gui-orchestrator
- **Directory:** `go/cmd/gui-orchestrator`
- **Purpose:** Starts the GUI orchestrator HTTP server which exposes status and control APIs and serves static files.
- **CLI Flags:** `--bind`, `--port`, `--static-dir`, `--log-file`, `--dev`.
- **Dependencies:** Uses the internal `orchestrator` package which relies on `github.com/go-chi/chi/v5`, `github.com/fsnotify/fsnotify`, and `golang.org/x/time/rate`.
- **Config Files:** When not running in dev mode, reads credentials from `/srv/orch_user.json`.
- **Output:** Serves HTTP endpoints and optionally writes an access log (default `/log/gui_access.log`).

### Key Endpoints (via `orchestrator/http`)
- `GET /api/status` – returns uptime and role status
- `POST /api/control` – accepts orchestration commands
- `GET /api/metrics` – exposes basic metrics
- `GET /static/*` – serves files from the configured static directory

## indexserver
- **Directory:** `go/cmd/indexserver`
- **Purpose:** Walks the filesystem to build a map of filenames to paths. Query strings read from `/srv/index/query` return newline-delimited results in `/srv/index/results`.
- **Dependencies:** Standard library only.
- **Config Files:** `/srv/index/query` used for lookup requests.
- **Output:** `/srv/index/results` with matching paths.

## physics-server
- **Directory:** `go/cmd/physics-server`
- **Purpose:** Reads JSON job files from `/mnt/physics_jobs/`, performs a simple position update simulation, writes state to `/sim/world.json` and `/sim/result.json`, and logs progress.
- **Dependencies:** Standard library packages (`encoding/json`, `log`, `time`).
- **Config Files:** `/mnt/physics_jobs/physics_job_*.json` input files.
- **Output:** `/sim/world.json`, `/sim/result.json`, `/srv/physics/status`, and log entries in `/srv/trace/sim.log`.

## srvctl
- **Directory:** `go/cmd/srvctl`
- **Purpose:** Basic service management tool. The `announce` subcommand records service metadata under `/srv/services/<name>`.
- **CLI Flags:** For `announce` – `-name`, `-version` followed by a service path.
- **Dependencies:** Standard library only.
- **Config Files:** None.
- **Output:** Creates `/srv/services/<name>/info` and `/srv/services/<name>/ctl`.

## orchestrator (library)
- **Directory:** `go/orchestrator`
- **Purpose:** Wraps the HTTP server implementation for reuse. The `New` function returns an orchestrator using the configuration defined in `http/server.go`.
- **Dependencies:** Delegates to `orchestrator/http`.

## orchestrator/http
- **Directory:** `go/orchestrator/http`
- **Purpose:** Implements the web server used by the GUI orchestrator. Provides routing, middleware, basic auth, rate limiting and optional static file watching.
- **Key Endpoints:** as listed above under gui-orchestrator.
- **Dependencies:** `github.com/go-chi/chi/v5`, `github.com/fsnotify/fsnotify`, `golang.org/x/time/rate`.
- **Config Files:** Optional static directory contents when `Dev` mode is enabled; log file path; credentials provided via the parent package.
- **Output:** Access log file if configured.

## orchestrator/api
- **Directory:** `go/orchestrator/api`
- **Purpose:** Defines HTTP handlers for `/api/control` and `/api/status` along with request/response structures.
- **Dependencies:** Standard library only.

## orchestrator/static
- **Directory:** `go/orchestrator/static`
- **Purpose:** Serves static files under `/static/` via a simple `http.FileServer` wrapper.
- **Dependencies:** Standard library only.

## agent_sdk
- **Directory:** `go/agent_sdk`
- **Purpose:** Provides `AgentContext` which loads role metadata from `/srv/agent_meta` and world state from `/srv/world_state`. Supplies tracing and lifecycle helpers for agents.
- **Dependencies:** Standard library (`context`, `encoding/json`, `log`, `os`, etc.).
- **Config Files:** `/srv/agent_meta/*.txt`, `/srv/agent_meta/last_goal.json`, `/srv/world_state/world.json`.
- **Output:** Logs agent lifecycle events via `log.Print`.

## 9p
- **Directory:** `go/9p`
- **Purpose:** Simple in-memory multiplexer used for 9P service testing.
- **Dependencies:** Standard library (`context`, `sync`).
- **Output:** No direct files; returns byte slices to callers.

## internal/tooling
- **Directory:** `go/internal/tooling`
- **Purpose:** Lightweight CLI base using `github.com/spf13/cobra`. Exposes a root command and a `version` subcommand.
- **Dependencies:** `github.com/spf13/cobra`.
- **Config Files:** None.
- **Output:** CLI text on stdout/stderr.


