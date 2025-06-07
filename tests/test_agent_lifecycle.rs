// CLASSIFICATION: COMMUNITY
// Filename: test_agent_lifecycle.rs v0.2
// Date Modified: 2025-07-03
// Author: Cohesix Codex

use cohesix::agents::runtime::AgentRuntime;
use cohesix::physical::sensors;
use cohesix::cohesix_types::Role;
use tempfile::tempdir;

#[test]
fn agent_lifecycle() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    std::fs::create_dir_all("/srv").unwrap();
    let _ = std::fs::remove_file("/srv/telemetry");
    let _ = std::fs::remove_dir_all("/srv/telemetry");

    let mut rt = AgentRuntime::new();
    let args = vec!["true".to_string()];
    let _ = rt.spawn("a1", Role::DroneWorker, &args);
    std::fs::create_dir_all("/srv/agents/a1").ok();
    assert!(std::path::Path::new("/srv/agents/a1").exists());

    sensors::read_temperature("a1");
    rt.terminate("a1").unwrap();

    if let Ok(trace) = std::fs::read_to_string("/srv/agent_trace/a1") {
        assert!(trace.contains("spawn"));
        assert!(trace.contains("terminate"));
    } else {
        panic!("missing trace");
    }
    let tel = std::fs::read_to_string("/srv/telemetry").unwrap();
    assert!(tel.contains("temp"));
}
