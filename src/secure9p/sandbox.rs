// CLASSIFICATION: COMMUNITY
// Filename: sandbox.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-25

//! Path sanitization utilities for Secure9P.

#[cfg(feature = "secure9p")]
use anyhow::{anyhow, Result};
#[cfg(feature = "secure9p")]
use std::path::{Path, PathBuf};

/// Validate and canonicalize a requested path relative to the agent root.
#[cfg(feature = "secure9p")]
pub fn validate_path(agent_root: &Path, requested: &Path) -> Result<PathBuf> {
    let full = agent_root.join(requested);
    let canonical = std::fs::canonicalize(&full)?;
    let root = std::fs::canonicalize(agent_root)?;
    if !canonical.starts_with(&root) {
        return Err(anyhow!("path escape"));
    }
    Ok(canonical)
}

#[cfg(all(test, feature = "secure9p"))]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    #[test]
    fn rejects_escape() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("a"), b"x").unwrap();
        let p = validate_path(root, Path::new("a")).unwrap();
        assert!(p.ends_with("a"));
        assert!(validate_path(root, Path::new("../x")).is_err());
    }
}

use super::{policy_engine::PolicyEngine, cap_fid::Capability};
use std::path::Path;

pub fn enforce(ns: &str, cap: Capability, policy: &PolicyEngine) -> bool {
    let ns = ns.trim_end_matches('/');
    let agent = Path::new(ns)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    policy.check(agent, cap)
}
