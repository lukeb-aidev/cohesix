// Author: Lukas Bower
// Purpose: Share the authoritative coh argument schema with desktop forms and the CLI.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
//! The CLI and desktop bridge validate arguments against this one schema.
#![allow(missing_docs)]

use serde::Serialize;
use std::path::PathBuf;

/// Review framing only; scenario selection cannot alter evidence or its authority.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Scenario {
    /// General evidence review.
    #[default]
    Generic,
    /// Incident reconstruction.
    Incident,
    /// Configuration or policy change review.
    Change,
    /// Maintenance and lifecycle review.
    Maintenance,
    /// Model or software rollout review.
    Rollout,
    /// Cross-hive relay review.
    Federation,
}

use clap::{Args, Parser, Subcommand};
use cohesix_net_constants::COHESIX_TCP_CONSOLE_PORT;
use cohsh::RoleArg;

#[derive(Debug, Parser)]
#[command(author = "Lukas Bower", version, about = "Cohesix host bridges")]
pub struct Cli {
    /// Role to use when attaching to Secure9P.
    #[arg(long, default_value_t = RoleArg::Queen)]
    pub role: RoleArg,

    /// Optional capability ticket payload.
    #[arg(long)]
    pub ticket: Option<String>,
    /// Resolve a delegated ticket from env:NAME or file:/absolute/path, never argv bytes.
    #[arg(long, conflicts_with = "ticket")]
    pub ticket_ref: Option<String>,

    /// Path to the manifest-derived coh policy TOML.
    #[arg(long, value_name = "FILE")]
    pub policy: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect a generated workflow and its native owners without executing it.
    Plan(WorkflowArgs),
    /// Admit the next exact workflow ticket; native results advance later calls.
    Apply(WorkflowArgs),
    /// Read current ticket observations and signed terminal evidence once.
    Watch(WorkflowArgs),
    /// Explain generated stages, topology, refusals and compensation boundaries.
    Explain(WorkflowArgs),
    /// Verify every workflow terminal graph using independently enrolled custodians.
    Verify(WorkflowArgs),
    /// Submit separately admitted recovery linked to an original terminal graph.
    Recover(WorkflowArgs),
    /// Print compiler-owned provider and surface contracts without opening a transport.
    Providers,
    /// Operate one durable selected job or administer its narrow standing scope.
    Job(JobArgs),
    /// Inspect or install a digest-pinned user CUDA workload on its GPU host.
    Workload {
        #[command(subcommand)]
        command: WorkloadCommand,
    },
    /// Build, verify or install an exact signed host package without a transport.
    Package {
        #[command(subcommand)]
        command: PackageCommand,
    },
    /// Verify an enrolled subject; optionally issue a gateway-only ticket with an enrolled key.
    Identity {
        #[arg(long)]
        mapping: String,
        /// Pinned public JWKS file; required for JWT mappings. Token is read only from stdin.
        #[arg(long, conflicts_with = "local")]
        jwks: Option<PathBuf>,
        /// Use the kernel effective uid for a generated local mapping.
        #[arg(long)]
        local: bool,
        /// Privileged issuer enrollment (env:NAME or file:/absolute/path); omitted means proposal only.
        #[arg(long)]
        issuer_key_ref: Option<String>,
    },
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

#[derive(Debug, Subcommand)]
pub enum WorkloadCommand {
    /// Measure the selected GPU and report current headroom and admission caps.
    Diagnose {
        #[arg(long, value_name = "FILE")]
        executor_config: PathBuf,
    },
    /// Check package, typed bounds and digest before privileged installation.
    Inspect {
        #[arg(long, value_name = "FILE")]
        registration: PathBuf,
    },
    /// Add one immutable registration under the bridge owner's private state root.
    Register {
        #[arg(long, value_name = "FILE")]
        registration: PathBuf,
        #[arg(long, value_name = "DIR")]
        state_root: PathBuf,
    },
}

#[derive(Debug, Args)]
pub struct WorkflowArgs {
    /// Stable compiler-owned playbook id.
    pub workflow: String,
    /// Exact deployment, ticket requests and independent evidence trust paths.
    #[arg(long)]
    pub deployment: Option<PathBuf>,
    /// Compose a durable CUDA recipe using the same lifecycle commands.
    #[arg(long)]
    pub recipe: bool,
    /// With recover --recipe, request separately authorized cancellation of this stage.
    #[arg(long, requires = "recipe")]
    pub cancel_stage: Option<String>,
    #[command(flatten)]
    pub connect: ConnectArgs,
}

#[derive(Debug, Subcommand)]
pub enum PackageCommand {
    /// Sign the exact compiler-owned artifact inventory and generate its file SBOM.
    Build {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        source_sha256: String,
        #[arg(long)]
        key_id: String,
        /// Explicit env:NAME or file:/absolute/path containing a hex Ed25519 seed.
        #[arg(long)]
        signing_key_ref: String,
    },
    /// Check signature, inventory, architecture, schemas and SBOM without execution.
    Verify {
        #[arg(long)]
        input: PathBuf,
        /// Independently enrolled package trust policy, outside the package.
        #[arg(long)]
        trust: PathBuf,
    },
    /// Install to a new directory on the matching host; service activation is separate.
    Install {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        trust: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Debug, Parser)]
pub struct InspectArgs {
    #[command(flatten)]
    pub connect: ConnectArgs,
    /// Canonical evidence-pack directory. Offline mode opens no transport.
    #[arg(long, value_name = "DIR")]
    pub input: Option<PathBuf>,
    /// Emit stable JSON instead of escaped structured text.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Parser)]
pub struct AttestArgs {
    #[command(flatten)]
    pub inspect: InspectArgs,
    /// Independently enrolled trust policy; never inferred from target evidence.
    #[arg(long, value_name = "FILE")]
    pub trust_policy: Option<PathBuf>,
    /// Retain the public request and signed response for offline verification.
    #[arg(long, value_name = "FILE", conflicts_with = "input")]
    pub record: Option<PathBuf>,
}

#[derive(Debug, Parser)]
pub struct DiffArgs {
    /// Pack directory, tcp://host:port, or http(s)://gateway URL.
    #[arg(long)]
    pub left: String,
    /// Pack directory, tcp://host:port, or http(s)://gateway URL.
    #[arg(long)]
    pub right: String,
    /// TCP authentication token; environment resolution matches other coh commands.
    #[arg(long)]
    pub auth_token: Option<String>,
    /// REST request authentication token.
    #[arg(long)]
    pub rest_auth_token: Option<String>,
}

#[derive(Debug, Parser)]
pub struct JobArgs {
    #[command(flatten)]
    pub connect: ConnectArgs,
    #[command(subcommand)]
    pub command: JobCommand,
}

#[derive(Debug, Subcommand)]
pub enum JobCommand {
    /// Submit an exact JSON binding and raw ticket; uncertain replies need reconcile.
    Submit {
        /// Bounded JSON file containing binding and ticket objects.
        #[arg(long)]
        input: PathBuf,
    },
    /// Start a configured standing service recipe with a stable retry identity.
    StartApproved {
        #[arg(long)]
        scope_id: String,
        #[arg(long)]
        request_id: String,
    },
    /// Read retained execution and independent delivery state.
    Status { admission_id: String },
    /// Request cancellation without claiming native termination.
    Cancel { admission_id: String },
    /// Inspect target results for one identity without replaying its effect.
    Reconcile { admission_id: String },
    /// Inspect standing scope spending under a separate admin ticket.
    InspectScope { scope_id: String },
    /// Revoke future effects under a separate admin ticket.
    RevokeScope { scope_id: String },
}

#[derive(Debug, Parser)]
pub struct ConnectArgs {
    /// Secure9P host.
    #[arg(long, default_value = "127.0.0.1", global = true)]
    pub host: String,
    /// Secure9P port.
    #[arg(long, default_value_t = COHESIX_TCP_CONSOLE_PORT, global = true)]
    pub port: u16,
    /// REST gateway base URL for hive-gateway (optional).
    #[arg(long, value_name = "URL", global = true)]
    pub rest_url: Option<String>,
    /// Request auth token for REST mutating routes.
    #[arg(long, value_name = "TOKEN", global = true)]
    pub rest_auth_token: Option<String>,
    /// TCP console auth token.
    #[arg(long, global = true)]
    pub auth_token: Option<String>,
    /// Use the in-process mock backend.
    #[arg(long, default_value_t = false, global = true)]
    pub mock: bool,
}

#[derive(Debug, Parser)]
pub struct DoctorArgs {
    #[command(flatten)]
    pub connect: ConnectArgs,
    /// This host owns GPU execution; explicitly probe its local NVML/CUDA providers.
    #[arg(long)]
    pub local_gpu: bool,
    /// Selected local GPU executor configuration for fresh workload diagnostics.
    #[arg(long, requires = "local_gpu", value_name = "FILE")]
    pub gpu_executor_config: Option<PathBuf>,
    /// This deployment requires a usable native FUSE mount.
    #[arg(long)]
    pub require_fuse: bool,
    /// This is a development host requiring QEMU.
    #[arg(long)]
    pub developer_tools: bool,
    /// Verify this exact signed installation and its enrolled credential references.
    #[arg(long, requires_all = ["package_trust", "credential_refs"])]
    pub package: Option<PathBuf>,
    /// Independent package signer trust policy.
    #[arg(long, requires = "package")]
    pub package_trust: Option<PathBuf>,
    /// JSON map of generated credential names to explicit env:/file: references.
    #[arg(long, requires = "package")]
    pub credential_refs: Option<PathBuf>,
}

#[derive(Debug, Parser)]
pub struct MountArgs {
    #[command(flatten)]
    pub connect: ConnectArgs,
    /// Mount point on the host filesystem.
    #[arg(long, value_name = "DIR")]
    pub at: PathBuf,
}

#[derive(Debug, Parser)]
pub struct GpuArgs {
    #[command(flatten)]
    pub connect: ConnectArgs,
    /// Use the NVML backend when available.
    #[arg(long, default_value_t = false)]
    pub nvml: bool,
    #[command(subcommand)]
    pub command: GpuCommand,
}

#[derive(Debug, Parser)]
pub struct PeftArgs {
    #[command(flatten)]
    pub connect: ConnectArgs,
    #[command(subcommand)]
    pub command: PeftCommand,
}

#[derive(Debug, Subcommand)]
pub enum PeftCommand {
    /// Plan, apply or verify a private adapter release through an admitted host ticket.
    Release {
        #[arg(value_parser = ["plan", "apply", "watch", "explain", "verify", "recover"])]
        mode: String,
        #[arg(long, value_name = "FILE")]
        deployment: PathBuf,
    },
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
pub enum GpuCommand {
    /// List GPUs.
    List,
    /// Show GPU status.
    Status {
        #[arg(long)]
        gpu: String,
    },
    /// Request a GPU lease via /queen/ctl.
    Lease(GpuLeaseArgs),
    /// Submit, cancel, or observe a workload through a root-admitted WorkerGpu ticket.
    Workload {
        #[arg(long, value_parser = ["gpu.workload.submit", "gpu.workload.cancel", "gpu.workload.observe"])]
        action: String,
        #[arg(long)]
        spec: PathBuf,
    },
}

#[derive(Debug, Parser)]
pub struct GpuLeaseArgs {
    /// GPU identifier.
    #[arg(long)]
    pub gpu: String,
    /// Memory requested in MiB.
    #[arg(long)]
    pub mem_mb: u32,
    /// Stream count requested.
    #[arg(long)]
    pub streams: u8,
    /// Lease TTL in seconds.
    #[arg(long)]
    pub ttl_s: u32,
    /// Optional scheduling priority.
    #[arg(long)]
    pub priority: Option<u8>,
    /// Optional budget TTL override.
    #[arg(long)]
    pub budget_ttl_s: Option<u64>,
    /// Optional budget ops override.
    #[arg(long)]
    pub budget_ops: Option<u64>,
    /// Optional non-authoritative operation report output path.
    #[arg(
        long = "report-out",
        visible_alias = "receipt-out",
        value_name = "FILE"
    )]
    pub receipt_out: Option<PathBuf>,
}

#[derive(Debug, Parser)]
pub struct RunArgs {
    #[command(flatten)]
    pub connect: ConnectArgs,
    /// GPU identifier.
    #[arg(long)]
    pub gpu: String,
    /// Optional non-authoritative operation report output path.
    #[arg(
        long = "report-out",
        visible_alias = "receipt-out",
        value_name = "FILE"
    )]
    pub receipt_out: Option<PathBuf>,
    /// Command to execute (pass after `--`).
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "CMD"
    )]
    pub command: Vec<String>,
}

#[derive(Debug, Parser)]
pub struct TelemetryArgs {
    #[command(flatten)]
    pub connect: ConnectArgs,
    #[command(subcommand)]
    pub command: TelemetryCommand,
}

#[derive(Debug, Parser)]
pub struct EvidenceArgs {
    #[command(subcommand)]
    pub command: EvidenceCommand,
}

#[derive(Debug, Parser)]
pub struct FleetArgs {
    #[command(flatten)]
    pub connect: ConnectArgs,
    /// Repeatable hive targets in `name=url` form.
    #[arg(long = "hive", value_name = "NAME=URL")]
    pub hives: Vec<String>,
    #[command(subcommand)]
    pub command: FleetCommand,
}

#[derive(Debug, Subcommand)]
pub enum FleetCommand {
    /// Read `/proc/lifecycle/state` and `/proc/root/reachable` across hives.
    Status,
    /// Read `/proc/lease/summary` across hives.
    LeaseSummary,
    /// Read `/proc/pressure/*` across hives.
    Pressure,
}

#[derive(Debug, Subcommand)]
pub enum EvidenceCommand {
    /// Correlate a separately admitted recovery ticket with its exact original terminal.
    VerifyRecovery {
        #[arg(long)]
        original: PathBuf,
        #[arg(long)]
        original_trust: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        trust: PathBuf,
        #[arg(long)]
        cas: PathBuf,
    },
    /// Deliver one verified SIEM projection with durable retry state and an exact receiver ACK.
    Deliver {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        trust: PathBuf,
        #[arg(long)]
        cas: PathBuf,
        #[arg(long)]
        state_dir: PathBuf,
    },
    /// Export a derived projection after verifying signed causal evidence and CAS.
    Export {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        trust: PathBuf,
        #[arg(long)]
        cas: PathBuf,
        #[arg(long)]
        format: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Inspect the verified causal graph and its bounded, redacted JSON artifact observations.
    Story {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        trust: PathBuf,
        #[arg(long)]
        cas: PathBuf,
    },
    /// Verify signed causal records and CAS objects against a separate local trust file.
    Verify {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        trust: PathBuf,
        #[arg(long)]
        cas: PathBuf,
    },
    /// Export a deterministic evidence pack directory.
    Pack(Box<EvidencePackArgs>),
    /// Generate `timeline.ndjson` and `timeline.md` from an evidence pack.
    Timeline {
        #[arg(long, value_name = "DIR", alias = "in")]
        input: PathBuf,
        /// Add a sanitized recipe diagnostic, including to a retained partial pack.
        #[arg(long)]
        recipe_report: Option<PathBuf>,
        /// Review framing only; never changes evidence authority.
        #[arg(long, value_enum, default_value = "generic")]
        scenario: Scenario,
    },
}

#[derive(Debug, Parser)]
pub struct EvidencePackArgs {
    /// Bounded non-authoritative recipe report to explain in the canonical case.
    #[arg(long)]
    pub recipe_report: Option<PathBuf>,
    /// Signed causal graph; requires separately configured trust and immutable CAS.
    #[arg(long, requires_all = ["evidence_trust", "evidence_cas"])]
    pub causal_graph: Option<PathBuf>,
    #[arg(long, requires = "causal_graph")]
    pub evidence_trust: Option<PathBuf>,
    #[arg(long, requires = "causal_graph")]
    pub evidence_cas: Option<PathBuf>,
    #[command(flatten)]
    pub connect: ConnectArgs,
    /// Output directory for the evidence pack.
    #[arg(long, value_name = "DIR")]
    pub out: PathBuf,
    /// Include telemetry pulls under `telemetry/` inside the pack.
    #[arg(long, default_value_t = false)]
    pub with_telemetry: bool,
    /// Host copy of the source manifest (secret fields are redacted).
    #[arg(long, value_name = "FILE")]
    pub manifest: Option<PathBuf>,
    /// Host copy of the selected resolved manifest.
    #[arg(long, value_name = "FILE")]
    pub resolved_manifest: Option<PathBuf>,
    /// Bounded host serial excerpt; authentication and unstructured payloads are withheld.
    #[arg(long, value_name = "FILE")]
    pub serial_log: Option<PathBuf>,
    /// Canonical redacted live trace to include; legacy traces remain offline fixtures.
    #[arg(long, value_name = "FILE")]
    pub trace: Option<PathBuf>,
    /// Public challenge and signed response retained by coh attest --record.
    #[arg(long, value_name = "FILE")]
    pub attestation_record: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum TelemetryCommand {
    /// Pull telemetry bundles from /queen/telemetry.
    Pull {
        #[arg(long, value_name = "DIR")]
        out: PathBuf,
    },
}

/// Return the owning parser's form schema. This does not grant execution authority.
pub fn ui_schema() -> serde_json::Value {
    use clap::CommandFactory;
    fn node(command: &clap::Command) -> serde_json::Value {
        let fields: Vec<_> = command.get_arguments().filter(|a| !a.is_hide_set()).map(|a| {
            let choices: Vec<_> = a.get_value_parser().possible_values()
                .map(|values| values.filter(|v| !v.is_hide_set()).map(|v| v.get_name().to_owned()).collect())
                .unwrap_or_default();
            serde_json::json!({
                "id": a.get_id().as_str(), "long": a.get_long(), "index": a.get_index(),
                "help": a.get_help().map(ToString::to_string).unwrap_or_default(),
                "required": a.is_required_set(), "global": a.is_global_set(),
                "flag": matches!(a.get_action(), clap::ArgAction::SetTrue | clap::ArgAction::SetFalse),
                "multiple": matches!(a.get_action(), clap::ArgAction::Append),
                "choices": choices,
                "value_names": a.get_value_names().map(|names| names.iter().map(ToString::to_string).collect::<Vec<_>>()).unwrap_or_default(),
                "path": a.get_value_parser().type_id() == clap::builder::ValueParser::path_buf().type_id(),
                "default": a.get_default_values().iter().map(|s| s.to_string_lossy().into_owned()).collect::<Vec<_>>()
            })
        }).collect();
        serde_json::json!({"name":command.get_name(), "help":command.get_about().map(ToString::to_string).unwrap_or_default(),
            "fields":fields,"commands":command.get_subcommands().filter(|c| !c.is_hide_set() && c.get_name() != "help").map(node).collect::<Vec<_>>()})
    }
    let mut command = Cli::command();
    command.build();
    node(&command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_job_commands_share_one_rest_connection_schema() {
        let cli = Cli::try_parse_from([
            "coh",
            "--ticket-ref",
            "env:JOB_TICKET",
            "job",
            "--rest-url",
            "http://127.0.0.1:8080",
            "submit",
            "--input",
            "/tmp/job.json",
        ])
        .expect("selected job command");
        assert!(matches!(
            cli.command,
            Command::Job(JobArgs {
                command: JobCommand::Submit { .. },
                ..
            })
        ));
        assert!(Cli::try_parse_from(["coh", "job", "cancel", "../job"]).is_ok());
        assert!(matches!(
            Cli::try_parse_from([
                "coh",
                "job",
                "start-approved",
                "--scope-id",
                "service-1",
                "--request-id",
                "run-123"
            ])
            .expect("approved recipe command")
            .command,
            Command::Job(JobArgs {
                command: JobCommand::StartApproved { .. },
                ..
            })
        ));
        // The runtime validates ids before constructing a URL; the parser
        // preserves the original bytes for that deterministic refusal.
    }

    #[test]
    fn registered_workload_controls_require_explicit_local_paths() {
        let parsed = Cli::try_parse_from([
            "coh",
            "workload",
            "register",
            "--registration",
            "/tmp/registration.json",
            "--state-root",
            "/tmp/executor",
        ])
        .expect("privileged registration paths");
        assert!(matches!(
            parsed.command,
            Command::Workload {
                command: WorkloadCommand::Register { .. }
            }
        ));
        assert!(Cli::try_parse_from([
            "coh",
            "workload",
            "register",
            "--registration",
            "/tmp/registration.json",
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "coh",
            "workload",
            "diagnose",
            "--executor-config",
            "/tmp/executor.json",
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "coh",
            "doctor",
            "--gpu-executor-config",
            "/tmp/executor.json",
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "coh",
            "doctor",
            "--local-gpu",
            "--gpu-executor-config",
            "/tmp/executor.json",
        ])
        .is_ok());
    }
}
