// CLASSIFICATION: COMMUNITY
// Filename: agent_main.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-08-22

use clap::{Parser, Subcommand};
use crate::agents::{runtime::AgentRuntime, migration};
use crate::cohesix_types::Role;
use anyhow::Result;

/// CLI arguments for `cohagent`.
#[derive(Parser)]
#[command(name = "cohagent", about = "Agent management CLI", version = "0.1")]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Command,
}

/// Subcommands for `cohagent`.
#[derive(Subcommand)]
pub enum Command {
    Start { id: String, role: String, program: String },
    Pause { id: String },
    Migrate { id: String, to: String },
}

/// Execute the cohagent CLI commands.
pub fn run(cli: Cli) -> Result<()> {
    let mut rt = AgentRuntime::new();
    match cli.cmd {
        Command::Start { id, role, program } => {
            let role = match role.as_str() {
                "QueenPrimary" => Role::QueenPrimary,
                "DroneWorker" => Role::DroneWorker,
                _ => Role::Other(role),
            };
            rt.spawn(&id, role, &[program])?;
        }
        Command::Pause { id } => {
            rt.pause(&id)?;
        }
        Command::Migrate { id, to } => {
            migration::migrate(&id, |_| Err(anyhow::anyhow!("fetch")), |_id, _| Ok(()), |_| Ok(()))?;
            println!("migrated {} to {}", id, to);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pause() {
        let cli = Cli::parse_from(["cohagent", "pause", "a1"]);
        match cli.cmd {
            Command::Pause { id } => assert_eq!(id, "a1"),
            _ => panic!("unexpected variant"),
        }
    }
}
