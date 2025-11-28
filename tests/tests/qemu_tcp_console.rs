// Author: Lukas Bower

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use cohesix_ticket::Role;
use cohsh::{Shell, TcpTransport, Transport};
use tests::TEST_TIMEOUT;

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
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                let trimmed = line.trim();
                if trimmed == "AUTH changeme" {
                    writeln!(stream, "OK AUTH").unwrap();
                } else if trimmed.starts_with("ATTACH") {
                    server_log
                        .lock()
                        .expect("ticket log poisoned")
                        .push(trimmed.to_owned());
                    writeln!(stream, "OK ATTACH role=queen session-{attempts}").unwrap();
                } else if trimmed.starts_with("TAIL") {
                    if attempts == 1 {
                        writeln!(stream, "OK TAIL path=/log/queen.log").unwrap();
                        writeln!(stream, "queen boot").unwrap();
                        stream.flush().unwrap();
                        break;
                    } else {
                        writeln!(stream, "OK TAIL path=/log/queen.log").unwrap();
                        writeln!(stream, "queen boot").unwrap();
                        writeln!(stream, "reconnected line").unwrap();
                        writeln!(stream, "END").unwrap();
                    }
                } else if trimmed == "PING" {
                    writeln!(stream, "PONG").unwrap();
                    writeln!(stream, "OK PING reply=pong").unwrap();
                }
                line.clear();
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
