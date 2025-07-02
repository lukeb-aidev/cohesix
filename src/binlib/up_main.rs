// CLASSIFICATION: COMMUNITY
// Filename: up_main.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-22

use crate::prelude::*;
use clap::{Parser, Subcommand};
use crate::CohError;
use crate::cli::federation;

/// CLI wrapper for `cohup`.
#[derive(Parser)]
#[command(name = "cohup", about = "Federation CLI", version = "1.0")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Federation commands.
#[derive(Subcommand)]
pub enum Commands {
    Join { #[arg(long)] peer: String },
    ListPeers,
}

/// Execute the command logic.
pub fn run(cli: Cli) -> Result<(), CohError> {
    match cli.command {
        Commands::Join { peer } => {
            let app = federation::build();
            let matches = app.get_matches_from(vec!["join", "--peer", &peer]);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_join() {
        let cli = Cli::parse_from(["cohup", "join", "--peer", "x"]);
        match cli.command {
            Commands::Join { peer } => assert_eq!(peer, "x"),
            _ => panic!("bad parse"),
        }
    }
}
