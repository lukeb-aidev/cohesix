// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-05

//! Distributed orchestration layer.
//!
//! Provides Queen and Worker coordination as well as
//! federation between Queens.

pub mod protocol;
pub mod queen;
pub mod worker;
pub mod federation;
pub mod failover;
