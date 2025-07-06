// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-05

#[cfg(feature = "std")]
pub mod edge_fallback;
#[cfg(feature = "std")]
pub mod failover;
#[cfg(feature = "std")]
pub mod federation;
/// Distributed orchestration layer.
//
/// Provides Queen and Worker coordination as well as
/// federation between Queens.
#[cfg(feature = "std")]
pub mod protocol;
#[cfg(feature = "std")]
pub mod queen;
#[cfg(feature = "std")]
pub mod worker;
