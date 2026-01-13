// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate AuditFS journaling, decisions logging, and bounds.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use cohesix_ticket::Role;
use nine_door::{
    AuditConfig, AuditLimits, HostNamespaceConfig, NineDoor, NineDoorError, PolicyConfig,
    PolicyLimits, PolicyRuleSpec, ReplayConfig,
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
        tag: 200,
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

fn read_text(
    client: &mut nine_door::InProcessConnection,
    fid: u32,
    path: &[String],
) -> String {
    client.walk(1, fid, path).expect("walk");
    client.open(fid, OpenMode::read_only()).expect("open");
    let data = client.read(fid, 0, MAX_MSIZE).expect("read");
    client.clunk(fid).expect("clunk");
    String::from_utf8(data).expect("utf8")
}

#[test]
fn auditfs_records_policy_actions_and_denies_writes() {
    let policy = PolicyConfig::enabled(
        vec![PolicyRuleSpec {
            id: "queen-ctl".to_owned(),
            target: "/queen/ctl".to_owned(),
        }],
        PolicyLimits {
            queue_max_entries: 8,
            queue_max_bytes: 512,
            ctl_max_bytes: 256,
            status_max_bytes: 128,
        },
    );
    let audit = AuditConfig::enabled(
        AuditLimits {
            journal_max_bytes: 512,
            decisions_max_bytes: 256,
        },
        ReplayConfig::disabled(),
    );
    let server = NineDoor::new_with_host_policy_audit_config(
        HostNamespaceConfig::disabled(),
        policy,
        audit,
    );

    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client.attach(1, Role::Queen).expect("attach");

    let queue_path = vec!["actions".to_owned(), "queue".to_owned()];
    client.walk(1, 2, &queue_path).expect("walk queue");
    client
        .open(2, OpenMode::write_append())
        .expect("open queue");
    let approval = br#"{"id":"approval-1","target":"/queen/ctl","decision":"approve"}"#;
    client.write(2, approval).expect("append approval");
    client.clunk(2).expect("clunk queue");

    let decisions_path = vec!["audit".to_owned(), "decisions".to_owned()];
    let decisions_text = read_text(&mut client, 3, &decisions_path);
    assert!(decisions_text.contains("\"policy-action\""));
    assert!(decisions_text.contains("approval-1"));

    client.walk(1, 4, &decisions_path).expect("walk decisions");
    client
        .open(4, OpenMode::write_append())
        .expect("open decisions");
    let err = client
        .write(4, br#"{"noop":true}"#)
        .expect_err("decisions write denied");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::Permission),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn auditfs_enforces_append_only_and_truncates() {
    let audit = AuditConfig::enabled(
        AuditLimits {
            journal_max_bytes: 128,
            decisions_max_bytes: 64,
        },
        ReplayConfig::disabled(),
    );
    let server = NineDoor::new_with_host_policy_audit_config(
        HostNamespaceConfig::disabled(),
        PolicyConfig::disabled(),
        audit,
    );

    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client.attach(1, Role::Queen).expect("attach");

    let journal_path = vec!["audit".to_owned(), "journal".to_owned()];
    client.walk(1, 2, &journal_path).expect("walk journal");
    client
        .open(2, OpenMode::write_append())
        .expect("open journal");

    let payload_one = format!("{{\"event\":\"{}\"}}\n", "a".repeat(60));
    assert!(payload_one.len() < 128);
    client
        .write(2, payload_one.as_bytes())
        .expect("append one");

    let err = write_with_offset(&mut client, 2, 0, payload_one.as_bytes())
        .expect_err("random write rejected");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::Invalid),
        other => panic!("unexpected error: {other:?}"),
    }

    let payload_two = format!("{{\"event\":\"{}\"}}\n", "b".repeat(60));
    client
        .write(2, payload_two.as_bytes())
        .expect("append two");
    client.clunk(2).expect("clunk journal");

    let export_path = vec!["audit".to_owned(), "export".to_owned()];
    let export_text = read_text(&mut client, 3, &export_path);
    let export: serde_json::Value =
        serde_json::from_str(export_text.trim()).expect("export json");
    let base = export["journal_base"].as_u64().expect("journal_base");
    let next = export["journal_next"].as_u64().expect("journal_next");
    assert!(next > 0);
    assert!(base > 0, "journal_base should advance after truncation");
}
