// CLASSIFICATION: COMMUNITY
// Filename: cohesix_types.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-06-17

//! Shared types for Cohesix modules.

use std::fs;
use std::env;

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
    /// Placeholder for unknown or unsupported syscall variants.
    Unknown,
}

/// Runtime roles recognised by Cohesix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    QueenPrimary,
    DroneWorker,
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
            "DroneWorker" => Role::DroneWorker,
            "KioskInteractive" => Role::KioskInteractive,
            "GlassesAgent" => Role::GlassesAgent,
            "SensorRelay" => Role::SensorRelay,
            "SimulatorTest" => Role::SimulatorTest,
            other => Role::Other(other.to_string()),
        }
    }
}

