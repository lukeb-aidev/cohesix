// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-21

use clap::Parser;
use cohesix::CohError;
use scenario_compiler::compiler::ScenarioCompiler;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[clap(long)]
    input: PathBuf,
    #[clap(long)]
    output: PathBuf,
}

fn main() -> Result<(), CohError> {
    let args = Args::parse();
    ScenarioCompiler::compile(&args.input, &args.output)?;
    Ok(())
}
