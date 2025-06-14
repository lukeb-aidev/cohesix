// CLASSIFICATION: COMMUNITY
// Filename: cap_fid.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-07-26

//! Capability-enforced Fid wrapper.
//!
//! The [`Capability`] type aliases the internal [`Cap`] bitflag set
//! so other modules can depend on a stable name.

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

/// Public alias for [`Cap`] to present a stable API.
pub type Capability = Cap;

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
