// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: CLI entry point for the coh host bridge tool.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! CLI entry point for the Cohesix host bridge tool.

use std::path::PathBuf;
use std::env;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use coh::console::ConsoleSession;
use coh::{gpu, mount, run as coh_run, telemetry, CohAudit};
use coh::policy::{default_policy_path, load_policy, CohPolicy};
use cohsh::client::{CohClient, InProcessTransport};
use cohsh::RoleArg;
use cohesix_net_constants::COHESIX_TCP_CONSOLE_PORT;
use cohesix_ticket::Role;
use gpu_bridge_host::auto_bridge;
use nine_door::NineDoor;

#[derive(Debug, Parser)]
#[command(author = "Lukas Bower", version, about = "Cohesix host bridges")]
struct Cli {
    /// Role to use when attaching to Secure9P.
    #[arg(long, default_value_t = RoleArg::Queen)]
    role: RoleArg,

    /// Optional capability ticket payload.
    #[arg(long)]
    ticket: Option<String>,

    /// Path to the manifest-derived coh policy TOML.
    #[arg(long, value_name = "FILE")]
    policy: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Mount a Secure9P namespace via FUSE.
    Mount(MountArgs),
    /// GPU discovery and lease operations.
    Gpu(GpuArgs),
    /// Run a host command with lease validation and breadcrumb logging.
    Run(RunArgs),
    /// Telemetry pull operations.
    Telemetry(TelemetryArgs),
}

#[derive(Debug, Parser)]
struct ConnectArgs {
    /// Secure9P host.
    #[arg(long, default_value = "127.0.0.1", global = true)]
    host: String,
    /// Secure9P port.
    #[arg(long, default_value_t = COHESIX_TCP_CONSOLE_PORT, global = true)]
    port: u16,
    /// TCP console auth token (default: changeme).
    #[arg(long, global = true)]
    auth_token: Option<String>,
    /// Use the in-process mock backend.
    #[arg(long, default_value_t = false, global = true)]
    mock: bool,
}

#[derive(Debug, Parser)]
struct MountArgs {
    #[command(flatten)]
    connect: ConnectArgs,
    /// Mount point on the host filesystem.
    #[arg(long, value_name = "DIR")]
    at: PathBuf,
}

#[derive(Debug, Parser)]
struct GpuArgs {
    #[command(flatten)]
    connect: ConnectArgs,
    /// Use the NVML backend when available.
    #[arg(long, default_value_t = false)]
    nvml: bool,
    #[command(subcommand)]
    command: GpuCommand,
}

#[derive(Debug, Subcommand)]
enum GpuCommand {
    /// List GPUs.
    List,
    /// Show GPU status.
    Status { #[arg(long)] gpu: String },
    /// Request a GPU lease via /queen/ctl.
    Lease(GpuLeaseArgs),
}

#[derive(Debug, Parser)]
struct GpuLeaseArgs {
    /// GPU identifier.
    #[arg(long)]
    gpu: String,
    /// Memory requested in MiB.
    #[arg(long)]
    mem_mb: u32,
    /// Stream count requested.
    #[arg(long)]
    streams: u8,
    /// Lease TTL in seconds.
    #[arg(long)]
    ttl_s: u32,
    /// Optional scheduling priority.
    #[arg(long)]
    priority: Option<u8>,
    /// Optional budget TTL override.
    #[arg(long)]
    budget_ttl_s: Option<u64>,
    /// Optional budget ops override.
    #[arg(long)]
    budget_ops: Option<u64>,
}

#[derive(Debug, Parser)]
struct RunArgs {
    #[command(flatten)]
    connect: ConnectArgs,
    /// GPU identifier.
    #[arg(long)]
    gpu: String,
    /// Command to execute (pass after `--`).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, value_name = "CMD")]
    command: Vec<String>,
}

#[derive(Debug, Parser)]
struct TelemetryArgs {
    #[command(flatten)]
    connect: ConnectArgs,
    #[command(subcommand)]
    command: TelemetryCommand,
}

#[derive(Debug, Subcommand)]
enum TelemetryCommand {
    /// Pull telemetry bundles from /queen/telemetry.
    Pull { #[arg(long, value_name = "DIR")] out: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let policy_path = resolve_policy_path(cli.policy)?;
    let policy = load_policy(&policy_path)?;
    let role = Role::from(cli.role);
    match cli.command {
        Command::Mount(args) => run_mount(role, cli.ticket.as_deref(), &policy, args),
        Command::Gpu(args) => run_gpu(role, cli.ticket.as_deref(), &policy, args),
        Command::Run(args) => run_run(role, cli.ticket.as_deref(), &policy, args),
        Command::Telemetry(args) => run_telemetry(role, cli.ticket.as_deref(), &policy, args),
    }
}

fn resolve_policy_path(cli_path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = cli_path {
        return Ok(path);
    }
    if let Ok(value) = std::env::var("COH_POLICY") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    Ok(default_policy_path())
}

fn run_mount(role: Role, ticket: Option<&str>, policy: &CohPolicy, args: MountArgs) -> Result<()> {
    let mut audit = CohAudit::new();
    if args.connect.mock {
        mount::mock_mount(&args.at, policy)?;
        audit.push_ack(cohsh_core::wire::AckStatus::Ok, "MOUNT", Some("mode=mock"));
        emit_audit(audit);
        return Ok(());
    }
    let client = match connect_console(&args.connect, policy, role, ticket) {
        Ok(client) => client,
        Err(err) => {
            let mut audit = CohAudit::new();
            let detail = format!("reason={err}");
            audit.push_ack(cohsh_core::wire::AckStatus::Err, "MOUNT", Some(detail.as_str()));
            emit_audit(audit);
            return Err(err);
        }
    };
    audit.push_ack(cohsh_core::wire::AckStatus::Ok, "MOUNT", Some("mode=fuse"));
    emit_audit(audit);
    match mount::mount_console(client, policy, &args.at) {
        Ok(()) => Ok(()),
        Err(err) => {
            let mut audit = CohAudit::new();
            let detail = format!("reason={err}");
            audit.push_ack(cohsh_core::wire::AckStatus::Err, "MOUNT", Some(detail.as_str()));
            emit_audit(audit);
            Err(err)
        }
    }
}

fn run_gpu(role: Role, ticket: Option<&str>, policy: &CohPolicy, args: GpuArgs) -> Result<()> {
    if args.connect.mock && args.nvml {
        return Err(anyhow!("--mock and --nvml are mutually exclusive"));
    }
    let mut audit = CohAudit::new();
    if args.connect.mock || args.nvml {
        let (_server, mut client) = match connect_mock(role, ticket, true, args.nvml) {
            Ok(value) => value,
            Err(err) => {
                let mut audit = CohAudit::new();
                let detail = format!("reason={err}");
                audit.push_ack(cohsh_core::wire::AckStatus::Err, "GPU", Some(detail.as_str()));
                emit_audit(audit);
                return Err(err);
            }
        };
        let result = match args.command {
            GpuCommand::List => gpu::list(&mut client, &mut audit),
            GpuCommand::Status { gpu } => gpu::status(&mut client, &mut audit, &gpu),
            GpuCommand::Lease(lease) => {
                let args = gpu::GpuLeaseArgs {
                    gpu_id: lease.gpu,
                    mem_mb: lease.mem_mb,
                    streams: lease.streams,
                    ttl_s: lease.ttl_s,
                    priority: lease.priority,
                    budget_ttl_s: lease.budget_ttl_s,
                    budget_ops: lease.budget_ops,
                };
                gpu::lease(&mut client, &mut audit, &args)
            }
        };
        handle_result(result, audit, "GPU")
    } else {
        let mut client = match connect_console(&args.connect, policy, role, ticket) {
            Ok(client) => client,
            Err(err) => {
                let mut audit = CohAudit::new();
                let detail = format!("reason={err}");
                audit.push_ack(cohsh_core::wire::AckStatus::Err, "GPU", Some(detail.as_str()));
                emit_audit(audit);
                return Err(err);
            }
        };
        let result = match args.command {
            GpuCommand::List => gpu::list(&mut client, &mut audit),
            GpuCommand::Status { gpu } => gpu::status(&mut client, &mut audit, &gpu),
            GpuCommand::Lease(lease) => {
                let args = gpu::GpuLeaseArgs {
                    gpu_id: lease.gpu,
                    mem_mb: lease.mem_mb,
                    streams: lease.streams,
                    ttl_s: lease.ttl_s,
                    priority: lease.priority,
                    budget_ttl_s: lease.budget_ttl_s,
                    budget_ops: lease.budget_ops,
                };
                gpu::lease(&mut client, &mut audit, &args)
            }
        };
        handle_result(result, audit, "GPU")
    }
}

fn run_run(role: Role, ticket: Option<&str>, policy: &CohPolicy, args: RunArgs) -> Result<()> {
    let mut audit = CohAudit::new();
    if args.connect.mock {
        let (_server, mut client) = match connect_mock(role, ticket, true, false) {
            Ok(value) => value,
            Err(err) => {
                let mut audit = CohAudit::new();
                let detail = format!("reason={err}");
                audit.push_ack(cohsh_core::wire::AckStatus::Err, "RUN", Some(detail.as_str()));
                emit_audit(audit);
                return Err(err);
            }
        };
        let spec = coh_run::RunSpec {
            gpu_id: args.gpu,
            command: args.command,
        };
        let result = coh_run::execute(&mut client, policy, &mut audit, &spec);
        handle_result(result, audit, "RUN")
    } else {
        let mut client = match connect_console(&args.connect, policy, role, ticket) {
            Ok(client) => client,
            Err(err) => {
                let mut audit = CohAudit::new();
                let detail = format!("reason={err}");
                audit.push_ack(cohsh_core::wire::AckStatus::Err, "RUN", Some(detail.as_str()));
                emit_audit(audit);
                return Err(err);
            }
        };
        let spec = coh_run::RunSpec {
            gpu_id: args.gpu,
            command: args.command,
        };
        let result = coh_run::execute(&mut client, policy, &mut audit, &spec);
        handle_result(result, audit, "RUN")
    }
}

fn run_telemetry(
    role: Role,
    ticket: Option<&str>,
    policy: &CohPolicy,
    args: TelemetryArgs,
) -> Result<()> {
    let mut audit = CohAudit::new();
    if args.connect.mock {
        let (_server, mut client) = match connect_mock(role, ticket, false, false) {
            Ok(value) => value,
            Err(err) => {
                let mut audit = CohAudit::new();
                let detail = format!("reason={err}");
                audit.push_ack(
                    cohsh_core::wire::AckStatus::Err,
                    "TELEMETRY",
                    Some(detail.as_str()),
                );
                emit_audit(audit);
                return Err(err);
            }
        };
        let result = match args.command {
            TelemetryCommand::Pull { out } => {
                telemetry::pull(&mut client, policy, &out, &mut audit)
            }
        };
        match result {
            Ok(_) => {
                emit_audit(audit);
                Ok(())
            }
            Err(err) => handle_result(Err(err), audit, "TELEMETRY"),
        }
    } else {
        let mut client = match connect_console(&args.connect, policy, role, ticket) {
            Ok(client) => client,
            Err(err) => {
                let mut audit = CohAudit::new();
                let detail = format!("reason={err}");
                audit.push_ack(
                    cohsh_core::wire::AckStatus::Err,
                    "TELEMETRY",
                    Some(detail.as_str()),
                );
                emit_audit(audit);
                return Err(err);
            }
        };
        let result = match args.command {
            TelemetryCommand::Pull { out } => {
                telemetry::pull(&mut client, policy, &out, &mut audit)
            }
        };
        match result {
            Ok(_) => {
                emit_audit(audit);
                Ok(())
            }
            Err(err) => handle_result(Err(err), audit, "TELEMETRY"),
        }
    }
}

fn handle_result(result: Result<()>, mut audit: CohAudit, verb: &str) -> Result<()> {
    match result {
        Ok(()) => {
            emit_audit(audit);
            Ok(())
        }
        Err(err) => {
            let detail = format!("reason={err}");
            audit.push_ack(cohsh_core::wire::AckStatus::Err, verb, Some(detail.as_str()));
            emit_audit(audit);
            Err(err)
        }
    }
}

fn emit_audit(audit: CohAudit) {
    for line in audit.lines() {
        println!("{line}");
    }
}

fn connect_mock(
    role: Role,
    ticket: Option<&str>,
    seed_gpu: bool,
    nvml: bool,
) -> Result<(NineDoor, CohClient<InProcessTransport>)> {
    #[cfg(not(feature = "nvml"))]
    if nvml {
        return Err(anyhow!(
            "nvml feature disabled; rebuild coh with --features nvml or use --mock"
        ));
    }
    let server = NineDoor::new();
    if seed_gpu {
        let bridge = auto_bridge(!nvml)?;
        let snapshot = bridge.serialise_namespace()?;
        server.install_gpu_nodes(&snapshot)?;
    }
    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let client = CohClient::connect(transport, role, ticket)?;
    Ok((server, client))
}

fn resolve_auth_token(cli_token: Option<&str>) -> String {
    if let Some(token) = cli_token {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    if let Ok(value) = env::var("COH_AUTH_TOKEN") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    if let Ok(value) = env::var("COHSH_AUTH_TOKEN") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    "changeme".to_owned()
}

fn connect_console(
    args: &ConnectArgs,
    policy: &CohPolicy,
    role: Role,
    ticket: Option<&str>,
) -> Result<ConsoleSession> {
    let auth_token = resolve_auth_token(args.auth_token.as_deref());
    ConsoleSession::connect(
        &args.host,
        args.port,
        auth_token.as_str(),
        role,
        ticket,
        policy.retry,
    )
    .with_context(|| format!("failed to connect to {}:{}", args.host, args.port))
}
