// CLASSIFICATION: COMMUNITY
// Filename: cohrun.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

use clap::{Parser, Subcommand};
use cohesix::sim::physics_demo;

#[derive(Parser)]
#[command(name = "cohrun", about = "Run Cohesix demo scenarios", version = "0.1")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    PhysicsDemo,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::PhysicsDemo => physics_demo::run_demo(),
    }
}
