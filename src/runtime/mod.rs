// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.1
// Date Modified: 2026-09-30
// Module list was previously conditional on `target_os != "uefi"`.
// Cohesix is now pure UEFI; all runtime modules are always built.
// Author: Lukas Bower

use crate::prelude::*;
//! Runtime subsystem modules

pub mod env;

pub mod ipc;

pub mod loader;

pub mod role_config;

pub mod service_registry;

pub use env::init::initialize_runtime_env;

pub use service_registry::{ServiceRegistry, TestRegistryGuard};
