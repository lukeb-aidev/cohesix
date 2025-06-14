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
    fs::write(srv_dir.join("cohrole"), "DroneWorker").unwrap();
    ServiceRegistry::reset();
    ServiceRegistry::register_service("mock1", srv_dir.join("mock1").to_str().unwrap());
    let h = ServiceRegistry::lookup("mock1").expect("lookup failed");
    assert_eq!(h.path, srv_dir.join("mock1").to_str().unwrap());
}

#[test]
#[serial]
fn role_visibility() {
    fs::create_dir_all("srv").unwrap();
    fs::write(srv_dir.join("cohrole"), "DroneWorker").unwrap();
    ServiceRegistry::reset();
    ServiceRegistry::register_service("worker_only", srv_dir.join("wo").to_str().unwrap());
    fs::write(srv_dir.join("cohrole"), "KioskInteractive").unwrap();
    assert!(ServiceRegistry::lookup("worker_only").is_none());
    fs::write(srv_dir.join("cohrole"), "QueenPrimary").unwrap();
    assert!(ServiceRegistry::lookup("worker_only").is_some());
}

#[test]
#[serial]
fn unregister() {
    fs::create_dir_all("srv").unwrap();
    fs::write(srv_dir.join("cohrole"), "DroneWorker").unwrap();
    ServiceRegistry::reset();
    ServiceRegistry::register_service("tmp", srv_dir.join("tmp").to_str().unwrap());
    ServiceRegistry::unregister_service("tmp");
    assert!(ServiceRegistry::lookup("tmp").is_none());
}
