// CLASSIFICATION: COMMUNITY
// Filename: policy.rs v0.3
// Author: Lukas Bower
// Date Modified: 2027-01-31

use alloc::vec::Vec;
use alloc::string::String;
use serde::Deserialize;
use core::str::FromStr;

/// Access type for sandbox policy checks.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Access {
    /// Read-only operation
    Read,
    /// Write or mutate operation
    Write,
}

/// Simple allowlist policy by path prefix.
#[derive(Clone, Default, Deserialize)]
pub struct SandboxPolicy {
    /// Paths that may be read.
    pub read: Vec<String>,
    /// Paths that may be written or created.
    pub write: Vec<String>,
}

impl SandboxPolicy {
    /// Load policy from a JSON file containing `read` and `write` arrays.
    #[cfg(feature = "posix")]
    pub fn from_file(path: &std::path::Path) -> std::io::Result<Self> {
        let txt = std::fs::read_to_string(path)?;
        Self::from_str(&txt).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }



    /// Determine if the given access is allowed on `path`.
    pub fn allows(&self, path: &str, access: Access) -> bool {
        match access {
            Access::Read => self.read.iter().any(|p| path.starts_with(p)),
            Access::Write => self.write.iter().any(|p| path.starts_with(p)),
        }
    }
}

impl FromStr for SandboxPolicy {
    type Err = serde_json::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}
