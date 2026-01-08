// Author: Lukas Bower
// Purpose: Validate TCP CLI framing and acknowledgement handling.

#![cfg(feature = "tcp")]

use std::io::{Cursor, Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use cohesix_ticket::Role;
use cohsh::{Shell, TcpTransport};

fn write_frame(stream: &mut std::net::TcpStream, line: &str) {
    let total_len = line.len().saturating_add(4) as u32;
    stream.write_all(&total_len.to_le_bytes()).unwrap();
    stream.write_all(line.as_bytes()).unwrap();
}

fn read_frame(reader: &mut std::io::BufReader<std::net::TcpStream>) -> Option<String> {
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
fn tcp_shell_attach_and_ping_emit_acknowledgements() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming().take(1) {
            let mut stream = stream.unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            while let Some(line) = read_frame(&mut reader) {
                let trimmed = line.trim();
                if trimmed == "AUTH changeme" {
                    write_frame(&mut stream, "OK AUTH");
                } else if trimmed.starts_with("ATTACH") {
                    write_frame(&mut stream, "OK ATTACH role=queen");
                } else if trimmed == "PING" {
                    write_frame(&mut stream, "PONG");
                    write_frame(&mut stream, "OK PING reply=pong");
                }
            }
        }
    });

    let transport = TcpTransport::new("127.0.0.1", port)
        .with_timeout(Duration::from_millis(200))
        .with_auth_token("changeme");
    let writer = Cursor::new(Vec::new());
    let mut shell = Shell::new(transport, writer);

    shell
        .attach(Role::Queen, None)
        .expect("attach should succeed");
    shell
        .execute("ping")
        .expect("ping should succeed once attached");

    let (_transport, writer) = shell.into_parts();
    let output = String::from_utf8(writer.into_inner()).expect("utf8 output");
    assert!(output.contains("[console] OK AUTH"));
    assert!(output.contains("[console] OK ATTACH role=queen"));
    assert!(output.contains("ping: pong"));
    assert!(!output.contains("detached shell"));
}
