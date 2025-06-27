// CLASSIFICATION: COMMUNITY
// Filename: test_scenario_engine.rs v0.3
// Date Modified: 2026-09-21
// Author: Cohesix Codex

use cohesix::sim::agent_scenario::ScenarioEngine;
use serial_test::serial;
use std::fs;
use tempfile::tempdir;

// This test validates that ScenarioEngine can execute a minimal scenario file.
// If it fails, run with `RUST_BACKTRACE=full` and ensure the `boot/scenario.json`
// path is correct and writable.

#[test]
#[serial]
fn run_scenario() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    fs::create_dir_all("boot").unwrap();
    let scenario = r#"{
        "id": "scn1",
        "agents": [
            {"id": "a1", "role": "DroneWorker", "cmd": "true"}
        ]
    }"#;
    fs::write("boot/scenario.json", scenario).unwrap();
    ScenarioEngine::run(std::path::Path::new("boot/scenario.json")).unwrap();
    assert!(std::path::Path::new("/srv/scenario_result/scn1").exists());
}

#[test]
#[serial]
fn run_scenario_invalid_cmd() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    fs::create_dir_all("boot").unwrap();
    let scenario = r#"{
        "id": "scn_bad",
        "agents": [
            {"id": "a1", "role": "DroneWorker", "cmd": "/no/such/cmd"}
        ]
    }"#;
    fs::write("boot/scenario.json", scenario).unwrap();
    let err = ScenarioEngine::run(std::path::Path::new("boot/scenario.json"))
        .expect_err("scenario should fail");
    assert!(err.to_string().contains("failed to spawn agent"));
}
