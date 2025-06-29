// CLASSIFICATION: COMMUNITY
// Filename: test_service_registry.rs v0.3
// Date Modified: 2026-09-30
// Author: Cohesix Codex

use cohesix::runtime::{ServiceRegistry, TestRegistryGuard};
use env_logger;
use serial_test::serial;
use std::{env, fs};

#[test]
#[serial]
fn register_and_lookup() {
    let _ = env_logger::builder().is_test(true).try_init();
    let _guard = TestRegistryGuard::new();
    ServiceRegistry::clear_all().unwrap();
    let srv_dir = std::env::temp_dir();
    fs::create_dir_all(&srv_dir).unwrap();
    let _ = fs::remove_file("/srv/cohrole");
    let prev = env::var("COHROLE").ok();
    env::set_var("COHROLE", "DroneWorker");
    assert!(ServiceRegistry::list_services().unwrap().is_empty());
    ServiceRegistry::register_service("mock1", srv_dir.join("mock1").to_str().unwrap()).unwrap();
    let list = ServiceRegistry::list_services().unwrap();
    assert_eq!(list, vec!["mock1".to_string()]);
    let h = ServiceRegistry::lookup("mock1").unwrap().expect("lookup failed");
    assert_eq!(h.path, srv_dir.join("mock1").to_str().unwrap());
    match prev {
        Some(v) => env::set_var("COHROLE", v),
        None => env::remove_var("COHROLE"),
    }
}

#[test]
#[serial]
fn role_visibility() {
    let _ = env_logger::builder().is_test(true).try_init();
    let _guard = TestRegistryGuard::new();
    ServiceRegistry::clear_all().unwrap();
    let srv_dir = std::env::temp_dir();
    fs::create_dir_all(&srv_dir).unwrap();
    let _ = fs::remove_file("/srv/cohrole");
    let prev = env::var("COHROLE").ok();
    env::set_var("COHROLE", "DroneWorker");
    assert!(ServiceRegistry::list_services().unwrap().is_empty());
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
    let _ = env_logger::builder().is_test(true).try_init();
    let _guard = TestRegistryGuard::new();
    ServiceRegistry::clear_all().unwrap();
    let srv_dir = std::env::temp_dir();
    fs::create_dir_all(&srv_dir).unwrap();
    let _ = fs::remove_file("/srv/cohrole");
    let prev = env::var("COHROLE").ok();
    env::set_var("COHROLE", "DroneWorker");
    assert!(ServiceRegistry::list_services().unwrap().is_empty());
    ServiceRegistry::register_service("tmp", srv_dir.join("tmp").to_str().unwrap()).unwrap();
    assert!(ServiceRegistry::lookup("tmp").unwrap().is_some());
    ServiceRegistry::unregister_service("tmp").unwrap();
    assert!(ServiceRegistry::lookup("tmp").unwrap().is_none());
    assert!(ServiceRegistry::list_services().unwrap().is_empty());
    match prev {
        Some(v) => env::set_var("COHROLE", v),
        None => env::remove_var("COHROLE"),
    }
}
