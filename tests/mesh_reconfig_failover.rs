// CLASSIFICATION: COMMUNITY
// Filename: mesh_reconfig_failover.rs v0.1
// Date Modified: 2025-07-09
// Author: Cohesix Codex

use cohesix::worker::queen_watchdog::QueenWatchdog;
use std::fs;
use std::thread::sleep;
use std::time::Duration;

#[test]
fn promotes_on_missed_heartbeats() {
    fs::create_dir_all("/srv/queen").unwrap();
    fs::write("/srv/queen/heartbeat", "1").unwrap();
    let mut wd = QueenWatchdog::new(3);
    wd.check();
    sleep(Duration::from_millis(600));
    fs::remove_file("/srv/queen/heartbeat").unwrap();
    for _ in 0..3 { wd.check(); }
    let role = fs::read_to_string("/srv/queen/role").unwrap();
    assert_eq!(role, "QueenPrimary");
}
