// Author: Lukas Bower
// Purpose: Exercise bounded A2A endpoint registration, authentication and malformed traffic.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const AUTH: &str = "m28e-a2a-protocol-fixture";

struct Gateway(Child);

impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn exchange(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", body.len()).unwrap();
    for (key, value) in headers {
        write!(stream, "{key}: {value}\r\n").unwrap();
    }
    write!(stream, "\r\n{body}").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn status(response: &str) -> &str {
    response
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
}

fn gateway() -> (Gateway, u16) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut child = Gateway(
        Command::new(env!("CARGO_BIN_EXE_hive-gateway"))
            .args([
                "--mock",
                "--bind",
                &format!("127.0.0.1:{port}"),
                "--request-auth-token",
                AUTH,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(8);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline);
        assert!(child.0.try_wait().unwrap().is_none());
        thread::sleep(Duration::from_millis(25));
    }
    (child, port)
}

#[test]
fn selected_endpoint_refuses_unauthenticated_and_bad_origin_requests() {
    let (_gateway, port) = gateway();
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"tasks/get","params":{"id":"job-1"}}"#;
    assert_eq!(
        status(&exchange(
            port,
            "GET",
            "/.well-known/agent-card.json",
            &[],
            ""
        )),
        "401"
    );
    assert_eq!(status(&exchange(port, "POST", "/a2a", &[], body)), "401");
    assert_eq!(
        status(&exchange(
            port,
            "POST",
            "/a2a",
            &[
                ("x-cohesix-auth", AUTH),
                ("Origin", "http://attacker.invalid")
            ],
            body
        )),
        "403"
    );
    assert_eq!(
        status(&exchange(
            port,
            "POST",
            "/a2a",
            &[("x-cohesix-auth", AUTH)],
            body
        )),
        "401"
    );
    assert_eq!(
        status(&exchange(port, "POST", "/a2a", &[], &"x".repeat(8193))),
        "413"
    );
    assert_eq!(status(&exchange(port, "GET", "/docs", &[], "")), "200");
}
