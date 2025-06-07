// CLASSIFICATION: COMMUNITY
// Filename: federation.rs v1.0
// Author: Codex
// Date Modified: 2025-06-07

//! Federation CLI helpers for `cohup`.

use clap::{Arg, Command};
use std::fs;

/// Build the federation CLI subcommands.
pub fn build() -> Command {
    Command::new("federation")
        .about("Federation management")
        .subcommand(
            Command::new("join")
                .about("Join a peer queen")
                .arg(Arg::new("peer").long("peer").required(true)),
        )
        .subcommand(Command::new("list-peers").about("List known peers"))
}

/// Execute the federation command.
pub fn exec(matches: &clap::ArgMatches) -> anyhow::Result<()> {
    match matches.subcommand() {
        Some(("join", sub)) => {
            if let Some(peer) = sub.get_one::<String>("peer") {
                fs::create_dir_all("/srv/federation/requests").ok();
                fs::write(format!("/srv/federation/requests/{}", peer), b"join")?;
            }
        }
        Some(("list-peers", _)) => {
            if let Ok(entries) = fs::read_dir("/srv/federation/known_hosts") {
                for e in entries.flatten() {
                    println!("{}", e.file_name().to_string_lossy());
                }
            }
        }
        _ => {}
    }
    Ok(())
}
