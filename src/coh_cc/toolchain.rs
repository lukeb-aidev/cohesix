// CLASSIFICATION: COMMUNITY
// Filename: toolchain.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-18

use std::path::{Path, PathBuf};

use crate::coh_cc::logging;

#[derive(Clone)]
pub struct Toolchain {
    base: PathBuf,
}

impl Toolchain {
    pub fn new<P: Into<PathBuf>>(base: P) -> anyhow::Result<Self> {
        let base = base.into();
        if base.as_os_str().is_empty() {
            anyhow::bail!("toolchain directory cannot be empty");
        }
        let canon = base.canonicalize().unwrap_or(base.clone());
        if !canon.starts_with("/mnt/data") {
            anyhow::bail!("toolchain directory must be under /mnt/data");
        }
        Ok(Toolchain { base: canon })
    }

    pub fn get_tool_path(&self, tool_name: &str) -> anyhow::Result<PathBuf> {
        if tool_name.trim().is_empty() {
            anyhow::bail!("tool name cannot be empty");
        }
        if tool_name.contains('/') || tool_name.contains("..") {
            anyhow::bail!("invalid tool name");
        }
        let path = self.base.join(tool_name);
        logging::log(
            "INFO",
            "toolchain",
            Path::new(tool_name),
            &path,
            &[],
            "resolve",
        );
        Ok(path)
    }

    /// Return the path to `cargo` ensuring it exists and resides in the toolchain.
    pub fn get_cargo(&self) -> anyhow::Result<PathBuf> {
        let path = self.get_tool_path("cargo")?;
        if !path.exists() {
            anyhow::bail!("cargo not found in toolchain");
        }
        Ok(path)
    }

    pub fn base(&self) -> &Path {
        &self.base
    }
}
