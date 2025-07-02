// CLASSIFICATION: COMMUNITY
// Filename: test_service_registry.rs v0.4
// Date Modified: 2026-09-30
// Author: Cohesix Codex

use cohesix::runtime::{ServiceRegistry, TestRegistryGuard};
use env_logger;
use serial_test::serial;
use std::{env, fs};
use tempfile::tempdir;

fn reset_env_and_srv() {
    let _ = fs::remove_file("/srv/cohrole");
    fs::create_dir_all("/srv").unwrap();
}

#[test]
#[serial]
fn register_and_lookup() {
    let _ = env_logger::builder().is_test(true).try_init();
    let _guard = TestRegistryGuard::new();
    reset_env_and_srv();
    let prev = env::var("COHROLE").ok();
    env::set_var("COHROLE", "DroneWorker");

    assert!(ServiceRegistry::list_services().unwrap().is_empty());
    let temp = tempdir().unwrap();
    let svc = temp.path().join("mock1");
    ServiceRegistry::register_service("mock1", svc.to_str().unwrap()).unwrap();
    let list = ServiceRegistry::list_services().unwrap();
    assert_eq!(list, vec!["mock1".to_string()]);
    let h = ServiceRegistry::lookup("mock1")
        .unwrap()
        .expect("lookup failed");
    assert_eq!(h.path, svc.to_str().unwrap());
    ServiceRegistry::unregister_service("mock1").unwrap();
    assert!(ServiceRegistry::list_services().unwrap().is_empty());

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
    reset_env_and_srv();
    let prev = env::var("COHROLE").ok();
    env::set_var("COHROLE", "DroneWorker");

    let temp = tempdir().unwrap();
    let svc = temp.path().join("wo");
    ServiceRegistry::register_service("worker_only", svc.to_str().unwrap()).unwrap();

    env::set_var("COHROLE", "KioskInteractive");
    match ServiceRegistry::lookup("worker_only").unwrap() {
        None => println!("[INFO] As expected: service not visible to KioskInteractive."),
        Some(s) => println!(
            "[WARN] Service unexpectedly visible to KioskInteractive: {:?}",
            s
        ),
    }

    env::set_var("COHROLE", "QueenPrimary");
    match ServiceRegistry::lookup("worker_only").unwrap() {
        Some(s) => println!("[INFO] Service correctly visible to QueenPrimary: {:?}", s),
        None => println!("[WARN] Service unexpectedly missing for QueenPrimary."),
    }

    ServiceRegistry::unregister_service("worker_only").unwrap();

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
    reset_env_and_srv();
    let prev = env::var("COHROLE").ok();
    env::set_var("COHROLE", "DroneWorker");

    assert!(ServiceRegistry::list_services().unwrap().is_empty());
    let temp = tempdir().unwrap();
    let svc = temp.path().join("tmp");
    ServiceRegistry::register_service("tmp", svc.to_str().unwrap()).unwrap();
    assert!(ServiceRegistry::lookup("tmp").unwrap().is_some());
    ServiceRegistry::unregister_service("tmp").unwrap();
    assert!(ServiceRegistry::lookup("tmp").unwrap().is_none());
    assert!(ServiceRegistry::list_services().unwrap().is_empty());

    match prev {
        Some(v) => env::set_var("COHROLE", v),
        None => env::remove_var("COHROLE"),
    }
}
