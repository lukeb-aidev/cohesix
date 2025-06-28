// CLASSIFICATION: COMMUNITY
// Filename: test_worker_join.rs v0.5
// Date Modified: 2026-10-31
// Author: Lukas Bower

use cohesix::orchestrator::{queen::Queen, worker::Worker};
use std::fs;
use cohesix::seL4::syscall::exec;
use std::io;

#[test]
fn worker_join_ack() {
    fs::create_dir_all("/srv/registry/join").unwrap();
    fs::create_dir_all("/srv/registry/ack").unwrap();
    fs::create_dir_all("/srv/worker").unwrap();
    let _ = fs::remove_file("/srv/registry/ack/w1.msg");

    let mut q = Queen::new(5).unwrap();
    let w = Worker::new("w1", "/srv/registry");
    let result = spawn_worker();
    println!("Worker join result: {:?}", result);
    assert!(matches!(result, Err(_)), "Expected exec to fail, got: {:?}", result);
    q.process_joins();
    assert!(w.check_ack().is_none());
}

fn spawn_worker() -> io::Result<()> {
    exec("worker", &[])
}
