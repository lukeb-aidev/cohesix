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
use std::collections::HashMap;
use std::process::Command;

const DEFAULT_SYSTEMD_UNITS: &[&str] = &["cohesix-agent.service", "ssh.service"];
const DEFAULT_K8S_NODES: &[&str] = &["node-1"];
const DEFAULT_NVIDIA_GPUS: &[&str] = &["0"];
const STATUS_LINE_CAP: usize = 256;

/// Discovered host provider topology under the mounted `/host` namespace.
#[derive(Debug, Clone)]
pub struct HostTopology {
    systemd_units: Vec<String>,
    k8s_nodes: Vec<String>,
    nvidia_gpus: Vec<String>,
    docker_enabled: bool,
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

    /// Discover the host-side provider topology currently mounted in the VM.
    pub fn discover_topology<T: Transport>(
        &self,
        transport: &mut T,
        session: &cohsh::Session,
    ) -> HostTopology {
        let mut systemd_units = Vec::new();
        let mut k8s_nodes = Vec::new();
        let mut nvidia_gpus = Vec::new();
        let mut docker_enabled = false;
        let mount = self.mount();

        if self.providers.contains(&HostProvider::Systemd) {
            let path = format!("{mount}/systemd");
            if let Ok(entries) = transport.list(session, &path) {
                systemd_units = entries;
            }
        }
        if self.providers.contains(&HostProvider::K8s) {
            let path = format!("{mount}/k8s/node");
            if let Ok(entries) = transport.list(session, &path) {
                k8s_nodes = entries;
            }
        }
        if self.providers.contains(&HostProvider::Docker) {
            let path = format!("{mount}/docker");
            docker_enabled = transport.list(session, &path).is_ok();
        }
        if self.providers.contains(&HostProvider::Nvidia) {
            let path = format!("{mount}/nvidia/gpu");
            if let Ok(entries) = transport.list(session, &path) {
                nvidia_gpus = entries;
            }
        }

        HostTopology {
            systemd_units,
            k8s_nodes,
            nvidia_gpus,
            docker_enabled,
        }
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

    /// Publish live data for each provider using the supplied session and topology.
    pub fn publish_live<T: Transport>(
        &self,
        transport: &mut T,
        session: &cohsh::Session,
        topology: &HostTopology,
    ) -> Result<()> {
        for provider in &self.providers {
            self.publish_live_provider(transport, session, topology, *provider)?;
        }
        Ok(())
    }

    /// Publish live data for a single provider.
    pub fn publish_live_provider<T: Transport>(
        &self,
        transport: &mut T,
        session: &cohsh::Session,
        topology: &HostTopology,
        provider: HostProvider,
    ) -> Result<()> {
        match provider {
            HostProvider::Systemd => self.publish_systemd_live(transport, session, topology),
            HostProvider::K8s => self.publish_k8s_live(transport, session, topology),
            HostProvider::Docker => self.publish_docker_live(transport, session, topology),
            HostProvider::Nvidia => self.publish_nvidia_live(transport, session, topology),
            HostProvider::Jetson | HostProvider::Net => Ok(()),
        }
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

    fn publish_k8s_mock<T: Transport>(&self, transport: &mut T, session: &cohsh::Session) -> Result<()> {
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

    fn publish_systemd_live<T: Transport>(
        &self,
        transport: &mut T,
        session: &cohsh::Session,
        topology: &HostTopology,
    ) -> Result<()> {
        if topology.systemd_units.is_empty() {
            return Ok(());
        }
        for unit in &topology.systemd_units {
            let status = format!("{}/systemd/{unit}/status", self.mount());
            let line = systemd_unit_status(unit).unwrap_or_else(|err| {
                let reason = sanitize_value(&err.to_string());
                format!("state=unknown reason={reason}")
            });
            let payload = ensure_line_terminated(&line);
            transport
                .write(session, &status, payload.as_bytes())
                .with_context(|| format!("write {status}"))?;
        }
        Ok(())
    }

    fn publish_k8s_live<T: Transport>(
        &self,
        transport: &mut T,
        session: &cohsh::Session,
        topology: &HostTopology,
    ) -> Result<()> {
        if topology.k8s_nodes.is_empty() {
            return Ok(());
        }
        let snapshot = kubectl_nodes().unwrap_or_else(|err| {
            let mut map = HashMap::new();
            let reason = sanitize_value(&err.to_string());
            map.insert("__error__".to_owned(), format!("state=unknown reason={reason}"));
            map
        });
        for node in &topology.k8s_nodes {
            let status = format!("{}/k8s/node/{node}/status", self.mount());
            let line = snapshot
                .get(node)
                .cloned()
                .or_else(|| snapshot.get("__error__").cloned())
                .unwrap_or_else(|| "state=unknown".to_owned());
            let payload = ensure_line_terminated(&line);
            transport
                .write(session, &status, payload.as_bytes())
                .with_context(|| format!("write {status}"))?;
        }
        Ok(())
    }

    fn publish_docker_live<T: Transport>(
        &self,
        transport: &mut T,
        session: &cohsh::Session,
        topology: &HostTopology,
    ) -> Result<()> {
        if !topology.docker_enabled {
            return Ok(());
        }
        let status_path = format!("{}/docker/status", self.mount());
        let line = docker_status_line().unwrap_or_else(|err| {
            let reason = sanitize_value(&err.to_string());
            format!("state=unknown reason={reason}")
        });
        let payload = ensure_line_terminated(&line);
        transport
            .write(session, &status_path, payload.as_bytes())
            .with_context(|| format!("write {status_path}"))?;
        Ok(())
    }

    fn publish_nvidia_live<T: Transport>(
        &self,
        transport: &mut T,
        session: &cohsh::Session,
        topology: &HostTopology,
    ) -> Result<()> {
        if topology.nvidia_gpus.is_empty() {
            return Ok(());
        }
        let snapshot = nvidia_status_snapshot().unwrap_or_else(|err| {
            let mut map = HashMap::new();
            map.insert(
                "__error__".to_owned(),
                NvidiaStatus {
                    status_line: {
                        let reason = sanitize_value(&err.to_string());
                        format!("state=unknown reason={reason}")
                    },
                    thermal_line: "temp_c=unknown".to_owned(),
                },
            );
            map
        });
        for gpu in &topology.nvidia_gpus {
            let status_path = format!("{}/nvidia/gpu/{gpu}/status", self.mount());
            let thermal_path = format!("{}/nvidia/gpu/{gpu}/thermal", self.mount());
            let entry = snapshot
                .get(gpu)
                .cloned()
                .or_else(|| snapshot.get("__error__").cloned())
                .unwrap_or_else(|| NvidiaStatus {
                    status_line: "state=unknown".to_owned(),
                    thermal_line: "temp_c=unknown".to_owned(),
                });
            let status_payload = ensure_line_terminated(&entry.status_line);
            transport
                .write(session, &status_path, status_payload.as_bytes())
                .with_context(|| format!("write {status_path}"))?;
            let thermal_payload = ensure_line_terminated(&entry.thermal_line);
            transport
                .write(session, &thermal_path, thermal_payload.as_bytes())
                .with_context(|| format!("write {thermal_path}"))?;
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

fn ensure_line_terminated(line: &str) -> String {
    let trimmed = line.trim();
    let mut out = truncate_to_boundary(trimmed, STATUS_LINE_CAP.saturating_sub(1));
    out.push('\n');
    out
}

fn truncate_to_boundary(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_owned();
    }
    let mut end = 0usize;
    for (idx, ch) in input.char_indices() {
        let next = idx + ch.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    input[..end].to_owned()
}

fn sanitize_value(input: &str) -> String {
    let trimmed = input.trim();
    let mut out = String::new();
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric()
            || matches!(ch, '-' | '_' | '.' | ':' | '/' | ',')
        {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("unknown");
    }
    out
}

fn systemd_unit_status(unit: &str) -> Result<String> {
    let output = Command::new("systemctl")
        .args(["show", unit, "--property=ActiveState,SubState"])
        .output()
        .with_context(|| format!("systemctl show {unit}"))?;
    if !output.status.success() {
        bail!("systemctl show {unit} failed");
    }
    let text = String::from_utf8(output.stdout).context("systemctl output not UTF-8")?;
    let mut state = "unknown";
    let mut sub = "unknown";
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("ActiveState=") {
            state = value.trim();
        } else if let Some(value) = line.strip_prefix("SubState=") {
            sub = value.trim();
        }
    }
    Ok(format!(
        "state={} sub={}",
        sanitize_value(state),
        sanitize_value(sub)
    ))
}

fn kubectl_nodes() -> Result<HashMap<String, String>> {
    let output = Command::new("kubectl")
        .args(["get", "nodes", "--no-headers"])
        .output()
        .context("kubectl get nodes")?;
    if !output.status.success() {
        bail!("kubectl get nodes failed");
    }
    let text = String::from_utf8(output.stdout).context("kubectl output not UTF-8")?;
    let mut snapshot = HashMap::new();
    for line in text.lines() {
        let tokens = line.split_whitespace().collect::<Vec<_>>();
        if tokens.len() < 2 {
            continue;
        }
        let name = tokens[0].to_owned();
        let state = sanitize_value(&tokens[1].to_ascii_lowercase());
        let role = tokens.get(2).copied().unwrap_or("unknown");
        let version = tokens.last().copied().unwrap_or("unknown");
        let line = format!(
            "state={} role={} version={}",
            state,
            sanitize_value(role),
            sanitize_value(version)
        );
        snapshot.insert(name, line);
    }
    if snapshot.is_empty() {
        bail!("kubectl returned no nodes");
    }
    Ok(snapshot)
}

fn docker_status_line() -> Result<String> {
    let output = Command::new("docker")
        .args([
            "info",
            "--format",
            "{{.ServerVersion}} {{.Containers}} {{.ContainersRunning}} {{.ContainersPaused}} {{.ContainersStopped}}",
        ])
        .output()
        .context("docker info")?;
    if !output.status.success() {
        bail!("docker info failed");
    }
    let text = String::from_utf8(output.stdout).context("docker info not UTF-8")?;
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    if tokens.len() < 5 {
        bail!("docker info output incomplete");
    }
    Ok(format!(
        "version={} containers={} running={} paused={} stopped={}",
        sanitize_value(tokens[0]),
        sanitize_value(tokens[1]),
        sanitize_value(tokens[2]),
        sanitize_value(tokens[3]),
        sanitize_value(tokens[4])
    ))
}

#[derive(Debug, Clone)]
struct NvidiaStatus {
    status_line: String,
    thermal_line: String,
}

fn nvidia_status_snapshot() -> Result<HashMap<String, NvidiaStatus>> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .context("nvidia-smi query")?;
    if !output.status.success() {
        bail!("nvidia-smi query failed");
    }
    let text = String::from_utf8(output.stdout).context("nvidia-smi output not UTF-8")?;
    let mut snapshot = HashMap::new();
    for line in text.lines() {
        let parts = line.split(',').map(|part| part.trim()).collect::<Vec<_>>();
        if parts.len() < 6 {
            continue;
        }
        let id = parts[0];
        let util = sanitize_value(parts[1]);
        let mem_used = sanitize_value(parts[2]);
        let mem_total = sanitize_value(parts[3]);
        let temp = sanitize_value(parts[4]);
        let power = sanitize_value(parts[5]);
        let status_line = format!(
            "util_pct={} mem_used_mb={} mem_total_mb={} temp_c={} power_w={}",
            util, mem_used, mem_total, temp, power
        );
        let thermal_line = format!("temp_c={}", temp);
        snapshot.insert(
            id.to_owned(),
            NvidiaStatus {
                status_line,
                thermal_line,
            },
        );
    }
    if snapshot.is_empty() {
        bail!("nvidia-smi returned no data");
    }
    Ok(snapshot)
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
