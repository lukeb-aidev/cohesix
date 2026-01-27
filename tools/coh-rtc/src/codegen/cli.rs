// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Emit cohsh CLI scripts derived from the manifest.
// Author: Lukas Bower

use crate::ir::{Manifest, Role};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub fn emit_cli_script(manifest: &Manifest, path: &Path) -> Result<()> {
    let queen_role = manifest
        .tickets
        .iter()
        .find(|ticket| matches!(ticket.role, Role::Queen))
        .map(|ticket| ticket.role)
        .ok_or_else(|| anyhow::anyhow!("manifest does not include a queen ticket"))?;

    let mut contents = format!(
        "# Author: Lukas Bower\n# Purpose: Boot smoke using attach/log/quit flows.\nattach {}\nEXPECT OK\n",
        queen_role.as_str()
    );
    if manifest.ecosystem.host.enable {
        let providers = if manifest.ecosystem.host.providers.is_empty() {
            "(none)".to_owned()
        } else {
            manifest
                .ecosystem
                .host
                .providers
                .iter()
                .map(|provider| format_provider(provider))
                .collect::<Vec<_>>()
                .join(", ")
        };
        contents.push_str(&format!(
            "# /host namespace enabled at {} (providers: {})\n",
            manifest.ecosystem.host.mount_at, providers
        ));
    }
    let policy = &manifest.ecosystem.policy;
    contents.push_str(&format!(
        "# /policy gate enabled={} queue_max_entries={} queue_max_bytes={} ctl_max_bytes={} status_max_bytes={}\n",
        policy.enable,
        policy.queue_max_entries,
        policy.queue_max_bytes,
        policy.ctl_max_bytes,
        policy.status_max_bytes
    ));
    let audit = &manifest.ecosystem.audit;
    contents.push_str(&format!(
        "# /audit enabled={} journal_max_bytes={} decisions_max_bytes={} replay_enable={} replay_max_entries={} replay_ctl_max_bytes={} replay_status_max_bytes={}\n",
        audit.enable,
        audit.journal_max_bytes,
        audit.decisions_max_bytes,
        audit.replay_enable,
        audit.replay_max_entries,
        audit.replay_ctl_max_bytes,
        audit.replay_status_max_bytes
    ));
    if policy.rules.is_empty() {
        contents.push_str("# policy rules: (none)\n");
    } else {
        for rule in &policy.rules {
            contents.push_str(&format!("# policy rule {} -> {}\n", rule.id, rule.target));
        }
    }
    contents.push_str("log\nEXPECT OK\nWAIT 250\nquit\n");
    fs::write(path, contents)
        .with_context(|| format!("failed to write cli script {}", path.display()))?;
    Ok(())
}

fn format_provider(provider: &crate::ir::HostProvider) -> &'static str {
    match provider {
        crate::ir::HostProvider::Systemd => "systemd",
        crate::ir::HostProvider::K8s => "k8s",
        crate::ir::HostProvider::Nvidia => "nvidia",
        crate::ir::HostProvider::Jetson => "jetson",
        crate::ir::HostProvider::Net => "net",
    }
}
