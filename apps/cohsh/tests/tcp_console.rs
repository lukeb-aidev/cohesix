// Author: Lukas Bower
#![cfg(feature = "tcp")]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread;

use cohesix_ticket::Role;
use cohsh::{TcpTransport, Transport};

#[test]
fn tcp_transport_handles_attach_and_tail() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind test listener");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept client");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut line = String::new();
        reader.read_line(&mut line).expect("read attach");
        assert!(line.starts_with("ATTACH queen"));
        writeln!(stream, "OK session").expect("write ok");
        line.clear();
        reader.read_line(&mut line).expect("read tail");
        assert!(line.starts_with("TAIL /log/queen.log"));
        writeln!(stream, "boot line").expect("write line");
        writeln!(stream, "END").expect("write end");
    });

    let mut transport = TcpTransport::new("127.0.0.1", port);
    let session = transport.attach(Role::Queen, None).expect("attach queen");
    let logs = transport
        .tail(&session, "/log/queen.log")
        .expect("tail log");
    assert_eq!(logs, vec!["boot line".to_owned()]);
}
