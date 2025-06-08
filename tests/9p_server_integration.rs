// CLASSIFICATION: COMMUNITY
// Filename: 9p_server_integration.rs v0.1
// Date Modified: 2025-07-13
// Author: Cohesix Codex

use cohesix_9p::{FsConfig, FsServer};
use ninep::client::TcpClient;
use serial_test::serial;

fn start_test_server(port: u16) -> FsServer {
    let cfg = FsConfig {
        root: "/".into(),
        port,
        readonly: false,
    };
    let srv = FsServer::new(cfg);
    srv.start().expect("start server");
    srv
}

#[test]
#[ignore]
#[serial]
fn local_read_write() {
    let _srv = start_test_server(5650);
    let mut client = TcpClient::new_tcp("tester".to_string(), "127.0.0.1:5650", "/").unwrap();
    client
        .create(
            "/",
            "file",
            ninep::fs::Perm::OWNER_READ | ninep::fs::Perm::OWNER_WRITE,
            ninep::fs::Mode::FILE,
        )
        .unwrap();
    client.write("/file", 0, b"hello").unwrap();
    let data = client.read("/file").unwrap();
    assert_eq!(data, b"hello");
}

#[test]
#[ignore]
#[serial]
fn permission_denied() {
    let _srv = start_test_server(5651);
    let mut client = TcpClient::new_tcp("tester".to_string(), "127.0.0.1:5651", "/").unwrap();
    let res = client.write("/proc/x", 0, b"x");
    assert!(res.is_err());
}
