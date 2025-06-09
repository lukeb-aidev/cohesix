// CLASSIFICATION: COMMUNITY
// Filename: cohrun.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-21

use clap::Parser;
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Run Cohesix demo scenarios")] 
struct Args {
    /// Scenario file to execute
    #[arg(long)]
    scenario: PathBuf,
}

fn main() -> anyhow::Result<()> {
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

