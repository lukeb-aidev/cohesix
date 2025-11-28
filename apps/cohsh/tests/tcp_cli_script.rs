// Author: Lukas Bower
#![cfg(feature = "tcp")]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn tcp_script_executes_against_basic_server() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind listener");
    let port = listener.local_addr().expect("listener addr").port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.expect("accept stream");
            let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
            let mut line = String::new();
            while reader.read_line(&mut line).expect("read line") > 0 {
                let trimmed = line.trim();
                if trimmed.starts_with("ATTACH") {
                    writeln!(stream, "OK ATTACH role=queen").expect("ack attach");
                } else if trimmed.starts_with("TAIL") {
                    writeln!(stream, "OK TAIL path=/log/queen.log").expect("ack tail");
                    writeln!(stream, "queen boot").expect("write boot");
                    writeln!(stream, "heart line").expect("write heart");
                    writeln!(stream, "END").expect("write end");
                } else if trimmed == "PING" {
                    writeln!(stream, "PONG").expect("pong");
                }
                line.clear();
            }
            break;
        }
    });

    let script_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests/cli/tcp_basic.cohsh");
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
        .join("tests/cli/tcp_basic.cohsh");
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
