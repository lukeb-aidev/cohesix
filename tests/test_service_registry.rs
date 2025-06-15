// CLASSIFICATION: COMMUNITY
// Filename: test_service_registry.rs v0.1
// Date Modified: 2025-06-19
// Author: Cohesix Codex

use cohesix::runtime::ServiceRegistry;
use serial_test::serial;
use std::fs;

#[test]
#[serial]
fn register_and_lookup() {
    let srv_dir = std::env::temp_dir();
    fs::create_dir_all(&srv_dir).unwrap();
    fs::write("/srv/cohrole", "DroneWorker").unwrap();
    ServiceRegistry::reset().unwrap();
    ServiceRegistry::register_service("mock1", srv_dir.join("mock1").to_str().unwrap()).unwrap();
    let h = ServiceRegistry::lookup("mock1").unwrap().expect("lookup failed");
    assert_eq!(h.path, srv_dir.join("mock1").to_str().unwrap());
}

#[test]
#[serial]
fn role_visibility() {
    let srv_dir = std::env::temp_dir();
    fs::create_dir_all(&srv_dir).unwrap();
    fs::write(srv_dir.join("cohrole"), "DroneWorker").unwrap();
    ServiceRegistry::reset().unwrap();
    ServiceRegistry::register_service("worker_only", srv_dir.join("wo").to_str().unwrap()).unwrap();
    fs::write("/srv/cohrole", "KioskInteractive").unwrap();
    assert!(ServiceRegistry::lookup("worker_only").unwrap().is_none());
    fs::write("/srv/cohrole", "QueenPrimary").unwrap();
    assert!(ServiceRegistry::lookup("worker_only").unwrap().is_some());
}

#[test]
#[serial]
fn unregister() {
    let srv_dir = std::env::temp_dir();
    fs::create_dir_all(&srv_dir).unwrap();
    fs::write("/srv/cohrole", "DroneWorker").unwrap();
    ServiceRegistry::reset().unwrap();
    ServiceRegistry::register_service("tmp", srv_dir.join("tmp").to_str().unwrap()).unwrap();
    ServiceRegistry::unregister_service("tmp").unwrap();
    assert!(ServiceRegistry::lookup("tmp").unwrap().is_none());
}
