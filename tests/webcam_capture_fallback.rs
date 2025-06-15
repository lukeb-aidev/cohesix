// CLASSIFICATION: COMMUNITY
// Filename: webcam_capture_fallback.rs v0.1
// Author: Cohesix Codex
// Date Modified: 2025-07-13

use cohesix::webcam::capture;
use std::fs;
use tempfile::tempdir;
use serial_test::serial;

#[test]
#[serial]
fn capture_dummy_when_missing() {
    let dir = tempdir().unwrap();
    let out = dir.path().join("frame.jpg");
    unsafe {
        std::env::set_var("VIDEO_DEVICE", dir.path().join("missing").to_str().unwrap());
    }
    capture::capture_jpeg(out.to_str().unwrap()).unwrap();
    assert!(out.exists());
    let data = fs::read(&out).unwrap();
    assert!(!data.is_empty());
}
