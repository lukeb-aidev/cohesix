// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-05

pub mod edge_fallback;
pub mod failover;
pub mod federation;
/// Distributed orchestration layer.
//
/// Provides Queen and Worker coordination as well as
/// federation between Queens.
pub mod protocol;
pub mod queen;
pub mod worker;
