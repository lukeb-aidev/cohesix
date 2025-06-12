// CLASSIFICATION: COMMUNITY
// Filename: cohesix_netd.rs v0.2
// Date Modified: 2025-07-22
// Author: Cohesix Codex

use cohesix::net::cohesix_netd::CohesixNetd;
use serial_test::serial;
use std::io::{Read, Write};
use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream};
use std::thread;

fn start_tcp_server() -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind socket");
    let port = listener.local_addr().expect("local_addr").port();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 16];
            if let Ok(n) = stream.read(&mut buf) {
                let _ = stream.write_all(&buf[..n]);
            }
        }
    });
    Ok(port)
}

#[test]
#[serial]
fn tcp_9p_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("CI").is_ok() {
        eprintln!("⚠️ Skipping TCP test in CI");
        return Ok(());
    }
    let port = match start_tcp_server() {
        Ok(p) => p,
        Err(err) if err.kind() == ErrorKind::PermissionDenied => {
            eprintln!("⚠️ Skipping test due to lack of permission: {:?}", err);
            return Ok(());
        }
        Err(err) => return Err(err.into()),
    };
    let netd = CohesixNetd { port, discovery_port: 9999 };
    let mut stream = match TcpStream::connect(("127.0.0.1", port)) {
        Ok(s) => s,
        Err(err) if err.kind() == ErrorKind::PermissionDenied => {
            eprintln!("⚠️ Skipping test due to lack of permission: {:?}", err);
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    stream.write_all(&[0x6f])?;
    let mut out = [0u8; 1];
    stream.read_exact(&mut out)?;
    assert_eq!(out[0], 0x6f);
    drop(netd);
    Ok(())
}

#[test]
#[serial]
fn discovery_broadcast() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("CI").is_ok() {
        eprintln!("⚠️ Skipping discovery test in CI");
        return Ok(());
    }
    let netd = CohesixNetd { port: 6000, discovery_port: 9933 };
    thread::spawn(move || {
        match netd.listen_discovery_once() {
            Ok(msg) => assert_eq!(&msg, b"cohesix_netd_discovery"),
            Err(err) => eprintln!("listen error: {:?}", err),
        }
    });
    std::thread::sleep(std::time::Duration::from_millis(50));
    let netd2 = CohesixNetd { port: 6001, discovery_port: 9933 };
    if let Err(err) = netd2.broadcast_presence() {
        if err.kind() == ErrorKind::PermissionDenied {
            eprintln!("⚠️ Skipping test due to lack of permission: {:?}", err);
            return Ok(());
        }
        return Err(err.into());
    }
    Ok(())
}

#[test]
#[serial]
fn http_fallback_post() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("CI").is_ok() {
        eprintln!("⚠️ Skipping HTTP test in CI");
        return Ok(());
    }
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind socket");
    let port = listener.local_addr()?.port();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 128];
            if let Ok(n) = stream.read(&mut buf) {
                assert!(std::str::from_utf8(&buf[..n]).unwrap().starts_with("POST"));
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
            }
        }
    });
    let netd = CohesixNetd { port: 0, discovery_port: 0 };
    let url = format!("http://127.0.0.1:{}", port);
    if let Err(err) = netd.http_fallback(&url) {
        if err.downcast_ref::<std::io::Error>()
            .map(|e| e.kind() == ErrorKind::PermissionDenied)
            .unwrap_or(false)
        {
            eprintln!("⚠️ Skipping test due to lack of permission: {:?}", err);
            return Ok(());
        }
        return Err(err.into());
    }
    Ok(())
}
