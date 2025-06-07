// CLASSIFICATION: COMMUNITY
// Filename: cohagent.rs v0.1
// Date Modified: 2025-07-04
// Author: Lukas Bower

use clap::{Parser, Subcommand};
use cohesix::agents::runtime::AgentRuntime;
use cohesix::agents::migration;
use cohesix::cohesix_types::Role;
use anyhow::Result;

#[derive(Parser)]
#[command(name = "cohagent")]
#[command(about = "Agent management CLI", version = "0.1")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    Start { id: String, role: String, program: String },
    Pause { id: String },
    Migrate { id: String, to: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
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
