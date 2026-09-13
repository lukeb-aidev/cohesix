// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: CLI entry point for the coh host bridge tool.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! CLI entry point for the Cohesix host bridge tool.

use std::env;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use coh::console::ConsoleSession;
use coh::policy::{default_policy_path, load_policy, CohPolicy};
use coh::rest::RestSession;
use coh::{
    doctor, evidence, evidence_timeline, fleet, gpu, mount, operator, peft, run as coh_run,
    telemetry, CohAccess, CohAudit,
};
use cohesix_net_constants::COHESIX_TCP_CONSOLE_PORT;
use cohesix_ticket::Role;
use cohsh::client::{CohClient, InProcessTransport};
use cohsh::RoleArg;
use gpu_bridge_host::{auto_bridge, auto_bridge_with_registry, build_publish_lines};
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
    /// Explain bounded live state or a canonical offline evidence pack.
    Inspect(InspectArgs),
    /// Compare two evidence packs or explicit tcp:// or REST target URLs.
    Diff(DiffArgs),
    /// Evaluate attestation evidence without treating measurements as signatures.
    Attest(AttestArgs),
    /// Validate and summarize a canonical trace without opening a transport.
    Trace {
        #[arg(long, value_name = "FILE")]
        input: PathBuf,
    },
    /// Alias for evidence pack; identical layout, bounds and redaction.
    Bundle(EvidencePackArgs),
    /// Run deterministic environment checks.
    Doctor(DoctorArgs),
    /// Mount a Secure9P namespace via FUSE.
    Mount(MountArgs),
    /// GPU discovery and lease operations.
    Gpu(GpuArgs),
    /// PEFT/LoRA lifecycle operations.
    Peft(PeftArgs),
    /// Run a host command with lease validation and breadcrumb logging.
    Run(RunArgs),
    /// Telemetry pull operations.
    Telemetry(TelemetryArgs),
    /// Read-only multi-hive fan-in commands.
    Fleet(FleetArgs),
    /// Evidence pack and timeline operations.
    Evidence(EvidenceArgs),
}

#[derive(Debug, Parser)]
struct InspectArgs {
    #[command(flatten)]
    connect: ConnectArgs,
    /// Canonical evidence-pack directory. Offline mode opens no transport.
    #[arg(long, value_name = "DIR")]
    input: Option<PathBuf>,
    /// Emit stable JSON instead of escaped structured text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Parser)]
struct AttestArgs {
    #[command(flatten)]
    inspect: InspectArgs,
    /// Independently enrolled trust policy; never inferred from target evidence.
    #[arg(long, value_name = "FILE")]
    trust_policy: Option<PathBuf>,
    /// Retain the public request and signed response for offline verification.
    #[arg(long, value_name = "FILE", conflicts_with = "input")]
    record: Option<PathBuf>,
}

#[derive(Debug, Parser)]
struct DiffArgs {
    /// Pack directory, tcp://host:port, or http(s)://gateway URL.
    #[arg(long)]
    left: String,
    /// Pack directory, tcp://host:port, or http(s)://gateway URL.
    #[arg(long)]
    right: String,
    /// TCP authentication token; environment resolution matches other coh commands.
    #[arg(long)]
    auth_token: Option<String>,
    /// REST request authentication token.
    #[arg(long)]
    rest_auth_token: Option<String>,
}

#[derive(Debug, Parser)]
struct ConnectArgs {
    /// Secure9P host.
    #[arg(long, default_value = "127.0.0.1", global = true)]
    host: String,
    /// Secure9P port.
    #[arg(long, default_value_t = COHESIX_TCP_CONSOLE_PORT, global = true)]
    port: u16,
    /// REST gateway base URL for hive-gateway (optional).
    #[arg(long, value_name = "URL", global = true)]
    rest_url: Option<String>,
    /// Request auth token for REST mutating routes.
    #[arg(long, value_name = "TOKEN", global = true)]
    rest_auth_token: Option<String>,
    /// TCP console auth token.
    #[arg(long, global = true)]
    auth_token: Option<String>,
    /// Use the in-process mock backend.
    #[arg(long, default_value_t = false, global = true)]
    mock: bool,
}

#[derive(Debug, Parser)]
struct DoctorArgs {
    #[command(flatten)]
    connect: ConnectArgs,
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

#[derive(Debug, Parser)]
struct PeftArgs {
    #[command(flatten)]
    connect: ConnectArgs,
    #[command(subcommand)]
    command: PeftCommand,
}

#[derive(Debug, Subcommand)]
enum PeftCommand {
    /// Export a LoRA job directory from /queen/export/lora_jobs.
    Export {
        #[arg(long)]
        job: String,
        #[arg(long, value_name = "DIR")]
        out: PathBuf,
    },
    /// Import adapter artifacts into the host registry.
    Import {
        #[arg(long)]
        model: String,
        #[arg(long, value_name = "DIR")]
        from: PathBuf,
        #[arg(long)]
        job: String,
        #[arg(long, value_name = "DIR")]
        export: PathBuf,
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,
        /// Publish the updated GPU model registry into /gpu/models (live VM only).
        #[arg(long, alias = "refresh-gpu-models")]
        publish: bool,
    },
    /// Activate a model pointer.
    Activate {
        #[arg(long)]
        model: String,
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,
    },
    /// Roll back to the previous model pointer.
    Rollback {
        #[arg(long, value_name = "DIR")]
        registry: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum GpuCommand {
    /// List GPUs.
    List,
    /// Show GPU status.
    Status {
        #[arg(long)]
        gpu: String,
    },
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
    /// Optional receipt output path.
    #[arg(long, value_name = "FILE")]
    receipt_out: Option<PathBuf>,
}

#[derive(Debug, Parser)]
struct RunArgs {
    #[command(flatten)]
    connect: ConnectArgs,
    /// GPU identifier.
    #[arg(long)]
    gpu: String,
    /// Optional receipt output path.
    #[arg(long, value_name = "FILE")]
    receipt_out: Option<PathBuf>,
    /// Command to execute (pass after `--`).
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "CMD"
    )]
    command: Vec<String>,
}

#[derive(Debug, Parser)]
struct TelemetryArgs {
    #[command(flatten)]
    connect: ConnectArgs,
    #[command(subcommand)]
    command: TelemetryCommand,
}

#[derive(Debug, Parser)]
struct EvidenceArgs {
    #[command(subcommand)]
    command: EvidenceCommand,
}

#[derive(Debug, Parser)]
struct FleetArgs {
    #[command(flatten)]
    connect: ConnectArgs,
    /// Repeatable hive targets in `name=url` form.
    #[arg(long = "hive", value_name = "NAME=URL")]
    hives: Vec<String>,
    #[command(subcommand)]
    command: FleetCommand,
}

#[derive(Debug, Subcommand)]
enum FleetCommand {
    /// Read `/proc/lifecycle/state` and `/proc/root/reachable` across hives.
    Status,
    /// Read `/proc/lease/summary` across hives.
    LeaseSummary,
    /// Read `/proc/pressure/*` across hives.
    Pressure,
}

#[derive(Debug, Subcommand)]
enum EvidenceCommand {
    /// Export a deterministic evidence pack directory.
    Pack(Box<EvidencePackArgs>),
    /// Generate `timeline.ndjson` and `timeline.md` from an evidence pack.
    Timeline {
        #[arg(long, value_name = "DIR", alias = "in")]
        input: PathBuf,
        /// Review framing only; never changes evidence authority.
        #[arg(long, value_enum, default_value = "generic")]
        scenario: evidence_timeline::Scenario,
    },
}

#[derive(Debug, Parser)]
struct EvidencePackArgs {
    #[command(flatten)]
    connect: ConnectArgs,
    /// Output directory for the evidence pack.
    #[arg(long, value_name = "DIR")]
    out: PathBuf,
    /// Include telemetry pulls under `telemetry/` inside the pack.
    #[arg(long, default_value_t = false)]
    with_telemetry: bool,
    /// Host copy of the source manifest (secret fields are redacted).
    #[arg(long, value_name = "FILE")]
    manifest: Option<PathBuf>,
    /// Host copy of the selected resolved manifest.
    #[arg(long, value_name = "FILE")]
    resolved_manifest: Option<PathBuf>,
    /// Bounded host serial excerpt; authentication and unstructured payloads are withheld.
    #[arg(long, value_name = "FILE")]
    serial_log: Option<PathBuf>,
    /// Canonical redacted live trace to include; legacy traces remain offline fixtures.
    #[arg(long, value_name = "FILE")]
    trace: Option<PathBuf>,
    /// Public challenge and signed response retained by coh attest --record.
    #[arg(long, value_name = "FILE")]
    attestation_record: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum TelemetryCommand {
    /// Pull telemetry bundles from /queen/telemetry.
    Pull {
        #[arg(long, value_name = "DIR")]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let policy_path = resolve_policy_path(cli.policy)?;
    let role = Role::from(cli.role);
    match cli.command {
        Command::Inspect(args) => {
            run_inspect(role, cli.ticket.as_deref(), &policy_path, args, false)
        }
        Command::Attest(args) => run_attest(role, cli.ticket.as_deref(), &policy_path, args),
        Command::Diff(args) => {
            let left =
                read_diff_source(&args.left, &args, role, cli.ticket.as_deref(), &policy_path)?;
            let right = read_diff_source(
                &args.right,
                &args,
                role,
                cli.ticket.as_deref(),
                &policy_path,
            )?;
            anyhow::ensure!(
                left.violations.is_empty() && right.violations.is_empty(),
                "diff-inconsistent-source"
            );
            println!(
                "{}",
                serde_json::to_string_pretty(&operator::diff(&left, &right)?)?
            );
            Ok(())
        }
        Command::Trace { input } => {
            let payload = operator::read_bounded(&input, operator::MAX_BYTES)?;
            let policy = cohsh_core::trace::TracePolicy::new(
                operator::MAX_BYTES as u32,
                cohsh::SECURE9P_MSIZE,
                cohsh_core::MAX_LINE_LEN as u32,
            );
            let trace = cohsh_core::trace::TraceLog::decode(&payload, policy)?;
            let capture = if let Some(metadata) = &trace.capture {
                let expected = cohsh::trace_capture::policy_digest(
                    policy,
                    cohsh::CohshPolicy::from_generated().trace.max_duration_ms,
                );
                cohsh::trace_capture::replay_lines(&trace, &expected)?;
                Some(
                    serde_json::json!({"backend":if metadata.backend == 1 { "tcp-console" } else { "rest-projection" },"completion":metadata.completion,"captured_unix_ms":metadata.captured_unix_ms,"target_sha256":hex::encode(metadata.target_sha256),"session_sha256":hex::encode(metadata.session_sha256),"manifest_sha256":hex::encode(metadata.manifest_sha256),"image_sha256":hex::encode(metadata.image_sha256),"identity_binding":"caller-supplied"}),
                )
            } else {
                None
            };
            println!(
                "{}",
                serde_json::json!({"schema":"cohesix-trace-summary/v1", "source_class":"offline-trace", "bytes":payload.len(), "sha256":operator::digest(&payload), "frames":trace.frames.len(), "acknowledgements":trace.ack_lines.len(), "capture":capture,"proof":"none"})
            );
            Ok(())
        }
        Command::Bundle(pack) => run_evidence(
            role,
            cli.ticket.as_deref(),
            &policy_path,
            EvidenceArgs {
                command: EvidenceCommand::Pack(Box::new(pack)),
            },
        ),
        Command::Doctor(args) => run_doctor(role, cli.ticket.as_deref(), &policy_path, args),
        Command::Mount(args) => {
            let policy = load_policy(&policy_path)?;
            run_mount(role, cli.ticket.as_deref(), &policy, args)
        }
        Command::Gpu(args) => {
            let policy = load_policy(&policy_path)?;
            run_gpu(role, cli.ticket.as_deref(), &policy, args)
        }
        Command::Peft(args) => {
            let policy = load_policy(&policy_path)?;
            run_peft(role, cli.ticket.as_deref(), &policy, args)
        }
        Command::Run(args) => {
            let policy = load_policy(&policy_path)?;
            run_run(role, cli.ticket.as_deref(), &policy, args)
        }
        Command::Telemetry(args) => {
            let policy = load_policy(&policy_path)?;
            run_telemetry(role, cli.ticket.as_deref(), &policy, args)
        }
        Command::Fleet(args) => run_fleet(args),
        Command::Evidence(args) => run_evidence(role, cli.ticket.as_deref(), &policy_path, args),
    }
}

fn run_attest(
    role: Role,
    ticket: Option<&str>,
    policy_path: &Path,
    args: AttestArgs,
) -> Result<()> {
    let Some(trust_path) = args.trust_policy else {
        anyhow::ensure!(
            args.record.is_none(),
            "attestation-record-requires-trust-policy"
        );
        return run_inspect(role, ticket, policy_path, args.inspect, true);
    };
    let trust = operator::read_bounded(&trust_path, cohesix_attestation::MAX_POLICY_BYTES)?;
    let result = if let Some(input) = args.inspect.input {
        let snapshot = operator::inspect_pack(&input)?;
        anyhow::ensure!(snapshot.violations.is_empty(), "inconsistent-evidence");
        let path = operator::confined_path(&input, "attachments/attestation-record.json")?;
        let record = operator::read_bounded(&path, coh::attestation::MAX_RECORD_BYTES)?;
        coh::attestation::offline(&trust, &record)?
    } else {
        let policy = load_policy(policy_path)?;
        anyhow::ensure!(!args.inspect.connect.mock, "mock-attestation-forbidden");
        let class = if resolve_rest_url(args.inspect.connect.rest_url.as_deref()).is_some() {
            "host-projection"
        } else {
            "live-console"
        };
        let mut client = connect_access(&args.inspect.connect, &policy, role, ticket)?;
        let (result, record) = coh::attestation::live(&mut client, &trust, class)?;
        if let (Some(path), Some(record)) = (args.record, record) {
            operator::write_atomic(&path, &serde_json::to_vec_pretty(&record)?)?;
        }
        result
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    anyhow::ensure!(
        result.verdict == "PASS",
        "attestation-non-attested: {}",
        result.reason
    );
    Ok(())
}

fn run_inspect(
    role: Role,
    ticket: Option<&str>,
    policy_path: &Path,
    args: InspectArgs,
    attestation: bool,
) -> Result<()> {
    let snapshot = if let Some(input) = args.input {
        operator::inspect_pack(&input)?
    } else {
        let policy = load_policy(policy_path)?;
        if args.connect.mock {
            let (_server, mut client) = connect_mock(role, ticket, false, false)?;
            operator::inspect_live(&mut client, "mock")?
        } else {
            let class = if resolve_rest_url(args.connect.rest_url.as_deref()).is_some() {
                "host-projection"
            } else {
                "live-console"
            };
            let mut client = connect_access(&args.connect, &policy, role, ticket)?;
            operator::inspect_live(&mut client, class)?
        }
    };
    if attestation {
        let result = operator::attest(&snapshot);
        println!("{}", serde_json::to_string_pretty(&result)?);
        anyhow::bail!("attestation-non-attested: {}", result.reason);
    }
    if args.json {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    } else {
        print!("{}", snapshot.render()?);
    }
    anyhow::ensure!(
        snapshot.violations.is_empty(),
        "inspect-invariant-violation"
    );
    Ok(())
}

fn read_diff_source(
    source: &str,
    args: &DiffArgs,
    role: Role,
    ticket: Option<&str>,
    policy_path: &Path,
) -> Result<operator::Snapshot> {
    if let Some(endpoint) = source.strip_prefix("tcp://") {
        let (host, port) = endpoint
            .rsplit_once(':')
            .context("diff TCP source requires host:port")?;
        anyhow::ensure!(
            !host.is_empty()
                && !host.contains(['/', '@'])
                && !host.chars().any(char::is_whitespace),
            "diff-tcp-host"
        );
        let connect = ConnectArgs {
            host: host.to_owned(),
            port: port.parse().context("diff-tcp-port")?,
            rest_url: None,
            rest_auth_token: None,
            auth_token: args.auth_token.clone(),
            mock: false,
        };
        let mut client = connect_console(&connect, &load_policy(policy_path)?, role, ticket)?;
        operator::inspect_live(&mut client, "live-console")
    } else if source.starts_with("http://") || source.starts_with("https://") {
        anyhow::ensure!(
            role == Role::Queen,
            "rest transport supports queen role only"
        );
        let mut client = RestSession::connect(
            source.to_owned(),
            resolve_rest_auth_token(args.rest_auth_token.as_deref()),
        );
        operator::inspect_live(&mut client, "host-projection")
    } else {
        operator::inspect_pack(Path::new(source.strip_prefix("pack:").unwrap_or(source)))
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

fn run_doctor(
    role: Role,
    ticket: Option<&str>,
    policy_path: &Path,
    args: DoctorArgs,
) -> Result<()> {
    let mut audit = CohAudit::new();
    let config = doctor::DoctorConfig {
        role,
        ticket: ticket.map(|value| value.to_owned()),
        policy_path: policy_path.to_path_buf(),
        mock: args.connect.mock,
    };
    let result = doctor::run(config, &mut audit);
    handle_result(result, audit, "DOCTOR")
}

fn run_mount(role: Role, ticket: Option<&str>, policy: &CohPolicy, args: MountArgs) -> Result<()> {
    let mut audit = CohAudit::new();
    if args.connect.mock {
        mount::mock_mount(&args.at, policy)?;
        audit.push_ack(cohsh_core::wire::AckStatus::Ok, "MOUNT", Some("mode=mock"));
        emit_audit(audit);
        return Ok(());
    }
    if let Some(rest_url) = resolve_rest_url(args.connect.rest_url.as_deref()) {
        if role != Role::Queen {
            let mut audit = CohAudit::new();
            audit.push_ack(
                cohsh_core::wire::AckStatus::Err,
                "MOUNT",
                Some("reason=rest-transport-queen-only"),
            );
            emit_audit(audit);
            return Err(anyhow!("rest transport supports queen role only"));
        }
        let _rest_lock = match mount::RestMountLock::acquire(rest_url.as_str()) {
            Ok(lock) => lock,
            Err(err) => {
                let mut audit = CohAudit::new();
                let detail = format!("reason={err}");
                audit.push_ack(
                    cohsh_core::wire::AckStatus::Err,
                    "MOUNT",
                    Some(detail.as_str()),
                );
                emit_audit(audit);
                return Err(err);
            }
        };
        let rest_auth_token = resolve_rest_auth_token(args.connect.rest_auth_token.as_deref());
        let client = RestSession::connect(rest_url.as_str(), rest_auth_token);
        audit.push_ack(cohsh_core::wire::AckStatus::Ok, "MOUNT", Some("mode=rest"));
        emit_audit(audit);
        return mount::mount_rest(client, policy, &args.at);
    }
    let client = match connect_console(&args.connect, policy, role, ticket) {
        Ok(client) => client,
        Err(err) => {
            let mut audit = CohAudit::new();
            let detail = format!("reason={err}");
            audit.push_ack(
                cohsh_core::wire::AckStatus::Err,
                "MOUNT",
                Some(detail.as_str()),
            );
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
            audit.push_ack(
                cohsh_core::wire::AckStatus::Err,
                "MOUNT",
                Some(detail.as_str()),
            );
            emit_audit(audit);
            Err(err)
        }
    }
}

fn run_gpu(role: Role, ticket: Option<&str>, policy: &CohPolicy, args: GpuArgs) -> Result<()> {
    if args.connect.mock && args.nvml {
        return Err(anyhow!("--mock and --nvml are mutually exclusive"));
    }
    let bounds = evidence::build_local_bounds();
    let mut audit = CohAudit::new();
    if args.connect.mock || args.nvml {
        let (_server, mut client) = match connect_mock(role, ticket, true, args.nvml) {
            Ok(value) => value,
            Err(err) => {
                let mut audit = CohAudit::new();
                let detail = format!("reason={err}");
                audit.push_ack(
                    cohsh_core::wire::AckStatus::Err,
                    "GPU",
                    Some(detail.as_str()),
                );
                emit_audit(audit);
                return Err(err);
            }
        };
        let result = match args.command {
            GpuCommand::List => gpu::list(&mut client, &mut audit),
            GpuCommand::Status { gpu } => gpu::status(&mut client, &mut audit, &gpu),
            GpuCommand::Lease(lease) => {
                let GpuLeaseArgs {
                    gpu,
                    mem_mb,
                    streams,
                    ttl_s,
                    priority,
                    budget_ttl_s,
                    budget_ops,
                    receipt_out,
                } = lease;
                let args = gpu::GpuLeaseArgs {
                    gpu_id: gpu,
                    mem_mb,
                    streams,
                    ttl_s,
                    priority,
                    budget_ttl_s,
                    budget_ops,
                };
                gpu::lease_with_receipt(
                    &mut client,
                    &mut audit,
                    &args,
                    receipt_out.as_deref(),
                    &bounds,
                )
            }
        };
        handle_result(result, audit, "GPU")
    } else {
        let mut client = match connect_access(&args.connect, policy, role, ticket) {
            Ok(client) => client,
            Err(err) => {
                let mut audit = CohAudit::new();
                let detail = format!("reason={err}");
                audit.push_ack(
                    cohsh_core::wire::AckStatus::Err,
                    "GPU",
                    Some(detail.as_str()),
                );
                emit_audit(audit);
                return Err(err);
            }
        };
        let result = match args.command {
            GpuCommand::List => gpu::list(&mut client, &mut audit),
            GpuCommand::Status { gpu } => gpu::status(&mut client, &mut audit, &gpu),
            GpuCommand::Lease(lease) => {
                let GpuLeaseArgs {
                    gpu,
                    mem_mb,
                    streams,
                    ttl_s,
                    priority,
                    budget_ttl_s,
                    budget_ops,
                    receipt_out,
                } = lease;
                let args = gpu::GpuLeaseArgs {
                    gpu_id: gpu,
                    mem_mb,
                    streams,
                    ttl_s,
                    priority,
                    budget_ttl_s,
                    budget_ops,
                };
                gpu::lease_with_receipt(
                    &mut client,
                    &mut audit,
                    &args,
                    receipt_out.as_deref(),
                    &bounds,
                )
            }
        };
        handle_result(result, audit, "GPU")
    }
}

fn run_run(role: Role, ticket: Option<&str>, policy: &CohPolicy, args: RunArgs) -> Result<()> {
    let mut audit = CohAudit::new();
    let bounds = evidence::build_local_bounds();
    let receipt_out = args.receipt_out.clone();
    if args.connect.mock {
        let (_server, mut client) = match connect_mock(role, ticket, true, false) {
            Ok(value) => value,
            Err(err) => {
                let mut audit = CohAudit::new();
                let detail = format!("reason={err}");
                audit.push_ack(
                    cohsh_core::wire::AckStatus::Err,
                    "RUN",
                    Some(detail.as_str()),
                );
                emit_audit(audit);
                return Err(err);
            }
        };
        let spec = coh_run::RunSpec {
            gpu_id: args.gpu,
            command: args.command,
        };
        let result = coh_run::execute_with_receipt(
            &mut client,
            policy,
            &mut audit,
            &spec,
            receipt_out.as_deref(),
            &bounds,
        );
        handle_result(result, audit, "RUN")
    } else {
        let mut client = match connect_access(&args.connect, policy, role, ticket) {
            Ok(client) => client,
            Err(err) => {
                let mut audit = CohAudit::new();
                let detail = format!("reason={err}");
                audit.push_ack(
                    cohsh_core::wire::AckStatus::Err,
                    "RUN",
                    Some(detail.as_str()),
                );
                emit_audit(audit);
                return Err(err);
            }
        };
        let spec = coh_run::RunSpec {
            gpu_id: args.gpu,
            command: args.command,
        };
        let result = coh_run::execute_with_receipt(
            &mut client,
            policy,
            &mut audit,
            &spec,
            receipt_out.as_deref(),
            &bounds,
        );
        handle_result(result, audit, "RUN")
    }
}

fn run_peft(role: Role, ticket: Option<&str>, policy: &CohPolicy, args: PeftArgs) -> Result<()> {
    let mut audit = CohAudit::new();
    match args.command {
        PeftCommand::Export { job, out } => {
            if args.connect.mock {
                let (server, mut client) = match connect_mock(role, ticket, true, false) {
                    Ok(value) => value,
                    Err(err) => {
                        let mut audit = CohAudit::new();
                        let detail = format!("reason={err}");
                        audit.push_ack(
                            cohsh_core::wire::AckStatus::Err,
                            "PEFT",
                            Some(detail.as_str()),
                        );
                        emit_audit(audit);
                        return Err(err);
                    }
                };
                seed_peft_export_job(&server, &job)?;
                let spec = peft::PeftExportSpec {
                    job_id: job,
                    out_dir: out,
                };
                let result = peft::export_job(&mut client, policy, &spec, &mut audit);
                handle_result(result.map(|_| ()), audit, "PEFT")
            } else {
                let mut client = match connect_access(&args.connect, policy, role, ticket) {
                    Ok(client) => client,
                    Err(err) => {
                        let mut audit = CohAudit::new();
                        let detail = format!("reason={err}");
                        audit.push_ack(
                            cohsh_core::wire::AckStatus::Err,
                            "PEFT",
                            Some(detail.as_str()),
                        );
                        emit_audit(audit);
                        return Err(err);
                    }
                };
                let spec = peft::PeftExportSpec {
                    job_id: job,
                    out_dir: out,
                };
                let result = peft::export_job(&mut client, policy, &spec, &mut audit);
                handle_result(result.map(|_| ()), audit, "PEFT")
            }
        }
        PeftCommand::Import {
            model,
            from,
            job,
            export,
            registry,
            publish,
        } => {
            let registry_root =
                registry.unwrap_or_else(|| PathBuf::from(&policy.peft.import.registry_root));
            let spec = peft::PeftImportSpec {
                model_id: model.clone(),
                adapter_dir: from,
                export_root: export,
                job_id: job,
                registry_root: registry_root.clone(),
            };
            let result = peft::import_adapter(policy, &spec, &mut audit).and_then(|summary| {
                if publish {
                    if args.connect.mock {
                        let (_server, mut client) = connect_mock(role, ticket, true, false)?;
                        publish_gpu_registry(
                            &mut client,
                            Some(&registry_root),
                            args.connect.mock,
                            &mut audit,
                        )?;
                    } else {
                        let mut client = connect_access(&args.connect, policy, role, ticket)?;
                        publish_gpu_registry(
                            &mut client,
                            Some(&registry_root),
                            args.connect.mock,
                            &mut audit,
                        )?;
                    }
                }
                Ok(summary)
            });
            if let Ok(summary) = &result {
                audit.push_line(format!(
                    "peft import model={} manifest={}",
                    summary.model_id,
                    summary.manifest_path.display()
                ));
            }
            handle_result(result.map(|_| ()), audit, "PEFT")
        }
        PeftCommand::Activate { model, registry } => {
            let registry_root =
                registry.unwrap_or_else(|| PathBuf::from(&policy.peft.import.registry_root));
            if args.connect.mock {
                let (_server, mut client) = match connect_mock(role, ticket, true, false) {
                    Ok(value) => value,
                    Err(err) => {
                        let mut audit = CohAudit::new();
                        let detail = format!("reason={err}");
                        audit.push_ack(
                            cohsh_core::wire::AckStatus::Err,
                            "PEFT",
                            Some(detail.as_str()),
                        );
                        emit_audit(audit);
                        return Err(err);
                    }
                };
                let spec = peft::PeftActivateSpec {
                    model_id: model,
                    registry_root,
                };
                let result = peft::activate_model(policy, &spec, &mut audit).and_then(|()| {
                    publish_gpu_registry(
                        &mut client,
                        Some(&spec.registry_root),
                        args.connect.mock,
                        &mut audit,
                    )
                });
                handle_result(result, audit, "PEFT")
            } else {
                let mut client = match connect_access(&args.connect, policy, role, ticket) {
                    Ok(client) => client,
                    Err(err) => {
                        let mut audit = CohAudit::new();
                        let detail = format!("reason={err}");
                        audit.push_ack(
                            cohsh_core::wire::AckStatus::Err,
                            "PEFT",
                            Some(detail.as_str()),
                        );
                        emit_audit(audit);
                        return Err(err);
                    }
                };
                let spec = peft::PeftActivateSpec {
                    model_id: model,
                    registry_root,
                };
                let result = peft::activate_model(policy, &spec, &mut audit).and_then(|()| {
                    publish_gpu_registry(
                        &mut client,
                        Some(&spec.registry_root),
                        args.connect.mock,
                        &mut audit,
                    )
                });
                handle_result(result, audit, "PEFT")
            }
        }
        PeftCommand::Rollback { registry } => {
            let registry_root =
                registry.unwrap_or_else(|| PathBuf::from(&policy.peft.import.registry_root));
            if args.connect.mock {
                let (_server, mut client) = match connect_mock(role, ticket, true, false) {
                    Ok(value) => value,
                    Err(err) => {
                        let mut audit = CohAudit::new();
                        let detail = format!("reason={err}");
                        audit.push_ack(
                            cohsh_core::wire::AckStatus::Err,
                            "PEFT",
                            Some(detail.as_str()),
                        );
                        emit_audit(audit);
                        return Err(err);
                    }
                };
                let spec = peft::PeftRollbackSpec { registry_root };
                let result = peft::rollback_model(policy, &spec, &mut audit).and_then(|()| {
                    publish_gpu_registry(
                        &mut client,
                        Some(&spec.registry_root),
                        args.connect.mock,
                        &mut audit,
                    )
                });
                handle_result(result, audit, "PEFT")
            } else {
                let mut client = match connect_access(&args.connect, policy, role, ticket) {
                    Ok(client) => client,
                    Err(err) => {
                        let mut audit = CohAudit::new();
                        let detail = format!("reason={err}");
                        audit.push_ack(
                            cohsh_core::wire::AckStatus::Err,
                            "PEFT",
                            Some(detail.as_str()),
                        );
                        emit_audit(audit);
                        return Err(err);
                    }
                };
                let spec = peft::PeftRollbackSpec { registry_root };
                let result = peft::rollback_model(policy, &spec, &mut audit).and_then(|()| {
                    publish_gpu_registry(
                        &mut client,
                        Some(&spec.registry_root),
                        args.connect.mock,
                        &mut audit,
                    )
                });
                handle_result(result, audit, "PEFT")
            }
        }
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
        let mut client = match connect_access(&args.connect, policy, role, ticket) {
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

fn run_fleet(args: FleetArgs) -> Result<()> {
    if args.connect.mock {
        return Err(anyhow!(
            "fleet commands require REST gateways; --mock is not supported"
        ));
    }
    let targets = fleet::parse_hive_targets(
        args.hives.as_slice(),
        resolve_rest_url(args.connect.rest_url.as_deref()).as_deref(),
    )?;
    let rest_auth_token = resolve_rest_auth_token(args.connect.rest_auth_token.as_deref());
    let lines = match args.command {
        FleetCommand::Status => fleet::fleet_status(&targets, rest_auth_token.as_deref()),
        FleetCommand::LeaseSummary => {
            fleet::fleet_lease_summary(&targets, rest_auth_token.as_deref())
        }
        FleetCommand::Pressure => fleet::fleet_pressure(&targets, rest_auth_token.as_deref()),
    };
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

fn run_evidence(
    role: Role,
    ticket: Option<&str>,
    policy_path: &Path,
    args: EvidenceArgs,
) -> Result<()> {
    match args.command {
        EvidenceCommand::Pack(pack) => {
            let policy = load_policy(policy_path)?;
            let mut audit = CohAudit::new();
            let bounds = if pack.connect.mock {
                evidence::build_local_bounds()
            } else if let Some(rest_url) = resolve_rest_url(pack.connect.rest_url.as_deref()) {
                let rest_auth_token =
                    resolve_rest_auth_token(pack.connect.rest_auth_token.as_deref());
                let rest = RestSession::connect(rest_url, rest_auth_token);
                rest.bounds()
                    .unwrap_or_else(|_| evidence::build_local_bounds())
            } else {
                evidence::build_local_bounds()
            };

            let spec = evidence::EvidencePackSpec {
                out_dir: pack.out,
                with_telemetry: pack.with_telemetry,
            };

            let result = if pack.connect.mock {
                let (_server, mut client) = connect_mock(role, ticket, true, false)?;
                evidence::export_pack(&mut client, &policy, &bounds, &spec, &mut audit).map(|_| ())
            } else {
                let mut access = connect_access(&pack.connect, &policy, role, ticket)?;
                evidence::export_pack(&mut access, &policy, &bounds, &spec, &mut audit).map(|_| ())
            };
            if result.is_ok() {
                evidence::attach_artifacts(
                    &spec.out_dir,
                    pack.manifest.as_deref(),
                    pack.resolved_manifest.as_deref(),
                    pack.serial_log.as_deref(),
                    pack.trace.as_deref(),
                    pack.attestation_record.as_deref(),
                )?;
                let digest = operator::read_bounded(&spec.out_dir.join("pack.sha256"), 65)?;
                audit.push_line(format!(
                    "evidence pack sha256={}",
                    std::str::from_utf8(&digest)?.trim()
                ));
            }
            handle_result(result, audit, "EVIDENCE")
        }
        EvidenceCommand::Timeline { input, scenario } => {
            let mut audit = CohAudit::new();
            let result =
                evidence_timeline::write_timeline_with_scenario(&input, scenario).map(|summary| {
                    audit.push_line(format!(
                        "evidence timeline events={} ndjson={} markdown={}",
                        summary.events,
                        summary.ndjson_path.display(),
                        summary.markdown_path.display()
                    ));
                });
            handle_result(result, audit, "EVIDENCE")
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
            audit.push_ack(
                cohsh_core::wire::AckStatus::Err,
                verb,
                Some(detail.as_str()),
            );
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

#[allow(clippy::large_enum_variant)]
enum AccessHandle {
    Console(ConsoleSession),
    Rest(RestSession),
}

impl CohAccess for AccessHandle {
    fn list_dir(&mut self, path: &str, max_bytes: usize) -> Result<Vec<String>> {
        match self {
            AccessHandle::Console(session) => session.list_dir(path, max_bytes),
            AccessHandle::Rest(session) => session.list_dir(path, max_bytes),
        }
    }

    fn read_file(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
        match self {
            AccessHandle::Console(session) => session.read_file(path, max_bytes),
            AccessHandle::Rest(session) => session.read_file(path, max_bytes),
        }
    }

    fn tail_file(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
        match self {
            AccessHandle::Console(session) => session.tail_file(path, max_bytes),
            AccessHandle::Rest(session) => session.tail_file(path, max_bytes),
        }
    }

    fn write_append(&mut self, path: &str, payload: &[u8]) -> Result<usize> {
        match self {
            AccessHandle::Console(session) => session.write_append(path, payload),
            AccessHandle::Rest(session) => session.write_append(path, payload),
        }
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

const INSECURE_PLACEHOLDER_TOKEN: &str = concat!("change", "me");

fn resolve_auth_token(cli_token: Option<&str>) -> Result<String> {
    if let Some(token) = cli_token {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            if trimmed == INSECURE_PLACEHOLDER_TOKEN {
                return Err(anyhow!(
                    "tcp auth token uses insecure placeholder token; set --auth-token or COH_AUTH_TOKEN/COHSH_AUTH_TOKEN"
                ));
            }
            return Ok(trimmed.to_owned());
        }
    }
    if let Ok(value) = env::var("COH_AUTH_TOKEN") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            if trimmed == INSECURE_PLACEHOLDER_TOKEN {
                return Err(anyhow!(
                    "tcp auth token uses insecure placeholder token; set --auth-token or COH_AUTH_TOKEN/COHSH_AUTH_TOKEN"
                ));
            }
            return Ok(trimmed.to_owned());
        }
    }
    if let Ok(value) = env::var("COHSH_AUTH_TOKEN") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            if trimmed == INSECURE_PLACEHOLDER_TOKEN {
                return Err(anyhow!(
                    "tcp auth token uses insecure placeholder token; set --auth-token or COH_AUTH_TOKEN/COHSH_AUTH_TOKEN"
                ));
            }
            return Ok(trimmed.to_owned());
        }
    }
    Err(anyhow!(
        "tcp auth token must be configured with --auth-token or COH_AUTH_TOKEN/COHSH_AUTH_TOKEN"
    ))
}

fn resolve_rest_url(cli_value: Option<&str>) -> Option<String> {
    if let Some(value) = cli_value {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }
    if let Ok(value) = env::var("COH_REST_URL") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }
    if let Ok(value) = env::var("HIVE_GATEWAY_URL") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }
    None
}

fn resolve_rest_auth_token(cli_value: Option<&str>) -> Option<String> {
    if let Some(value) = cli_value {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }
    for key in [
        "COH_REST_AUTH_TOKEN",
        "COHSH_REST_AUTH_TOKEN",
        "HIVE_GATEWAY_REQUEST_AUTH_TOKEN",
    ] {
        if let Ok(value) = env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

fn connect_console(
    args: &ConnectArgs,
    policy: &CohPolicy,
    role: Role,
    ticket: Option<&str>,
) -> Result<ConsoleSession> {
    let auth_token = resolve_auth_token(args.auth_token.as_deref())?;
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

fn connect_access(
    args: &ConnectArgs,
    policy: &CohPolicy,
    role: Role,
    ticket: Option<&str>,
) -> Result<AccessHandle> {
    if let Some(rest_url) = resolve_rest_url(args.rest_url.as_deref()) {
        if role != Role::Queen {
            return Err(anyhow!("rest transport supports queen role only"));
        }
        let rest_auth_token = resolve_rest_auth_token(args.rest_auth_token.as_deref());
        return Ok(AccessHandle::Rest(RestSession::connect(
            rest_url,
            rest_auth_token,
        )));
    }
    let console = connect_console(args, policy, role, ticket)?;
    Ok(AccessHandle::Console(console))
}

fn publish_gpu_registry<C: coh::CohAccess>(
    client: &mut C,
    registry_root: Option<&PathBuf>,
    mock: bool,
    audit: &mut CohAudit,
) -> Result<()> {
    let bridge = auto_bridge_with_registry(mock, registry_root.map(|path| path.as_path()))?;
    let snapshot = bridge.serialise_namespace()?;
    let publish = build_publish_lines(&snapshot)?;
    let path = "/gpu/bridge/ctl";
    for line in &publish.lines {
        client.write_append(path, line.as_bytes())?;
    }
    let detail = format!(
        "path={path} bytes={} sha256={}",
        publish.bytes.len(),
        publish.sha256
    );
    audit.push_ack(
        cohsh_core::wire::AckStatus::Ok,
        "ECHO",
        Some(detail.as_str()),
    );
    Ok(())
}

fn seed_peft_export_job(server: &NineDoor, job_id: &str) -> Result<()> {
    let telemetry = b"telemetry-v1\\n";
    let base_model = b"vision-base-v1\\n";
    let policy = b"[policy]\\nname = \"default\"\\n";
    server
        .set_lora_export_job(job_id, telemetry, base_model, policy)
        .context("seed mock peft export job")?;
    Ok(())
}
