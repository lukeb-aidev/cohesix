// CLASSIFICATION: COMMUNITY
// Filename: cap_fid.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-23

//! Capability-enforced Fid wrapper.

#[cfg(feature = "secure9p")]
use anyhow::{anyhow, Result};
#[cfg(feature = "secure9p")]
use bitflags::bitflags;
#[cfg(feature = "secure9p")]
use std::path::PathBuf;

#[cfg(feature = "secure9p")]
bitflags! {
    pub struct Cap: u8 {
        const READ = 0b0001;
        const WRITE = 0b0010;
        const REMOVE = 0b0100;
    }
}

#[cfg(feature = "secure9p")]
#[derive(Clone)]
pub struct CapFid {
    pub path: PathBuf,
    caps: Cap,
}

#[cfg(feature = "secure9p")]
impl CapFid {
    pub fn new(path: PathBuf, caps: Cap) -> Self {
        Self { path, caps }
    }

    pub fn check(&self, need: Cap) -> Result<()> {
        if self.caps.contains(need) { Ok(()) } else { Err(anyhow!("AccessDenied")) }
    }
}

#[cfg(all(test, feature = "secure9p"))]
mod tests {
    use super::*;
    #[test]
    fn denies_missing_cap() {
        let f = CapFid::new(PathBuf::from("/a"), Cap::READ);
        assert!(f.check(Cap::WRITE).is_err());
    }
}
