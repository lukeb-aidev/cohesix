// CLASSIFICATION: COMMUNITY
// Filename: config.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-07-18

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use crate::{coh_bail, CohError};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(author, version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
    #[arg(long, default_value = "tcc")]
    pub backend: String,
    #[arg(long, default_value = "/mnt/data/toolchain")]
    pub toolchain_dir: String,
    #[arg(long)]
    pub trace: bool,
    #[arg(long = "sandbox-info")]
    pub sandbox_info: bool,
}

#[derive(Subcommand)]
pub enum Command {
    Build {
        source: String,
        #[arg(short = 'o')]
        out: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        sysroot: String,
        #[arg(last = true)]
        flags: Vec<String>,
    },
}

/// Resolved configuration after CLI parsing.
#[derive(Debug, Clone)]
pub struct Config {
    pub backend: String,
    pub trace: bool,
    pub target: String,
    pub sysroot: PathBuf,
    pub toolchain_dir: PathBuf,
}

impl Config {
    pub fn from_cli(cli: &Cli) -> Result<Self, CohError> {
        let (target, sysroot) = match &cli.command {
            Command::Build {
                target, sysroot, ..
            } => (target.clone(), PathBuf::from(sysroot)),
        };
        let sysroot_canon = sysroot.canonicalize().unwrap_or(sysroot);
        if !sysroot_canon.starts_with("/mnt/data") {
            coh_bail!("sysroot must be under /mnt/data");
        }
        let tc_canon = PathBuf::from(&cli.toolchain_dir)
            .canonicalize()
            .unwrap_or(PathBuf::from(&cli.toolchain_dir));
        if !tc_canon.starts_with("/mnt/data") {
            coh_bail!("toolchain-dir must be under /mnt/data");
        }
        if target.is_empty() {
            coh_bail!("target must not be empty");
        }
        Ok(Config {
            backend: cli.backend.clone(),
            trace: cli.trace,
            target,
            sysroot: sysroot_canon,
            toolchain_dir: tc_canon,
        })
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
