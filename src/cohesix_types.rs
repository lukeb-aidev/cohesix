// CLASSIFICATION: COMMUNITY
// Filename: cohesix_types.rs v1.4
// Author: Lukas Bower
// Date Modified: 2026-10-28

use std::env;
/// Shared types for Cohesix modules.
use std::fs;

/// Enumeration of sandbox-mediated syscalls used by userland services.
#[derive(Debug, Clone)]
pub enum Syscall {
    /// Spawn a new process with optional arguments.
    Spawn { program: String, args: Vec<String> },
    /// Grant a capability string to a target entity.
    CapGrant { target: String, capability: String },
    /// Mount a source path to a destination within the namespace.
    Mount { src: String, dest: String },
    /// Execute a binary directly.
    Exec { path: String },
    /// Apply a namespace description to the current process.
    ApplyNamespace,
    /// Placeholder for unknown or unsupported syscall variants.
    Unknown,
}

/// Runtime roles recognised by Cohesix.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Role {
    QueenPrimary,
    RegionalQueen,
    BareMetalQueen,
    DroneWorker,
    InteractiveAiBooth,
    KioskInteractive,
    GlassesAgent,
    SensorRelay,
    SimulatorTest,
    Other(String),
}

/// Utility manifest for determining the current role.
pub struct RoleManifest;

impl RoleManifest {
    /// Detect the current role via `/srv/cohrole` or the `COHROLE` env var.
    pub fn current_role() -> Role {
        let role_str = fs::read_to_string("/srv/cohrole")
            .ok()
            .or_else(|| env::var("COHROLE").ok())
            .unwrap_or_else(|| "QueenPrimary".to_string());
        match role_str.trim() {
            "QueenPrimary" => Role::QueenPrimary,
            "RegionalQueen" => Role::RegionalQueen,
            "BareMetalQueen" => Role::BareMetalQueen,
            "DroneWorker" => Role::DroneWorker,
            "InteractiveAiBooth" => Role::InteractiveAiBooth,
            "KioskInteractive" => Role::KioskInteractive,
            "GlassesAgent" => Role::GlassesAgent,
            "SensorRelay" => Role::SensorRelay,
            "SimulatorTest" => Role::SimulatorTest,
            other => Role::Other(other.to_string()),
        }
    }
}

/// Names of roles recognised by the runtime.
pub const VALID_ROLES: &[&str] = &[
    "QueenPrimary",
    "RegionalQueen",
    "BareMetalQueen",
    "DroneWorker",
    "InteractiveAiBooth",
    "KioskInteractive",
    "GlassesAgent",
    "SensorRelay",
    "SimulatorTest",
];

impl RoleManifest {
    /// Check whether `role` matches a known role name.
    pub fn is_valid_role(role: &str) -> bool {
        VALID_ROLES.contains(&role)
    }
}
