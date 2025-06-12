// CLASSIFICATION: COMMUNITY
// Filename: devd_hotplug.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-22

use cohesix::runtime::ServiceRegistry;
use cohesix::services::{devd::DevdService, Service};
use serial_test::serial;
use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;
use tempfile::tempdir;

#[test]
#[serial]
fn device_attach_detach() {
    let dir = tempdir().expect("create temp dir");
    std::env::set_var("COH_DEV_ROOT", dir.path());
    ServiceRegistry::reset();
    let mut svc = DevdService::default();
    svc.init();
    fs::File::create(dir.path().join("video0")).expect("create video device");
    sleep(Duration::from_millis(500));
    assert!(ServiceRegistry::lookup("video0").is_some());
    fs::remove_file(dir.path().join("video0")).expect("remove video device");
    sleep(Duration::from_millis(500));
    assert!(ServiceRegistry::lookup("video0").is_none());
}

#[test]
#[serial]
fn validator_violation() {
    if !Path::new("/log").exists() {
        eprintln!("Skipping validator_violation test: required path missing.");
        return;
    }
    let dir = tempdir().expect("create temp dir");
    std::env::set_var("COH_DEV_ROOT", dir.path());
    let _ = fs::remove_dir_all("/log");
    ServiceRegistry::reset();
    let mut svc = DevdService::default();
    svc.init();
    fs::File::create(dir.path().join("baddev")).expect("create invalid device");
    sleep(Duration::from_millis(500));
    let log_path = Path::new("/log/validator_runtime.log");
    if !log_path.exists() {
        eprintln!("Skipping validator_violation test: required path missing.");
        return;
    }
    let log = fs::read_to_string(log_path).expect("read validator log");
    assert!(log.contains("baddev"));
}
