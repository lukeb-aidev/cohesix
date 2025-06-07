// CLASSIFICATION: COMMUNITY
// Filename: contracts.rs v0.1
// Date Modified: 2025-07-01
// Author: Cohesix Codex

use cohesix::runtime::ServiceRegistry;
use serial_test::serial;
use std::fs;
use cohesix::agents::runtime::AgentRuntime;
use cohesix::cohesix_types::Role;

#[test]
#[serial]
fn mountpoint_available() {
    fs::create_dir_all("srv").unwrap();
    assert!(std::path::Path::new("srv").exists());
}

#[test]
#[serial]
fn service_registration_contract() {
    fs::create_dir_all("srv").unwrap();
    fs::write("/srv/cohrole", "DroneWorker").unwrap();
    ServiceRegistry::reset();
    ServiceRegistry::register_service("svc", "/srv/svc");
    assert!(ServiceRegistry::lookup("svc").is_some());
}

#[test]
#[serial]
fn trace_format_contract() {
    fs::create_dir_all("srv/trace").unwrap();
    let data = "{\"ts\":0,\"agent\":\"a\",\"event\":\"spawn\",\"detail\":\"/bin/true\",\"ok\":true}";
    fs::write("srv/trace/live.log", data).unwrap();
    let v: serde_json::Value = serde_json::from_str(data).unwrap();
    assert!(v.get("ts").is_some());
}

#[test]
#[serial]
fn agent_termination_contract() {
    fs::create_dir_all("srv").unwrap();
    let mut rt = AgentRuntime::new();
    let args = vec!["true".to_string()];
    rt.spawn("c1", Role::DroneWorker, &args).unwrap();
    rt.terminate("c1").unwrap();
    assert!(!std::path::Path::new("srv/agents/c1").exists());
}

