// CLASSIFICATION: COMMUNITY
// Filename: up_main.rs v0.1
// Author: Lukas Bower
// Date Modified: 2029-10-05

use crate::cli::federation;
use crate::{new_err, CohError};
#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use clap::{Parser, Subcommand};

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
    Join {
        #[arg(long)]
        peer: String,
    },
    ListPeers,
}

/// Execute the command logic.
pub fn run(cli: Cli) -> Result<(), CohError> {
    match cli.command {
        Commands::Join { peer } => {
            let app = federation::build();
            let matches = app
                .try_get_matches_from(vec!["cohup", "connect", "--peer", &peer])
                .map_err(|err| new_err(format!("federation connect invocation invalid: {err}")))?;
            federation::exec(&matches)?;
        }
        Commands::ListPeers => {
            let app = federation::build();
            let matches = app
                .try_get_matches_from(vec!["cohup", "list-peers"])
                .map_err(|err| new_err(format!("federation list-peers invocation invalid: {err}")))?;
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
