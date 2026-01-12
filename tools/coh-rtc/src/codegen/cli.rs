// Author: Lukas Bower
// Purpose: Emit cohsh CLI scripts derived from the manifest.

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
