// CLASSIFICATION: COMMUNITY
// Filename: test_physics_loop.rs v0.1
// Date Modified: 2025-07-22

#![cfg(feature = "rapier")]
// Author: Cohesix Codex

use cohesix::sim::rapier_bridge::{SimBridge, SimCommand};
use rapier3d::prelude::*;
use tempfile::tempdir;

#[test]
fn physics_loop_advances() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    std::fs::create_dir_all("sim").unwrap();

    let bridge = SimBridge::start();
    bridge.send(SimCommand::AddSphere {
        radius: 1.0,
        position: vector![0.0, 2.0, 0.0],
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    bridge.send(SimCommand::ApplyForce {
        id: RigidBodyHandle::from_raw_parts(0, 0),
        force: vector![0.0, -1.0, 0.0],
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    let state = std::fs::read_to_string("sim/state").unwrap_or_default();
    assert!(!state.trim().is_empty(), "state should not be empty");
}
