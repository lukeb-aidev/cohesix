// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate UI provider bounds, cursor resume, and audit denials.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use cohesix_ticket::Role;
use nine_door::{
    AuditConfig, HostNamespaceConfig, InProcessConnection, NineDoor, NineDoorError, PolicyConfig,
    PolicyLimits, PolicyRuleSpec, UiProviderConfig,
};
use secure9p_codec::{ErrorCode, OpenMode, MAX_MSIZE};

const ROOT_FID: u32 = 1;

fn attach_queen(server: &NineDoor) -> InProcessConnection {
    let mut client = server.connect().expect("create session");
    client.version(MAX_MSIZE).expect("version handshake");
    client.attach(ROOT_FID, Role::Queen).expect("attach queen");
    client
}

fn read_all(
    client: &mut InProcessConnection,
    fid: u32,
    path: &[String],
) -> Result<Vec<u8>, NineDoorError> {
    client.walk(ROOT_FID, fid, path)?;
    client.open(fid, OpenMode::read_only())?;
    let mut offset = 0u64;
    let mut data = Vec::new();
    loop {
        let chunk = client.read(fid, offset, MAX_MSIZE)?;
        if chunk.is_empty() {
            break;
        }
        offset = offset.saturating_add(chunk.len() as u64);
        data.extend_from_slice(&chunk);
        if chunk.len() < MAX_MSIZE as usize {
            break;
        }
    }
    client.clunk(fid)?;
    Ok(data)
}

fn assert_log_contains(client: &mut InProcessConnection, needle: &str) {
    let path = vec!["log".to_owned(), "queen.log".to_owned()];
    let log = read_all(client, 10, &path).expect("read queen log");
    let text = String::from_utf8(log).expect("log utf8");
    assert!(
        text.contains(needle),
        "expected log to contain '{needle}', got: {text}"
    );
}

#[test]
fn ui_provider_cursor_resume_and_eof() {
    let mut rules = Vec::new();
    for idx in 0..120usize {
        let id = format!("ui-rule-{idx:03}");
        let target = format!("/target/{idx:03}/{}", "a".repeat(24));
        rules.push(PolicyRuleSpec { id, target });
    }
    let policy = PolicyConfig::enabled(rules, PolicyLimits::default());
    let server = NineDoor::new_with_host_policy_audit_ui_config(
        HostNamespaceConfig::disabled(),
        policy,
        AuditConfig::disabled(),
        UiProviderConfig::default(),
    );
    let mut client = attach_queen(&server);

    let path = vec![
        "policy".to_owned(),
        "preflight".to_owned(),
        "diff".to_owned(),
    ];
    client.walk(ROOT_FID, 2, &path).expect("walk diff");
    client.open(2, OpenMode::read_only()).expect("open diff");

    let mut offset = 0u64;
    let mut total = 0usize;
    loop {
        let chunk = client.read(2, offset, MAX_MSIZE).expect("read diff chunk");
        if chunk.is_empty() {
            break;
        }
        if total == 0 {
            assert_eq!(chunk.len(), MAX_MSIZE as usize);
        }
        offset = offset.saturating_add(chunk.len() as u64);
        total = total.saturating_add(chunk.len());
        if chunk.len() < MAX_MSIZE as usize {
            break;
        }
    }
    let eof = client.read(2, offset, MAX_MSIZE).expect("read diff eof");
    assert!(eof.is_empty());
    assert!(total > MAX_MSIZE as usize);
    assert!(total <= 32 * 1024);
    client.clunk(2).expect("clunk diff");
}

#[test]
fn ui_provider_oversize_read_logs_audit() {
    let policy = PolicyConfig::enabled(Vec::new(), PolicyLimits::default());
    let server = NineDoor::new_with_host_policy_audit_ui_config(
        HostNamespaceConfig::disabled(),
        policy,
        AuditConfig::disabled(),
        UiProviderConfig::default(),
    );
    let mut client = attach_queen(&server);

    let path = vec![
        "policy".to_owned(),
        "preflight".to_owned(),
        "diff".to_owned(),
    ];
    client.walk(ROOT_FID, 2, &path).expect("walk diff");
    client.open(2, OpenMode::read_only()).expect("open diff");
    let err = client
        .read(2, 0, MAX_MSIZE + 1)
        .expect_err("oversize read rejected");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::TooBig),
        other => panic!("unexpected error: {other:?}"),
    }
    client.clunk(2).expect("clunk diff");
    assert_log_contains(
        &mut client,
        "ui-provider outcome=deny reason=oversize-read provider=policy/preflight/diff",
    );
}

#[test]
fn ui_provider_disabled_logs_audit() {
    let policy = PolicyConfig::enabled(Vec::new(), PolicyLimits::default());
    let server = NineDoor::new_with_host_policy_audit_ui_config(
        HostNamespaceConfig::disabled(),
        policy,
        AuditConfig::disabled(),
        UiProviderConfig::disabled(),
    );
    let mut client = attach_queen(&server);

    let path = vec![
        "policy".to_owned(),
        "preflight".to_owned(),
        "req".to_owned(),
    ];
    let err = client.walk(ROOT_FID, 2, &path).expect_err("walk disabled");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::NotFound),
        other => panic!("unexpected error: {other:?}"),
    }
    assert_log_contains(
        &mut client,
        "ui-provider outcome=deny reason=disabled provider=policy/preflight/req",
    );
}
