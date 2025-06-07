// CLASSIFICATION: COMMUNITY
// Filename: test_worker_join.rs v0.1
// Date Modified: 2025-07-04
// Author: Cohesix Codex

use cohesix::orchestrator::{queen::Queen, worker::Worker};
use std::fs;

#[test]
fn worker_join_ack() {
    fs::create_dir_all("/srv/registry/join").unwrap();
    fs::create_dir_all("/srv/registry/ack").unwrap();
    fs::create_dir_all("/srv/worker").unwrap();

    let mut q = Queen::new(5).unwrap();
    let w = Worker::new("w1", "/srv/registry");
    w.join("127.0.0.1").unwrap();
    q.process_joins();
    let ack = w.check_ack();
    assert!(ack.is_some());
    let ack = ack.unwrap();
    assert_eq!(ack.worker_id, "w1");
}
