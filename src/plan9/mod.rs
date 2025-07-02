// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-18

use crate::prelude::*;
/// Plan 9 userland utilities.
pub mod fs;
pub mod namespace;
pub mod shell;
pub mod shim;
pub mod syscalls;

pub mod srv_registry;
