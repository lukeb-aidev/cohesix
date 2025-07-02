// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-08-27

/// Filesystem utilities root module.

#[cfg(feature = "minimal_uefi")]
pub use crate::kernel::fs::fat::open_bin;
use crate::prelude::*;

pub mod overlay;
