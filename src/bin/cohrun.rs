// CLASSIFICATION: COMMUNITY
// Filename: cohrun.rs v0.6
// Author: Lukas Bower
// Date Modified: 2025-07-22

use crate::prelude::*;
use clap::Parser;
use cohesix::telemetry::trace::init_panic_hook;
use cohesix::binlib::run_main::{Cli, run};

fn main() {
    init_panic_hook();
    let cli = Cli::parse();
    run(cli);
}
