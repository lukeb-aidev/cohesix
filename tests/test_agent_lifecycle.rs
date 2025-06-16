// CLASSIFICATION: COMMUNITY
// Filename: test_agent_lifecycle.rs v0.2
// Date Modified: 2025-07-03
// Author: Cohesix Codex

use cohesix::agents::runtime::AgentRuntime;
use cohesix::cohesix_types::Role;
use cohesix::physical::sensors;
use tempfile::tempdir;

#[test]
fn agent_lifecycle() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    std::fs::create_dir_all("/srv").unwrap();
    let agents = dir.path().join("agents");
    let traces = dir.path().join("trace");
    std::env::set_var("COHESIX_AGENTS_DIR", &agents);
    std::env::set_var("COHESIX_AGENT_TRACE_DIR", &traces);
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::create_dir_all(&traces).unwrap();

    let mut rt = AgentRuntime::new();
    let args = vec!["true".to_string()];
    let _ = rt.spawn("a1", Role::DroneWorker, &args);
    assert!(agents.join("a1").exists());

    sensors::read_temperature("a1");
    rt.terminate("a1").unwrap();

    if let Ok(trace) = std::fs::read_to_string(traces.join("a1")) {
        assert!(trace.contains("spawn"));
        assert!(trace.contains("terminate"));
    } else {
        panic!("missing trace");
    }
    let tel = std::fs::read_to_string("/srv/telemetry").unwrap();
    assert!(tel.contains("temp"));
}
