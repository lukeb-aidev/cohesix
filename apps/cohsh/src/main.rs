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

#[cfg(feature = "tcp")]
use cohsh::TcpTransport;
use cohsh::{NineDoorTransport, QemuTransport, RoleArg, Shell, Transport};

#[derive(Copy, Clone, Debug, ValueEnum)]
enum TransportKind {
    Mock,
    Qemu,
    #[cfg(feature = "tcp")]
    Tcp,
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
    #[cfg_attr(feature = "tcp", arg(long, value_enum, default_value_t = TransportKind::Tcp))]
    #[cfg_attr(
        not(feature = "tcp"),
        arg(long, value_enum, default_value_t = TransportKind::Mock)
    )]
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

    /// Hostname or IP address for the TCP transport.
    #[cfg(feature = "tcp")]
    #[arg(long, default_value = "127.0.0.1")]
    tcp_host: String,

    /// TCP port for the remote console listener.
    #[cfg(feature = "tcp")]
    #[arg(long, default_value_t = 31337)]
    tcp_port: u16,

    /// Authentication token required by the TCP console listener.
    #[cfg(feature = "tcp")]
    #[arg(long, default_value = "changeme")]
    auth_token: String,
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
    #[cfg(feature = "tcp")]
    let (tcp_host, tcp_port, auth_token) = {
        let mut host = cli.tcp_host.clone();
        if let Ok(value) = env::var("COHSH_TCP_HOST") {
            if host == "127.0.0.1" {
                host = value;
            }
        }
        let mut port = cli.tcp_port;
        if let Ok(value) = env::var("COHSH_TCP_PORT") {
            if let Ok(parsed) = value.parse::<u16>() {
                port = parsed;
            }
        }
        let mut token = cli.auth_token.clone();
        if let Ok(value) = env::var("COHSH_AUTH_TOKEN") {
            if token == "changeme" {
                token = value;
            }
        }
        (host, port, token)
    };

    let transport: Box<dyn Transport> = match cli.transport {
        TransportKind::Mock => Box::new(NineDoorTransport::new(nine_door::NineDoor::new())),
        TransportKind::Qemu => Box::new(QemuTransport::new(
            cli.qemu_bin.clone(),
            cli.qemu_out_dir.clone(),
            cli.qemu_gic_version.clone(),
            qemu_args,
        )),
        #[cfg(feature = "tcp")]
        TransportKind::Tcp => Box::new(
            TcpTransport::new(tcp_host.clone(), tcp_port)
                .with_timeout(std::time::Duration::from_secs(5))
                .with_heartbeat_interval(std::time::Duration::from_secs(15))
                .with_auth_token(auth_token.clone()),
        ),
    };
    let mut shell = Shell::new(transport, writer);
    shell.write_line("Welcome to Cohesix. Type 'help' for commands.")?;
    let mut auto_log = false;
    if let Some(role_arg) = cli.role {
        let role = Role::from(role_arg);
        match shell.attach(role, cli.ticket.as_deref()) {
            Ok(()) => {
                if cli.script.is_none() {
                    match cli.transport {
                        TransportKind::Qemu => auto_log = true,
                        #[cfg(feature = "tcp")]
                        TransportKind::Tcp => auto_log = true,
                        _ => {}
                    }
                }
            }
            Err(error) => {
                if cli.script.is_some() {
                    return Err(error);
                }
                #[cfg(feature = "tcp")]
                let transport_hint = if matches!(cli.transport, TransportKind::Tcp) {
                    "TCP attach failed"
                } else {
                    "attach failed"
                };
                #[cfg(not(feature = "tcp"))]
                let transport_hint = "attach failed";
                eprintln!("Error: {transport_hint}: {error}");
                shell.write_line("detached shell: run 'attach <role>' to connect")?;
            }
        }
    } else {
        shell.write_line("detached shell: run 'attach <role>' to connect")?;
    }
    if auto_log {
        shell.execute("log")?;
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
