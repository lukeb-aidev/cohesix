// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate export control schema in NineDoor.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use cohesix_ticket::Role;
use nine_door::{InProcessConnection, NineDoor, NineDoorError};
use secure9p_codec::{ErrorCode, OpenMode, MAX_MSIZE};

fn attach_queen(server: &NineDoor) -> InProcessConnection {
    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client.attach(1, Role::Queen).expect("attach");
    client
}

#[test]
fn export_ctl_accepts_open_and_close() {
    let server = NineDoor::new();
    let mut client = attach_queen(&server);

    let ctl_path = vec!["queen".to_owned(), "export".to_owned(), "ctl".to_owned()];
    client.walk(1, 2, &ctl_path).expect("walk export ctl");
    client
        .open(2, OpenMode::write_append())
        .expect("open export ctl");

    let open = r#"{"op":"open","id":"window-1","ttl_s":60}"#;
    client.write(2, open.as_bytes()).expect("open window");

    let close = r#"{"op":"close","id":"window-1","reason":"done"}"#;
    client.write(2, close.as_bytes()).expect("close window");

    let close_missing = r#"{"op":"close","id":"missing","reason":"done"}"#;
    let err = client
        .write(2, close_missing.as_bytes())
        .expect_err("missing window should fail");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::Invalid),
        other => panic!("unexpected error: {other:?}"),
    }
}
