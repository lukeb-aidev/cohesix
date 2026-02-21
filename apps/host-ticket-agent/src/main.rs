// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: CLI entry point for processing host control tickets from /host/tickets/*.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Host ticket agent binary.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{ArgAction, Parser};
use coh::policy::{default_policy_path, load_policy, CohPolicy};
use cohesix_ticket::Role;
use cohsh::{NineDoorTransport, RestTransport, RoleArg, Session, TcpTransport, Transport};
use host_ticket_agent::executors::ExecutorConfig;
use host_ticket_agent::{
    process_tickets_once, relay, unix_time_ms_now, HostTicketManifest, DEFAULT_CURSOR_STATE_PATH,
    DEFAULT_RELAY_WAL_PATH, DEFAULT_RESOLVED_MANIFEST_PATH,
};
use nine_door::{HostNamespaceConfig, HostProvider, HostTicketPolicy, NineDoor};

/// CLI arguments for `host-ticket-agent`.
#[derive(Debug, Parser)]
#[command(author, version, about = "Cohesix host control ticket agent")]
struct Args {
    /// Role used to attach to the namespace.
    #[arg(long, default_value_t = RoleArg::Queen)]
    role: RoleArg,
    /// Optional capability ticket payload.
    #[arg(long)]
    ticket: Option<String>,
    /// Path to `out/manifests/root_task_resolved.json`.
    #[arg(long, value_name = "FILE", default_value = DEFAULT_RESOLVED_MANIFEST_PATH)]
    manifest: PathBuf,
    /// Cursor state file for deterministic resume.
    #[arg(long, value_name = "FILE", default_value = DEFAULT_CURSOR_STATE_PATH)]
    cursor: PathBuf,
    /// Optional mount override (defaults to manifest value).
    #[arg(long, value_name = "PATH")]
    mount: Option<String>,
    /// Poll interval in milliseconds when running continuously.
    #[arg(long, default_value_t = 1000)]
    poll_ms: u64,
    /// Enable federated relay forwarding (`ecosystem.host.federation`).
    #[arg(long, action = ArgAction::SetTrue)]
    relay: bool,
    /// Relay WAL state file for deterministic resume.
    #[arg(long, value_name = "FILE", default_value = DEFAULT_RELAY_WAL_PATH)]
    relay_wal: PathBuf,
    /// Process one pass and exit.
    #[arg(long, action = ArgAction::SetTrue)]
    run_once: bool,
    /// Use mock in-process NineDoor.
    #[arg(long, action = ArgAction::SetTrue)]
    mock: bool,
    /// REST gateway base URL.
    #[arg(long, value_name = "URL")]
    rest_url: Option<String>,
    /// REST request auth token.
    #[arg(long, value_name = "TOKEN")]
    rest_auth_token: Option<String>,
    /// TCP host for direct console mode.
    #[arg(long, default_value = "127.0.0.1")]
    tcp_host: String,
    /// TCP port for direct console mode.
    #[arg(long, default_value_t = cohsh::COHSH_TCP_PORT)]
    tcp_port: u16,
    /// TCP auth token for direct console mode.
    #[arg(long, default_value = "changeme")]
    auth_token: String,
    /// Optional host policy path for PEFT defaults.
    #[arg(long, value_name = "FILE")]
    policy: Option<PathBuf>,
    /// Optional explicit PEFT registry root override.
    #[arg(long, value_name = "DIR")]
    registry_root: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut manifest = HostTicketManifest::from_resolved_manifest(&args.manifest)?;
    if let Some(mount) = args.mount.as_deref() {
        manifest = manifest.with_mount_path(mount)?;
    }
    if !manifest.enabled {
        println!(
            "host-ticket-agent: tickets disabled (manifest={}, mount={})",
            args.manifest.display(),
            manifest.mount_path
        );
        return Ok(());
    }

    let registry_root = args
        .registry_root
        .clone()
        .unwrap_or_else(|| resolve_registry_root(args.policy.as_deref()));
    let executor_config = ExecutorConfig {
        mount: manifest.mount_path.clone(),
        registry_root,
    };

    let role = Role::from(args.role);
    let mut transport = build_transport(&args, &manifest)?;
    let session = transport
        .attach(role, args.ticket.as_deref())
        .context("attach host-ticket-agent session")?;

    loop {
        let now_unix_ms = unix_time_ms_now();
        let pass = process_tickets_once(
            transport.as_mut(),
            &session,
            &manifest,
            &args.cursor,
            &executor_config,
            now_unix_ms,
        );
        match pass {
            Ok(summary) => {
                if summary.seen > 0 || summary.terminal_updates() > 0 {
                    println!(
                        "host-ticket-agent: seen={} succeeded={} failed={} expired={} skipped_terminal={} skipped_remote_target={} cursor={}",
                        summary.seen,
                        summary.succeeded,
                        summary.failed,
                        summary.expired,
                        summary.skipped_terminal,
                        summary.skipped_remote_target,
                        args.cursor.display()
                    );
                }
            }
            Err(err) if args.run_once => return Err(err),
            Err(err) => {
                eprintln!("host-ticket-agent: pass failed: {err}");
            }
        }

        if args.relay {
            let relay_pass =
                relay::relay_once(transport.as_mut(), &session, &manifest, &args.relay_wal);
            match relay_pass {
                Ok(summary) => {
                    if summary.seen > 0
                        || summary.forwarded > 0
                        || summary.remote_write_failures > 0
                        || summary.queue_depth > 0
                    {
                        println!(
                            "host-ticket-agent relay: seen={} candidates={} deduped={} forwarded={} remote_failures={} backpressure={} queue_depth={} wal={}",
                            summary.seen,
                            summary.candidates,
                            summary.deduped,
                            summary.forwarded,
                            summary.remote_write_failures,
                            summary.backpressure_drops,
                            summary.queue_depth,
                            args.relay_wal.display()
                        );
                    }
                }
                Err(err) if args.run_once => return Err(err),
                Err(err) => eprintln!("host-ticket-agent relay: pass failed: {err}"),
            }
        }

        if args.run_once {
            break;
        }
        thread::sleep(Duration::from_millis(args.poll_ms.max(100)));
    }

    let _ = transport.quit(&session);
    Ok(())
}

fn build_transport(args: &Args, manifest: &HostTicketManifest) -> Result<Box<dyn Transport>> {
    if args.mock {
        return build_mock_transport(manifest);
    }
    if let Some(rest_url) = args.rest_url.as_deref() {
        let auth = args
            .rest_auth_token
            .clone()
            .or_else(resolve_rest_auth_token_from_env);
        return Ok(Box::new(RestTransport::new(rest_url, auth)));
    }
    Ok(Box::new(
        TcpTransport::new(args.tcp_host.clone(), args.tcp_port)
            .with_auth_token(args.auth_token.clone()),
    ))
}

fn build_mock_transport(manifest: &HostTicketManifest) -> Result<Box<dyn Transport>> {
    let providers = [
        HostProvider::Systemd,
        HostProvider::K8s,
        HostProvider::Docker,
        HostProvider::Nvidia,
    ];
    let base = HostNamespaceConfig::enabled(manifest.mount_path.as_str(), &providers)
        .context("configure mock host namespace")?;
    let ticket_policy = manifest_ticket_policy(manifest)?;
    let host = base.with_ticket_policy(ticket_policy);
    let server = NineDoor::new_with_host_config(host);
    Ok(Box::new(NineDoorTransport::new(server)))
}

fn manifest_ticket_policy(manifest: &HostTicketManifest) -> Result<HostTicketPolicy> {
    if !manifest.enabled {
        return Ok(HostTicketPolicy::disabled());
    }
    let actions = manifest
        .action_allowlist
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let lifecycle = manifest
        .lifecycle
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    HostTicketPolicy::enabled(
        manifest.request_schema.as_str(),
        manifest.result_schema.as_str(),
        manifest.max_line_bytes,
        actions.as_slice(),
        lifecycle.as_slice(),
    )
    .map_err(|err| anyhow::anyhow!("mock host ticket policy: {err}"))
}

fn resolve_registry_root(policy_path: Option<&Path>) -> PathBuf {
    let path = policy_path
        .map(Path::to_path_buf)
        .unwrap_or_else(default_policy_path);
    match load_policy(path.as_path()) {
        Ok(policy) => PathBuf::from(policy.peft.import.registry_root),
        Err(err) => {
            eprintln!("host-ticket-agent: {} (using generated coh defaults)", err);
            PathBuf::from(CohPolicy::from_generated().peft.import.registry_root)
        }
    }
}

fn resolve_rest_auth_token_from_env() -> Option<String> {
    for key in [
        "HIVE_GATEWAY_REQUEST_AUTH_TOKEN",
        "COHSH_REST_AUTH_TOKEN",
        "COH_REST_AUTH_TOKEN",
    ] {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

#[allow(dead_code)]
fn _ping_transport(transport: &mut dyn Transport, session: &Session) -> Result<String> {
    transport.ping(session)
}
