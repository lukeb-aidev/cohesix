// CLASSIFICATION: COMMUNITY
// Filename: 9p_server.rs v0.1
// Date Modified: 2025-07-14
// Author: Cohesix Codex

#[path = "../src/lib/9p/server.rs"]
mod server;
#[path = "../src/lib/9p/protocol.rs"]
mod protocol;
use server::handle_9p_session;
use protocol::{parse_message, P9Message};
use serial_test::serial;

fn msg(op: u8, path: &str) -> Vec<u8> {
    let mut v = vec![op];
    v.extend_from_slice(path.as_bytes());
    v
}

#[test]
#[serial]
fn walk_srv() {
    std::fs::create_dir_all("/srv").unwrap();
    std::fs::write("/srv/cohrole", "QueenPrimary").unwrap();
    let resp = handle_9p_session(&msg(0x03, "/srv"));
    assert!(matches!(parse_message(&resp), P9Message::Rwalk));
}

#[test]
#[serial]
fn worker_write_denied() {
    std::fs::write("/srv/cohrole", "DroneWorker").unwrap();
    let resp = handle_9p_session(&msg(0x09, "/proc/x"));
    assert!(matches!(parse_message(&resp), P9Message::Unknown(0xfd)));
}
