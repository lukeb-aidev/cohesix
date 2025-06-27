// CLASSIFICATION: COMMUNITY
// Filename: test_service_registry.rs v0.2
// Date Modified: 2026-09-24
// Author: Cohesix Codex

use cohesix::runtime::ServiceRegistry;
use serial_test::serial;
use std::{env, fs};

#[test]
#[serial]
fn register_and_lookup() {
    let srv_dir = std::env::temp_dir();
    fs::create_dir_all(&srv_dir).unwrap();
    let _ = fs::remove_file("/srv/cohrole");
    let prev = env::var("COHROLE").ok();
    env::set_var("COHROLE", "DroneWorker");
    ServiceRegistry::reset().unwrap();
    ServiceRegistry::register_service("mock1", srv_dir.join("mock1").to_str().unwrap()).unwrap();
    let h = ServiceRegistry::lookup("mock1")
        .unwrap()
        .expect("lookup failed");
    assert_eq!(h.path, srv_dir.join("mock1").to_str().unwrap());
    match prev {
        Some(v) => env::set_var("COHROLE", v),
        None => env::remove_var("COHROLE"),
    }
}

#[test]
#[serial]
fn role_visibility() {
    let srv_dir = std::env::temp_dir();
    fs::create_dir_all(&srv_dir).unwrap();
    let _ = fs::remove_file("/srv/cohrole");
    let prev = env::var("COHROLE").ok();
    env::set_var("COHROLE", "DroneWorker");
    ServiceRegistry::reset().unwrap();
    ServiceRegistry::register_service("worker_only", srv_dir.join("wo").to_str().unwrap()).unwrap();
    env::set_var("COHROLE", "KioskInteractive");
    assert!(ServiceRegistry::lookup("worker_only").unwrap().is_none());
    env::set_var("COHROLE", "QueenPrimary");
    assert!(ServiceRegistry::lookup("worker_only").unwrap().is_some());
    match prev {
        Some(v) => env::set_var("COHROLE", v),
        None => env::remove_var("COHROLE"),
    }
}

#[test]
#[serial]
fn unregister() {
    let srv_dir = std::env::temp_dir();
    fs::create_dir_all(&srv_dir).unwrap();
    let _ = fs::remove_file("/srv/cohrole");
    let prev = env::var("COHROLE").ok();
    env::set_var("COHROLE", "DroneWorker");
    ServiceRegistry::reset().unwrap();
    ServiceRegistry::register_service("tmp", srv_dir.join("tmp").to_str().unwrap()).unwrap();
    ServiceRegistry::unregister_service("tmp").unwrap();
    assert!(ServiceRegistry::lookup("tmp").unwrap().is_none());
    match prev {
        Some(v) => env::set_var("COHROLE", v),
        None => env::remove_var("COHROLE"),
    }
}
