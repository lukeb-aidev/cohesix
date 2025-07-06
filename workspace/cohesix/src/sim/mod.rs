// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.4
// Author: Lukas Bower
// Date Modified: 2025-07-22

pub mod agent_scenario;
pub mod introspect;
#[cfg(feature = "rapier")]
pub mod physics_adapter;
#[cfg(feature = "rapier")]
pub mod physics_demo;
/// Simulation subsystem modules.

#[cfg(feature = "rapier")]
pub mod rapier_bridge;
