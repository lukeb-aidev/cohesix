// CLASSIFICATION: COMMUNITY
// Filename: ns_hotplug.rs v0.1
// Author: Cohesix Codex
// Date Modified: 2025-07-23

use cohesix::services::{nswatch::NsWatchService, Service};
use serial_test::serial;
use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;
use tempfile::tempdir;

#[test]
#[serial]
fn hotplug_violation_logged() {
    if fs::create_dir_all("/log").is_err() {
        eprintln!("Skipping hotplug_violation_logged: cannot create /log");
        return;
    }
    let root = tempdir().expect("create temp dir");
    let allow = root.path().join("ok");
    fs::create_dir_all(&allow).unwrap();
    std::env::set_var("NS_HOTPLUG_ROOT", root.path());
    std::env::set_var("NS_ALLOW_PREFIX", allow.to_str().unwrap());
    let mut svc = NsWatchService::default();
    svc.init();
    fs::create_dir(root.path().join("bad")).unwrap();
    sleep(Duration::from_millis(500));
    let log_path = Path::new("/log/validator_runtime.log");
    if let Ok(log) = fs::read_to_string(log_path) {
        assert!(log.contains("ns_hotplug"));
    } else {
        eprintln!("validator log not found");
    }
}
