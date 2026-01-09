// Author: Lukas Bower

use std::collections::VecDeque;
use std::io::Cursor;

use anyhow::Result;
use cohsh::{Session, Shell, Transport};
use cohesix_ticket::Role;
use secure9p_codec::SessionId;

#[derive(Default)]
struct ScriptTransport {
    pending_ack: VecDeque<String>,
    attach_ack: Option<String>,
    ping_ack: Option<String>,
    tail_ack: Option<String>,
    write_ack: Option<String>,
    tail_lines: Vec<String>,
}

impl Transport for ScriptTransport {
    fn attach(&mut self, role: Role, _ticket: Option<&str>) -> Result<Session> {
        if let Some(ack) = self.attach_ack.as_ref() {
            self.pending_ack.push_back(ack.clone());
        }
        Ok(Session::new(SessionId::from_raw(1), role))
    }

    fn ping(&mut self, _session: &Session) -> Result<String> {
        if let Some(ack) = self.ping_ack.as_ref() {
            self.pending_ack.push_back(ack.clone());
        }
        Ok("pong".to_owned())
    }

    fn tail(&mut self, _session: &Session, _path: &str) -> Result<Vec<String>> {
        if let Some(ack) = self.tail_ack.as_ref() {
            self.pending_ack.push_back(ack.clone());
        }
        Ok(self.tail_lines.clone())
    }

    fn write(&mut self, _session: &Session, _path: &str, _payload: &[u8]) -> Result<()> {
        if let Some(ack) = self.write_ack.as_ref() {
            self.pending_ack.push_back(ack.clone());
        }
        Ok(())
    }

    fn drain_acknowledgements(&mut self) -> Vec<String> {
        self.pending_ack.drain(..).collect()
    }
}

#[test]
fn script_allows_256_lines() {
    let script = "help\n".repeat(256);
    let transport = ScriptTransport::default();
    let mut shell = Shell::new(transport, Cursor::new(Vec::new()));
    shell
        .run_script(Cursor::new(script.into_bytes()))
        .expect("256 statements should be accepted");
}

#[test]
fn script_rejects_257_lines() {
    let script = "help\n".repeat(257);
    let transport = ScriptTransport::default();
    let mut shell = Shell::new(transport, Cursor::new(Vec::new()));
    let err = shell
        .run_script(Cursor::new(script.into_bytes()))
        .expect_err("257 statements should be rejected");
    let message = err.to_string();
    assert!(message.contains("line 257"));
    assert!(message.contains("help"));
    assert!(message.contains("last response: <none>"));
}

#[test]
fn script_wait_enforces_bounds() {
    let transport = ScriptTransport::default();
    let mut shell = Shell::new(transport, Cursor::new(Vec::new()));
    shell
        .run_script(Cursor::new(b"WAIT 2000\n".as_slice()))
        .expect("WAIT 2000 should be accepted");
    let err = shell
        .run_script(Cursor::new(b"WAIT 2001\n".as_slice()))
        .expect_err("WAIT 2001 should be rejected");
    assert!(err.to_string().contains("WAIT exceeds max"));
}

#[test]
fn expect_requires_prior_command() {
    let transport = ScriptTransport::default();
    let mut shell = Shell::new(transport, Cursor::new(Vec::new()));
    let err = shell
        .run_script(Cursor::new(b"EXPECT OK\n".as_slice()))
        .expect_err("EXPECT should fail without a command");
    let message = err.to_string();
    assert!(message.contains("line 1"));
    assert!(message.contains("EXPECT OK"));
    assert!(message.contains("last response: <none>"));
}

#[test]
fn expect_variants_match_last_response() {
    let transport = ScriptTransport {
        attach_ack: Some("OK ATTACH role=Queen".to_owned()),
        ping_ack: Some("ERR PING reason=denied".to_owned()),
        ..Default::default()
    };
    let mut shell = Shell::new(transport, Cursor::new(Vec::new()));
    let script = "\
attach queen
EXPECT OK
EXPECT SUBSTR role=Queen
EXPECT NOT ERR
ping
EXPECT ERR
";
    shell
        .run_script(Cursor::new(script.as_bytes()))
        .expect("EXPECT variants should pass");
}

#[test]
fn streaming_expect_uses_ack_line() {
    let transport = ScriptTransport {
        attach_ack: Some("OK ATTACH role=Queen".to_owned()),
        tail_ack: Some("OK TAIL path=/log/queen.log".to_owned()),
        tail_lines: vec!["payload-one".to_owned(), "payload-two".to_owned()],
        ..Default::default()
    };
    let mut shell = Shell::new(transport, Cursor::new(Vec::new()));
    let script = "\
attach queen
tail /log/queen.log
EXPECT SUBSTR payload-one
";
    let err = shell
        .run_script(Cursor::new(script.as_bytes()))
        .expect_err("EXPECT should evaluate against ack line");
    let message = err.to_string();
    assert!(message.contains("line 3"));
    assert!(message.contains("EXPECT SUBSTR payload-one"));
    assert!(message.contains("last response: OK TAIL path=/log/queen.log"));
}
