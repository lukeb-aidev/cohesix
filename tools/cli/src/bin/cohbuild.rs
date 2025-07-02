// CLASSIFICATION: COMMUNITY
// Filename: cohbuild.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-21

use clap::Parser;
use cohesix::CohError;
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Compile Cohesix modules")]
struct Args {
    /// Input source file
    #[arg(long)]
    input: PathBuf,
    /// Output binary path
    #[arg(long)]
    output: PathBuf,
}

fn main() -> Result<(), CohError> {
    let args = Args::parse();
    let mode = std::env::var("COH_MODE").unwrap_or_else(|_| "prod".into());
    let src = fs::read(&args.input)?;
    if mode == "dev" {
        println!("[cohbuild/dev] reading {} bytes", src.len());
    }
    fs::write(&args.output, src)?;
    println!("Built {} -> {}", args.input.display(), args.output.display());
    Ok(())
}

