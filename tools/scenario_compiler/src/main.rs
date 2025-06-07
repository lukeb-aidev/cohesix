// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-25

use scenario_compiler::compiler::ScenarioCompiler;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[clap(long)]
    input: PathBuf,
    #[clap(long)]
    output: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    ScenarioCompiler::compile(&args.input, &args.output)?;
    Ok(())
}

