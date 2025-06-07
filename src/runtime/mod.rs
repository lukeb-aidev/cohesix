// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.0
// Date Modified: 2025-06-02
// Author: Lukas Bower

//! Runtime subsystem modules

pub mod env;
pub mod ipc;
pub mod service_registry;

pub use env::init::initialize_runtime_env;
pub use service_registry::ServiceRegistry;
