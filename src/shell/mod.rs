// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-22

use crate::prelude::*;
/// Shell utilities including BusyBox runner.

#[cfg(feature = "busybox_client")]
pub mod busybox_runner;
