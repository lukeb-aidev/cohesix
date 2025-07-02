// CLASSIFICATION: COMMUNITY
// Filename: cohrun_cli.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-08-16
// Renamed to avoid collision with main cohrun binary.

use clap::Parser;
use cohesix::CohError;
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Run Cohesix demo scenarios")] 
struct Args {
    /// Scenario file to execute
    #[arg(long)]
    scenario: PathBuf,
}

fn main() -> Result<(), CohError> {
    let args = Args::parse();
    let mode = std::env::var("COH_MODE").unwrap_or_else(|_| "prod".into());
    let data = fs::read_to_string(&args.scenario)?;
    if mode == "dev" {
        println!(
            "[cohrun/dev] running {} ({} bytes)",
            args.scenario.display(),
            data.len()
        );
    } else {
        println!("[cohrun] running {}", args.scenario.display());
    }
    Ok(())
}

