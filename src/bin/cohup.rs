// CLASSIFICATION: COMMUNITY
// Filename: cohup.rs v1.1
// Author: Codex
// Date Modified: 2025-07-21

use clap::{Parser, Subcommand};
use cohesix::cli::federation;
use cohesix::telemetry::trace::init_panic_hook;

#[derive(Parser)]
#[command(name = "cohup", about = "Federation CLI", version = "1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Join { #[arg(long)] peer: String },
    ListPeers,
}

fn main() -> anyhow::Result<()> {
    init_panic_hook();
    let cli = Cli::parse();
    match cli.command {
        Commands::Join { peer } => {
            let app = federation::build();
            let matches = app
                .get_matches_from(vec!["join", "--peer", &peer]);
            federation::exec(&matches)?;
        }
        Commands::ListPeers => {
            let app = federation::build();
            let matches = app.get_matches_from(vec!["list-peers"]);
            federation::exec(&matches)?;
        }
    }
    Ok(())
}
