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
use clap::Parser;
use coh::cli::*;
use coh::console::ConsoleSession;
use coh::policy::{default_policy_path, load_policy, CohPolicy};
use coh::rest::RestSession;
use coh::{
    doctor, evidence, evidence_timeline, fleet, gpu, mount, operator, peft, run as coh_run,
    telemetry, CohAccess, CohAudit,
};
use cohesix_ticket::Role;
use cohsh::client::{CohClient, InProcessTransport};
use gpu_bridge_host::{auto_bridge, auto_bridge_with_registry, build_publish_lines};
use nine_door::NineDoor;

fn main() -> Result<()> {
    if std::env::args().skip(1).eq(["--ui-schema"]) {
        println!("{}", coh::cli::ui_schema());
        return Ok(());
    }
    let desktop_args: Vec<_> = std::env::args().skip(1).collect();
    if desktop_args
        .first()
        .is_some_and(|arg| arg == "--ui-read-report")
    {
        anyhow::ensure!(
            desktop_args.len() == 2,
            "ui-report requires one artifact path"
        );
        let path = Path::new(&desktop_args[1]);
        anyhow::ensure!(path.is_absolute(), "ui-report requires an absolute path");
        let bytes = operator::read_bounded(path, 1024 * 1024)?;
        let clean = operator::sanitize(&bytes)?;
        let mut value: serde_json::Value = serde_json::from_str(&clean)?;
        operator::redact_display_content(&mut value);
        println!(
            "{}",
            serde_json::json!({"source":path,"sha256":operator::digest(&bytes),"proof":"unverified host report; verify with independently enrolled trust","report":value})
        );
        return Ok(());
    }
    let mut cli = Cli::parse();
    if let Some(reference) = &cli.ticket_ref {
        cli.ticket = Some(cohesix_authority::secret::resolve_reference(reference)?);
    }
    let policy_path = resolve_policy_path(cli.policy)?;
    let role = Role::from(cli.role);
    match cli.command {
        Command::Plan(args) => {
            if args.recipe {
                anyhow::ensure!(args.workflow == "cuda-reference", "not_registered recipe");
                let report = if let Some(path) = &args.deployment {
                    coh::recipe::plan(&coh::recipe::load(path)?)?
                } else {
                    coh::recipe::contract()?
                };
                println!("{}", serde_json::to_string_pretty(&report)?);
                return Ok(());
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&coh::workflow::plan(&args.workflow)?)?
            );
            Ok(())
        }
        Command::Explain(args) => {
            let report = if args.recipe {
                anyhow::ensure!(args.workflow == "cuda-reference", "not_registered recipe");
                if let Some(path) = &args.deployment {
                    coh::recipe::inspect(
                        &coh::recipe::load(path)?,
                        gpu_bridge_host::workload::now_ms()?,
                    )?
                } else {
                    coh::recipe::contract()?
                }
            } else {
                coh::workflow::plan(&args.workflow)?
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::Apply(args) => {
            run_workflow("apply", role, cli.ticket.as_deref(), &policy_path, args)
        }
        Command::Watch(args) => {
            run_workflow("watch", role, cli.ticket.as_deref(), &policy_path, args)
        }
        Command::Verify(args) => {
            run_workflow("verify", role, cli.ticket.as_deref(), &policy_path, args)
        }
        Command::Recover(args) => {
            run_workflow("recover", role, cli.ticket.as_deref(), &policy_path, args)
        }

        Command::Workload { command } => {
            let report = match command {
                WorkloadCommand::Diagnose { executor_config } => {
                    gpu_bridge_host::registered::diagnose(&executor_config)?
                }
                WorkloadCommand::Inspect { registration } => {
                    gpu_bridge_host::registered::inspect(&registration)?
                }
                WorkloadCommand::Register {
                    registration,
                    state_root,
                } => {
                    let digest = gpu_bridge_host::registered::install(&registration, &state_root)?;
                    serde_json::json!({
                        "schema":"cohesix-cuda-registration-install/v1",
                        "registration_sha256":digest,
                        "state_root":state_root,
                        "authoritative":false,
                        "execution":"not_started",
                    })
                }
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::Package { command } => {
            let report = match command {
                PackageCommand::Build {
                    input,
                    out,
                    profile,
                    source_sha256,
                    key_id,
                    signing_key_ref,
                } => coh::package::build(
                    &input,
                    &out,
                    &profile,
                    &source_sha256,
                    &key_id,
                    &signing_key_ref,
                )?,
                PackageCommand::Verify { input, trust } => coh::package::verify(
                    &input,
                    &coh::package::load_external_trust(&input, &trust)?,
                )?,
                PackageCommand::Install { input, trust, out } => coh::package::install(
                    &input,
                    &out,
                    &coh::package::load_external_trust(&input, &trust)?,
                )?,
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::Providers => {
            print!("{}", cohesix_authority::provider::registry_json());
            Ok(())
        }
        Command::Job(args) => run_selected_job(args, role, cli.ticket.as_deref()),
        Command::Identity {
            mapping,
            jwks,
            local,
            issuer_key_ref,
        } => {
            use std::io::Read;
            let (mapping, actions, graph) = cohesix_identity::generated_policy(&mapping)?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            let request = if local {
                cohesix_identity::map_local(&mapping, now, &graph, &actions)?
            } else {
                let path = jwks.ok_or_else(|| {
                    anyhow!("JWT mapping requires --jwks; token is read from stdin")
                })?;
                let mut keys = Vec::new();
                std::fs::File::open(path)?
                    .take((cohesix_identity::MAX_KEYSET_BYTES + 1) as u64)
                    .read_to_end(&mut keys)?;
                let mut token = Vec::new();
                std::io::stdin()
                    .take((cohesix_identity::MAX_TOKEN_BYTES + 2) as u64)
                    .read_to_end(&mut token)?;
                if token.last() == Some(&b'\n') {
                    token.pop();
                }
                cohesix_identity::map_jwt(&mapping, &token, &keys, now, &graph, &actions)?
            };
            if let Some(reference) = issuer_key_ref {
                let secret = cohesix_authority::secret::resolve_reference(&reference)?;
                let issuer = cohesix_identity::delegation::gateway_issuer(&secret);
                let ticket = request.issue(&issuer)?;
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "event":"identity-issuance", "mapping_id":mapping.id,
                        "credential_sha256":request.credential_sha256(),
                        "provider_graph_sha256":graph, "result":"issued: gateway_enforced"
                    })
                );
                println!(
                    "{}",
                    serde_json::json!({
                        "schema":"cohesix-identity-exchange/v1", "status":"OK",
                        "authoritative":false, "identity_class":"gateway_enforced",
                        "mapping_id":mapping.id, "provider_graph_sha256":graph,
                        "credential_sha256":request.credential_sha256(),
                        "expires_unix_s":request.expires_unix_s(), "ticket":ticket
                    })
                );
            } else {
                println!("{}", serde_json::to_string(&request)?);
            }
            Ok(())
        }
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
        local_gpu: args.local_gpu,
        gpu_executor_config: args.gpu_executor_config,
        require_fuse: args.require_fuse,
        developer_tools: args.developer_tools,
        package: match (args.package, args.package_trust, args.credential_refs) {
            (Some(root), Some(trust), Some(credential_refs)) => Some(doctor::PackageConfig {
                root,
                trust,
                credential_refs,
            }),
            (None, None, None) => None,
            _ => {
                return Err(anyhow!(
                    "package doctor requires package, trust and credential references"
                ))
            }
        },
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
        let client = RestSession::connect(rest_url.as_str(), rest_auth_token)
            .with_optional_delegated_ticket(ticket);
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
    if matches!(args.command, GpuCommand::Workload { .. }) && (args.connect.mock || args.nvml) {
        return Err(anyhow!(
            "GPU workload tickets require an explicit live target connection"
        ));
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
            GpuCommand::Workload { action, spec } => {
                let bytes = gpu_bridge_host::workload::read_file(&spec, 2048)?;
                gpu::workload_ticket(&mut client, &mut audit, &action, &bytes)
            }
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
                gpu::lease_with_report(
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
            GpuCommand::Workload { action, spec } => {
                let bytes = gpu_bridge_host::workload::read_file(&spec, 2048)?;
                gpu::workload_ticket(&mut client, &mut audit, &action, &bytes)
            }
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
                gpu::lease_with_report(
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
        let result = coh_run::execute_with_report(
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
        let result = coh_run::execute_with_report(
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
        PeftCommand::Release { mode, deployment } => {
            anyhow::ensure!(!args.connect.mock, "EPERM native-release-mock");
            let deployment = peft::controller::load(&deployment)?;
            let now = u64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_millis(),
            )?;
            let report = if mode == "apply" {
                let mut client = connect_access(&args.connect, policy, role, ticket)?;
                peft::controller::advance(&deployment, &mode, Some(&mut client), now)?
            } else {
                peft::controller::advance(&deployment, &mode, None, now)?
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
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

fn run_workflow(
    verb: &str,
    role: Role,
    ticket: Option<&str>,
    policy_path: &Path,
    args: WorkflowArgs,
) -> Result<()> {
    anyhow::ensure!(
        !args.connect.mock,
        "not_supported workflow-mock-use-explicit-rehearsal"
    );
    let path = args
        .deployment
        .ok_or_else(|| anyhow!("not_enabled workflow-deployment"))?;
    if args.recipe {
        anyhow::ensure!(args.workflow == "cuda-reference", "not_registered recipe");
        let deployment = coh::recipe::load(&path)?;
        let now = gpu_bridge_host::workload::now_ms()?;
        let report = if verb == "verify" || verb == "watch" {
            coh::recipe::inspect(&deployment, now)?
        } else {
            let policy = load_policy(policy_path)?;
            let mut access = connect_access(&args.connect, &policy, role, ticket)?;
            coh::recipe::advance(
                &mut access,
                &deployment,
                now,
                verb == "recover",
                args.cancel_stage.as_deref(),
            )?
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        anyhow::ensure!(
            verb != "verify" || report["all_steps_verified"] == true,
            "unverified recipe-terminal"
        );
        return Ok(());
    }
    let deployment = coh::workflow::load(&path, &args.workflow)?;
    let result = if verb == "verify" {
        let report = coh::workflow::inspect(&deployment)?;
        anyhow::ensure!(
            report["all_steps_verified"] == true,
            "unverified workflow-terminal"
        );
        report
    } else {
        let policy = load_policy(policy_path)?;
        let mut access = connect_access(&args.connect, &policy, role, ticket)?;
        match verb {
            "apply" => coh::workflow::apply(&mut access, &deployment)?,
            "watch" => coh::workflow::watch(&mut access, &deployment)?,
            "recover" => coh::workflow::recover(&mut access, &deployment)?,
            _ => return Err(anyhow!("unsupported workflow lifecycle")),
        }
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
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
        EvidenceCommand::VerifyRecovery {
            original,
            original_trust,
            input,
            trust,
            cas,
        } => {
            let original = operator::read_bounded(&original, cohesix_evidence::MAX_GRAPH_BYTES)?;
            let original_trust: cohesix_evidence::Trust =
                serde_json::from_slice(&operator::read_bounded(&original_trust, 65_536)?)?;
            let original = cohesix_evidence::verify(&original, &original_trust, |artifact| {
                cohesix_evidence::verify_cas(&cas, artifact)
            })?;
            let bytes = operator::read_bounded(&input, cohesix_evidence::MAX_GRAPH_BYTES)?;
            let trust: cohesix_evidence::Trust =
                serde_json::from_slice(&operator::read_bounded(&trust, 65_536)?)?;
            let recovery =
                cohesix_evidence::verify_recovery(&original, &bytes, &trust, |artifact| {
                    cohesix_evidence::verify_cas(&cas, artifact)
                })?;
            println!("{}", serde_json::to_string(&recovery)?);
            Ok(())
        }
        EvidenceCommand::Deliver {
            input,
            trust,
            cas,
            state_dir,
        } => {
            let bytes = operator::read_bounded(&input, cohesix_evidence::MAX_GRAPH_BYTES)?;
            let trust: cohesix_evidence::Trust =
                serde_json::from_slice(&operator::read_bounded(&trust, 65_536)?)?;
            let verified = cohesix_evidence::verify(&bytes, &trust, |artifact| {
                cohesix_evidence::verify_cas(&cas, artifact)
            })?;
            let now = u64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_millis(),
            )?;
            println!(
                "{}",
                serde_json::to_string(&coh::export::delivery::deliver(
                    &verified, &state_dir, now
                )?)?
            );
            Ok(())
        }
        EvidenceCommand::Export {
            input,
            trust,
            cas,
            format,
            out,
        } => {
            let bytes = operator::read_bounded(&input, cohesix_evidence::MAX_GRAPH_BYTES)?;
            let trust: cohesix_evidence::Trust =
                serde_json::from_slice(&operator::read_bounded(&trust, 65_536)?)?;
            let verified = cohesix_evidence::verify(&bytes, &trust, |artifact| {
                cohesix_evidence::verify_cas(&cas, artifact)
            })?;
            operator::write_atomic(&out, &coh::export::render(&verified, &format)?)?;
            Ok(())
        }
        EvidenceCommand::Story { input, trust, cas } => {
            println!("{}", operator::story(&input, &trust, &cas)?);
            Ok(())
        }
        EvidenceCommand::Verify { input, trust, cas } => {
            let bytes = operator::read_bounded(&input, cohesix_evidence::MAX_GRAPH_BYTES)?;
            let trust_bytes = operator::read_bounded(&trust, 65_536)?;
            let trust: cohesix_evidence::Trust = serde_json::from_slice(&trust_bytes)?;
            let verified = cohesix_evidence::verify(&bytes, &trust, |artifact| {
                cohesix_evidence::verify_cas(&cas, artifact)
            })?;
            println!("{}", serde_json::to_string(&verified)?);
            Ok(())
        }
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
                if let Some(report) = &pack.recipe_report {
                    coh::recipe::attach_diagnostic(&spec.out_dir, report)?;
                }
                if let (Some(graph), Some(trust), Some(cas)) =
                    (&pack.causal_graph, &pack.evidence_trust, &pack.evidence_cas)
                {
                    evidence::attach_causal_graph(&spec.out_dir, graph, trust, cas)?;
                }
                let digest = operator::read_bounded(&spec.out_dir.join("pack.sha256"), 65)?;
                audit.push_line(format!(
                    "evidence pack sha256={}",
                    std::str::from_utf8(&digest)?.trim()
                ));
            }
            handle_result(result, audit, "EVIDENCE")
        }
        EvidenceCommand::Timeline {
            input,
            recipe_report,
            scenario,
        } => {
            let mut audit = CohAudit::new();
            if let Some(report) = recipe_report {
                coh::recipe::attach_diagnostic(&input, &report)?;
            }
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

fn run_selected_job(args: JobArgs, role: Role, ticket: Option<&str>) -> Result<()> {
    anyhow::ensure!(
        role == Role::Queen && !args.connect.mock,
        "selected jobs require an authenticated Queen REST gateway"
    );
    let url = resolve_rest_url(args.connect.rest_url.as_deref())
        .context("selected jobs require --rest-url or COH_REST_URL")?;
    let mut client = cohesix_rest::GatewayClient::new(url);
    if let Some(auth) = resolve_rest_auth_token(args.connect.rest_auth_token.as_deref()) {
        client = client.with_request_auth_token(auth);
    }
    if let Some(ticket) = ticket {
        client = client.with_delegated_ticket(ticket);
    }
    let report = match args.command {
        JobCommand::Submit { input } => {
            use std::io::Read;
            anyhow::ensure!(
                input.is_file() && !input.is_symlink(),
                "selected job input must be a regular file"
            );
            let mut bytes = Vec::new();
            std::fs::File::open(&input)?
                .take(4097)
                .read_to_end(&mut bytes)?;
            anyhow::ensure!(bytes.len() <= 4096, "ELIMIT selected job input");
            let request: serde_json::Value = serde_json::from_slice(&bytes)?;
            client.submit_selected_job(&request)?
        }
        JobCommand::Status { admission_id } => client.selected_job_status(&admission_id)?,
        JobCommand::Cancel { admission_id } => client.request_selected_job_cancel(&admission_id)?,
        JobCommand::Reconcile { admission_id } => client.reconcile_selected_job(&admission_id)?,
        JobCommand::InspectScope { scope_id } => client.inspect_standing_scope(&scope_id)?,
        JobCommand::RevokeScope { scope_id } => client.revoke_standing_scope(&scope_id)?,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
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
        return Ok(AccessHandle::Rest(
            RestSession::connect(rest_url, rest_auth_token).with_optional_delegated_ticket(ticket),
        ));
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
