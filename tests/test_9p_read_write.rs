// CLASSIFICATION: COMMUNITY
// Filename: test_9p_read_write.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-23

use cohesix_9p::{FsConfig, FsServer};
use ninep::client::TcpClient;
use ninep::fs::{Perm, Mode};
use serial_test::serial;
use tempfile::tempdir;

#[test]
#[serial]
fn ninep_read_write_roundtrip() {
    const PORT: u16 = 5652;
    if std::net::TcpListener::bind(("127.0.0.1", PORT)).is_err() {
        eprintln!("skipping ninep_read_write_roundtrip: port {PORT} unavailable");
        return;
    }
    let dir = tempdir().unwrap();
    let root = dir.path().join("fs");
    std::fs::create_dir(&root).unwrap();
    let mut srv = FsServer::new(FsConfig { root: root.clone(), port: PORT, readonly: false });
    srv.start().unwrap();

    let mut client = TcpClient::new_tcp("tester".into(), &format!("127.0.0.1:{PORT}"), "/").unwrap();
    client.create("/", "file", Perm::OWNER_READ | Perm::OWNER_WRITE, Mode::FILE).unwrap();
    client.write("/file", 0, b"hello").unwrap();
    let data = client.read("/file").unwrap();
    assert_eq!(data, b"hello");
}
