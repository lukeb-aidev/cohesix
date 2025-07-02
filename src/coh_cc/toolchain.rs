// CLASSIFICATION: COMMUNITY
// Filename: toolchain.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-12-09

use crate::{coh_bail, CohError};
use std::path::{Path, PathBuf};

use crate::coh_cc::logging;

#[derive(Clone)]
pub struct Toolchain {
    base: PathBuf,
}

impl Toolchain {
    pub fn new<P: Into<PathBuf>>(base: P) -> Result<Self, CohError> {
        let base = base.into();
        if base.as_os_str().is_empty() {
            coh_bail!("toolchain directory cannot be empty");
        }
        let canon = base.canonicalize().unwrap_or(base.clone());
        let allowed_root =
            std::env::var("COHESIX_TOOLCHAIN_ROOT").unwrap_or_else(|_| "/mnt/data".into());
        let allowed_root = Path::new(&allowed_root)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(allowed_root));
        if !canon.starts_with(&allowed_root) {
            coh_bail!(
                "toolchain directory must be under {}",
                allowed_root.display()
            );
        }
        Ok(Toolchain { base: canon })
    }

    pub fn get_tool_path(&self, tool_name: &str) -> Result<PathBuf, CohError> {
        if tool_name.trim().is_empty() {
            coh_bail!("tool name cannot be empty");
        }
        if tool_name.contains('/') || tool_name.contains("..") {
            coh_bail!("invalid tool name");
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
    pub fn get_cargo(&self) -> Result<PathBuf, CohError> {
        let path = self.get_tool_path("cargo")?;
        if !path.exists() {
            coh_bail!("cargo not found in toolchain");
        }
        Ok(path)
    }

    pub fn base(&self) -> &Path {
        &self.base
    }
}
