// CLASSIFICATION: COMMUNITY
// Filename: test_scenario_engine.rs v0.2
// Date Modified: 2025-07-03
// Author: Cohesix Codex

use cohesix::sim::agent_scenario::ScenarioEngine;
use std::fs;
use tempfile::tempdir;

#[test]
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
