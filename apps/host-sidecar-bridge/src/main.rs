// Author: Lukas Bower
// Purpose: CLI entry point for the host-sidecar bridge tool.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Host-sidecar bridge CLI that publishes provider data into `/host`.

use anyhow::{Context, Result};
use clap::{ArgAction, Parser, ValueEnum};
use cohsh::NineDoorTransport;
use host_sidecar_bridge::{default_providers, HostSidecarBridge};
use nine_door::{HostNamespaceConfig, HostProvider, NineDoor};

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
    Nvidia,
    Jetson,
    Net,
}

impl From<ProviderArg> for HostProvider {
    fn from(value: ProviderArg) -> Self {
        match value {
            ProviderArg::Systemd => HostProvider::Systemd,
            ProviderArg::K8s => HostProvider::K8s,
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
            .publish(&mut transport)
            .context("publish mock provider data")?;
        println!("mock sidecar published providers at {}", bridge.mount());
        return Ok(());
    }

    #[cfg(feature = "tcp")]
    {
        let mut transport =
            TcpTransport::new(args.tcp_host, args.tcp_port).with_auth_token(args.auth_token);
        bridge
            .publish(&mut transport)
            .context("publish provider data over tcp")?;
        println!("sidecar published providers at {}", bridge.mount());
        Ok(())
    }

    #[cfg(not(feature = "tcp"))]
    {
        anyhow::bail!("tcp transport disabled; rebuild with --features tcp or use --mock");
    }
}
