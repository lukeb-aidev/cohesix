// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.1
// Date Modified: 2026-09-30
// Author: Lukas Bower

//! Runtime subsystem modules

pub mod env;
#[cfg(not(target_os = "uefi"))]
pub mod ipc;
#[cfg(not(target_os = "uefi"))]
pub mod loader;
#[cfg(not(target_os = "uefi"))]
pub mod role_config;
#[cfg(not(target_os = "uefi"))]
pub mod service_registry;

pub use env::init::initialize_runtime_env;
#[cfg(not(target_os = "uefi"))]
pub use service_registry::{ServiceRegistry, TestRegistryGuard};
