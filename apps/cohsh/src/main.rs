// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: CLI entry point for the Cohesix shell prototype.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! CLI entry point for the Cohesix shell prototype.

use std::env;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(feature = "tcp")]
use std::sync::atomic::AtomicU64;
#[cfg(feature = "tcp")]
use std::sync::Mutex;
#[cfg(feature = "tcp")]
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, ValueEnum};
use cohesix_ticket::Role;
use env_logger::Env;
use log::LevelFilter;

#[cfg(feature = "tcp")]
use cohsh::{
    default_policy_path, load_policy, tcp_debug_enabled, validate_script, AutoAttach,
    NineDoorTransport, PolicyOverrides, QemuTransport, RoleArg, SessionPool, Shell,
    Transport, TransportFactory,
};
#[cfg(not(feature = "tcp"))]
use cohsh::{
    default_policy_path, load_policy, validate_script, AutoAttach, NineDoorTransport,
    PolicyOverrides, QemuTransport, RoleArg, SessionPool, Shell, Transport, TransportFactory,
};
#[cfg(feature = "tcp")]
use cohsh::{PooledTcpTransport, SharedTcpTransport, TcpTransport, COHSH_TCP_PORT};

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

    /// Validate a script file without executing it.
    #[arg(long, value_name = "FILE", conflicts_with = "script")]
    check: Option<PathBuf>,

    /// Path to the manifest-derived cohsh policy TOML.
    #[arg(long, value_name = "FILE")]
    policy: Option<PathBuf>,

    /// Override cohsh pool control session capacity.
    #[arg(long)]
    pool_control_sessions: Option<u16>,

    /// Override cohsh pool telemetry session capacity.
    #[arg(long)]
    pool_telemetry_sessions: Option<u16>,

    /// Override retry max attempts.
    #[arg(long)]
    retry_max_attempts: Option<u8>,

    /// Override retry backoff in milliseconds.
    #[arg(long)]
    retry_backoff_ms: Option<u64>,

    /// Override retry ceiling in milliseconds.
    #[arg(long)]
    retry_ceiling_ms: Option<u64>,

    /// Override retry timeout in milliseconds.
    #[arg(long)]
    retry_timeout_ms: Option<u64>,

    /// Override heartbeat interval in milliseconds.
    #[arg(long)]
    heartbeat_interval_ms: Option<u64>,

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

    /// Enable verbose TCP handshake logging.
    #[arg(short = 'v', long, default_value_t = false)]
    verbose: bool,

    /// Hostname or IP address for the TCP transport.
    #[cfg(feature = "tcp")]
    #[arg(long, default_value = "127.0.0.1")]
    tcp_host: String,

    /// TCP port for the remote console listener.
    #[cfg(feature = "tcp")]
    #[arg(long, default_value_t = COHSH_TCP_PORT)]
    tcp_port: u16,

    /// Authentication token required by the TCP console listener.
    #[cfg(feature = "tcp")]
    #[arg(long, default_value = "changeme")]
    auth_token: String,

    /// Enable verbose TCP handshake logging.
    #[cfg(feature = "tcp")]
    #[arg(long, default_value_t = false)]
    tcp_debug: bool,
}

fn init_logging(verbose: bool) {
    let default_level = if verbose {
        LevelFilter::Debug
    } else {
        LevelFilter::Warn
    };
    let mut builder =
        env_logger::Builder::from_env(Env::default().default_filter_or(default_level.as_str()));
    builder.format_timestamp_millis();
    let _ = builder.try_init();
}

fn parse_env_number<T>(key: &str) -> Result<Option<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match env::var(key) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                trimmed
                    .parse::<T>()
                    .map(Some)
                    .map_err(|err| anyhow!("invalid {key} value '{trimmed}': {err}"))
            }
        }
        Err(env::VarError::NotPresent) => Ok(None),
        Err(err) => Err(anyhow!("failed to read {key}: {err}")),
    }
}

fn env_override<T>(cli_value: Option<T>, key: &str) -> Result<Option<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    if cli_value.is_some() {
        return Ok(cli_value);
    }
    parse_env_number(key)
}

fn resolve_policy_path(cli_path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = cli_path {
        return Ok(path);
    }
    if let Ok(value) = env::var("COHSH_POLICY") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    Ok(default_policy_path())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose);
    let stdout = io::stdout();
    let writer = stdout.lock();
    let mut qemu_args = cli.qemu_args.clone();
    if let Ok(extra) = env::var("COHSH_QEMU_ARGS") {
        if !extra.trim().is_empty() {
            qemu_args.extend(extra.split_whitespace().map(|arg| arg.to_owned()));
        }
    }
    #[cfg(feature = "tcp")]
    let (tcp_host, tcp_port, auth_token, tcp_debug) = {
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
        let tcp_debug = cli.tcp_debug || tcp_debug_enabled() || cli.verbose;
        (host, port, token, tcp_debug)
    };

    let policy_path = resolve_policy_path(cli.policy.clone())?;
    let overrides = PolicyOverrides {
        pool_control_sessions: env_override(
            cli.pool_control_sessions,
            "COHSH_POOL_CONTROL_SESSIONS",
        )?,
        pool_telemetry_sessions: env_override(
            cli.pool_telemetry_sessions,
            "COHSH_POOL_TELEMETRY_SESSIONS",
        )?,
        retry_max_attempts: env_override(cli.retry_max_attempts, "COHSH_RETRY_MAX_ATTEMPTS")?,
        retry_backoff_ms: env_override(cli.retry_backoff_ms, "COHSH_RETRY_BACKOFF_MS")?,
        retry_ceiling_ms: env_override(cli.retry_ceiling_ms, "COHSH_RETRY_CEILING_MS")?,
        retry_timeout_ms: env_override(cli.retry_timeout_ms, "COHSH_RETRY_TIMEOUT_MS")?,
        heartbeat_interval_ms: env_override(
            cli.heartbeat_interval_ms,
            "COHSH_HEARTBEAT_INTERVAL_MS",
        )?,
    };
    let policy = load_policy(&policy_path)
        .with_context(|| format!("failed to load cohsh policy {}", policy_path.display()))?;
    let policy = policy.with_overrides(&overrides).with_context(|| {
        format!(
            "invalid cohsh policy overrides for {}",
            policy_path.display()
        )
    })?;

    if let Some(script_path) = cli.check {
        let file = File::open(&script_path)
            .with_context(|| format!("failed to open script {script_path:?}"))?;
        validate_script(BufReader::new(file))?;
        println!("check ok: {}", script_path.display());
        return Ok(());
    }

    let (transport, pool_factory): (Box<dyn Transport>, Option<Arc<dyn TransportFactory>>) =
        match cli.transport {
            TransportKind::Mock => {
                let server = nine_door::NineDoor::new();
                let pool_server = server.clone();
                let factory = Arc::new(move || {
                    Ok(Box::new(NineDoorTransport::new(pool_server.clone()))
                        as Box<dyn Transport + Send>)
                });
                (Box::new(NineDoorTransport::new(server)), Some(factory))
            }
            TransportKind::Qemu => (
                Box::new(QemuTransport::new(
                    cli.qemu_bin.clone(),
                    cli.qemu_out_dir.clone(),
                    cli.qemu_gic_version.clone(),
                    qemu_args,
                )),
                None,
            ),
            #[cfg(feature = "tcp")]
            TransportKind::Tcp => {
                let retry = policy.retry;
                let heartbeat = policy.heartbeat;
                let shared = Arc::new(Mutex::new(
                    TcpTransport::new(tcp_host.clone(), tcp_port)
                        .with_retry_policy(retry)
                        .with_heartbeat_interval(Duration::from_millis(heartbeat.interval_ms))
                        .with_auth_token(auth_token.clone())
                        .with_tcp_debug(tcp_debug),
                ));
                let transport = Box::new(SharedTcpTransport::new(Arc::clone(&shared)));
                let pool_shared = Arc::clone(&shared);
                let pool_session_ids = Arc::new(AtomicU64::new(2));
                let factory = Arc::new(move || {
                    Ok(Box::new(PooledTcpTransport::new(
                        Arc::clone(&pool_shared),
                        Arc::clone(&pool_session_ids),
                    )) as Box<dyn Transport + Send>)
                });
                (transport, Some(factory))
            }
        };
    let mut shell = Shell::new(transport, writer);
    if let Some(factory) = pool_factory {
        let pool = SessionPool::new(
            policy.pool.control_sessions,
            policy.pool.telemetry_sessions,
            factory,
        );
        shell = shell.with_pool(pool);
    }
    shell.write_line("Welcome to Cohesix. Type 'help' for commands.")?;

    if let Some(script_path) = cli.script {
        if let Some(role_arg) = cli.role {
            let role = Role::from(role_arg);
            shell.attach(role, cli.ticket.as_deref())?;
        } else {
            shell.write_line("detached shell: run 'attach <role>' to connect")?;
        }
        let file = File::open(&script_path)
            .with_context(|| format!("failed to open script {script_path:?}"))?;
        shell.run_script(BufReader::new(file))?;
    } else {
        let auto_role = cli.role.map(Role::from);
        let auto_attach = auto_role.map(|role| AutoAttach {
            role,
            ticket: cli.ticket.clone(),
            attempts: 0,
            max_attempts: 1,
            auto_log: {
                #[cfg(feature = "tcp")]
                {
                    matches!(cli.transport, TransportKind::Qemu | TransportKind::Tcp)
                }
                #[cfg(not(feature = "tcp"))]
                {
                    matches!(cli.transport, TransportKind::Qemu)
                }
            },
        });
        if auto_attach.is_none() {
            shell.write_line("detached shell: run 'attach <role>' to connect")?;
        }
        shell.repl_with_autologin(auto_attach)?;
    }
    Ok(())
}
