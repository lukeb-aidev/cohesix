// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate ticket scope, quota, and expiry enforcement for UI interactions.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::time::{SystemTime, UNIX_EPOCH};

use cohesix_ticket::{
    BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer, TicketQuotas, TicketScope, TicketVerb,
};
use nine_door::{InProcessConnection, NineDoor, NineDoorError, ShardLayout};
use secure9p_codec::{ErrorCode, OpenMode, MAX_MSIZE};

const QUEEN_SECRET: &str = "queen-secret";
const WORKER_SECRET: &str = "worker-secret";

#[test]
fn read_only_ticket_write_denied() {
    let server = NineDoor::new();
    server.register_ticket_secret(Role::Queen, QUEEN_SECRET);

    let claims = TicketClaims::new(
        Role::Queen,
        BudgetSpec::unbounded(),
        None,
        MountSpec::empty(),
        unix_time_ms(),
    )
    .with_scopes(vec![TicketScope::new("/queen/ctl", TicketVerb::Read, 0)]);
    let token = TicketIssuer::new(QUEEN_SECRET)
        .issue(claims)
        .unwrap()
        .encode()
        .unwrap();

    let mut client = attach_with_ticket(&server, Role::Queen, None, &token);
    let ctl_path = vec!["queen".to_owned(), "ctl".to_owned()];
    client.walk(1, 2, &ctl_path).unwrap();
    let err = client.open(2, OpenMode::write_append()).unwrap_err();
    match err {
        NineDoorError::Protocol { code, message } => {
            assert_eq!(code, ErrorCode::Permission);
            assert!(message.contains("EPERM"));
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let mut log_reader = attach_queen(&server);
    let log_text = read_log_text(&mut log_reader, 3);
    assert!(log_text.contains("ui-ticket outcome=deny reason=scope"));
    assert_eq!(server.pipeline_metrics().ui_denies, 1);
}

#[test]
fn bandwidth_quota_breach_returns_elimit() {
    let server = NineDoor::new();
    server.register_ticket_secret(Role::Queen, QUEEN_SECRET);

    let quotas = TicketQuotas {
        bandwidth_bytes: Some(8),
        cursor_resumes: None,
        cursor_advances: None,
    };
    let claims = TicketClaims::new(
        Role::Queen,
        BudgetSpec::unbounded(),
        None,
        MountSpec::empty(),
        unix_time_ms(),
    )
    .with_quotas(quotas);
    let token = TicketIssuer::new(QUEEN_SECRET)
        .issue(claims)
        .unwrap()
        .encode()
        .unwrap();

    let mut client = attach_with_ticket(&server, Role::Queen, None, &token);
    let proc_path = vec!["proc".to_owned(), "boot".to_owned()];
    client.walk(1, 2, &proc_path).unwrap();
    client.open(2, OpenMode::read_only()).unwrap();
    let err = client.read(2, 0, 16).unwrap_err();
    match err {
        NineDoorError::Protocol { code, message } => {
            assert_eq!(code, ErrorCode::TooBig);
            assert!(message.contains("ELIMIT"));
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let ok_data = client.read(2, 0, 4).unwrap();
    assert!(!ok_data.is_empty());

    let mut log_reader = attach_queen(&server);
    let log_text = read_log_text(&mut log_reader, 3);
    assert!(log_text.contains("ui-ticket outcome=deny reason=bandwidth"));
    assert_eq!(server.pipeline_metrics().ui_denies, 1);
}

#[test]
fn expired_ticket_replay_is_deterministic() {
    let server = NineDoor::new();
    server.register_ticket_secret(Role::Queen, QUEEN_SECRET);

    let now_ms = unix_time_ms();
    let issued_at_ms = now_ms.saturating_sub(2_000);
    let budget = BudgetSpec::unbounded().with_ttl(Some(1));
    let claims = TicketClaims::new(
        Role::Queen,
        budget,
        None,
        MountSpec::empty(),
        issued_at_ms,
    );
    let token = TicketIssuer::new(QUEEN_SECRET)
        .issue(claims)
        .unwrap()
        .encode()
        .unwrap();

    let err = attach_with_ticket_result(&server, Role::Queen, None, &token).unwrap_err();
    assert_expired_error(&err);

    let err = attach_with_ticket_result(&server, Role::Queen, None, &token).unwrap_err();
    assert_expired_error(&err);
    assert_eq!(server.pipeline_metrics().ui_denies, 2);
}

#[test]
fn cursor_resume_quota_is_enforced() {
    let server = NineDoor::new();
    server.register_ticket_secret(Role::Queen, QUEEN_SECRET);
    server.register_ticket_secret(Role::WorkerHeartbeat, WORKER_SECRET);

    let mut queen = attach_queen(&server);
    spawn_worker(&mut queen);

    let mut worker = attach_worker(&server, "worker-1");
    let telemetry_path = worker_telemetry_path("worker-1");
    worker.walk(1, 2, &telemetry_path).unwrap();
    worker.open(2, OpenMode::write_append()).unwrap();
    worker.write(2, b"heartbeat 1\n").unwrap();
    worker.clunk(2).unwrap();

    let quotas = TicketQuotas {
        bandwidth_bytes: Some(1_024),
        cursor_resumes: Some(1),
        cursor_advances: None,
    };
    let claims = TicketClaims::new(
        Role::Queen,
        BudgetSpec::unbounded(),
        None,
        MountSpec::empty(),
        unix_time_ms(),
    )
    .with_quotas(quotas);
    let token = TicketIssuer::new(QUEEN_SECRET)
        .issue(claims)
        .unwrap()
        .encode()
        .unwrap();

    let mut reader = attach_with_ticket(&server, Role::Queen, None, &token);
    reader.walk(1, 2, &telemetry_path).unwrap();
    reader.open(2, OpenMode::read_only()).unwrap();
    let _ = reader.read(2, 0, 32).unwrap();
    let _ = reader.read(2, 0, 32).unwrap();
    let err = reader.read(2, 0, 32).unwrap_err();
    match err {
        NineDoorError::Protocol { code, message } => {
            assert_eq!(code, ErrorCode::TooBig);
            assert!(message.contains("ELIMIT"));
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let mut log_reader = attach_queen(&server);
    let log_text = read_log_text(&mut log_reader, 5);
    assert!(log_text.contains("ui-ticket outcome=deny reason=cursor-resume"));
}

fn attach_queen(server: &NineDoor) -> InProcessConnection {
    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client.attach(1, Role::Queen).expect("attach queen");
    client
}

fn attach_with_ticket(
    server: &NineDoor,
    role: Role,
    identity: Option<&str>,
    ticket: &str,
) -> InProcessConnection {
    attach_with_ticket_result(server, role, identity, ticket).expect("attach ticket")
}

fn attach_with_ticket_result(
    server: &NineDoor,
    role: Role,
    identity: Option<&str>,
    ticket: &str,
) -> Result<InProcessConnection, NineDoorError> {
    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client.attach_with_identity(1, role, identity, Some(ticket))?;
    Ok(client)
}

fn attach_worker(server: &NineDoor, id: &str) -> InProcessConnection {
    let issuer = TicketIssuer::new(WORKER_SECRET);
    let claims = TicketClaims::new(
        Role::WorkerHeartbeat,
        BudgetSpec::default_heartbeat(),
        Some(id.to_owned()),
        MountSpec::empty(),
        unix_time_ms(),
    );
    let token = issuer.issue(claims).unwrap().encode().unwrap();
    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client
        .attach_with_identity(1, Role::WorkerHeartbeat, Some(id), Some(&token))
        .expect("attach worker");
    client
}

fn spawn_worker(client: &mut InProcessConnection) {
    let ctl_path = vec!["queen".to_owned(), "ctl".to_owned()];
    client.walk(1, 2, &ctl_path).unwrap();
    client.open(2, OpenMode::write_append()).unwrap();
    let payload = "{\"spawn\":\"heartbeat\",\"ticks\":5}\n";
    client.write(2, payload.as_bytes()).unwrap();
    client.clunk(2).unwrap();
}

fn worker_telemetry_path(worker_id: &str) -> Vec<String> {
    ShardLayout::default().worker_telemetry_path(worker_id)
}

fn read_log_text(client: &mut InProcessConnection, fid: u32) -> String {
    let log_path = vec!["log".to_owned(), "queen.log".to_owned()];
    client.walk(1, fid, &log_path).expect("walk /log/queen.log");
    client
        .open(fid, OpenMode::read_only())
        .expect("open /log/queen.log");
    let data = client.read(fid, 0, MAX_MSIZE).expect("read log");
    client.clunk(fid).expect("clunk log fid");
    String::from_utf8(data).expect("log utf8")
}

fn assert_expired_error(err: &NineDoorError) {
    match err {
        NineDoorError::Protocol { code, message } => {
            assert_eq!(code, &ErrorCode::Permission);
            assert!(message.contains("expired"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
