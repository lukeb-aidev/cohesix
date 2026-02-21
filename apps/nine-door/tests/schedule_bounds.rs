// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate schedule queue bounds in NineDoor.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use cohesix_ticket::Role;
use nine_door::{InProcessConnection, NineDoor, NineDoorError};
use secure9p_codec::{ErrorCode, OpenMode, MAX_MSIZE};

const QUEUE_MAX: usize = 64;

fn attach_queen(server: &NineDoor) -> InProcessConnection {
    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client.attach(1, Role::Queen).expect("attach");
    client
}

#[test]
fn schedule_queue_enforces_max_entries() {
    let server = NineDoor::new();
    let mut client = attach_queen(&server);

    let ctl_path = vec!["queen".to_owned(), "schedule".to_owned(), "ctl".to_owned()];
    client.walk(1, 2, &ctl_path).expect("walk schedule ctl");
    client
        .open(2, OpenMode::write_append())
        .expect("open schedule ctl");

    for idx in 0..QUEUE_MAX {
        let payload = format!(
            "{{\"id\":\"sched-{idx}\",\"role\":\"worker-heartbeat\",\"priority\":1,\"ticks\":1,\"budget_ms\":1}}"
        );
        client
            .write(2, payload.as_bytes())
            .expect("write schedule entry");
    }

    let overflow =
        r#"{"id":"sched-over","role":"worker-heartbeat","priority":1,"ticks":1,"budget_ms":1}"#;
    let err = client
        .write(2, overflow.as_bytes())
        .expect_err("queue should be full");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::TooBig),
        other => panic!("unexpected error: {other:?}"),
    }
}
