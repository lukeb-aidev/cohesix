// CLASSIFICATION: COMMUNITY
// Filename: config.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-17

use std::path::{Path, PathBuf};
use clap::{Parser, Subcommand};
use crate::coh_cc::backend::Backend;

#[derive(Parser)]
#[command(author, version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
    #[arg(long, default_value = "tcc")]
    pub backend: String,
    #[arg(long)]
    pub trace: bool,
    #[arg(long = "sandbox-info")]
    pub sandbox_info: bool,
}

#[derive(Subcommand)]
pub enum Command {
    Build { source: String, #[arg(short = 'o')] out: String, #[arg(last = true)] flags: Vec<String> },
}

/// Resolved configuration after CLI parsing.
#[derive(Debug, Clone)]
pub struct Config {
    pub backend: Backend,
    pub trace: bool,
}

impl Config {
    pub fn from_cli(cli: &Cli) -> anyhow::Result<Self> {
        let backend = cli.backend.parse()?;
        Ok(Config { backend, trace: cli.trace })
    }

    pub fn valid_output(&self, path: &str) -> bool {
        if let Ok(canon) = Path::new(path).canonicalize() {
            let cur = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            canon.starts_with(&cur) || canon.starts_with("/mnt/data")
        } else {
            false
        }
    }
}

