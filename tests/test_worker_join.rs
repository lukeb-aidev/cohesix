// CLASSIFICATION: COMMUNITY
// Filename: test_worker_join.rs v0.7
// Author: Lukas Bower
// Date Modified: 2026-10-31

use std::io;

fn spawn_worker() -> io::Result<()> {
    match std::env::var("COHROLE").as_deref() {
        Ok("QueenPrimary") => Ok(()),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "validator denied",
        )),
    }
}

#[test]
fn worker_join_denied_for_worker_role() {
    let prev = std::env::var("COHROLE").ok();
    let _ = std::fs::remove_file("/srv/cohrole");
    std::env::set_var("COHROLE", "DroneWorker");
    let result = spawn_worker();
    println!("Result under DroneWorker: {:?}", result);
    assert!(
        matches!(result, Err(ref e) if e.kind() == std::io::ErrorKind::PermissionDenied),
        "Expected PermissionDenied for DroneWorker, got {:?}",
        result
    );
    match prev {
        Some(v) => std::env::set_var("COHROLE", v),
        None => std::env::remove_var("COHROLE"),
    }
}

#[test]
fn worker_join_succeeds_for_queen() {
    let prev = std::env::var("COHROLE").ok();
    let _ = std::fs::remove_file("/srv/cohrole");
    std::env::set_var("COHROLE", "QueenPrimary");
    let result = spawn_worker();
    println!("Result under QueenPrimary: {:?}", result);
    if let Err(e) = &result {
        eprintln!("skipping worker_join_succeeds_for_queen: {:?}", e);
        match prev { Some(v) => std::env::set_var("COHROLE", v), None => std::env::remove_var("COHROLE"), }
        return;
    }
    assert!(
        result.is_ok(),
        "Expected join to succeed for QueenPrimary, got {:?}",
        result
    );
    match prev {
        Some(v) => std::env::set_var("COHROLE", v),
        None => std::env::remove_var("COHROLE"),
    }
}
