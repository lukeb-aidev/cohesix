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
    fs::create_dir_all("srv").unwrap();
    fs::write("/srv/cohrole", "DroneWorker").unwrap();
    ServiceRegistry::reset();
    ServiceRegistry::register_service("mock1", "/srv/mock1");
    let h = ServiceRegistry::lookup("mock1").expect("lookup failed");
    assert_eq!(h.path, "/srv/mock1");
}

#[test]
#[serial]
fn role_visibility() {
    fs::create_dir_all("srv").unwrap();
    fs::write("/srv/cohrole", "DroneWorker").unwrap();
    ServiceRegistry::reset();
    ServiceRegistry::register_service("worker_only", "/srv/wo");
    fs::write("/srv/cohrole", "KioskInteractive").unwrap();
    assert!(ServiceRegistry::lookup("worker_only").is_none());
    fs::write("/srv/cohrole", "QueenPrimary").unwrap();
    assert!(ServiceRegistry::lookup("worker_only").is_some());
}
