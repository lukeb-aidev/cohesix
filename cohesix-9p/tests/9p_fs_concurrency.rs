// CLASSIFICATION: COMMUNITY
// Filename: 9p_fs_concurrency.rs v0.1
// Date Modified: 2026-12-31
// Author: Cohesix Codex

use cohesix_9p::fs::InMemoryFs;
use std::sync::Arc;
use std::thread;

#[test]
fn concurrent_access() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write("/a", b"start", "t");
    let mut handles = Vec::new();
    for i in 0..4 {
        let fs_cl = fs.clone();
        handles.push(thread::spawn(move || {
            for j in 0..50 {
                let path = format!("/file{}_{}", i, j);
                fs_cl.write(&path, b"x", "t");
                let _ = fs_cl.read(&path, "t");
            }
        }));
    }
    for h in handles {
        h.join().expect("thread failed");
    }
}
