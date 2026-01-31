// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: CLI entry point for the host-sidecar bridge tool.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Host-sidecar bridge CLI that publishes provider data into `/host`.

use anyhow::{Context, Result};
use clap::{ArgAction, Parser, ValueEnum};
use cohsh::NineDoorTransport;
#[cfg(feature = "tcp")]
use cohsh::{default_policy_path, load_policy, CohshPolicy, Transport};
use host_sidecar_bridge::{default_providers, HostSidecarBridge};
use nine_door::{HostNamespaceConfig, HostProvider, NineDoor};
use std::path::PathBuf;
#[cfg(feature = "tcp")]
use std::thread;
#[cfg(feature = "tcp")]
use std::time::{Duration, Instant};

#[cfg(feature = "tcp")]
use cohsh::TcpTransport;

/// CLI options for the host-sidecar bridge.
#[derive(Debug, Parser)]
#[command(author, version, about = "Cohesix host sidecar bridge")]
struct Args {
    /// Enable deterministic mock mode (in-process NineDoor).
    #[arg(long, action = ArgAction::SetTrue)]
    mock: bool,

    /// Mount point for the /host namespace.
    #[arg(long, default_value = "/host")]
    mount: String,

    /// Provider to publish (repeat for multiple).
    #[arg(long, value_enum)]
    provider: Vec<ProviderArg>,

    /// Path to the manifest-derived cohsh policy TOML (polling defaults).
    #[arg(long, value_name = "FILE")]
    policy: Option<PathBuf>,

    /// Run continuously, polling providers on their configured interval.
    #[arg(long, action = ArgAction::SetTrue)]
    watch: bool,

    /// TCP host for a live NineDoor console (non-mock).
    #[cfg(feature = "tcp")]
    #[arg(long, default_value = "127.0.0.1")]
    tcp_host: String,

    /// TCP port for a live NineDoor console (non-mock).
    #[cfg(feature = "tcp")]
    #[arg(long, default_value_t = cohsh::COHSH_TCP_PORT)]
    tcp_port: u16,

    /// Authentication token for the TCP console (non-mock).
    #[cfg(feature = "tcp")]
    #[arg(long, default_value = "changeme")]
    auth_token: String,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ProviderArg {
    Systemd,
    K8s,
    Docker,
    Nvidia,
    Jetson,
    Net,
}

impl From<ProviderArg> for HostProvider {
    fn from(value: ProviderArg) -> Self {
        match value {
            ProviderArg::Systemd => HostProvider::Systemd,
            ProviderArg::K8s => HostProvider::K8s,
            ProviderArg::Docker => HostProvider::Docker,
            ProviderArg::Nvidia => HostProvider::Nvidia,
            ProviderArg::Jetson => HostProvider::Jetson,
            ProviderArg::Net => HostProvider::Net,
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let providers = if args.provider.is_empty() {
        default_providers()
    } else {
        args.provider.into_iter().map(HostProvider::from).collect()
    };
    let bridge = HostSidecarBridge::new(&args.mount, providers)
        .context("build host sidecar bridge configuration")?;

    if args.mock {
        let host_config = HostNamespaceConfig::enabled(bridge.mount(), bridge.providers())
            .context("configure mock host namespace")?;
        let server = NineDoor::new_with_host_config(host_config);
        let mut transport = NineDoorTransport::new(server);
        bridge
            .publish_mock(&mut transport)
            .context("publish mock provider data")?;
        println!("mock sidecar published providers at {}", bridge.mount());
        return Ok(());
    }

    #[cfg(feature = "tcp")]
    {
        let mut transport =
            TcpTransport::new(args.tcp_host, args.tcp_port).with_auth_token(args.auth_token);
        let session = bridge.attach(&mut transport)?;
        let topology = bridge.discover_topology(&mut transport, &session);
        if args.watch {
            let policy = resolve_policy(args.policy.as_ref())?;
            let mut schedules = build_schedules(bridge.providers(), &policy);
            if schedules.is_empty() {
                anyhow::bail!("no live providers selected for watch mode");
            }
            loop {
                let now = Instant::now();
                let mut next_wake: Option<Instant> = None;
                for schedule in &mut schedules {
                    if now >= schedule.next_due {
                        bridge.publish_live_provider(
                            &mut transport,
                            &session,
                            &topology,
                            schedule.provider,
                        )?;
                        schedule.next_due = now.checked_add(schedule.interval).unwrap_or(now);
                    }
                    next_wake = Some(match next_wake {
                        Some(current) => current.min(schedule.next_due),
                        None => schedule.next_due,
                    });
                }
                if let Some(next_due) = next_wake {
                    let wait = next_due.saturating_duration_since(Instant::now());
                    if !wait.is_zero() {
                        thread::sleep(wait);
                    }
                }
            }
        } else {
            bridge
                .publish_live(&mut transport, &session, &topology)
                .context("publish provider data over tcp")?;
            let _ = transport.quit(&session);
            println!("sidecar published providers at {}", bridge.mount());
            Ok(())
        }
    }

    #[cfg(not(feature = "tcp"))]
    {
        anyhow::bail!("tcp transport disabled; rebuild with --features tcp or use --mock");
    }
}

#[cfg(feature = "tcp")]
#[derive(Debug, Clone, Copy)]
struct ProviderSchedule {
    provider: HostProvider,
    interval: Duration,
    next_due: Instant,
}

#[cfg(feature = "tcp")]
fn resolve_policy(path: Option<&PathBuf>) -> Result<CohshPolicy> {
    let attempted = match path {
        Some(path) => load_policy(path).with_context(|| format!("load policy {}", path.display())),
        None => {
            let path = default_policy_path();
            load_policy(&path).with_context(|| format!("load policy {}", path.display()))
        }
    };
    match attempted {
        Ok(policy) => Ok(policy),
        Err(err) => {
            eprintln!("host-sidecar-bridge: {err} (using generated defaults)");
            Ok(CohshPolicy::from_generated())
        }
    }
}

#[cfg(feature = "tcp")]
fn build_schedules(providers: &[HostProvider], policy: &CohshPolicy) -> Vec<ProviderSchedule> {
    let mut schedules = Vec::new();
    let now = Instant::now();
    for provider in providers {
        let interval_ms = match provider {
            HostProvider::Nvidia => policy.host_telemetry.nvidia_poll_ms,
            HostProvider::Systemd => policy.host_telemetry.systemd_poll_ms,
            HostProvider::Docker => policy.host_telemetry.docker_poll_ms,
            HostProvider::K8s => policy.host_telemetry.k8s_poll_ms,
            HostProvider::Jetson | HostProvider::Net => continue,
        };
        schedules.push(ProviderSchedule {
            provider: *provider,
            interval: Duration::from_millis(interval_ms.max(1)),
            next_due: now,
        });
    }
    schedules
}
