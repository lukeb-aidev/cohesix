// Author: Lukas Bower
// Purpose: Execute admitted Kubernetes intents through UID-fenced native APIs and verify terminal scheduling state.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use anyhow::{anyhow, bail, Result};
use cohsh::{Session, Transport};
use host_sidecar_bridge::kubernetes::Kubernetes;
use std::time::{Duration, Instant};

use super::{arg_str, provider_pending, target_components, ExecutorConfig};
use crate::HostTicketSpec;

/// Provider dispatch preserves admission/WAL ownership in host-ticket-agent.
pub fn execute(
    _transport: &mut dyn Transport,
    _session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    let node = resolve_node(spec)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let native = Kubernetes::from_environment(Duration::from_secs(30))?;
    let before = native.node(&node)?;
    let mut evicted = Vec::new();
    super::observation::prepare(config)?;
    let after = match spec.action.as_str() {
        "k8s.lease.sync" => before.clone(),
        "k8s.cordon" | "k8s.drain" => {
            let cordoned = native.cordon(&before).map_err(|_| {
                provider_pending("kubernetes cordon requires native reconciliation")
            })?;
            if spec.action == "k8s.drain" {
                let candidates = native
                    .drain_candidates(&node)
                    .map_err(|_| provider_pending("kubernetes drain inventory unavailable"))?;
                for pod in candidates {
                    native.evict(&pod).map_err(|_| provider_pending("kubernetes eviction refused or pending; disruption budgets remain enforced"))?;
                    evicted.push(pod);
                }
                loop {
                    let pending = native.drain_candidates(&node).map_err(|_| {
                        provider_pending("kubernetes drain observation unavailable")
                    })?;
                    if pending.is_empty() {
                        break;
                    }
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(provider_pending(
                            "kubernetes drain deadline; terminal state unavailable",
                        ));
                    }
                    std::thread::sleep(remaining.min(Duration::from_millis(100)));
                }
                let final_node = native.node(&node).map_err(|_| {
                    provider_pending("kubernetes final node observation unavailable")
                })?;
                if final_node.uid != before.uid || !final_node.unschedulable {
                    return Err(provider_pending("kubernetes drain node identity changed"));
                }
                final_node
            } else {
                cordoned
            }
        }
        _ => bail!("EPERM unsupported kubernetes action"),
    };
    let reference = super::observation::retain_native(
        config,
        spec,
        &serde_json::json!({
            "source":"kubernetes-api", "before":before, "after":after,
            "evicted":evicted, "exclusions":["daemonset", "mirror", "terminal-pod"],
            "event_cursor":after.resource_version,
        }),
        &format!("kubernetes-node:{}:{}", after.uid, after.resource_version),
    )
    .map_err(|_| provider_pending("kubernetes native outcome requires durable publication"))?;
    Ok(reference)
}

fn resolve_node(spec: &HostTicketSpec) -> Result<String> {
    if let Some(node) = arg_str(spec, "node") {
        return Ok(node.into());
    }
    let parts = target_components(spec);
    let index = parts
        .iter()
        .position(|part| *part == "node")
        .ok_or_else(|| anyhow!("EPERM missing kubernetes node"))?;
    parts
        .get(index + 1)
        .map(|node| (*node).into())
        .ok_or_else(|| anyhow!("EPERM missing kubernetes node"))
}
