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

    let contents = format!(
        "# Author: Lukas Bower\n# Purpose: Boot smoke using attach/log/quit flows.\nattach {}\nEXPECT OK\nlog\nEXPECT OK\nWAIT 250\nquit\n",
        queen_role.as_str()
    );
    fs::write(path, contents)
        .with_context(|| format!("failed to write cli script {}", path.display()))?;
    Ok(())
}
