// Author: Lukas Bower
// Purpose: Exercise the compiled MCP HTTP endpoint, transport bounds and authentication failures.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const AUTH: &str = "m28d-local-request-auth-test";

struct GatewayChild(Child);

impl Drop for GatewayChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn exchange(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect gateway");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("read timeout");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    )
    .expect("request head");
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n").expect("request header");
    }
    write!(stream, "\r\n{body}").expect("request body");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("gateway response");
    response
}

fn code(response: &str) -> &str {
    response
        .lines()
        .next()
        .expect("status line")
        .split_whitespace()
        .nth(1)
        .expect("HTTP status")
}

fn gateway() -> (GatewayChild, u16) {
    let reserved = TcpListener::bind(("127.0.0.1", 0)).expect("reserve port");
    let port = reserved.local_addr().expect("local address").port();
    drop(reserved);
    let mut child = GatewayChild(
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
            .expect("start gateway"),
    );
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) {
            drop(stream);
            return (child, port);
        }
        assert!(Instant::now() < deadline, "gateway did not start");
        assert!(child.0.try_wait().expect("poll gateway").is_none());
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn selected_http_transport_has_bounded_authenticated_json_rpc() {
    let (_gateway, port) = gateway();
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}}"#;
    let accept = ("Accept", "application/json, text/event-stream");
    let content = ("Content-Type", "application/json");
    let auth = ("x-cohesix-auth", AUTH);

    assert_eq!(code(&exchange(port, "GET", "/mcp", &[], "")), "401");
    assert_eq!(code(&exchange(port, "GET", "/mcp", &[auth], "")), "405");
    assert_eq!(
        code(&exchange(port, "POST", "/mcp", &[accept, content], body)),
        "401"
    );
    assert_eq!(
        code(&exchange(
            port,
            "POST",
            "/mcp",
            &[accept, content, auth, ("Origin", "http://attacker.invalid")],
            body,
        )),
        "403"
    );
    assert_eq!(
        code(&exchange(port, "POST", "/mcp", &[accept, auth], body)),
        "415"
    );
    assert_eq!(
        code(&exchange(
            port,
            "POST",
            "/mcp",
            &[
                ("Accept", "application/json;q=0, text/event-stream"),
                content,
                auth
            ],
            body
        )),
        "406"
    );
    assert_eq!(
        code(&exchange(
            port,
            "POST",
            "/mcp",
            &[accept, ("Content-Type", "application/jsonp"), auth],
            body
        )),
        "415"
    );
    let response = exchange(port, "POST", "/mcp", &[accept, content, auth], body);
    assert_eq!(code(&response), "200");
    assert!(response.contains("2025-11-25"));
    assert!(response.contains("cohesix-hive-gateway"));

    let bad_version = body.replace("2025-11-25", "2024-11-05");
    let response = exchange(port, "POST", "/mcp", &[accept, content, auth], &bad_version);
    assert_eq!(code(&response), "200");
    assert!(response.contains("unsupported MCP revision"));
    let response = exchange(port, "POST", "/mcp", &[accept, content, auth], "{");
    assert!(response.contains("invalid JSON-RPC request"));
    assert_eq!(
        code(&exchange(
            port,
            "POST",
            "/mcp",
            &[accept, content, auth],
            &"x".repeat(8193)
        )),
        "413"
    );

    let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
    assert_eq!(
        code(&exchange(
            port,
            "POST",
            "/mcp",
            &[
                accept,
                content,
                auth,
                ("MCP-Protocol-Version", "2025-11-25")
            ],
            list
        )),
        "403"
    );
    let initialized = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    assert_eq!(
        code(&exchange(
            port,
            "POST",
            "/mcp",
            &[accept, content, auth],
            initialized
        )),
        "400"
    );
    assert_eq!(
        code(&exchange(
            port,
            "POST",
            "/mcp",
            &[
                accept,
                content,
                auth,
                ("MCP-Protocol-Version", "2025-11-25")
            ],
            initialized
        )),
        "202"
    );
    assert_eq!(code(&exchange(port, "GET", "/docs", &[], "")), "200");
}

#[test]
fn local_stdio_writes_only_protocol_messages_to_stdout() {
    let directory = tempfile::tempdir().expect("temporary credential directory");
    let ticket_path = directory.path().join("delegated-ticket");
    std::fs::write(&ticket_path, "cohesix-ticket-stdio-local-fixture-12345")
        .expect("write test credential");
    let mut child = Command::new(env!("CARGO_BIN_EXE_hive-gateway"))
        .args([
            "--mock",
            "--mcp-stdio",
            "--mcp-stdio-ticket-ref",
            &format!("file:{}", ticket_path.display()),
            "--request-auth-token",
            AUTH,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch local MCP transport");
    let stdin = child.stdin.as_mut().expect("stdio input");
    writeln!(stdin, "{}", r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#)
        .expect("send initialize");
    writeln!(
        stdin,
        "{}",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
    )
    .expect("send notification");
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("local MCP exit");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 JSON-RPC output");
    let rows: Vec<_> = stdout.lines().collect();
    assert_eq!(rows.len(), 1, "stdio must not log to stdout");
    let response: serde_json::Value = serde_json::from_str(rows[0]).expect("JSON-RPC message");
    assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
}
