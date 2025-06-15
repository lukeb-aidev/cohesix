// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.4
// Date Modified: 2025-07-09
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Cohesix · Worker Sub‑crate (root module)
//
// Groups all logic specific to a Worker‑role node.
//
// Current sub‑modules
// -------------------
// * `args` – thin wrapper around `cli::WorkerOpts` for legacy code
// * `cli`  – Clap‑based command‑line parser (see `go/cmd` counterpart)
//
// Future work
// -----------
// * `runtime` – main event‑loop & service supervisor
// * `metrics` – Prometheus (or OpenTelemetry) exporter
// ─────────────────────────────────────────────────────────────

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Legacy argument parser wrapper. Prefer `cli::WorkerOpts`.
pub mod args;

pub mod hotplug;
pub mod world_sync;
pub mod vision_overlay;
pub mod queen_watchdog;
pub mod role_memory;

use crate::runtime::ServiceRegistry;

/// Register worker services during initialization.
pub fn register_services() {
    let _ = ServiceRegistry::register_service("cuda", "/srv/cuda");
    let _ = ServiceRegistry::register_service("shell", "/srv/shell_out");
    let _ = ServiceRegistry::register_service("diag", "/srv/diagnostics");
}
