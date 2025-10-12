// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! CLI entry point for the Cohesix shell prototype.

use std::env;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use cohesix_ticket::Role;

use cohsh::{NineDoorTransport, QemuTransport, RoleArg, Shell, Transport};

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

    /// Optional capability ticket payload or worker identity string.
    #[arg(long)]
    ticket: Option<String>,

    /// Execute commands from a script file instead of starting an interactive shell.
    #[arg(long)]
    script: Option<PathBuf>,

    /// Select the transport backing the shell session.
    #[arg(long, value_enum, default_value_t = TransportKind::Mock)]
    transport: TransportKind,

    /// Path to the QEMU binary when using the qemu transport.
    #[arg(long, default_value = "qemu-system-aarch64")]
    qemu_bin: String,

    /// Directory containing staged Cohesix artefacts for QEMU boots.
    #[arg(long, default_value = "out/cohesix")]
    qemu_out_dir: PathBuf,

    /// Optional override for the GIC version passed to QEMU.
    #[arg(long, default_value = "2")]
    qemu_gic_version: String,

    /// Extra arguments forwarded to QEMU when using the qemu transport.
    #[arg(long = "qemu-arg", value_name = "ARG")]
    qemu_args: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let stdout = io::stdout();
    let writer = stdout.lock();
    let mut qemu_args = cli.qemu_args.clone();
    if let Ok(extra) = env::var("COHSH_QEMU_ARGS") {
        if !extra.trim().is_empty() {
            qemu_args.extend(extra.split_whitespace().map(|arg| arg.to_owned()));
        }
    }
    let transport: Box<dyn Transport> = match cli.transport {
        TransportKind::Mock => Box::new(NineDoorTransport::new(nine_door::NineDoor::new())),
        TransportKind::Qemu => Box::new(QemuTransport::new(
            cli.qemu_bin.clone(),
            cli.qemu_out_dir.clone(),
            cli.qemu_gic_version.clone(),
            qemu_args,
        )),
    };
    let mut shell = Shell::new(transport, writer);
    shell.write_line("Welcome to Cohesix. Type 'help' for commands.")?;
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
