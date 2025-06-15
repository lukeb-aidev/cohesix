// CLASSIFICATION: COMMUNITY
// Filename: 9p_server.rs v0.2
// Date Modified: 2025-07-22
// Author: Cohesix Codex

#[path = "../src/lib/9p/server.rs"]
mod server;
#[path = "../src/lib/9p/protocol.rs"]
mod protocol;
use server::handle_9p_session;
use protocol::{parse_message, P9Message};
use serial_test::serial;
use tempfile::tempdir;
use std::io;
use std::path::PathBuf;

struct TestEnv {
    _dir: tempfile::TempDir,
    cohrole: PathBuf,
}

fn setup(role: &str) -> io::Result<TestEnv> {
    let dir = tempdir()?;
    let cohrole = dir.path().join("cohrole");
    std::fs::write(&cohrole, role)?;
    Ok(TestEnv { _dir: dir, cohrole })
}

fn msg(op: u8, path: &str) -> Vec<u8> {
    let mut v = vec![op];
    v.extend_from_slice(path.as_bytes());
    v
}

#[test]
#[serial]
fn walk_srv() -> io::Result<()> {
    let env = setup("QueenPrimary")?;
    unsafe {
        std::env::set_var("COHROLE_PATH", &env.cohrole);
    }
    let resp = handle_9p_session(&msg(0x03, "/srv"));
    unsafe {
        std::env::remove_var("COHROLE_PATH");
    }
    assert!(matches!(parse_message(&resp), P9Message::Rwalk));
    Ok(())
}

#[test]
#[serial]
fn worker_write_denied() -> io::Result<()> {
    let env = setup("DroneWorker")?;
    unsafe {
        std::env::set_var("COHROLE_PATH", &env.cohrole);
    }
    let resp = handle_9p_session(&msg(0x09, "/proc/x"));
    unsafe {
        std::env::remove_var("COHROLE_PATH");
    }
    assert!(matches!(parse_message(&resp), P9Message::Unknown(0xfd)));
    Ok(())
}
