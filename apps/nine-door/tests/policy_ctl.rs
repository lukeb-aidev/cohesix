// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate policy apply/rollback control schema in NineDoor.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use cohesix_ticket::Role;
use nine_door::{HostNamespaceConfig, InProcessConnection, NineDoor, NineDoorError, PolicyConfig, PolicyLimits};
use secure9p_codec::{ErrorCode, OpenMode, MAX_MSIZE};

fn attach_queen(server: &NineDoor) -> InProcessConnection {
    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client.attach(1, Role::Queen).expect("attach");
    client
}

#[test]
fn policy_ctl_enforces_apply_and_rollback_schema() {
    let policy = PolicyConfig::enabled(Vec::new(), PolicyLimits::default());
    let server = NineDoor::new_with_host_and_policy_config(HostNamespaceConfig::disabled(), policy);
    let mut client = attach_queen(&server);

    let ctl_path = vec!["policy".to_owned(), "ctl".to_owned()];
    client.walk(1, 2, &ctl_path).expect("walk policy ctl");
    client
        .open(2, OpenMode::write_append())
        .expect("open policy ctl");

    let bad_sha = r#"{"op":"apply","id":"rev-1","sha256":"deadbeef"}"#;
    let err = client
        .write(2, bad_sha.as_bytes())
        .expect_err("invalid sha should fail");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::Invalid),
        other => panic!("unexpected error: {other:?}"),
    }

    let sha = "a".repeat(64);
    let apply = format!(r#"{{"op":"apply","id":"rev-1","sha256":"{sha}"}}"#);
    client
        .write(2, apply.as_bytes())
        .expect("apply policy revision");

    let rollback_wrong = r#"{"op":"rollback","id":"rev-other"}"#;
    let err = client
        .write(2, rollback_wrong.as_bytes())
        .expect_err("rollback id mismatch should fail");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::Invalid),
        other => panic!("unexpected error: {other:?}"),
    }

    let rollback = r#"{"op":"rollback","id":"rev-1"}"#;
    client
        .write(2, rollback.as_bytes())
        .expect("rollback policy revision");
}
