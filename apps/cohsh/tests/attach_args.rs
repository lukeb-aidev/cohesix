// Author: Lukas Bower

use cohesix_ticket::Role;
use cohsh::{Shell, Transport};
use secure9p_wire::SessionId;

#[derive(Default)]
struct RecordingTransport {
    attach_calls: usize,
    last_role: Option<Role>,
    last_ticket: Option<Option<String>>,
}

impl Transport for RecordingTransport {
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> anyhow::Result<cohsh::Session> {
        self.attach_calls += 1;
        self.last_role = Some(role);
        self.last_ticket = Some(ticket.map(str::to_owned));
        Ok(cohsh::Session::new(SessionId::from_raw(42), role))
    }

    fn tail(&mut self, _session: &cohsh::Session, _path: &str) -> anyhow::Result<Vec<String>> {
        unimplemented!("tail not expected in attach tests")
    }

    fn write(
        &mut self,
        _session: &cohsh::Session,
        _path: &str,
        _payload: &[u8],
    ) -> anyhow::Result<()> {
        unimplemented!("write not expected in attach tests")
    }
}

fn new_shell() -> Shell<RecordingTransport, Vec<u8>> {
    Shell::new(RecordingTransport::default(), Vec::new())
}

#[test]
fn attach_requires_role() {
    let mut shell = new_shell();
    let err = shell
        .execute("attach")
        .expect_err("missing role should error");
    assert!(err.to_string().contains("requires a role"));
}

#[test]
fn attach_accepts_role_only() {
    let mut shell = new_shell();
    shell
        .execute("attach queen")
        .expect("attach with role should succeed");
    let (transport, _writer) = shell.into_parts();
    assert_eq!(transport.attach_calls, 1);
    assert_eq!(transport.last_role, Some(Role::Queen));
    assert_eq!(transport.last_ticket, Some(None));
}

#[test]
fn attach_accepts_role_and_ticket() {
    let mut shell = new_shell();
    shell
        .execute("attach queen TICKET123")
        .expect("attach with ticket should succeed");
    let (transport, _writer) = shell.into_parts();
    assert_eq!(transport.attach_calls, 1);
    assert_eq!(transport.last_role, Some(Role::Queen));
    assert_eq!(transport.last_ticket, Some(Some("TICKET123".to_owned())));
}

#[test]
fn attach_rejects_extra_arguments() {
    let mut shell = new_shell();
    let err = shell
        .execute("attach queen extra extra2")
        .expect_err("extra args should be rejected");
    assert!(err
        .to_string()
        .contains("takes at most two arguments: role and optional ticket"));
}

#[test]
fn attach_rejects_when_already_attached() {
    let mut shell = new_shell();
    shell
        .execute("attach queen")
        .expect("initial attach should succeed");
    let err = shell
        .execute("attach queen")
        .expect_err("second attach should be rejected");
    assert!(err
        .to_string()
        .contains("already attached; run 'quit' to close the current session"));
    let (transport, _writer) = shell.into_parts();
    assert_eq!(transport.attach_calls, 1);
}

#[test]
fn login_alias_uses_same_rules() {
    let mut shell = new_shell();
    let err = shell.execute("login").expect_err("login needs role");
    assert!(err.to_string().contains("login requires a role"));
}
