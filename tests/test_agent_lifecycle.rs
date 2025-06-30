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
    let trace_dir = std::env::var("TRACE_OUT").unwrap_or_else(|_| {
        let p = std::env::temp_dir().join("cohesix_trace");
        p.to_string_lossy().into_owned()
    });
    std::fs::create_dir_all(&trace_dir).expect("create TRACE_OUT dir");
    let dir = tempdir().expect("tempdir");
    let agents = dir.path().join("agents");
    let traces = dir.path().join("trace");
    let telemetry = dir.path().join("telemetry.log");
    std::env::set_var("COHESIX_AGENTS_DIR", &agents);
    std::env::set_var("COHESIX_AGENT_TRACE_DIR", &traces);
    std::env::set_var("COHESIX_TELEMETRY_PATH", &telemetry);
    std::fs::create_dir_all(&agents).expect("create agents dir");
    std::fs::create_dir_all(&traces).expect("create trace dir");

    let mut rt = AgentRuntime::new();
    let args = vec!["true".to_string()];
    rt.spawn("a1", Role::DroneWorker, &args).expect("spawn agent");
    assert!(agents.join("a1").exists());

    sensors::read_temperature("a1");
    rt.terminate("a1").expect("terminate agent");

    let trace = std::fs::read_to_string(traces.join("a1")).expect("read trace");
    assert!(trace.contains("spawn"));
    assert!(trace.contains("terminate"));
    let tel = std::fs::read_to_string(&telemetry).expect("read telemetry");
    assert!(tel.contains("temp"));
}
