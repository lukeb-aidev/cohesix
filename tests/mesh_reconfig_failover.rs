// CLASSIFICATION: COMMUNITY
// Filename: mesh_reconfig_failover.rs v0.2
// Date Modified: 2025-08-01
// Author: Cohesix Codex

use cohesix::worker::queen_watchdog::QueenWatchdog;
use std::env;
use std::fs;
use std::thread::sleep;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn promotes_on_missed_heartbeats() {
    let dir = tempdir().expect("Failed to create temp queen dir");
    let qdir = dir.path().join("queen");
    fs::create_dir_all(&qdir).expect("Failed to create queen dir");
    unsafe {
        env::set_var("COHESIX_QUEEN_DIR", &qdir);
    }

    let hb = qdir.join("heartbeat");
    fs::write(&hb, "1").expect("Failed to write heartbeat");
    let mut wd = QueenWatchdog::new(3);
    wd.check();
    sleep(Duration::from_millis(600));
    fs::remove_file(&hb).expect("Failed to remove heartbeat");
    for _ in 0..3 {
        wd.check();
    }
    let role = fs::read_to_string(qdir.join("role"))
        .expect("Failed to bind or promote: check test permissions or replace with temp socket");
    assert_eq!(role, "QueenPrimary");
    unsafe {
        env::remove_var("COHESIX_QUEEN_DIR");
    }
}
