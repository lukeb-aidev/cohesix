// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! CLI entry point for the Cohesix shell prototype.

use std::fs::File;
use std::io::{self, BufReader};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use cohesix_ticket::Role;

use cohsh::{NineDoorTransport, RoleArg, Shell};

#[derive(Copy, Clone, Debug, ValueEnum)]
enum TransportKind {
    Mock,
    Qemu,
}

/// Cohesix shell command-line arguments.
#[derive(Debug, Parser)]
#[command(author = "Lukas Bower", version, about = "Cohesix shell prototype", long_about = None)]
struct Cli {
    /// Attach immediately as the supplied role.
    #[arg(long)]
    role: Option<RoleArg>,

    /// Optional capability ticket payload.
    #[arg(long)]
    ticket: Option<String>,

    /// Execute commands from a script file instead of starting an interactive shell.
    #[arg(long)]
    script: Option<PathBuf>,

    /// Select the transport backing the shell session.
    #[arg(long, value_enum, default_value_t = TransportKind::Mock)]
    transport: TransportKind,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let stdout = io::stdout();
    let writer = stdout.lock();
    let transport = match cli.transport {
        TransportKind::Mock | TransportKind::Qemu => {
            NineDoorTransport::new(nine_door::NineDoor::new())
        }
    };
    let mut shell = Shell::new(transport, writer);
    if let Some(role_arg) = cli.role {
        shell.attach(Role::from(role_arg), cli.ticket.as_deref())?;
    } else {
        shell.write_line("detached shell: run 'attach <role>' to connect")?;
    }
    if let Some(script_path) = cli.script {
        let file = File::open(&script_path)
            .with_context(|| format!("failed to open script {script_path:?}"))?;
        shell.run_script(BufReader::new(file))?;
    } else {
        shell.repl()?;
    }
    Ok(())
}
