// CLASSIFICATION: COMMUNITY
// Filename: cohup.rs v1.2
// Author: Codex
// Date Modified: 2025-07-22

use crate::prelude::*;
use clap::Parser;
use cohesix::binlib::up_main::{run, Cli};
use cohesix::telemetry::trace::init_panic_hook;
use crate::CohError;

fn main() -> Result<(), CohError> {
    init_panic_hook();
    let cli = Cli::parse();
    run(cli)
}
