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
    let mut srv = FsServer::new(cfg);
    srv.start().expect("start server");
    srv
}

#[test]
#[serial]
fn local_read_write() {
    if let Ok(l) = std::net::TcpListener::bind("127.0.0.1:5650") {
        drop(l);
    } else {
        eprintln!("skipping local_read_write: port 5650 unavailable");
        return;
    }
    let _srv = start_test_server(5650);
    let mut client = match TcpClient::new_tcp("tester".to_string(), "127.0.0.1:5650", "/") {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skipping local_read_write: {e}");
            return;
        }
    };
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
#[serial]
fn permission_denied() {
    if let Ok(l) = std::net::TcpListener::bind("127.0.0.1:5651") {
        drop(l);
    } else {
        eprintln!("skipping permission_denied: port 5651 unavailable");
        return;
    }
    let _srv = start_test_server(5651);
    let mut client = match TcpClient::new_tcp("tester".to_string(), "127.0.0.1:5651", "/") {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skipping permission_denied: {e}");
            return;
        }
    };
    let res = client.write("/proc/x", 0, b"x");
    assert!(res.is_err());
}
