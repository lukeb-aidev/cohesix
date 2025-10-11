// Author: Lukas Bower
#![forbid(unsafe_code)]

use cohesix_ticket::Role;
use nine_door::{NineDoor, NineDoorError};
use secure9p_wire::{ErrorCode, OpenMode, MAX_MSIZE};

#[test]
fn attach_walk_read_and_write() {
    let server = NineDoor::new();
    let mut client = server.connect().expect("create session");
    client.version(MAX_MSIZE).expect("version handshake");
    client.attach(1, Role::Queen).expect("attach");

    let proc_path = vec!["proc".to_owned(), "boot".to_owned()];
    client.walk(1, 2, &proc_path).expect("walk /proc/boot");
    client
        .open(2, OpenMode::read_only())
        .expect("open /proc/boot");
    let data = client.read(2, 0, MAX_MSIZE).expect("read /proc/boot");
    let text = String::from_utf8(data).expect("utf8");
    assert!(text.contains("Cohesix boot"));

    let queen_ctl = vec!["queen".to_owned(), "ctl".to_owned()];
    client.walk(1, 3, &queen_ctl).expect("walk /queen/ctl");
    client
        .open(3, OpenMode::write_append())
        .expect("open /queen/ctl for append");
    let payload = b"{\"spawn\":\"heartbeat\",\"ticks\":5}\n";
    let written = client.write(3, payload).expect("write /queen/ctl");
    assert_eq!(written as usize, payload.len());
}

#[test]
fn queen_bind_is_session_scoped() {
    let server = NineDoor::new();
    let mut queen1 = server.connect().expect("create queen session");
    queen1.version(MAX_MSIZE).expect("version handshake");
    queen1.attach(1, Role::Queen).expect("queen attach");

    let queen_ctl = vec!["queen".to_owned(), "ctl".to_owned()];
    queen1.walk(1, 2, &queen_ctl).expect("walk /queen/ctl");
    queen1
        .open(2, OpenMode::write_append())
        .expect("open /queen/ctl for append");

    let spawn_payload = b"{\"spawn\":\"heartbeat\",\"ticks\":5}\n";
    queen1
        .write(2, spawn_payload)
        .expect("spawn heartbeat worker");

    let bind_payload = b"{\"bind\":{\"from\":\"/worker/worker-1\",\"to\":\"/queen\"}}\n";
    queen1
        .write(2, bind_payload)
        .expect("bind worker telemetry over /queen");

    let queen_remap = vec!["queen".to_owned(), "telemetry".to_owned()];
    queen1
        .walk(1, 4, &queen_remap)
        .expect("walk remapped queen telemetry");
    queen1
        .open(4, OpenMode::read_only())
        .expect("open remapped telemetry");
    let telemetry = queen1
        .read(4, 0, MAX_MSIZE)
        .expect("read remapped telemetry");
    assert!(telemetry.is_empty());

    let mut queen2 = server.connect().expect("create second queen session");
    queen2.version(MAX_MSIZE).expect("version handshake");
    queen2.attach(1, Role::Queen).expect("queen attach");
    queen2
        .walk(1, 2, &queen_ctl)
        .expect("second queen still sees /queen/ctl");
}

#[test]
fn worker_bind_command_rejected() {
    let server = NineDoor::new();
    let mut queen = server.connect().expect("create queen session");
    queen.version(MAX_MSIZE).expect("version handshake");
    queen.attach(1, Role::Queen).expect("queen attach");

    let queen_ctl = vec!["queen".to_owned(), "ctl".to_owned()];
    queen.walk(1, 2, &queen_ctl).expect("walk /queen/ctl");
    queen
        .open(2, OpenMode::write_append())
        .expect("open /queen/ctl");
    let spawn_payload = b"{\"spawn\":\"heartbeat\",\"ticks\":5}\n";
    queen
        .write(2, spawn_payload)
        .expect("spawn heartbeat worker");

    let mut worker = server.connect().expect("create worker session");
    worker.version(MAX_MSIZE).expect("version handshake");
    worker
        .attach_with_identity(1, Role::WorkerHeartbeat, Some("worker-1"))
        .expect("worker attach");

    let queen_path = vec!["queen".to_owned(), "ctl".to_owned()];
    let err = worker
        .walk(1, 2, &queen_path)
        .expect_err("worker walk /queen/ctl");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::Permission),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn queen_mounts_registered_service() {
    let server = NineDoor::new();
    server
        .register_service("logs", &["log"])
        .expect("register log service");

    let mut queen = server.connect().expect("create queen session");
    queen.version(MAX_MSIZE).expect("version handshake");
    queen.attach(1, Role::Queen).expect("queen attach");

    let queen_ctl = vec!["queen".to_owned(), "ctl".to_owned()];
    queen.walk(1, 2, &queen_ctl).expect("walk /queen/ctl");
    queen
        .open(2, OpenMode::write_append())
        .expect("open /queen/ctl");

    let mount_payload = b"{\"mount\":{\"service\":\"logs\",\"at\":\"/alias\"}}\n";
    queen
        .write(2, mount_payload)
        .expect("mount logs service at /alias");

    let alias_log = vec!["alias".to_owned(), "queen.log".to_owned()];
    queen.walk(1, 3, &alias_log).expect("walk alias log");
    queen
        .open(3, OpenMode::read_only())
        .expect("open alias log for read");

    let mut queen_second = server.connect().expect("create second queen session");
    queen_second.version(MAX_MSIZE).expect("version handshake");
    queen_second.attach(1, Role::Queen).expect("queen attach");
    let err = queen_second
        .walk(1, 2, &alias_log)
        .expect_err("second queen walk alias log");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::NotFound),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn spawn_emit_kill_logs_revocation() {
    let server = NineDoor::new();
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
    queen.clunk(2).expect("clunk ctl fid");

    let mut worker = server.connect().expect("create worker session");
    worker.version(MAX_MSIZE).expect("version handshake");
    worker
        .attach_with_identity(1, Role::WorkerHeartbeat, Some("worker-1"))
        .expect("worker attach");
    let telemetry = vec![
        "worker".to_owned(),
        "worker-1".to_owned(),
        "telemetry".to_owned(),
    ];
    worker.walk(1, 2, &telemetry).expect("walk telemetry");
    worker
        .open(2, OpenMode::write_append())
        .expect("open telemetry");
    worker.write(2, b"heartbeat 1\n").expect("write telemetry");

    queen.walk(1, 3, &queen_ctl).expect("walk /queen/ctl again");
    queen
        .open(3, OpenMode::write_append())
        .expect("reopen /queen/ctl");
    queen
        .write(3, b"{\"kill\":\"worker-1\"}\n")
        .expect("kill worker");
    queen.clunk(3).expect("clunk kill fid");

    let err = worker
        .write(2, b"heartbeat 2\n")
        .expect_err("write after kill");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::Closed),
        other => panic!("unexpected error: {other:?}"),
    }

    let queen_log = vec!["log".to_owned(), "queen.log".to_owned()];
    queen.walk(1, 4, &queen_log).expect("walk log");
    queen.open(4, OpenMode::read_only()).expect("open log");
    let log = String::from_utf8(queen.read(4, 0, MAX_MSIZE).expect("read log")).expect("log utf8");
    assert!(log.contains("spawned worker-1"));
    assert!(log.contains("killed worker-1"));
    assert!(log.contains("revoked worker-1: killed by queen"));
}
