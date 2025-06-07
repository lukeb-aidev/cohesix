// CLASSIFICATION: COMMUNITY
// Filename: test_agent_lifecycle.rs v0.1
// Date Modified: 2025-06-21
// Author: Cohesix Codex

use cohesix::agents::runtime::AgentRuntime;
use cohesix::physical::sensors;
use cohesix::cohesix_types::Role;
use tempfile::tempdir;

#[test]
fn agent_lifecycle() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    std::fs::create_dir_all("srv").unwrap();

    let mut rt = AgentRuntime::new();
    let args = vec!["true".to_string()];
    rt.spawn("a1", Role::DroneWorker, &args).unwrap();
    assert!(std::path::Path::new("srv/agents/a1").exists());

    sensors::read_temperature("a1");
    rt.terminate("a1").unwrap();

    let trace = std::fs::read_to_string("srv/agent_trace/a1").unwrap();
    assert!(trace.contains("spawn"));
    assert!(trace.contains("terminate"));
    let tel = std::fs::read_to_string("srv/telemetry").unwrap();
    assert!(tel.contains("temp"));
}
