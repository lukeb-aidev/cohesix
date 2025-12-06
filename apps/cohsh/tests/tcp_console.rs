// Author: Lukas Bower
#![cfg(feature = "tcp")]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use cohesix_ticket::Role;
use cohsh::proto::{parse_ack, AckStatus};
use cohsh::{TcpTransport, Transport};

#[test]
fn tcp_transport_handles_attach_and_tail() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind test listener");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept client");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut line = String::new();
        reader.read_line(&mut line).expect("read auth");
        assert!(line.starts_with("AUTH changeme"));
        writeln!(stream, "OK AUTH").expect("write auth ok");
        line.clear();
        reader.read_line(&mut line).expect("read attach");
        assert!(line.starts_with("ATTACH queen"));
        writeln!(stream, "OK ATTACH role=queen").expect("write ok");
        line.clear();
        reader.read_line(&mut line).expect("read tail");
        assert!(line.starts_with("TAIL /log/queen.log"));
        writeln!(stream, "OK TAIL path=/log/queen.log").expect("ack tail");
        writeln!(stream, "boot line").expect("write line");
        writeln!(stream, "END").expect("write end");
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
        let (mut stream, _) = listener.accept().expect("accept client");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut line = String::new();
        reader.read_line(&mut line).expect("read auth");
        assert!(line.starts_with("AUTH changeme"));
        // Deliberately remain silent to trigger the client's auth timeout.
        std::thread::sleep(std::time::Duration::from_millis(250));
        drop(stream);
    });

    let mut transport = TcpTransport::new("127.0.0.1", port).with_timeout(Duration::from_millis(200));
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
