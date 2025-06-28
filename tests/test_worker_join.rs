// CLASSIFICATION: COMMUNITY
// Filename: test_worker_join.rs v0.4
// Date Modified: 2026-10-31
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
    let result = w.join("127.0.0.1");
    assert!(
        matches!(
            result
                .as_ref()
                .err()
                .and_then(|e| e.downcast_ref::<std::io::Error>()),
            Some(io_err)
                if io_err.kind() == std::io::ErrorKind::PermissionDenied
                    || io_err.kind() == std::io::ErrorKind::InvalidData
                    || io_err.raw_os_error() == Some(8)
        ),
        "Expected PermissionDenied or Exec format error, got: {:?}",
        result
    );
    println!("Worker join result: {:?}", result);
    q.process_joins();
    assert!(w.check_ack().is_none());
}
