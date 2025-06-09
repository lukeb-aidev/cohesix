// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-17

use clap::Parser;
use cohesix::coh_cc::{config::{Cli, Command, Config}, compile, logging};

/// Entry point for the cohcc binary.
pub fn main_entry() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if cli.sandbox_info {
        println!("sandbox role: {}", std::env::var("COHROLE").unwrap_or_default());
        return Ok(());
    }
    let cfg = Config::from_cli(&cli)?;
    match cli.command {
        Command::Build { source, out, flags } => {
            compile(&source, &out, &flags, &cfg)
        }
    }
}

fn main() {
    if let Err(e) = main_entry() {
        logging::log_failure(&format!("{e}"));
        eprintln!("cohcc: {e}");
        std::process::exit(1);
    }
}

