// CLASSIFICATION: COMMUNITY
// Filename: physics_demo_test.rs v0.1
// Author: Cohesix Codex
// Date Modified: 2025-07-11

#[cfg(feature = "rapier")]
use cohesix::sim::physics_demo::run_demo;
#[cfg(feature = "rapier")]
use std::fs;
#[cfg(feature = "rapier")]
use std::path::Path;

#[cfg(feature = "rapier")]
#[test]
fn physics_demo_creates_trace() {
    fs::create_dir_all("trace").unwrap();
    run_demo();
    assert!(Path::new("/trace/last_sim.json").exists());
}
