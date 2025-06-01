// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.2
// Date Modified: 2025-06-01
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

/// Rich CLI based on `clap::Parser`.
pub mod cli;