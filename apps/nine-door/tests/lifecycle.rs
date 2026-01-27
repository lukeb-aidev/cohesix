// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate lifecycle namespace nodes and transitions.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::time::{SystemTime, UNIX_EPOCH};

use cohesix_ticket::{BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer};
use nine_door::{InProcessConnection, NineDoor, NineDoorError, ShardLayout};
use secure9p_codec::{ErrorCode, OpenMode, MAX_MSIZE};

const WORKER_SECRET: &str = "worker-secret";

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn attach_queen(server: &NineDoor) -> InProcessConnection {
    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client.attach(1, Role::Queen).expect("attach queen");
    client
}

fn attach_worker(server: &NineDoor, worker_id: &str) -> InProcessConnection {
    let issuer = TicketIssuer::new(WORKER_SECRET);
    let claims = TicketClaims::new(
        Role::WorkerHeartbeat,
        BudgetSpec::default_heartbeat(),
        Some(worker_id.to_owned()),
        MountSpec::empty(),
        unix_time_ms(),
    );
    let token = issuer
        .issue(claims)
        .expect("issue")
        .encode()
        .expect("encode");
    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client
        .attach_with_identity(1, Role::WorkerHeartbeat, Some(worker_id), Some(&token))
        .expect("attach worker");
    client
}

fn read_text(client: &mut InProcessConnection, fid: u32, path: &[String]) -> String {
    client.walk(1, fid, path).expect("walk");
    client.open(fid, OpenMode::read_only()).expect("open");
    let data = client.read(fid, 0, MAX_MSIZE).expect("read");
    client.clunk(fid).expect("clunk");
    String::from_utf8(data).expect("utf8")
}

fn write_line(client: &mut InProcessConnection, fid: u32, path: &[String], payload: &str) {
    client.walk(1, fid, path).expect("walk");
    client.open(fid, OpenMode::write_append()).expect("open");
    client.write(fid, payload.as_bytes()).expect("write");
    client.clunk(fid).expect("clunk");
}

fn spawn_worker(client: &mut InProcessConnection) {
    let ctl_path = vec!["queen".to_owned(), "ctl".to_owned()];
    write_line(
        client,
        2,
        &ctl_path,
        "{\"spawn\":\"heartbeat\",\"ticks\":5}\n",
    );
}

#[test]
fn lifecycle_proc_files_are_read_only() {
    let server = NineDoor::new();
    let mut client = attach_queen(&server);
    let targets = [
        vec![
            "proc".to_owned(),
            "lifecycle".to_owned(),
            "state".to_owned(),
        ],
        vec![
            "proc".to_owned(),
            "lifecycle".to_owned(),
            "reason".to_owned(),
        ],
        vec![
            "proc".to_owned(),
            "lifecycle".to_owned(),
            "since".to_owned(),
        ],
    ];
    for (idx, path) in targets.into_iter().enumerate() {
        client.walk(1, (idx + 2) as u32, &path).expect("walk");
        let err = client
            .open((idx + 2) as u32, OpenMode::write_append())
            .expect_err("open for write should fail");
        match err {
            NineDoorError::Protocol { code, .. } => {
                assert_eq!(code, ErrorCode::Permission);
            }
            other => panic!("unexpected error: {other:?}"),
        }
        client.clunk((idx + 2) as u32).ok();
    }
    let state = read_text(
        &mut client,
        10,
        &vec![
            "proc".to_owned(),
            "lifecycle".to_owned(),
            "state".to_owned(),
        ],
    );
    assert!(state.contains("state=ONLINE"));
}

#[test]
fn lifecycle_ctl_transitions_and_invalids() {
    let server = NineDoor::new();
    let mut client = attach_queen(&server);
    let state_path = vec![
        "proc".to_owned(),
        "lifecycle".to_owned(),
        "state".to_owned(),
    ];
    let ctl_path = vec!["queen".to_owned(), "lifecycle".to_owned(), "ctl".to_owned()];

    let state = read_text(&mut client, 2, &state_path);
    assert!(state.contains("state=ONLINE"));

    client.walk(1, 3, &ctl_path).expect("walk ctl");
    client.open(3, OpenMode::write_append()).expect("open ctl");
    let err = client.write(3, b"drain\n").expect_err("invalid drain");
    match err {
        NineDoorError::Protocol { code, message } => {
            assert_eq!(code, ErrorCode::Invalid);
            assert!(message.contains("invalid lifecycle transition"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
    client.clunk(3).expect("clunk ctl");

    let state = read_text(&mut client, 4, &state_path);
    assert!(state.contains("state=ONLINE"));

    write_line(&mut client, 5, &ctl_path, "cordon\n");
    let state = read_text(&mut client, 6, &state_path);
    assert!(state.contains("state=DRAINING"));

    write_line(&mut client, 7, &ctl_path, "drain\n");
    let state = read_text(&mut client, 8, &state_path);
    assert!(state.contains("state=QUIESCED"));

    write_line(&mut client, 9, &ctl_path, "resume\n");
    let state = read_text(&mut client, 10, &state_path);
    assert!(state.contains("state=ONLINE"));
}

#[test]
fn lifecycle_ctl_requires_queen() {
    let server = NineDoor::new_with_shard_layout(ShardLayout::default());
    server.register_ticket_secret(Role::WorkerHeartbeat, WORKER_SECRET);

    let mut queen = attach_queen(&server);
    spawn_worker(&mut queen);

    let mut worker = attach_worker(&server, "worker-1");
    let ctl_path = vec!["queen".to_owned(), "lifecycle".to_owned(), "ctl".to_owned()];
    let err = worker
        .walk(1, 2, &ctl_path)
        .expect_err("worker walk /queen/lifecycle/ctl");
    match err {
        NineDoorError::Protocol { code, .. } => {
            assert_eq!(code, ErrorCode::Permission);
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
