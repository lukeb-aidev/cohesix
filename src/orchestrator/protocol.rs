// CLASSIFICATION: COMMUNITY
// Filename: protocol.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-04

//! Orchestration protocol message types.
//!
//! Structures are serialized using MessagePack via `rmp-serde`.

use serde::{Deserialize, Serialize};

/// Join request sent from a Worker to a Queen.
#[derive(Debug, Serialize, Deserialize)]
pub struct JoinRequest {
    /// Unique worker identifier.
    pub worker_id: String,
    /// Advertised IP address.
    pub ip: String,
}

/// Worker role report after joining.
#[derive(Debug, Serialize, Deserialize)]
pub struct RoleReport {
    /// Worker identifier.
    pub worker_id: String,
    /// Current role string.
    pub role: String,
}

/// Ping packet to validate liveness.
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthPing {
    /// Worker identifier.
    pub worker_id: String,
    /// Unix timestamp of the ping.
    pub ts: u64,
}

/// Agent scheduling directive.
#[derive(Debug, Serialize, Deserialize)]
pub struct AgentSchedule {
    /// Agent identifier to spawn or migrate.
    pub agent_id: String,
    /// Target worker identifier.
    pub worker_id: String,
    /// Desired role for the agent.
    pub role: String,
}
