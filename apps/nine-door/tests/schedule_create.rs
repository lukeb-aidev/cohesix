// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate schedule control writes and /proc schedule observability.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use cohesix_ticket::Role;
use nine_door::{InProcessConnection, NineDoor};
use secure9p_codec::{OpenMode, MAX_MSIZE};

fn attach_queen(server: &NineDoor) -> InProcessConnection {
    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client.attach(1, Role::Queen).expect("attach");
    client
}

fn read_text(client: &mut InProcessConnection, fid: u32, path: &[String]) -> String {
    client.walk(1, fid, path).expect("walk");
    client.open(fid, OpenMode::read_only()).expect("open");
    let data = client.read(fid, 0, MAX_MSIZE).expect("read");
    client.clunk(fid).expect("clunk");
    String::from_utf8(data).expect("utf8")
}

#[test]
fn schedule_ctl_appends_and_updates_proc() {
    let server = NineDoor::new();
    let mut client = attach_queen(&server);

    let ctl_path = vec!["queen".to_owned(), "schedule".to_owned(), "ctl".to_owned()];
    client.walk(1, 2, &ctl_path).expect("walk schedule ctl");
    client
        .open(2, OpenMode::write_append())
        .expect("open schedule ctl");
    let payload =
        br#"{"id":"sched-1","role":"worker-heartbeat","priority":5,"ticks":10,"budget_ms":100}"#;
    let written = client.write(2, payload).expect("write schedule ctl");
    assert_eq!(written as usize, payload.len());
    client.clunk(2).expect("clunk schedule ctl");

    let summary_path = vec![
        "proc".to_owned(),
        "schedule".to_owned(),
        "summary".to_owned(),
    ];
    let summary = read_text(&mut client, 3, &summary_path);
    assert!(summary.contains("queue=1"));

    let queue_path = vec!["proc".to_owned(), "schedule".to_owned(), "queue".to_owned()];
    let queue = read_text(&mut client, 4, &queue_path);
    assert!(queue.contains("id=sched-1"));
}
