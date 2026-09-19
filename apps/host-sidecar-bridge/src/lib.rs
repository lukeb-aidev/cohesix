// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide host-sidecar bridge helpers for /host provider publication.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Host-sidecar adapters keep native observations separate from explicit fixtures.

/// Bounded Kubernetes API discovery and UID-fenced native operations.
pub mod kubernetes;

/// Bounded local Docker Engine identity and terminal observations.
pub mod docker;
/// Freshness-preserving projections of acknowledged host field-bus operations.
pub mod field_bus;
/// Bounded macOS service lifecycle and kernel process identity observations.
pub mod launchd;
/// Immutable macOS release attempts and bounded endpoint observations.
pub mod mac_release;
/// Bounded native systemd API observations and lifecycle calls.
pub mod native;
/// Bounded native read projections.
pub mod observations;
/// Provider output normalization helpers.
pub mod providers;
/// Source-scoped, durable native snapshot publication.
pub mod snapshots;

use anyhow::{bail, Context, Result};
use cohesix_ticket::Role;
use cohsh::Transport;
use nine_door::HostProvider;

const DEFAULT_SYSTEMD_UNITS: &[&str] = &["cohesix-agent.service", "ssh.service"];
const DEFAULT_K8S_NODES: &[&str] = &["node-1"];
const DEFAULT_NVIDIA_GPUS: &[&str] = &["0"];

/// Native diagnostic observations. These are not execution receipts or an
/// enrolled publication; publication always recollects through `Publisher`.
#[derive(Debug, Clone)]
pub struct HostTopology {
    /// Exact provider and bounded native entries collected from this host.
    pub observations: std::collections::BTreeMap<String, Vec<cohesix_authority::snapshot::Entry>>,
}

/// Default provider list used when no providers are specified explicitly.
pub fn default_providers() -> Vec<HostProvider> {
    vec![
        HostProvider::Systemd,
        HostProvider::K8s,
        HostProvider::Docker,
        HostProvider::Nvidia,
    ]
}

/// Host-sidecar bridge configuration.
#[derive(Debug, Clone)]
pub struct HostSidecarBridge {
    mount: String,
    providers: Vec<HostProvider>,
}

impl HostSidecarBridge {
    /// Construct a bridge for the supplied mount path and provider list.
    pub fn new(mount: impl AsRef<str>, providers: Vec<HostProvider>) -> Result<Self> {
        let mount = normalise_mount_path(mount.as_ref())?;
        Ok(Self { mount, providers })
    }

    /// Return the mount point used for `/host`.
    pub fn mount(&self) -> &str {
        &self.mount
    }

    /// Return the configured providers.
    pub fn providers(&self) -> &[HostProvider] {
        &self.providers
    }

    /// Attach to the transport as queen and verify the mount is reachable.
    pub fn attach<T: Transport>(&self, transport: &mut T) -> Result<cohsh::Session> {
        let session = transport
            .attach(Role::Queen, None)
            .context("host sidecar attach failed")?;
        self.ensure_mount(transport, &session)?;
        Ok(session)
    }

    /// Discover native provider names without reading target-seeded provider status or topology.
    pub fn discover_topology<T: Transport>(
        &self,
        _transport: &mut T,
        _session: &cohsh::Session,
    ) -> Result<HostTopology> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut observations = std::collections::BTreeMap::new();
        for provider in &self.providers {
            let name = provider.as_str();
            if observations.contains_key(name) {
                bail!("invalid duplicate native provider");
            }
            observations.insert(
                name.to_owned(),
                crate::observations::collect(name, deadline).map_err(|reason| {
                    anyhow::anyhow!("native discovery unavailable: {reason:?}")
                })?,
            );
        }
        Ok(HostTopology { observations })
    }

    /// Publish mock data for each provider using the supplied transport.
    pub fn publish_mock<T: Transport>(&self, transport: &mut T) -> Result<()> {
        let session = self.attach(transport)?;
        for provider in &self.providers {
            match provider {
                HostProvider::Systemd => self.publish_systemd_mock(transport, &session)?,
                HostProvider::K8s => self.publish_k8s_mock(transport, &session)?,
                HostProvider::Docker => self.publish_docker_mock(transport, &session)?,
                HostProvider::Nvidia => self.publish_nvidia_mock(transport, &session)?,
                HostProvider::Jetson | HostProvider::Net => {
                    let path = format!("{}/{}", self.mount(), provider.as_str());
                    let _ = transport.list(&session, &path);
                }
            }
        }
        let _ = transport.quit(&session);
        Ok(())
    }

    /// Publish every provider selected by compiled source enrollment. The
    /// publisher reserves durable sequences and withdraws failed native reads.
    pub fn publish_live<T: Transport>(
        &self,
        transport: &mut T,
        session: &cohsh::Session,
        publisher: &mut snapshots::Publisher,
    ) -> Result<Vec<snapshots::Publication>> {
        publisher.require_mount(self.mount())?;
        let providers = publisher.providers().to_vec();
        providers
            .iter()
            .map(|provider| self.publish_live_provider(transport, session, publisher, provider))
            .collect()
    }

    /// Publish one compiled source provider through the authenticated versioned
    /// snapshot interface. A target topology is never discovery authority.
    pub fn publish_live_provider<T: Transport>(
        &self,
        transport: &mut T,
        session: &cohsh::Session,
        publisher: &mut snapshots::Publisher,
        provider: &str,
    ) -> Result<snapshots::Publication> {
        publisher.require_mount(self.mount())?;
        publisher.publish(transport, session, provider)
    }

    fn ensure_mount<T: Transport>(
        &self,
        transport: &mut T,
        session: &cohsh::Session,
    ) -> Result<()> {
        transport
            .list(session, self.mount())
            .with_context(|| format!("mount {} not available", self.mount()))?;
        Ok(())
    }

    fn publish_systemd_mock<T: Transport>(
        &self,
        transport: &mut T,
        session: &cohsh::Session,
    ) -> Result<()> {
        for unit in DEFAULT_SYSTEMD_UNITS {
            let status = format!("{}/systemd/{unit}/status", self.mount());
            transport
                .write(session, &status, b"active\n")
                .with_context(|| format!("write {status}"))?;
        }
        Ok(())
    }

    fn publish_k8s_mock<T: Transport>(
        &self,
        transport: &mut T,
        session: &cohsh::Session,
    ) -> Result<()> {
        let nodes_root = format!("{}/k8s/node", self.mount());
        let entries = transport
            .list(session, &nodes_root)
            .with_context(|| format!("list {nodes_root}"))?;
        for node in DEFAULT_K8S_NODES {
            if !entries.iter().any(|entry| entry == node) {
                continue;
            }
            let status = format!("{}/k8s/node/{node}/status", self.mount());
            transport
                .write(session, &status, b"state=ready\n")
                .with_context(|| format!("write {status}"))?;
        }
        Ok(())
    }

    fn publish_docker_mock<T: Transport>(
        &self,
        transport: &mut T,
        session: &cohsh::Session,
    ) -> Result<()> {
        let status = format!("{}/docker/status", self.mount());
        transport
            .write(session, &status, b"state=ok\n")
            .with_context(|| format!("write {status}"))?;
        Ok(())
    }

    fn publish_nvidia_mock<T: Transport>(
        &self,
        transport: &mut T,
        session: &cohsh::Session,
    ) -> Result<()> {
        for gpu in DEFAULT_NVIDIA_GPUS {
            let status = format!("{}/nvidia/gpu/{gpu}/status", self.mount());
            transport
                .write(session, &status, b"state=ok\n")
                .with_context(|| format!("write {status}"))?;
            let thermal = format!("{}/nvidia/gpu/{gpu}/thermal", self.mount());
            transport
                .write(session, &thermal, b"temp_c=42\n")
                .with_context(|| format!("write {thermal}"))?;
        }
        Ok(())
    }
}

fn normalise_mount_path(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("mount path must not be empty");
    }
    if !trimmed.starts_with('/') {
        bail!("mount path must be absolute (start with '/')");
    }
    let cleaned = trimmed.trim_end_matches('/');
    if cleaned.is_empty() || cleaned == "/" {
        bail!("mount path must not be root");
    }
    for component in cleaned.split('/').filter(|c| !c.is_empty()) {
        if component == ".." {
            bail!("mount path must not include '..'");
        }
    }
    Ok(cleaned.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_providers_include_required_entries() {
        let providers = default_providers();
        assert!(providers.contains(&HostProvider::Systemd));
        assert!(providers.contains(&HostProvider::K8s));
        assert!(providers.contains(&HostProvider::Docker));
        assert!(providers.contains(&HostProvider::Nvidia));
    }

    #[test]
    fn normalise_mount_rejects_root() {
        assert!(normalise_mount_path("/").is_err());
    }

    #[test]
    fn normalise_mount_accepts_absolute_paths() {
        let mount = normalise_mount_path("/host/").unwrap();
        assert_eq!(mount, "/host");
    }
}
