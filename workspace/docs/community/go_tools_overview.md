// CLASSIFICATION: COMMUNITY
// Filename: go_tools_overview.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-09-01

# Go and Rust Tools Overview

This page lists all utility binaries in Cohesix. Linux helpers remain
implemented in Go while Plan9 services have been ported to Rust for
better integration with the rest of the platform. Tools communicate via
Secure9P when crossing process boundaries.

| Tool | Purpose | Documentation |
|------|---------|---------------|
| `coh-9p-helper` | TCP to Unix-socket 9P proxy | [COH_9P_HELPER](guides/COH_9P_HELPER.md) |
| `devwatcher` | File watcher writing events to `/dev/watch` *(Rust)* | [PLAN9_SERVICE_TOOLS](guides/PLAN9_SERVICE_TOOLS.md) |
| `gui-orchestrator` | Web dashboard for cluster management *(Go helper)* | [GUI_ORCHESTRATOR](architecture/GUI_ORCHESTRATOR.md) |
| `indexserver` | Simple file index exposing `/srv/index` *(Rust)* | [PLAN9_SERVICE_TOOLS](guides/PLAN9_SERVICE_TOOLS.md) |
| `physics-server` | Processes physics jobs under `/mnt/physics_jobs` *(Rust)* | [PLAN9_PHYSICS_SERVER](guides/PLAN9_PHYSICS_SERVER.md) |
| `srvctl` | Announces services under `/srv/services` *(Rust)* | [PLAN9_SERVICE_TOOLS](guides/PLAN9_SERVICE_TOOLS.md) |

All binaries support `-h` for help and log to `/srv/trace` or `/dev`
as appropriate.
