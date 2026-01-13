// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines tests for tests qemu_tcp_console.
// Author: Lukas Bower

use std::io::{BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use cohesix_ticket::Role;
use cohsh::{Shell, TcpTransport, Transport};
use tests::TEST_TIMEOUT;

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
fn tcp_console_script_recovers_from_disconnect() -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    let attach_log = Arc::new(Mutex::new(Vec::new()));
    let server_log = Arc::clone(&attach_log);
    let handle = thread::spawn(move || {
        let mut attempts = 0usize;
        for stream in listener.incoming() {
            attempts += 1;
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            while let Some(line) = read_frame(&mut reader) {
                let trimmed = line.trim();
                if trimmed == "AUTH changeme" {
                    write_frame(&mut stream, "OK AUTH");
                } else if trimmed.starts_with("ATTACH") {
                    server_log
                        .lock()
                        .expect("ticket log poisoned")
                        .push(trimmed.to_owned());
                    let response = format!("OK ATTACH role=queen session-{attempts}");
                    write_frame(&mut stream, response.as_str());
                } else if trimmed.starts_with("TAIL") {
                    if attempts == 1 {
                        write_frame(&mut stream, "OK TAIL path=/log/queen.log");
                        write_frame(&mut stream, "queen boot");
                        stream.flush().unwrap();
                        break;
                    } else {
                        write_frame(&mut stream, "OK TAIL path=/log/queen.log");
                        write_frame(&mut stream, "queen boot");
                        write_frame(&mut stream, "reconnected line");
                        write_frame(&mut stream, "END");
                    }
                } else if trimmed == "PING" {
                    write_frame(&mut stream, "PONG");
                    write_frame(&mut stream, "OK PING reply=pong");
                }
            }
            if attempts >= 2 {
                break;
            }
        }
    });

    let transport = TcpTransport::new("127.0.0.1", port)
        .with_timeout(TEST_TIMEOUT)
        .with_heartbeat_interval(Duration::from_millis(75))
        .with_max_retries(4);
    let mut shell = Shell::new(transport, Vec::new());
    shell.attach(Role::Queen, None)?;
    shell.execute("log")?;
    shell.execute("quit")?;
    let (transport, output) = shell.into_parts();
    let rendered = String::from_utf8(output)?;
    assert!(rendered.contains("attached session"));
    assert!(rendered.contains("queen boot"));
    assert!(rendered.contains("reconnected line"));
    assert!(rendered.contains("closing session"));
    drop(transport);
    handle.join().expect("server thread panicked");

    let tickets = attach_log.lock().expect("ticket log poisoned");
    assert!(tickets.iter().all(|line| line.starts_with("ATTACH queen")));
    assert!(tickets.len() >= 2);
    Ok(())
}

#[test]
fn worker_attach_without_ticket_is_rejected() {
    let mut transport = TcpTransport::new("127.0.0.1", 1).with_timeout(TEST_TIMEOUT);
    let err = transport
        .attach(Role::WorkerHeartbeat, None)
        .expect_err("missing ticket should be rejected");
    assert!(err.to_string().contains("ticket"));
}
