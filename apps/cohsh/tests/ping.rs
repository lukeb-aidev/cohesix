// Author: Lukas Bower

use std::io::Cursor;

use cohesix_ticket::Role;
use cohsh::{Session, Shell, Transport};
use secure9p_codec::SessionId;

#[derive(Default)]
struct StubTransport;

impl Transport for StubTransport {
    fn attach(&mut self, role: Role, _ticket: Option<&str>) -> anyhow::Result<Session> {
        Ok(Session::new(SessionId::from_raw(7), role))
    }

    fn kind(&self) -> &'static str {
        "stub"
    }

    fn ping(&mut self, session: &Session) -> anyhow::Result<String> {
        Ok(format!("attached as {:?} via stub", session.role()))
    }

    fn tail(&mut self, _session: &Session, _path: &str) -> anyhow::Result<Vec<String>> {
        unimplemented!("tail not used in ping tests")
    }

    fn write(&mut self, _session: &Session, _path: &str, _payload: &[u8]) -> anyhow::Result<()> {
        unimplemented!("write not used in ping tests")
    }
}

#[test]
fn ping_reports_detached_state() {
    let mut shell = Shell::new(StubTransport::default(), Cursor::new(Vec::new()));
    let err = shell
        .execute("ping")
        .expect_err("ping should fail when detached");
    assert!(err.to_string().contains("ping: not attached"));
    let (_transport, writer) = shell.into_parts();
    let rendered = String::from_utf8(writer.into_inner()).expect("valid utf8");
    assert!(rendered.contains("ping: not attached"));
}

#[test]
fn ping_reports_attachment_state() {
    let mut shell = Shell::new(StubTransport::default(), Cursor::new(Vec::new()));
    shell
        .attach(Role::Queen, None)
        .expect("attach should succeed");
    shell
        .execute("ping")
        .expect("ping should succeed when attached");
    let (_transport, writer) = shell.into_parts();
    let rendered = String::from_utf8(writer.into_inner()).expect("valid utf8");
    assert!(rendered.contains("ping: attached as Queen via stub"));
}
