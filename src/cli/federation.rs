// CLASSIFICATION: COMMUNITY
// Filename: federation.rs v1.1
// Author: Codex
// Date Modified: 2025-07-07

//! Federation CLI helpers for `cohup`.

use clap::{Arg, Command};
use std::fs;

/// Build the federation CLI subcommands.
pub fn build() -> Command {
    Command::new("federation")
        .about("Federation management")
        .subcommand(
            Command::new("connect")
                .about("Connect to a peer queen")
                .arg(Arg::new("peer").long("peer").required(true)),
        )
        .subcommand(
            Command::new("disconnect")
                .about("Disconnect from a peer")
                .arg(Arg::new("peer").long("peer").required(true)),
        )
        .subcommand(Command::new("list-peers").about("List known peers"))
        .subcommand(Command::new("monitor").about("Tail federation events"))
}

/// Execute the federation command.
pub fn exec(matches: &clap::ArgMatches) -> anyhow::Result<()> {
    match matches.subcommand() {
        Some(("connect", sub)) => {
            if let Some(peer) = sub.get_one::<String>("peer") {
                fs::create_dir_all("/srv/federation/requests").ok();
                fs::write(format!("/srv/federation/requests/{peer}.connect"), b"1")?;
            }
        }
        Some(("disconnect", sub)) => {
            if let Some(peer) = sub.get_one::<String>("peer") {
                fs::create_dir_all("/srv/federation/requests").ok();
                fs::write(format!("/srv/federation/requests/{peer}.disconnect"), b"1")?;
            }
        }
        Some(("list-peers", _)) => {
            if let Ok(entries) = fs::read_dir("/srv/federation/known_hosts") {
                for e in entries.flatten() {
                    println!("{}", e.file_name().to_string_lossy());
                }
            }
        }
        Some(("monitor", _)) => {
            if let Ok(data) = fs::read_to_string("/srv/federation/events.log") {
                println!("{}", data);
            }
        }
        _ => {}
    }
    Ok(())
}
