// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate PolicyFS nodes and approval gating in NineDoor.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use nine_door::{
    HostNamespaceConfig, HostProvider, NineDoor, NineDoorError, PolicyConfig, PolicyLimits,
    PolicyRuleSpec,
};
use secure9p_codec::{ErrorCode, OpenMode, MAX_MSIZE};

fn read_file(client: &mut nine_door::InProcessConnection, fid: u32, path: &[String]) -> String {
    client.walk(1, fid, path).expect("walk");
    client.open(fid, OpenMode::read_only()).expect("open");
    let data = client.read(fid, 0, MAX_MSIZE).expect("read");
    client.clunk(fid).expect("clunk");
    String::from_utf8(data).expect("utf8")
}

#[test]
fn policyfs_gate_requires_approval_and_consumes() {
    let host_config =
        HostNamespaceConfig::enabled("/host", &[HostProvider::Systemd, HostProvider::K8s])
            .expect("host config");
    let rules = vec![
        PolicyRuleSpec {
            id: "queen-ctl".to_owned(),
            target: "/queen/ctl".to_owned(),
        },
        PolicyRuleSpec {
            id: "systemd-restart".to_owned(),
            target: "/host/systemd/*/restart".to_owned(),
        },
    ];
    let limits = PolicyLimits {
        queue_max_entries: 8,
        queue_max_bytes: 512,
        ctl_max_bytes: 256,
        status_max_bytes: 256,
    };
    let policy = PolicyConfig::enabled(rules, limits);
    let server = NineDoor::new_with_host_and_policy_config(host_config, policy);

    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client
        .attach(1, cohesix_ticket::Role::Queen)
        .expect("attach");

    let rules_path = vec!["policy".to_owned(), "rules".to_owned()];
    let rules_text = read_file(&mut client, 2, &rules_path);
    assert!(rules_text.contains("\"systemd-restart\""));

    let restart_path = vec![
        "host".to_owned(),
        "systemd".to_owned(),
        "cohesix-agent.service".to_owned(),
        "restart".to_owned(),
    ];
    client.walk(1, 3, &restart_path).expect("walk restart");
    client
        .open(3, OpenMode::write_append())
        .expect("open restart");
    let err = client
        .write(3, b"restart")
        .expect_err("restart should be gated");
    match err {
        NineDoorError::Protocol { code, message } => {
            assert_eq!(code, ErrorCode::Permission);
            assert!(message.contains("EPERM"));
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let queue_path = vec!["actions".to_owned(), "queue".to_owned()];
    client.walk(1, 4, &queue_path).expect("walk queue");
    client
        .open(4, OpenMode::write_append())
        .expect("open queue");
    let approval = br#"{"id":"approval-1","target":"/host/systemd/cohesix-agent.service/restart","decision":"approve"}"#;
    client.write(4, approval).expect("append approval");
    client.clunk(4).expect("clunk queue");

    let status_path = vec![
        "actions".to_owned(),
        "approval-1".to_owned(),
        "status".to_owned(),
    ];
    let status_text = read_file(&mut client, 5, &status_path);
    assert!(status_text.contains("\"queued\""));

    client.walk(1, 6, &restart_path).expect("walk restart");
    client
        .open(6, OpenMode::write_append())
        .expect("open restart again");
    client.write(6, b"restart").expect("restart after approval");
    client.clunk(6).expect("clunk restart");

    let status_text = read_file(&mut client, 7, &status_path);
    assert!(status_text.contains("\"consumed\""));

    client.walk(1, 8, &restart_path).expect("walk restart");
    client
        .open(8, OpenMode::write_append())
        .expect("open restart replay");
    let err = client
        .write(8, b"restart")
        .expect_err("replay should be denied");
    match err {
        NineDoorError::Protocol { code, message } => {
            assert_eq!(code, ErrorCode::Permission);
            assert!(message.contains("EPERM"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
