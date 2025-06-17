// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-07-22

//! Simulation subsystem modules.

#[cfg(feature = "rapier")]
pub mod rapier_bridge;
pub mod agent_scenario;
#[cfg(feature = "rapier")]
pub mod physics_adapter;
pub mod introspect;
#[cfg(feature = "rapier")]
pub mod physics_demo;
#[cfg(feature = "rapier")]
pub mod webcam_tilt;
