// CLASSIFICATION: COMMUNITY
// Filename: args.rs v0.2
// Date Modified: 2025-06-01
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Cohesix · Worker Argument Parser
//
// Thin compatibility wrapper that mirrors the richer CLI located
// in `worker::cli::WorkerOpts`.  Existing code can keep calling:
//
//   let args = cohesix::worker::args::parse();
//
// while new code should depend on `WorkerOpts` directly.
// ─────────────────────────────────────────────────────────────

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
#[forbid(unsafe_code)]
#[warn(missing_docs)]
use clap::Parser;

/// Command-line flags recognised by a Cohesix Worker process.
///
/// Prefer `worker::cli::WorkerOpts` for new code; this struct
/// remains only for legacy compatibility.
#[derive(Debug, Parser, Clone)]
pub struct WorkerArgs {
    /// Address of the Queen controller (e.g. cohesix://queen)
    #[arg(long, default_value = "cohesix://queen")]
    pub queen_uri: String,

    /// Verbosity level (`error`, `warn`, `info`, `debug`, `trace`)
    #[arg(long, default_value = "info")]
    pub log_level: String,

    /// Incremental verbosity (`-v`, `-vv`, etc.)
    #[arg(short, action = clap::ArgAction::Count)]
    pub verbose: u8,
}

/// Parse `std::env::args()` into a [`WorkerArgs`] struct.
pub fn parse() -> WorkerArgs {
    WorkerArgs::parse()
}
