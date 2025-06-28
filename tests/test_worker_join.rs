// CLASSIFICATION: COMMUNITY
// Filename: test_worker_join.rs v0.2
// Date Modified: 2026-10-30
// Author: Cohesix Codex

use cohesix::orchestrator::{queen::Queen, worker::Worker};
use std::fs;

#[test]
fn worker_join_ack() {
    std::env::set_var("COHROLE", "QueenPrimary");
    fs::create_dir_all("/srv/registry/join").unwrap();
    fs::create_dir_all("/srv/registry/ack").unwrap();
    fs::create_dir_all("/srv/worker").unwrap();

    let mut q = Queen::new(5).unwrap();
    let w = Worker::new("w1", "/srv/registry");
    let res = w.join("127.0.0.1");
    if let Err(e) = &res {
        println!("Got expected error: {:?}", e);
    }
    res.unwrap();
    q.process_joins();
    let ack = w.check_ack();
    assert!(ack.is_some());
    let ack = ack.unwrap();
    assert_eq!(ack.worker_id, "w1");
}
