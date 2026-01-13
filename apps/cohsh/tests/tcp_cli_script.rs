// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate TCP CLI script execution with framed console protocol.
// Author: Lukas Bower
#![cfg(feature = "tcp")]

use std::io::{BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use assert_cmd::Command;
use predicates::prelude::*;

fn write_frame(stream: &mut std::net::TcpStream, line: &str) {
    let total_len = line.len().saturating_add(4) as u32;
    stream.write_all(&total_len.to_le_bytes()).unwrap();
    stream.write_all(line.as_bytes()).unwrap();
}

fn read_frame(reader: &mut BufReader<std::net::TcpStream>) -> Option<String> {
    let mut len_buf = [0u8; 4];
    if reader.read_exact(&mut len_buf).is_err() {
        return None;
    }
    let total_len = u32::from_le_bytes(len_buf) as usize;
    let payload_len = total_len.saturating_sub(4);
    let mut payload = vec![0u8; payload_len];
    if reader.read_exact(&mut payload).is_err() {
        return None;
    }
    String::from_utf8(payload).ok()
}

#[test]
fn tcp_script_executes_against_basic_server() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind listener");
    let port = listener.local_addr().expect("listener addr").port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.expect("accept stream");
            let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
            while let Some(line) = read_frame(&mut reader) {
                let trimmed = line.trim();
                if trimmed == "AUTH changeme" {
                    write_frame(&mut stream, "OK AUTH");
                } else if trimmed.starts_with("ATTACH") {
                    write_frame(&mut stream, "OK ATTACH role=queen");
                } else if trimmed.starts_with("TAIL") {
                    write_frame(&mut stream, "OK TAIL path=/log/queen.log");
                    write_frame(&mut stream, "queen boot");
                    write_frame(&mut stream, "heart line");
                    write_frame(&mut stream, "END");
                } else if trimmed == "PING" {
                    write_frame(&mut stream, "PONG");
                    write_frame(&mut stream, "OK PING reply=pong");
                }
            }
            break;
        }
    });

    let script_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("scripts")
        .join("cohsh")
        .join("tcp_basic.coh");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cohsh"));
    let assert = cmd
        .arg("--transport")
        .arg("tcp")
        .arg("--script")
        .arg(&script_path)
        .env("COHSH_TCP_PORT", port.to_string())
        .timeout(Duration::from_secs(10))
        .assert();

    assert
        .success()
        .stdout(predicate::str::contains("Welcome to Cohesix"))
        .stdout(predicate::str::contains("attached session"))
        .stdout(predicate::str::contains("as Queen"))
        .stdout(predicate::str::contains("Cohesix command surface:"))
        .stdout(predicate::str::contains("queen boot"))
        .stdout(predicate::str::contains("heart line"))
        .stdout(predicate::str::contains("closing session"));
}

#[test]
fn tcp_script_reports_connection_failure() {
    let unused_listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral");
    let port = unused_listener.local_addr().expect("listener addr").port();
    drop(unused_listener);

    let script_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("scripts")
        .join("cohsh")
        .join("tcp_basic.coh");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cohsh"));
    let assert = cmd
        .arg("--transport")
        .arg("tcp")
        .arg("--script")
        .arg(&script_path)
        .env("COHSH_TCP_PORT", port.to_string())
        .timeout(Duration::from_secs(8))
        .assert();

    assert.failure().stderr(predicate::str::contains(
        "failed to connect to Cohesix TCP console",
    ));
}

#[test]
fn tcp_interactive_attach_failure_keeps_prompt() {
    let unused_listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral");
    let port = unused_listener.local_addr().expect("listener addr").port();
    drop(unused_listener);

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cohsh"));
    let assert = cmd
        .arg("--transport")
        .arg("tcp")
        .arg("--tcp-port")
        .arg(port.to_string())
        .arg("--role")
        .arg("queen")
        .write_stdin("quit\n")
        .timeout(Duration::from_secs(10))
        .assert();

    assert
        .success()
        .stdout(predicate::str::contains("Welcome to Cohesix"))
        .stdout(predicate::str::contains(
            "detached shell: run 'attach <role>' to connect",
        ))
        .stdout(predicate::str::contains("coh> "))
        .stderr(predicate::str::contains("TCP attach failed"));
}
