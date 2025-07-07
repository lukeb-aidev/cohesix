// CLASSIFICATION: COMMUNITY
// Filename: go_tools_overview.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-09-01

# Go Tools Overview

This page lists all Go-based utilities in Cohesix. Each tool adheres to
Plan9 conventions and communicates via Secure9P when crossing process
boundaries.

| Tool | Purpose | Documentation |
|------|---------|---------------|
| `coh-9p-helper` | TCP to Unix-socket 9P proxy | [COH_9P_HELPER](guides/COH_9P_HELPER.md) |
| `devwatcher` | File watcher writing events to `/dev/watch` | [PLAN9_SERVICE_TOOLS](guides/PLAN9_SERVICE_TOOLS.md) |
| `gui-orchestrator` | Web dashboard for cluster management | [GUI_ORCHESTRATOR](architecture/GUI_ORCHESTRATOR.md) |
| `indexserver` | Simple file index exposing `/srv/index` | [PLAN9_SERVICE_TOOLS](guides/PLAN9_SERVICE_TOOLS.md) |
| `physics-server` | Processes physics jobs under `/mnt/physics_jobs` | [PLAN9_PHYSICS_SERVER](guides/PLAN9_PHYSICS_SERVER.md) |
| `srvctl` | Announces services under `/srv/services` | [PLAN9_SERVICE_TOOLS](guides/PLAN9_SERVICE_TOOLS.md) |

All binaries support `-h` for help and log to `/srv/trace` or `/dev`
as appropriate.
