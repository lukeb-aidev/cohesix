// CLASSIFICATION: COMMUNITY
// Filename: cli_cap.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-09-10

use clap::{Parser, Subcommand};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Capability management CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List capabilities for a worker
    List { worker: String },
    /// Grant a capability
    Grant { cap: String, to: String },
    /// Revoke a capability
    Revoke { cap: String, from: String },
}

fn current_role() -> String {
    fs::read_to_string("/srv/cohrole")
        .unwrap_or_else(|_| "Unknown".into())
        .trim()
        .to_string()
}

fn ensure_admin() {
    let role = current_role();
    if role != "QueenPrimary" && role != "SimulatorTest" {
        eprintln!("cohcap: role {role} not permitted");
        std::process::exit(1);
    }
}

fn list_caps(worker: String) -> anyhow::Result<()> {
    let base = std::env::var("CAP_BASE").unwrap_or_else(|_| "/srv/caps".into());
    let path = PathBuf::from(base).join(worker);
    if path.exists() {
        let data = fs::read_to_string(path)?;
        println!("{data}");
    } else {
        println!("no caps");
    }
    Ok(())
}

fn modify(worker: String, cap: String, add: bool) -> anyhow::Result<()> {
    ensure_admin();
    let base = std::env::var("CAP_BASE").unwrap_or_else(|_| "/srv/caps".into());
    let path = PathBuf::from(base).join(worker);
    let mut caps = if path.exists() {
        fs::read_to_string(&path)?.split(',').map(|s| s.to_string()).collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if add {
        if !caps.contains(&cap) {
            caps.push(cap);
        }
    } else {
        caps.retain(|c| c != &cap);
    }
    let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(&path)?;
    writeln!(f, "{}", caps.join(","))?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::List { worker } => list_caps(worker)?,
        Cmd::Grant { cap, to } => modify(to, cap, true)?,
        Cmd::Revoke { cap, from } => modify(from, cap, false)?,
    }
    Ok(())
}

