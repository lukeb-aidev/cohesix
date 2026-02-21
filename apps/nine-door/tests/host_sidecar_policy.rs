// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate host sidecar policy enforcement and audit logging.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use cohesix_ticket::{BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer};
use nine_door::{HostNamespaceConfig, HostProvider, NineDoor, NineDoorError};
use secure9p_codec::{ErrorCode, OpenMode, MAX_MSIZE};
use std::time::{SystemTime, UNIX_EPOCH};

fn issue_ticket(secret: &str, role: Role, subject: &str) -> String {
    let budget = match role {
        Role::Queen => BudgetSpec::unbounded(),
        Role::WorkerHeartbeat => BudgetSpec::default_heartbeat(),
        Role::WorkerGpu => BudgetSpec::default_gpu(),
        Role::WorkerBus | Role::WorkerLora => BudgetSpec::default_heartbeat(),
    };
    let issuer = TicketIssuer::new(secret);
    let claims = TicketClaims::new(
        role,
        budget,
        Some(subject.to_owned()),
        MountSpec::empty(),
        unix_time_ms(),
    );
    issuer.issue(claims).unwrap().encode().unwrap()
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn read_log_text(client: &mut nine_door::InProcessConnection, fid: u32) -> String {
    let log_path = vec!["log".to_owned(), "queen.log".to_owned()];
    client.walk(1, fid, &log_path).expect("walk /log/queen.log");
    client
        .open(fid, OpenMode::read_only())
        .expect("open /log/queen.log");
    let data = client.read(fid, 0, MAX_MSIZE).expect("read log");
    client.clunk(fid).expect("clunk log fid");
    String::from_utf8(data).expect("log utf8")
}

#[test]
fn host_namespace_disabled_omits_mount() {
    let server = NineDoor::new();
    let mut client = server.connect().expect("create session");
    client.version(MAX_MSIZE).expect("version handshake");
    client.attach(1, Role::Queen).expect("attach queen");
    let host_path = vec!["host".to_owned()];
    let err = client.walk(1, 2, &host_path).expect_err("walk /host");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::NotFound),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn host_control_write_requires_queen_and_audits() {
    let host_config = HostNamespaceConfig::enabled(
        "/host",
        &[
            HostProvider::Systemd,
            HostProvider::K8s,
            HostProvider::Nvidia,
        ],
    )
    .expect("host config");
    let server = NineDoor::new_with_host_config(host_config);
    server.register_ticket_secret(Role::WorkerHeartbeat, "worker");

    let ticket = issue_ticket("worker", Role::WorkerHeartbeat, "worker-1");

    let mut queen = server.connect().expect("create queen session");
    queen.version(MAX_MSIZE).expect("version handshake");
    queen.attach(1, Role::Queen).expect("queen attach");
    let queen_ctl = vec!["queen".to_owned(), "ctl".to_owned()];
    queen.walk(1, 2, &queen_ctl).expect("walk /queen/ctl");
    queen
        .open(2, OpenMode::write_append())
        .expect("open /queen/ctl");
    queen
        .write(2, b"{\"spawn\":\"heartbeat\",\"ticks\":5}\n")
        .expect("spawn worker");
    queen.clunk(2).expect("clunk /queen/ctl");

    let mut worker = server.connect().expect("create worker session");
    worker.version(MAX_MSIZE).expect("version handshake");
    worker
        .attach_with_identity(
            1,
            Role::WorkerHeartbeat,
            Some("worker-1"),
            Some(ticket.as_str()),
        )
        .expect("worker attach");

    let restart_path = vec![
        "host".to_owned(),
        "systemd".to_owned(),
        "cohesix-agent.service".to_owned(),
        "restart".to_owned(),
    ];
    worker.walk(1, 2, &restart_path).expect("walk restart");
    let err = worker
        .open(2, OpenMode::write_append())
        .expect_err("worker open restart");
    match err {
        NineDoorError::Protocol { code, message } => {
            assert_eq!(code, ErrorCode::Permission);
            assert!(message.contains("EPERM"));
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let log_text = read_log_text(&mut queen, 4);
    assert!(log_text.contains("host-write outcome=deny"));
    assert!(log_text.contains(&format!("ticket={ticket}")));
    assert!(log_text.contains("path=/host/systemd/cohesix-agent.service/restart"));

    queen.walk(1, 3, &restart_path).expect("queen walk restart");
    queen
        .open(3, OpenMode::write_append())
        .expect("queen open restart");
    let payload = b"restart";
    let written = queen.write(3, payload).expect("queen write restart");
    assert_eq!(written as usize, payload.len());

    let log_text = read_log_text(&mut queen, 5);
    assert!(log_text.contains("host-write outcome=allow"));
    assert!(log_text.contains("control=systemd.restart"));
}

#[test]
fn host_ticket_streams_validate_schema_and_bounds() {
    let host_config =
        HostNamespaceConfig::enabled("/host", &[HostProvider::Systemd]).expect("host config");
    let server = NineDoor::new_with_host_config(host_config);
    server.register_ticket_secret(Role::WorkerHeartbeat, "worker");
    let ticket = issue_ticket("worker", Role::WorkerHeartbeat, "worker-1");

    let mut queen = server.connect().expect("create queen session");
    queen.version(MAX_MSIZE).expect("version handshake");
    queen.attach(1, Role::Queen).expect("queen attach");
    let queen_ctl = vec!["queen".to_owned(), "ctl".to_owned()];
    queen.walk(1, 8, &queen_ctl).expect("walk /queen/ctl");
    queen
        .open(8, OpenMode::write_append())
        .expect("open /queen/ctl");
    queen
        .write(8, b"{\"spawn\":\"heartbeat\",\"ticks\":5}\n")
        .expect("spawn worker");
    queen.clunk(8).expect("clunk /queen/ctl");

    let mut worker = server.connect().expect("create worker session");
    worker.version(MAX_MSIZE).expect("version handshake");
    worker
        .attach_with_identity(
            1,
            Role::WorkerHeartbeat,
            Some("worker-1"),
            Some(ticket.as_str()),
        )
        .expect("worker attach");

    let spec_path = vec!["host".to_owned(), "tickets".to_owned(), "spec".to_owned()];
    worker.walk(1, 2, &spec_path).expect("walk ticket spec");
    let err = worker
        .open(2, OpenMode::write_append())
        .expect_err("worker open ticket spec");
    match err {
        NineDoorError::Protocol { code, message } => {
            assert_eq!(code, ErrorCode::Permission);
            assert!(message.contains("EPERM"));
        }
        other => panic!("unexpected error: {other:?}"),
    }

    queen
        .walk(1, 2, &spec_path)
        .expect("queen walk ticket spec");
    queen
        .open(2, OpenMode::write_append())
        .expect("queen open ticket spec");
    let valid_spec = br#"{"schema":"host-ticket/v1","id":"ticket-1","idempotency_key":"idem-1","action":"systemd.restart","target":"/host/systemd/cohesix-agent.service/restart"}"#;
    queen.write(2, valid_spec).expect("write valid ticket spec");
    let federated_spec = br#"{"schema":"host-ticket/v1","id":"ticket-2","idempotency_key":"idem-2","action":"systemd.restart","target":"/host/systemd/cohesix-agent.service/restart","expires_unix_ms":1893456000000,"source_hive":"hive-a","target_hive":"hive-b","relay_hop":1,"relay_correlation_id":"ticket-2:idem-2:hive-a:hive-b"}"#;
    queen
        .write(2, federated_spec)
        .expect("write federated ticket spec");
    queen.clunk(2).expect("clunk ticket spec");

    queen
        .walk(1, 3, &spec_path)
        .expect("queen walk ticket spec");
    queen
        .open(3, OpenMode::write_append())
        .expect("queen open ticket spec");
    let err = queen
        .write(3, b"{\"schema\":\"host-ticket/v1\",\"id\":\"bad\",\"idempotency_key\":\"idem\",\"action\":\"unknown\"}")
        .expect_err("write invalid allowlist action");
    match err {
        NineDoorError::Protocol { code, message } => {
            assert_eq!(code, ErrorCode::Invalid);
            assert!(message.contains("not allowlisted"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
    queen.clunk(3).expect("clunk invalid ticket spec");

    queen
        .walk(1, 9, &spec_path)
        .expect("queen walk ticket spec");
    queen
        .open(9, OpenMode::write_append())
        .expect("queen open ticket spec");
    let err = queen
        .write(9, br#"{"schema":"host-ticket/v1","id":"bad-fed","idempotency_key":"idem-fed","action":"systemd.restart","source_hive":"hive-a"}"#)
        .expect_err("write invalid federated ticket fields");
    match err {
        NineDoorError::Protocol { code, message } => {
            assert_eq!(code, ErrorCode::Invalid);
            assert!(message.contains("federation"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
    queen.clunk(9).expect("clunk invalid federated ticket spec");

    queen
        .walk(1, 4, &spec_path)
        .expect("queen walk ticket spec");
    queen
        .open(4, OpenMode::write_append())
        .expect("queen open ticket spec");
    let oversize_id = "a".repeat(2050);
    let oversize = format!(
        "{{\"schema\":\"host-ticket/v1\",\"id\":\"{oversize_id}\",\"idempotency_key\":\"idem-2\",\"action\":\"systemd.restart\"}}"
    );
    let err = queen
        .write(4, oversize.as_bytes())
        .expect_err("write oversize ticket line");
    match err {
        NineDoorError::Protocol { code, message } => {
            assert_eq!(code, ErrorCode::Invalid);
            assert!(message.contains("max_line_bytes"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
    queen.clunk(4).expect("clunk oversize ticket spec");

    let status_path = vec!["host".to_owned(), "tickets".to_owned(), "status".to_owned()];
    queen
        .walk(1, 5, &status_path)
        .expect("queen walk ticket status");
    queen
        .open(5, OpenMode::write_append())
        .expect("queen open ticket status");
    let status = br#"{"schema":"host-ticket-result/v1","id":"ticket-1","idempotency_key":"idem-1","action":"systemd.restart","state":"claimed","message":"accepted"}"#;
    queen.write(5, status).expect("write valid ticket status");
    queen.clunk(5).expect("clunk ticket status");

    let deadletter_path = vec![
        "host".to_owned(),
        "tickets".to_owned(),
        "deadletter".to_owned(),
    ];
    queen
        .walk(1, 6, &deadletter_path)
        .expect("queen walk deadletter");
    queen
        .open(6, OpenMode::write_append())
        .expect("queen open deadletter");
    let err = queen
        .write(6, b"{\"schema\":\"host-ticket-result/v1\",\"id\":\"ticket-1\",\"idempotency_key\":\"idem-1\",\"action\":\"systemd.restart\",\"state\":\"bogus\"}")
        .expect_err("write invalid ticket state");
    match err {
        NineDoorError::Protocol { code, message } => {
            assert_eq!(code, ErrorCode::Invalid);
            assert!(message.contains("state"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
    queen.clunk(6).expect("clunk deadletter");

    let log_text = read_log_text(&mut queen, 7);
    assert!(log_text.contains("control=tickets.spec"));
    assert!(log_text.contains("control=tickets.status"));
}
