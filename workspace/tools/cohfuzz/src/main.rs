// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-21

use clap::Parser;
use cohesix::CohError;
use cohfuzz::fuzzer::TraceFuzzer;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[clap(long)]
    input: PathBuf,
    #[clap(long)]
    role: String,
    #[clap(long)]
    iterations: usize,
}

fn main() -> Result<(), CohError> {
    let args = Args::parse();
    let fuzzer = TraceFuzzer::new(args.role);
    fuzzer.run(&args.input, args.iterations)?;
    Ok(())
}
