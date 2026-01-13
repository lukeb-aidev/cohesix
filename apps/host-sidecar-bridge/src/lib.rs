// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide host-sidecar bridge helpers for /host provider publication.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Host-sidecar bridge helpers that publish mock provider data into `/host`.

use anyhow::{bail, Context, Result};
use cohesix_ticket::Role;
use cohsh::Transport;
use nine_door::HostProvider;

const DEFAULT_SYSTEMD_UNITS: &[&str] = &["cohesix-agent.service", "ssh.service"];
const DEFAULT_K8S_NODES: &[&str] = &["node-1"];
const DEFAULT_NVIDIA_GPUS: &[&str] = &["0"];

/// Default provider list used when no providers are specified explicitly.
pub fn default_providers() -> Vec<HostProvider> {
    vec![
        HostProvider::Systemd,
        HostProvider::K8s,
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

    /// Publish mock data for each provider using the supplied transport.
    pub fn publish<T: Transport>(&self, transport: &mut T) -> Result<()> {
        let session = transport
            .attach(Role::Queen, None)
            .context("host sidecar attach failed")?;
        transport
            .list(&session, self.mount())
            .with_context(|| format!("mount {} not available", self.mount()))?;

        for provider in &self.providers {
            match provider {
                HostProvider::Systemd => self.publish_systemd(transport, &session)?,
                HostProvider::K8s => self.publish_k8s(transport, &session)?,
                HostProvider::Nvidia => self.publish_nvidia(transport, &session)?,
                HostProvider::Jetson | HostProvider::Net => {
                    let path = format!("{}/{}", self.mount(), provider.as_str());
                    let _ = transport.list(&session, &path);
                }
            }
        }

        let _ = transport.quit(&session);
        Ok(())
    }

    fn publish_systemd<T: Transport>(
        &self,
        transport: &mut T,
        session: &cohsh::Session,
    ) -> Result<()> {
        for unit in DEFAULT_SYSTEMD_UNITS {
            let status = format!("{}/systemd/{unit}/status", self.mount());
            transport
                .write(session, &status, b"active")
                .with_context(|| format!("write {status}"))?;
        }
        Ok(())
    }

    fn publish_k8s<T: Transport>(&self, transport: &mut T, session: &cohsh::Session) -> Result<()> {
        let nodes_root = format!("{}/k8s/node", self.mount());
        let entries = transport
            .list(session, &nodes_root)
            .with_context(|| format!("list {nodes_root}"))?;
        for node in DEFAULT_K8S_NODES {
            if !entries.iter().any(|entry| entry == node) {
                continue;
            }
            let cordon = format!("{}/k8s/node/{node}/cordon", self.mount());
            transport
                .read(session, &cordon)
                .with_context(|| format!("read {cordon}"))?;
        }
        Ok(())
    }

    fn publish_nvidia<T: Transport>(
        &self,
        transport: &mut T,
        session: &cohsh::Session,
    ) -> Result<()> {
        for gpu in DEFAULT_NVIDIA_GPUS {
            let status = format!("{}/nvidia/gpu/{gpu}/status", self.mount());
            transport
                .write(session, &status, b"ok")
                .with_context(|| format!("write {status}"))?;
            let thermal = format!("{}/nvidia/gpu/{gpu}/thermal", self.mount());
            transport
                .write(session, &thermal, b"42C")
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
