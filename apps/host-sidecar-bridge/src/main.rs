// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: CLI entry point for the host-sidecar bridge tool.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Host-sidecar bridge CLI that publishes provider data into `/host`.

use anyhow::{Context, Result};
use clap::{ArgAction, Parser, ValueEnum};
use cohsh::NineDoorTransport;
#[cfg(any(feature = "rest", feature = "tcp"))]
use cohsh::{default_policy_path, load_policy, CohshPolicy, Transport};
use host_sidecar_bridge::{default_providers, HostSidecarBridge};
use nine_door::{HostNamespaceConfig, HostProvider, NineDoor};
#[cfg(any(feature = "rest", feature = "tcp"))]
use std::env;
use std::path::PathBuf;
#[cfg(any(feature = "rest", feature = "tcp"))]
use std::thread;
#[cfg(any(feature = "rest", feature = "tcp"))]
use std::time::{Duration, Instant};

#[cfg(any(feature = "rest", feature = "tcp"))]
use cohsh::RestTransport;
#[cfg(feature = "tcp")]
use cohsh::TcpTransport;

/// CLI options for the host-sidecar bridge.
#[derive(Debug, Parser)]
#[command(author, version, about = "Cohesix host sidecar bridge")]
struct Args {
    /// Read one native provider without publishing or claiming execution.
    #[arg(long, value_enum, conflicts_with_all = ["mock", "watch", "rest_url", "native_systemd_unit", "native_systemd_discover", "native_kubernetes_nodes"])]
    native_provider: Option<ProviderArg>,

    /// Discover bounded native Cohesix/SSH systemd unit identities without a target transport.
    #[arg(long, conflicts_with_all = ["native_systemd_unit", "mock", "watch", "rest_url", "native_kubernetes_nodes"])]
    native_systemd_discover: bool,
    /// Discover bounded native Kubernetes node identities from an enrolled HTTPS API.
    #[arg(long, conflicts_with_all = ["native_systemd_unit", "mock", "watch", "rest_url"])]
    native_kubernetes_nodes: bool,
    /// Read exact systemd D-Bus identity without opening a target transport.
    #[arg(long, value_name = "UNIT", conflicts_with_all = ["mock", "watch", "rest_url"])]
    native_systemd_unit: Option<String>,
    /// Enable deterministic mock mode (in-process NineDoor).
    #[arg(long, action = ArgAction::SetTrue)]
    mock: bool,

    /// Exact generated host publisher identity (required outside mock/diagnostic mode).
    #[arg(long)]
    source_id: Option<String>,

    /// Existing private directory for the durable publisher cursor.
    #[arg(long)]
    state_dir: Option<PathBuf>,

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

    /// REST gateway base URL for hive-gateway publish mode.
    #[arg(long, value_name = "URL")]
    rest_url: Option<String>,

    /// Request auth token for REST write operations (header auth at hive-gateway edge).
    #[arg(long, value_name = "TOKEN")]
    rest_auth_token: Option<String>,

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
    #[value(alias = "net")]
    Network,
    Launchd,
    EndpointCompliance,
    Modbus,
    Dnp3,
}

impl ProviderArg {
    fn id(self) -> &'static str {
        match self {
            Self::Systemd => "systemd",
            Self::K8s => "k8s",
            Self::Docker => "docker",
            Self::Nvidia => "nvidia",
            Self::Jetson => "jetson",
            Self::Network => "network",
            Self::Launchd => "launchd",
            Self::EndpointCompliance => "endpoint_compliance",
            Self::Modbus => "modbus",
            Self::Dnp3 => "dnp3",
        }
    }
    fn mock_provider(self) -> Result<HostProvider> {
        Ok(match self {
            Self::Systemd => HostProvider::Systemd,
            Self::K8s => HostProvider::K8s,
            Self::Docker => HostProvider::Docker,
            Self::Nvidia => HostProvider::Nvidia,
            Self::Jetson => HostProvider::Jetson,
            Self::Network => HostProvider::Net,
            Self::Launchd | Self::EndpointCompliance | Self::Modbus | Self::Dnp3 => {
                anyhow::bail!("not_supported provider-mock")
            }
        })
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(provider) = args.native_provider {
        let observation = host_sidecar_bridge::observations::collect(
            provider.id(),
            std::time::Instant::now() + std::time::Duration::from_secs(10),
        );
        let (entries, reason) = match observation {
            Ok(entries) => (entries, None),
            Err(reason) => (Vec::new(), Some(reason)),
        };
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "schema":"cohesix-native-discovery/v1", "provider_id":provider.id(), "observed_mode":"live",
                "proof_class":"read_only", "authoritative":false, "available":reason.is_none(), "reason":reason, "entries":entries,
            }))?
        );
        return Ok(());
    }
    if args.native_systemd_discover {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "schema":"cohesix-native-discovery/v1", "provider_id":"systemd",
                "mode":"live", "proof_class":"read_only", "authoritative":false,
                "units":host_sidecar_bridge::native::discover_systemd_units()?,
            }))?
        );
        return Ok(());
    }
    if args.native_kubernetes_nodes {
        let client = host_sidecar_bridge::kubernetes::Kubernetes::from_environment(
            std::time::Duration::from_secs(5),
        )?;
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "schema":"cohesix-native-discovery/v1", "provider_id":"k8s",
                "mode":"live", "proof_class":"read_only", "authoritative":false, "nodes":client.nodes()?,
            }))?
        );
        return Ok(());
    }
    if let Some(unit) = args.native_systemd_unit.as_deref() {
        let observation = host_sidecar_bridge::native::observe_systemd(unit)?;
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "schema": "cohesix-native-observation/v1",
                "provider_id": "systemd", "source": "systemd-manager-dbus",
                "observed_mode": "live", "proof_class": "read_only", "data": observation,
            }))?
        );
        return Ok(());
    }
    let providers = if args.mock && !args.provider.is_empty() {
        args.provider
            .iter()
            .copied()
            .map(ProviderArg::mock_provider)
            .collect::<Result<Vec<_>>>()?
    } else {
        default_providers()
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

    #[cfg(any(feature = "rest", feature = "tcp"))]
    let mut publisher = {
        let source = args
            .source_id
            .as_deref()
            .context("live publication requires --source-id")?;
        let state = args
            .state_dir
            .as_deref()
            .context("live publication requires --state-dir")?;
        let enrollment = host_sidecar_bridge::snapshots::Enrollment::compiled(source)?;
        host_sidecar_bridge::snapshots::Publisher::open(enrollment, state)?
    };
    if let Some(rest_url) = args.rest_url.as_deref() {
        #[cfg(any(feature = "rest", feature = "tcp"))]
        {
            let token = args.rest_auth_token.clone().or_else(|| {
                env::var("HIVE_GATEWAY_REQUEST_AUTH_TOKEN")
                    .or_else(|_| env::var("COHSH_REST_AUTH_TOKEN"))
                    .or_else(|_| env::var("COH_REST_AUTH_TOKEN"))
                    .ok()
            });
            let mut transport = RestTransport::new(rest_url, token);
            return publish_native(&args, &bridge, &mut publisher, &mut transport);
        }
        #[cfg(not(any(feature = "rest", feature = "tcp")))]
        {
            let _ = rest_url;
            anyhow::bail!("rest transport disabled; rebuild with --features rest");
        }
    }
    #[cfg(feature = "tcp")]
    {
        let mut transport =
            TcpTransport::new(&args.tcp_host, args.tcp_port).with_auth_token(&args.auth_token);
        publish_native(&args, &bridge, &mut publisher, &mut transport)
    }
    #[cfg(not(feature = "tcp"))]
    {
        anyhow::bail!("tcp transport disabled; rebuild with --features tcp or use --mock");
    }
}

#[cfg(any(feature = "rest", feature = "tcp"))]
fn publish_native<T: Transport>(
    args: &Args,
    bridge: &HostSidecarBridge,
    publisher: &mut host_sidecar_bridge::snapshots::Publisher,
    transport: &mut T,
) -> Result<()> {
    let providers = if args.provider.is_empty() {
        publisher.providers().to_vec()
    } else {
        args.provider
            .iter()
            .map(|provider| provider.id().to_owned())
            .collect()
    };
    for (index, provider) in providers.iter().enumerate() {
        if !publisher.providers().contains(provider) || providers[..index].contains(provider) {
            anyhow::bail!("invalid or duplicate provider for selected source");
        }
    }
    let policy = resolve_policy(args.policy.as_ref())?;
    let session = bridge.attach(transport)?;
    let mut due = vec![Instant::now(); providers.len()];
    loop {
        for (index, provider) in providers.iter().enumerate() {
            if args.watch && Instant::now() < due[index] {
                continue;
            }
            let publication =
                bridge.publish_live_provider(transport, &session, publisher, provider)?;
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "schema":"cohesix-snapshot-publication/v1", "proof_class":"read_only", "authoritative":false,
                    "publication":publication,
                }))?
            );
            let interval = match provider.as_str() {
                "nvidia" => policy.host_telemetry.nvidia_poll_ms,
                "systemd" => policy.host_telemetry.systemd_poll_ms,
                "docker" => policy.host_telemetry.docker_poll_ms,
                "k8s" => policy.host_telemetry.k8s_poll_ms,
                _ => 1000,
            };
            due[index] = Instant::now()
                .checked_add(Duration::from_millis(interval.max(1)))
                .context("invalid provider polling interval")?;
        }
        if !args.watch {
            transport.quit(&session)?;
            return Ok(());
        }
        // Each provider retains its own configured minimum interval. Native
        // failures withdraw only that provider and do not stop the other rows.
        let next = due.iter().min().context("no enrolled native providers")?;
        thread::sleep(next.saturating_duration_since(Instant::now()));
    }
}

#[cfg(any(feature = "rest", feature = "tcp"))]
fn resolve_policy(path: Option<&PathBuf>) -> Result<CohshPolicy> {
    let attempted = match path {
        Some(path) => load_policy(path).with_context(|| format!("load policy {}", path.display())),
        None => {
            let path = default_policy_path();
            load_policy(&path).with_context(|| format!("load policy {}", path.display()))
        }
    };
    attempted
}
