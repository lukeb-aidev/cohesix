// CLASSIFICATION: COMMUNITY
// Filename: sim_deterministic.rs v0.1
// Author: Cohesix Codex
// Date Modified: 2025-07-13

use cohesix::sim::rapier_bridge::deterministic_harness;
use serial_test::serial;
use tempfile::tempdir;

#[test]
#[serial]
fn deterministic_output_matches() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let log1 = deterministic_harness(42, 5);
    let first = std::fs::read_to_string("/srv/trace/sim.log").unwrap();
    std::fs::remove_dir_all("srv").ok();
    std::fs::remove_dir_all("sim").ok();

    let _ = deterministic_harness(42, 5);
    let second = std::fs::read_to_string("/srv/trace/sim.log").unwrap();

    assert_eq!(first, second);
    assert_eq!(log1, deterministic_harness(42, 5));
}
