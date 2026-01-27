// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate ReplayFS control and status determinism.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use cohesix_ticket::Role;
use nine_door::{
    AuditConfig, AuditLimits, HostNamespaceConfig, NineDoor, NineDoorError, PolicyConfig,
    ReplayConfig,
};
use secure9p_codec::{Codec, ErrorCode, OpenMode, Request, RequestBody, ResponseBody, MAX_MSIZE};

fn write_with_offset(
    client: &mut nine_door::InProcessConnection,
    fid: u32,
    offset: u64,
    data: &[u8],
) -> Result<u32, NineDoorError> {
    let codec = Codec;
    let request = Request {
        tag: 201,
        body: RequestBody::Write {
            fid,
            offset,
            data: data.to_vec(),
        },
    };
    let frame = codec.encode_request(&request)?;
    let response_bytes = client.exchange_batch(&frame)?;
    let response = codec.decode_response(&response_bytes)?;
    match response.body {
        ResponseBody::Write { count } => Ok(count),
        ResponseBody::Error { code, message } => Err(NineDoorError::Protocol { code, message }),
        other => Err(NineDoorError::Protocol {
            code: ErrorCode::Invalid,
            message: format!("unexpected response: {other:?}"),
        }),
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut hash = OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

#[test]
fn replayfs_replays_control_sequence_and_bounds() {
    let audit = AuditConfig::enabled(
        AuditLimits {
            journal_max_bytes: 1024,
            decisions_max_bytes: 256,
        },
        ReplayConfig::enabled(8, 256, 256),
    );
    let server = NineDoor::new_with_host_policy_audit_config(
        HostNamespaceConfig::disabled(),
        PolicyConfig::disabled(),
        audit,
    );

    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client.attach(1, Role::Queen).expect("attach");

    let ctl_path = vec!["queen".to_owned(), "ctl".to_owned()];
    client.walk(1, 2, &ctl_path).expect("walk ctl");
    client.open(2, OpenMode::write_append()).expect("open ctl");
    client
        .write(2, br#"{"spawn":"heartbeat","ticks":10}"#)
        .expect("spawn worker");
    client
        .write(2, br#"{"kill":"worker-1"}"#)
        .expect("kill worker");
    client.clunk(2).expect("clunk ctl");

    let replay_ctl = vec!["replay".to_owned(), "ctl".to_owned()];
    client.walk(1, 3, &replay_ctl).expect("walk replay ctl");
    client
        .open(3, OpenMode::write_append())
        .expect("open replay ctl");
    client.write(3, br#"{"from":0}"#).expect("replay from zero");
    client.clunk(3).expect("clunk replay ctl");

    let status_path = vec!["replay".to_owned(), "status".to_owned()];
    client.walk(1, 4, &status_path).expect("walk status");
    client.open(4, OpenMode::read_only()).expect("open status");
    let status_data = client.read(4, 0, MAX_MSIZE).expect("read status");
    let status: serde_json::Value = serde_json::from_slice(&status_data).expect("status json");
    assert_eq!(status["state"], "ok");
    assert_eq!(status["entries"].as_u64().unwrap(), 2);
    let expected_hash = format!("{:016x}", fnv1a64(b"OK\nOK\n"));
    assert_eq!(status["sequence_fnv1a"], expected_hash);

    client.walk(1, 5, &replay_ctl).expect("walk replay ctl");
    client
        .open(5, OpenMode::write_append())
        .expect("open replay ctl");
    let err =
        write_with_offset(&mut client, 5, 0, br#"{"from":0}"#).expect_err("random write rejected");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::Invalid),
        other => panic!("unexpected error: {other:?}"),
    }

    let export_path = vec!["audit".to_owned(), "export".to_owned()];
    client.walk(1, 6, &export_path).expect("walk export");
    client.open(6, OpenMode::read_only()).expect("open export");
    let export_data = client.read(6, 0, MAX_MSIZE).expect("read export");
    let export: serde_json::Value = serde_json::from_slice(&export_data).expect("export json");
    let next = export["journal_next"].as_u64().expect("journal_next");
    let bad_payload = format!("{{\"from\":{}}}", next + 1);

    client.walk(1, 7, &replay_ctl).expect("walk replay ctl");
    client
        .open(7, OpenMode::write_append())
        .expect("open replay ctl");
    let err = client
        .write(7, bad_payload.as_bytes())
        .expect_err("replay beyond window rejected");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::Invalid),
        other => panic!("unexpected error: {other:?}"),
    }
}
