// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify SwarmUI warns, but does not fail, when using the placeholder TCP auth token.
// Author: Lukas Bower

use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use anyhow::Result;
use cohesix_ticket::Role;
use swarmui::{SwarmUiConfig, SwarmUiConsoleBackend};

fn write_frame(stream: &mut TcpStream, line: &str) {
    let total_len = line.len().saturating_add(4) as u32;
    stream.write_all(&total_len.to_le_bytes()).unwrap();
    stream.write_all(line.as_bytes()).unwrap();
    stream.flush().unwrap();
}

fn read_frame(reader: &mut BufReader<TcpStream>) -> Option<String> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).ok()?;
    let total_len = u32::from_le_bytes(len_buf) as usize;
    let payload_len = total_len.checked_sub(4)?;
    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload).ok()?;
    String::from_utf8(payload).ok()
}

#[test]
fn swarmui_allows_placeholder_token_with_warning() -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    thread::spawn(move || {
        for stream in listener.incoming().take(1) {
            let mut stream = stream.unwrap();
            write_frame(&mut stream, "OK AUTH detail=present-token");
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            while let Some(line) = read_frame(&mut reader) {
                let trimmed = line.trim();
                if trimmed == "AUTH changeme" {
                    write_frame(&mut stream, "OK AUTH");
                } else if trimmed.starts_with("ATTACH") {
                    write_frame(&mut stream, "OK ATTACH role=queen");
                    write_frame(&mut stream, "END");
                    break;
                }
            }
        }
    });

    let data_dir = std::env::temp_dir();
    let config = SwarmUiConfig::from_generated(data_dir);
    let mut backend = SwarmUiConsoleBackend::new(config, "127.0.0.1", port, "changeme");
    let transcript = backend.attach(Role::Queen, None);
    assert!(transcript.ok);
    assert!(transcript
        .lines
        .iter()
        .any(|line| line.starts_with("OK ATTACH role=queen")));
    assert!(transcript
        .lines
        .iter()
        .any(|line| line.contains("insecure placeholder token")));
    Ok(())
}
