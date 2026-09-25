// Author: Lukas Bower
// Purpose: Verify that the gateway cannot open disabled agent protocol routes or accept runtime widening.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

struct GatewayChild(Child);

impl Drop for GatewayChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_hive-gateway"));
    for key in [
        "HIVE_GATEWAY_AGENT_PROTOCOLS_ENABLED",
        "HIVE_GATEWAY_MCP_ENABLED",
        "HIVE_GATEWAY_A2A_ENABLED",
    ] {
        command.env_remove(key);
    }
    command
}

fn response_code(port: u16, path: &str) -> Option<u16> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(1))).ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut bytes = [0; 96];
    let count = stream.read(&mut bytes).ok()?;
    let first = std::str::from_utf8(&bytes[..count]).ok()?.lines().next()?;
    first.split_whitespace().nth(1)?.parse().ok()
}

#[test]
fn selected_protocol_routes_preserve_authenticated_rest() {
    let reserved = TcpListener::bind(("127.0.0.1", 0)).expect("reserve test port");
    let port = reserved.local_addr().expect("local address").port();
    drop(reserved);
    let mut child = GatewayChild(
        command()
            .args(["--mock", "--bind", &format!("127.0.0.1:{port}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start gateway"),
    );
    let deadline = Instant::now() + Duration::from_secs(8);
    while response_code(port, "/docs") != Some(200) {
        assert!(Instant::now() < deadline, "gateway did not start");
        assert!(child.0.try_wait().expect("poll gateway").is_none());
        thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(response_code(port, "/mcp"), Some(401));
    assert_eq!(
        response_code(port, "/.well-known/agent-card.json"),
        Some(401)
    );
    assert_eq!(response_code(port, "/a2a"), Some(405));
    for path in ["/v1/mcp", "/.well-known/agent.json"] {
        assert_eq!(response_code(port, path), Some(404), "{path}");
    }
    assert_ne!(
        response_code(port, "/v1/fs/cat?path=/host/tickets/status&max_bytes=2048"),
        Some(404),
        "existing authenticated ticket recovery route must remain registered"
    );
}

#[test]
fn runtime_overrides_cannot_widen_generated_controls() {
    for key in [
        "HIVE_GATEWAY_AGENT_PROTOCOLS_ENABLED",
        "HIVE_GATEWAY_MCP_ENABLED",
        "HIVE_GATEWAY_A2A_ENABLED",
    ] {
        let output = command()
            .arg("--mock")
            .env(key, "true")
            .output()
            .expect("run gateway");
        assert!(!output.status.success(), "{key}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("cannot override generated"));
    }
    let output = command()
        .args(["--mock", "--mcp-enabled"])
        .output()
        .expect("parse gateway arguments");
    assert!(!output.status.success());
}
