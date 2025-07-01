// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-10-28

use crate::prelude::*;
/// Role modules for additional Cohesix roles.

pub mod interactive_aibooth;
pub mod drone_worker;
pub mod kiosk_interactive;
pub mod glasses_agent;
pub mod sensor_relay;
pub mod simulator_test;

/// Dispatch role initialization based on the role string.
pub fn start_role(role: &str) {
    match role {
        "InteractiveAiBooth" => interactive_aibooth::start(),
        "DroneWorker" => drone_worker::start(),
        "KioskInteractive" => kiosk_interactive::start(),
        "GlassesAgent" => glasses_agent::start(),
        "SensorRelay" => sensor_relay::start(),
        "SimulatorTest" => simulator_test::start(),
        other => println!("[roles] unknown role: {}", other),
    }
}
