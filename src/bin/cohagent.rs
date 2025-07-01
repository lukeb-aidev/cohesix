// CLASSIFICATION: COMMUNITY
// Filename: cohagent.rs v0.3
// Date Modified: 2025-07-22
// Author: Lukas Bower

use crate::prelude::*;
use clap::Parser;
use cohesix::telemetry::trace::init_panic_hook;
use cohesix::binlib::agent_main::{Cli, run};
use cohesix::CohError;

fn main() -> Result<(), CohError> {
    init_panic_hook();
    let cli = Cli::parse();
    run(cli)
}
