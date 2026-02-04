// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate lease bounds in NineDoor.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use cohesix_ticket::Role;
use nine_door::{InProcessConnection, NineDoor, NineDoorError};
use secure9p_codec::{ErrorCode, OpenMode, MAX_MSIZE};

const ACTIVE_MAX: usize = 64;
const PREEMPTIONS_MAX: usize = 64;

fn attach_queen(server: &NineDoor) -> InProcessConnection {
    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client.attach(1, Role::Queen).expect("attach");
    client
}

#[test]
fn lease_active_and_preemptions_enforce_bounds() {
    let server = NineDoor::new();
    let mut client = attach_queen(&server);

    let ctl_path = vec!["queen".to_owned(), "lease".to_owned(), "ctl".to_owned()];
    client.walk(1, 2, &ctl_path).expect("walk lease ctl");
    client
        .open(2, OpenMode::write_append())
        .expect("open lease ctl");

    for idx in 0..ACTIVE_MAX {
        let payload = format!(
            "{{\"op\":\"grant\",\"id\":\"l{idx}\",\"subject\":\"s\",\"resource\":\"r\",\"ttl_s\":1,\"priority\":1}}"
        );
        client.write(2, payload.as_bytes()).expect("grant lease");
    }

    let overflow = r#"{"op":"grant","id":"lover","subject":"s","resource":"r","ttl_s":1,"priority":1}"#;
    let err = client
        .write(2, overflow.as_bytes())
        .expect_err("active list should be full");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::TooBig),
        other => panic!("unexpected error: {other:?}"),
    }

    for idx in 0..PREEMPTIONS_MAX {
        let payload = format!("{{\"op\":\"preempt\",\"id\":\"l{idx}\",\"reason\":\"x\"}}");
        client
            .write(2, payload.as_bytes())
            .expect("preempt lease");
    }

    let grant_again = r#"{"op":"grant","id":"lextra","subject":"s","resource":"r","ttl_s":1,"priority":1}"#;
    client
        .write(2, grant_again.as_bytes())
        .expect("grant after preemptions");

    let preempt_over = r#"{"op":"preempt","id":"lextra","reason":"x"}"#;
    let err = client
        .write(2, preempt_over.as_bytes())
        .expect_err("preemptions list should be full");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::TooBig),
        other => panic!("unexpected error: {other:?}"),
    }
}
