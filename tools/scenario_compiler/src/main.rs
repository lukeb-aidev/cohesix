// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-21

use scenario_compiler::compiler::ScenarioCompiler;
use clap::Parser;
use std::path::PathBuf;
use cohesix::telemetry::trace::init_panic_hook;

#[derive(Parser)]
struct Args {
    #[clap(long)]
    input: PathBuf,
    #[clap(long)]
    output: PathBuf,
}

fn main() -> anyhow::Result<()> {
    init_panic_hook();
    let args = Args::parse();
    ScenarioCompiler::compile(&args.input, &args.output)?;
    Ok(())
}

