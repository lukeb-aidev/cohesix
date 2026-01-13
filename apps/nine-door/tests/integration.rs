// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Integration tests for NineDoor host namespace and GPU workflow.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use cohesix_ticket::{BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer};
use nine_door::{Clock, InProcessConnection, NineDoor, NineDoorError};
use secure9p_codec::{ErrorCode, OpenMode, MAX_MSIZE};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use worker_gpu::{GpuLease, GpuWorker};

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
    server.register_ticket_secret(Role::WorkerHeartbeat, "worker-secret");
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
        .attach_with_identity(
            1,
            Role::WorkerHeartbeat,
            Some("worker-1"),
            Some(issue_ticket("worker-secret", Role::WorkerHeartbeat, "worker-1").as_str()),
        )
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
    server.register_ticket_secret(Role::WorkerHeartbeat, "worker-secret");
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
        .attach_with_identity(
            1,
            Role::WorkerHeartbeat,
            Some("worker-1"),
            Some(issue_ticket("worker-secret", Role::WorkerHeartbeat, "worker-1").as_str()),
        )
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

#[test]
fn queen_spawns_gpu_worker_and_runs_job() {
    let server = NineDoor::new();
    server.register_ticket_secret(Role::WorkerGpu, "gpu-secret");
    let bridge = gpu_bridge_host::GpuBridge::mock();
    let topology = bridge.serialise_namespace().expect("serialise namespace");
    server
        .install_gpu_nodes(&topology)
        .expect("install gpu nodes");

    let mut queen = attach_queen(&server);
    let active_model_path = vec!["gpu".to_owned(), "models".to_owned(), "active".to_owned()];
    queen
        .walk(1, 2, &active_model_path)
        .expect("walk active model");
    queen
        .open(2, OpenMode::read_only())
        .expect("open active model");
    let active_model = String::from_utf8(queen.read(2, 0, MAX_MSIZE).unwrap()).unwrap();
    assert!(active_model.contains("vision-lora-edge"));

    let spawn_payload = "{\"spawn\":\"gpu\",\"lease\":{\"gpu_id\":\"GPU-0\",\"mem_mb\":4096,\"streams\":2,\"ttl_s\":120,\"priority\":5}}\n";
    write_queen_command(&mut queen, spawn_payload);

    let mut worker = server.connect().expect("create gpu worker session");
    worker.version(MAX_MSIZE).expect("version handshake");
    worker
        .attach_with_identity(
            1,
            Role::WorkerGpu,
            Some("worker-1"),
            Some(issue_ticket("gpu-secret", Role::WorkerGpu, "worker-1").as_str()),
        )
        .expect("gpu worker attach");
    let job_path = vec!["gpu".to_owned(), "GPU-0".to_owned(), "job".to_owned()];
    worker.walk(1, 2, &job_path).expect("walk job path");
    worker
        .open(2, OpenMode::write_append())
        .expect("open job file");

    let lease = GpuLease::new("GPU-0", 4096, 2, 120, 5, "worker-1").unwrap();
    let gpu_worker = GpuWorker::new(worker.session_id(), lease);
    let descriptor = gpu_worker
        .vector_add(&[1.0f32, 2.0], &[3.0f32, 4.0])
        .expect("vector add descriptor");
    let payload = format!("{}\n", serde_json::to_string(&descriptor).unwrap());
    worker.write(2, payload.as_bytes()).expect("submit gpu job");

    let status_path = vec!["gpu".to_owned(), "GPU-0".to_owned(), "status".to_owned()];
    queen.walk(1, 3, &status_path).expect("walk status path");
    queen
        .open(3, OpenMode::read_only())
        .expect("open status file");
    let status = String::from_utf8(queen.read(3, 0, MAX_MSIZE).unwrap()).unwrap();
    assert!(status.contains("\"state\":\"QUEUED\""));
    assert!(status.contains("\"state\":\"OK\""));

    let telemetry_path = vec![
        "worker".to_owned(),
        "worker-1".to_owned(),
        "telemetry".to_owned(),
    ];
    queen
        .walk(1, 4, &telemetry_path)
        .expect("walk telemetry path");
    queen
        .open(4, OpenMode::read_only())
        .expect("open telemetry file");
    let telemetry = String::from_utf8(queen.read(4, 0, MAX_MSIZE).unwrap()).unwrap();
    assert!(telemetry.contains("\"state\":\"RUNNING\""));
    assert!(telemetry.contains("\"state\":\"OK\""));
}

#[test]
fn gpu_job_write_requires_utf8() {
    let server = NineDoor::new();
    server.register_ticket_secret(Role::WorkerGpu, "gpu-secret");
    let bridge = gpu_bridge_host::GpuBridge::mock();
    server
        .install_gpu_nodes(&bridge.serialise_namespace().unwrap())
        .expect("install gpu nodes");

    let mut queen = attach_queen(&server);
    let payload = "{\"spawn\":\"gpu\",\"lease\":{\"gpu_id\":\"GPU-0\",\"mem_mb\":2048,\"streams\":1,\"ttl_s\":60,\"priority\":1}}\n";
    write_queen_command(&mut queen, payload);

    let mut worker = attach_gpu_worker(&server, "worker-1");
    open_gpu_job_file(&mut worker, 2, "GPU-0");

    let err = worker
        .write(2, &[0xff])
        .expect_err("non-utf8 payload rejected");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::Invalid),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn gpu_job_descriptor_must_validate_payload() {
    let server = NineDoor::new();
    server.register_ticket_secret(Role::WorkerGpu, "gpu-secret");
    let bridge = gpu_bridge_host::GpuBridge::mock();
    server
        .install_gpu_nodes(&bridge.serialise_namespace().unwrap())
        .expect("install gpu nodes");

    let mut queen = attach_queen(&server);
    let payload = "{\"spawn\":\"gpu\",\"lease\":{\"gpu_id\":\"GPU-0\",\"mem_mb\":1024,\"streams\":1,\"ttl_s\":60,\"priority\":1}}\n";
    write_queen_command(&mut queen, payload);

    let mut worker = attach_gpu_worker(&server, "worker-1");
    open_gpu_job_file(&mut worker, 2, "GPU-0");

    let lease = GpuLease::new("GPU-0", 1024, 1, 60, 1, "worker-1").unwrap();
    let gpu_worker = GpuWorker::new(worker.session_id(), lease);
    let mut descriptor = gpu_worker
        .vector_add(&[1.0f32, 2.0], &[3.0f32, 4.0])
        .expect("descriptor");
    descriptor.bytes_hash =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned();
    let payload = format!("{}\n", serde_json::to_string(&descriptor).unwrap());
    let err = worker
        .write(2, payload.as_bytes())
        .expect_err("hash mismatch rejected");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::Invalid),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn gpu_lease_expiry_revokes_job_access() {
    let clock = Arc::new(TestClock::new());
    let server = NineDoor::new_with_clock(clock.clone());
    server.register_ticket_secret(Role::WorkerGpu, "gpu-secret");
    let bridge = gpu_bridge_host::GpuBridge::mock();
    server
        .install_gpu_nodes(&bridge.serialise_namespace().unwrap())
        .expect("install gpu nodes");

    let mut queen = attach_queen(&server);
    let payload = "{\"spawn\":\"gpu\",\"lease\":{\"gpu_id\":\"GPU-0\",\"mem_mb\":2048,\"streams\":1,\"ttl_s\":1,\"priority\":1}}\n";
    write_queen_command(&mut queen, payload);

    let mut worker = server.connect().expect("create gpu worker session");
    worker.version(MAX_MSIZE).expect("version handshake");
    worker
        .attach_with_identity(
            1,
            Role::WorkerGpu,
            Some("worker-1"),
            Some(issue_ticket("gpu-secret", Role::WorkerGpu, "worker-1").as_str()),
        )
        .expect("gpu worker attach");
    let job_path = vec!["gpu".to_owned(), "GPU-0".to_owned(), "job".to_owned()];
    worker.walk(1, 2, &job_path).expect("walk job path");
    worker
        .open(2, OpenMode::write_append())
        .expect("open job file");

    clock.advance(Duration::from_secs(2));

    let lease = GpuLease::new("GPU-0", 2048, 1, 1, 1, "worker-1").unwrap();
    let gpu_worker = GpuWorker::new(worker.session_id(), lease);
    let descriptor = gpu_worker
        .vector_add(&[1.0f32, 2.0], &[3.0f32, 4.0])
        .expect("descriptor");
    let payload = format!("{}\n", serde_json::to_string(&descriptor).unwrap());
    let err = worker
        .write(2, payload.as_bytes())
        .expect_err("write after ttl");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::Closed),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn trace_events_record_spawn_and_task_view() {
    let server = NineDoor::new();
    let mut queen = attach_queen(&server);
    write_queen_command(&mut queen, "{\"spawn\":\"heartbeat\",\"ticks\":5}\n");

    let events_path = vec!["trace".to_owned(), "events".to_owned()];
    let events = read_all(&mut queen, &events_path);
    assert!(events.contains("\"spawned worker-1"));

    let task_path = vec!["proc".to_owned(), "worker-1".to_owned(), "trace".to_owned()];
    let task_view = read_all(&mut queen, &task_path);
    assert!(task_view.contains("\"spawned worker-1"));
}

#[test]
fn trace_control_filters_categories() {
    let server = NineDoor::new();
    let mut queen = attach_queen(&server);

    let ctl_path = vec!["trace".to_owned(), "ctl".to_owned()];
    queen.walk(1, 2, &ctl_path).expect("walk /trace/ctl");
    queen
        .open(2, OpenMode::write_append())
        .expect("open /trace/ctl");
    let filter_payload = b"{\"set\":{\"level\":\"info\",\"cats\":[\"queen\"]}}\n";
    queen.write(2, filter_payload).expect("write trace filter");
    queen.clunk(2).expect("clunk trace ctl");

    write_queen_command(&mut queen, "{\"spawn\":\"heartbeat\",\"ticks\":5}\n");
    write_queen_command(&mut queen, "{\"budget\":{\"ttl_s\":60}}\n");

    let events_path = vec!["trace".to_owned(), "events".to_owned()];
    let events = read_all(&mut queen, &events_path);
    assert!(events.contains("updated default budget"));
    assert!(!events.contains("spawned worker-1"));

    let ctl_contents = read_all(&mut queen, &ctl_path);
    assert!(ctl_contents.contains("\"cats\":[\"queen\"]"));
}

fn attach_queen(server: &NineDoor) -> InProcessConnection {
    let mut client = server.connect().expect("create queen session");
    client.version(MAX_MSIZE).expect("version negotiation");
    client.attach(1, Role::Queen).expect("queen attach");
    client
}

fn write_queen_command(client: &mut InProcessConnection, payload: &str) {
    let path = vec!["queen".to_owned(), "ctl".to_owned()];
    client.walk(1, 2, &path).expect("walk /queen/ctl");
    client
        .open(2, OpenMode::write_append())
        .expect("open /queen/ctl");
    client.write(2, payload.as_bytes()).expect("write command");
    client.clunk(2).expect("clunk ctl fid");
}

fn attach_gpu_worker(server: &NineDoor, id: &str) -> InProcessConnection {
    let mut client = server.connect().expect("create gpu worker session");
    client.version(MAX_MSIZE).expect("version handshake");
    client
        .attach_with_identity(
            1,
            Role::WorkerGpu,
            Some(id),
            Some(issue_ticket("gpu-secret", Role::WorkerGpu, id).as_str()),
        )
        .expect("gpu worker attach");
    client
}

fn open_gpu_job_file(client: &mut InProcessConnection, fid: u32, gpu_id: &str) {
    let job_path = vec!["gpu".to_owned(), gpu_id.to_owned(), "job".to_owned()];
    client.walk(1, fid, &job_path).expect("walk gpu job path");
    client
        .open(fid, OpenMode::write_append())
        .expect("open gpu job file");
}

fn read_all(client: &mut InProcessConnection, path: &[String]) -> String {
    let fid = 97;
    client.walk(1, fid, path).expect("walk read path");
    client
        .open(fid, OpenMode::read_only())
        .expect("open read path");
    let mut offset = 0u64;
    let mut buffer = Vec::new();
    loop {
        let chunk = client
            .read(fid, offset, MAX_MSIZE)
            .expect("read path chunk");
        if chunk.is_empty() {
            break;
        }
        offset = offset + chunk.len() as u64;
        buffer.extend_from_slice(&chunk);
        if chunk.len() < MAX_MSIZE as usize {
            break;
        }
    }
    client.clunk(fid).expect("clunk read fid");
    String::from_utf8(buffer).expect("path utf8")
}

#[derive(Debug)]
struct TestClock {
    now: Mutex<Instant>,
}

impl TestClock {
    fn new() -> Self {
        Self {
            now: Mutex::new(Instant::now()),
        }
    }

    fn advance(&self, duration: Duration) {
        let mut guard = self.now.lock().expect("clock mutex");
        *guard += duration;
    }
}

impl Clock for TestClock {
    fn now(&self) -> Instant {
        *self.now.lock().expect("clock mutex")
    }
}
