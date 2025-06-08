// CLASSIFICATION: COMMUNITY
// Filename: cohesix_netd.rs v0.1
// Date Modified: 2025-07-13
// Author: Cohesix Codex

use cohesix::net::cohesix_netd::CohesixNetd;
use std::io::{Read, Write};
use std::net::{TcpStream, TcpListener};
use std::thread;
use serial_test::serial;

fn start_tcp_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 16];
        let n = stream.read(&mut buf).unwrap();
        stream.write_all(&buf[..n]).unwrap();
    });
    port
}

#[test]
#[serial]
fn tcp_9p_roundtrip() {
    let port = start_tcp_server();
    let netd = CohesixNetd { port, discovery_port: 9999 };
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(&[0x6f]).unwrap();
    let mut out = [0u8; 1];
    stream.read_exact(&mut out).unwrap();
    assert_eq!(out[0], 0x6f);
    drop(netd); // silence unused variable
}

#[test]
#[serial]
fn discovery_broadcast() {
    let netd = CohesixNetd { port: 6000, discovery_port: 9933 };
    thread::spawn(move || {
        let msg = netd.listen_discovery_once().unwrap();
        assert_eq!(&msg, b"cohesix_netd_discovery");
    });
    // give thread a moment
    std::thread::sleep(std::time::Duration::from_millis(50));
    let netd2 = CohesixNetd { port: 6001, discovery_port: 9933 };
    netd2.broadcast_presence().unwrap();
}

#[test]
#[serial]
fn http_fallback_post() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 128];
        let n = stream.read(&mut buf).unwrap();
        assert!(std::str::from_utf8(&buf[..n]).unwrap().starts_with("POST"));
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
    });
    let netd = CohesixNetd { port: 0, discovery_port: 0 };
    let url = format!("http://127.0.0.1:{}", port);
    netd.http_fallback(&url).unwrap();
}
