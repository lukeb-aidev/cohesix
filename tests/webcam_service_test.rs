// CLASSIFICATION: COMMUNITY
// Filename: webcam_service_test.rs v0.1
// Author: Cohesix Codex
// Date Modified: 2025-07-11

use cohesix::services::webcam::WebcamService;
use cohesix::services::Service;
use cohesix::runtime::ServiceRegistry;
use std::fs;
use std::path::Path;
use serial_test::serial;

#[test]
#[serial]
fn webcam_permission_check() {
    fs::create_dir_all("srv").unwrap();
    let _ = fs::remove_dir_all("/srv/webcam");
    fs::write("/srv/cohrole", "QueenPrimary").unwrap();
    ServiceRegistry::reset().unwrap();
    let mut svc = WebcamService::default();
    svc.init();
    assert!(!Path::new("/srv/webcam").exists());

    fs::write("/srv/cohrole", "DroneWorker").unwrap();
    ServiceRegistry::reset().unwrap();
    let mut svc = WebcamService::default();
    svc.init();
    assert!(Path::new("/srv/webcam").exists());
}
