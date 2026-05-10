// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate lease bounds in NineDoor.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;

use cohesix_ticket::Role;
use nine_door::{InProcessConnection, NineDoor};
use secure9p_codec::{OpenMode, MAX_MSIZE};

fn attach_queen(server: &NineDoor) -> InProcessConnection {
    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client.attach(1, Role::Queen).expect("attach");
    client
}

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("nine-door has workspace parent")
        .parent()
        .expect("workspace root has parent")
        .join(path)
}

fn generated_limit(key: &str) -> usize {
    let snippet = fs::read_to_string(repo_path("docs/snippets/root_task_manifest.md"))
        .expect("read generated root-task manifest snippet");
    let needle = format!("- `{key}`: `");
    for line in snippet.lines() {
        if let Some(value) = line
            .strip_prefix(&needle)
            .and_then(|rest| rest.split('`').next())
        {
            return value.parse().expect("generated manifest limit is numeric");
        }
    }
    panic!("generated manifest snippet is missing {key}");
}

fn read_text(client: &mut InProcessConnection, fid: u32, path: &[String]) -> String {
    client.walk(1, fid, path).expect("walk proc path");
    client
        .open(fid, OpenMode::read_only())
        .expect("open proc path");
    let data = client.read(fid, 0, MAX_MSIZE).expect("read proc path");
    client.clunk(fid).expect("clunk proc fid");
    String::from_utf8(data).expect("proc output should be utf8")
}

#[test]
fn lease_summary_reports_generated_bounds() {
    let server = NineDoor::new();
    let mut client = attach_queen(&server);
    let active_max = generated_limit("control_plane.lease.active_max_entries");
    let preemptions_max = generated_limit("control_plane.lease.preemptions_max_entries");

    let ctl_path = vec!["queen".to_owned(), "lease".to_owned(), "ctl".to_owned()];
    client.walk(1, 2, &ctl_path).expect("walk lease ctl");
    client
        .open(2, OpenMode::write_append())
        .expect("open lease ctl");

    let grant = b"{\"op\":\"grant\",\"id\":\"l1\",\"subject\":\"s\",\"resource\":\"r\",\"ttl_s\":1,\"priority\":1}";
    client.write(2, grant).expect("grant lease");
    let preempt = b"{\"op\":\"preempt\",\"id\":\"l1\",\"reason\":\"x\"}";
    client.write(2, preempt).expect("preempt lease");

    let summary_path = vec!["proc".to_owned(), "lease".to_owned(), "summary".to_owned()];
    let summary = read_text(&mut client, 3, &summary_path);
    assert!(summary.contains(&format!("max_active={active_max}")));
    assert!(summary.contains(&format!("max_preemptions={preemptions_max}")));
}
