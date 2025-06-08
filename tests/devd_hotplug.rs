// CLASSIFICATION: COMMUNITY
// Filename: devd_hotplug.rs v0.1
// Author: Cohesix Codex
// Date Modified: 2025-07-13

use cohesix::services::{devd::DevdService, Service};
use cohesix::runtime::ServiceRegistry;
use std::fs;
use std::thread::sleep;
use std::time::Duration;
use tempfile::tempdir;
use serial_test::serial;

#[test]
#[serial]
fn device_attach_detach() {
    let dir = tempdir().unwrap();
    std::env::set_var("COH_DEV_ROOT", dir.path());
    ServiceRegistry::reset();
    let mut svc = DevdService::default();
    svc.init();
    fs::File::create(dir.path().join("video0")).unwrap();
    sleep(Duration::from_millis(500));
    assert!(ServiceRegistry::lookup("video0").is_some());
    fs::remove_file(dir.path().join("video0")).unwrap();
    sleep(Duration::from_millis(500));
    assert!(ServiceRegistry::lookup("video0").is_none());
}

#[test]
#[serial]
fn validator_violation() {
    let dir = tempdir().unwrap();
    std::env::set_var("COH_DEV_ROOT", dir.path());
    let _ = fs::remove_dir_all("/log");
    ServiceRegistry::reset();
    let mut svc = DevdService::default();
    svc.init();
    fs::File::create(dir.path().join("baddev")).unwrap();
    sleep(Duration::from_millis(500));
    let log = fs::read_to_string("/log/validator_runtime.log").unwrap();
    assert!(log.contains("baddev"));
}
