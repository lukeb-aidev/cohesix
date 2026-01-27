// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate explicit /proc/9p/session lifecycle state reporting.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::Instant;

use cohesix_ticket::Role;
use nine_door::{Clock, InProcessConnection, NineDoor};
use secure9p_codec::{OpenMode, MAX_MSIZE};
use secure9p_core::{SessionLimits, ShortWritePolicy};

struct FixedClock {
    now: Instant,
}

impl FixedClock {
    fn new() -> Self {
        Self {
            now: Instant::now(),
        }
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Instant {
        self.now
    }
}

fn attach_queen(server: &NineDoor) -> InProcessConnection {
    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client.attach(1, Role::Queen).expect("attach");
    client
}

fn read_proc_text(client: &mut InProcessConnection, fid: u32, path: &[String]) -> String {
    client.walk(1, fid, path).expect("walk");
    client.open(fid, OpenMode::read_only()).expect("open");
    let data = client.read(fid, 0, MAX_MSIZE).expect("read");
    client.clunk(fid).expect("clunk");
    String::from_utf8(data).expect("utf8")
}

fn write_line(client: &mut InProcessConnection, fid: u32, path: &[String], payload: &str) {
    client.walk(1, fid, path).expect("walk");
    client.open(fid, OpenMode::write_append()).expect("open");
    client.write(fid, payload.as_bytes()).expect("write");
    client.clunk(fid).expect("clunk");
}

#[test]
fn session_state_tracks_setup_active_draining_closed() {
    let limits = SessionLimits {
        tags_per_session: 8,
        batch_frames: 4,
        short_write_policy: ShortWritePolicy::Reject,
    };
    let server = NineDoor::new_with_limits(Arc::new(FixedClock::new()), limits);

    let mut setup_session = server.connect().expect("setup session");
    let setup_id = setup_session.session_id();

    let mut observer = attach_queen(&server);
    let setup_state_path = vec![
        "proc".to_owned(),
        "9p".to_owned(),
        "session".to_owned(),
        setup_id.session().to_string(),
        "state".to_owned(),
    ];
    let setup_state = read_proc_text(&mut observer, 2, &setup_state_path);
    assert!(setup_state.contains("state=SETUP"));

    setup_session.version(MAX_MSIZE).expect("version");
    setup_session.attach(1, Role::Queen).expect("attach");
    let active_state = read_proc_text(&mut observer, 3, &setup_state_path);
    assert!(active_state.contains("state=ACTIVE"));

    let lifecycle_ctl = vec!["queen".to_owned(), "lifecycle".to_owned(), "ctl".to_owned()];
    write_line(&mut observer, 4, &lifecycle_ctl, "cordon\n");
    let draining_state = read_proc_text(&mut observer, 5, &setup_state_path);
    assert!(draining_state.contains("state=DRAINING"));

    drop(setup_session);
    let closed_state = read_proc_text(&mut observer, 6, &setup_state_path);
    assert!(closed_state.contains("state=CLOSED"));
}
