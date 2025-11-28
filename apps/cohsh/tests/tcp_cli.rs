// Author: Lukas Bower

#![cfg(feature = "tcp")]

use std::io::{BufRead, Cursor, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use cohesix_ticket::Role;
use cohsh::{Shell, TcpTransport};

#[test]
fn tcp_shell_attach_and_ping_emit_acknowledgements() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming().take(1) {
            let mut stream = stream.unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                let trimmed = line.trim();
                if trimmed == "AUTH changeme" {
                    writeln!(stream, "OK AUTH").unwrap();
                } else if trimmed.starts_with("ATTACH") {
                    writeln!(stream, "OK ATTACH role=queen").unwrap();
                } else if trimmed == "PING" {
                    writeln!(stream, "PONG").unwrap();
                    writeln!(stream, "OK PING reply=pong").unwrap();
                }
                line.clear();
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
