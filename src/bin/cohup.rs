// CLASSIFICATION: COMMUNITY
// Filename: cohup.rs v1.2
// Author: Codex
// Date Modified: 2025-07-22

use clap::Parser;
use cohesix::telemetry::trace::init_panic_hook;
use cohesix::binlib::up_main::{Cli, run};

fn main() -> anyhow::Result<()> {
    init_panic_hook();
    let cli = Cli::parse();
    run(cli)
}
