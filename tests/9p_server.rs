// CLASSIFICATION: COMMUNITY
// Filename: 9p_server.rs v0.3
// Date Modified: 2026-02-20
// Author: Cohesix Codex

#[path = "../src/lib/9p/protocol.rs"]
mod protocol;
extern crate alloc;
#[path = "../src/lib/9p/server.rs"]
mod server;
use protocol::{parse_message, P9Message};
use serial_test::serial;
use server::handle_9p_session;
use std::io;
use std::path::PathBuf;
use tempfile::tempdir;

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
    server::reset_fs();
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
    server::reset_fs();
    let env = setup("DroneWorker")?;
    unsafe {
        std::env::set_var("COHROLE_PATH", &env.cohrole);
    }
    let resp = handle_9p_session(&msg(0x09, "/proc/x"));
    unsafe {
        std::env::remove_var("COHROLE_PATH");
    }
    assert!(matches!(parse_message(&resp), P9Message::Rwrite));
    Ok(())
}

#[test]
#[serial]
fn queen_write_and_read() -> io::Result<()> {
    server::reset_fs();
    let env = setup("QueenPrimary")?;
    unsafe {
        std::env::set_var("COHROLE_PATH", &env.cohrole);
    }
    let resp = handle_9p_session(&msg(0x09, "/mnt/data|hello"));
    assert!(matches!(parse_message(&resp), P9Message::Rwrite));
    let rd = handle_9p_session(&msg(0x07, "/mnt/data"));
    unsafe {
        std::env::remove_var("COHROLE_PATH");
    }
    assert!(matches!(parse_message(&rd), P9Message::Rread));
    assert_eq!(&rd[1..], b"hello");
    Ok(())
}

#[test]
#[serial]
fn cross_role_read_access() -> io::Result<()> {
    server::reset_fs();
    let env_q = setup("QueenPrimary")?;
    unsafe {
        std::env::set_var("COHROLE_PATH", &env_q.cohrole);
    }
    let w = handle_9p_session(&msg(0x09, "/srv/shared|data"));
    assert!(matches!(parse_message(&w), P9Message::Rwrite));
    unsafe {
        std::env::remove_var("COHROLE_PATH");
    }

    let env_k = setup("KioskInteractive")?;
    unsafe {
        std::env::set_var("COHROLE_PATH", &env_k.cohrole);
    }
    let resp = handle_9p_session(&msg(0x07, "/srv/shared"));
    unsafe {
        std::env::remove_var("COHROLE_PATH");
    }
    assert!(matches!(parse_message(&resp), P9Message::Rread));
    assert_eq!(&resp[1..], b"data");
    Ok(())
}

#[test]
#[serial]
fn kiosk_write_denied_srv() -> io::Result<()> {
    server::reset_fs();
    let env = setup("KioskInteractive")?;
    unsafe {
        std::env::set_var("COHROLE_PATH", &env.cohrole);
    }
    let resp = handle_9p_session(&msg(0x09, "/srv/blocked|x"));
    unsafe {
        std::env::remove_var("COHROLE_PATH");
    }
    assert!(matches!(parse_message(&resp), P9Message::Unknown(0xfd)));
    Ok(())
}
