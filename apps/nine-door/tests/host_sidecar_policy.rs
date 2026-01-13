// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate host sidecar policy enforcement and audit logging.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use cohesix_ticket::{BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer};
use nine_door::{HostNamespaceConfig, HostProvider, NineDoor, NineDoorError};
use secure9p_codec::{ErrorCode, OpenMode, MAX_MSIZE};

fn issue_ticket(secret: &str, role: Role, subject: &str) -> String {
    let budget = match role {
        Role::Queen => BudgetSpec::unbounded(),
        Role::WorkerHeartbeat => BudgetSpec::default_heartbeat(),
        Role::WorkerGpu => BudgetSpec::default_gpu(),
    };
    let issuer = TicketIssuer::new(secret);
    let claims = TicketClaims::new(
        role,
        budget,
        Some(subject.to_owned()),
        MountSpec::empty(),
        0,
    );
    issuer.issue(claims).unwrap().encode().unwrap()
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
