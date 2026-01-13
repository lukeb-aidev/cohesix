// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate TCP console transport framing and attach flows.
// Author: Lukas Bower
#![cfg(feature = "tcp")]

use std::io::{BufReader, Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use cohesix_ticket::Role;
use cohsh::proto::{parse_ack, AckStatus};
use cohsh::{TcpTransport, Transport};

fn write_frame(stream: &mut std::net::TcpStream, line: &str) {
    let total_len = line.len().saturating_add(4) as u32;
    stream.write_all(&total_len.to_le_bytes()).expect("write len");
    stream.write_all(line.as_bytes()).expect("write payload");
}

fn read_frame(reader: &mut BufReader<std::net::TcpStream>) -> String {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).expect("read len");
    let total_len = u32::from_le_bytes(len_buf) as usize;
    let payload_len = total_len.saturating_sub(4);
    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload).expect("read payload");
    String::from_utf8(payload).expect("payload utf8")
}

#[test]
fn tcp_transport_handles_attach_and_tail() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind test listener");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept client");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let line = read_frame(&mut reader);
        assert!(line.starts_with("AUTH changeme"));
        write_frame(&mut stream, "OK AUTH");
        let line = read_frame(&mut reader);
        assert!(line.starts_with("ATTACH queen"));
        write_frame(&mut stream, "OK ATTACH role=queen");
        let line = read_frame(&mut reader);
        assert!(line.starts_with("TAIL /log/queen.log"));
        write_frame(&mut stream, "OK TAIL path=/log/queen.log");
        write_frame(&mut stream, "boot line");
        write_frame(&mut stream, "END");
    });

    let mut transport = TcpTransport::new("127.0.0.1", port);
    let session = transport.attach(Role::Queen, None).expect("attach queen");
    let attach_ack = transport.drain_acknowledgements();
    assert_eq!(attach_ack.len(), 2);
    let auth_ack = parse_ack(&attach_ack[0]).expect("parse auth ack");
    assert_eq!(auth_ack.status, AckStatus::Ok);
    assert_eq!(auth_ack.verb, "AUTH");
    let attach = parse_ack(&attach_ack[1]).expect("parse attach ack");
    assert_eq!(attach.status, AckStatus::Ok);
    assert_eq!(attach.verb, "ATTACH");

    let logs = transport
        .tail(&session, "/log/queen.log")
        .expect("tail log");
    assert_eq!(logs, vec!["boot line".to_owned()]);
    let tail_ack = transport.drain_acknowledgements();
    assert!(tail_ack
        .iter()
        .any(|line| line.starts_with("OK TAIL path=/log/queen.log")));
}

#[test]
fn tcp_transport_times_out_when_server_is_silent() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind test listener");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept client");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let line = read_frame(&mut reader);
        assert!(line.starts_with("AUTH changeme"));
        // Deliberately remain silent to trigger the client's auth timeout.
        std::thread::sleep(std::time::Duration::from_millis(250));
        drop(stream);
    });

    let mut transport =
        TcpTransport::new("127.0.0.1", port).with_timeout(Duration::from_millis(200));
    let result = transport.attach(Role::Queen, None);
    assert!(result.is_err());
    let err = format!("{result:?}");
    assert!(
        err.contains("authentication timed out")
            || err.contains("connection closed during authentication")
            || err.contains("authentication failed")
            || err.contains("connection closed by peer")
            || err.contains("failed to connect to Cohesix TCP console")
    );
}
