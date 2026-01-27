// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Ensure tail output for /proc/ingest/watch is line-oriented and stable.
// Author: Lukas Bower

use cohesix_ticket::Role;
use cohsh::{NineDoorTransport, Shell};
use nine_door::NineDoor;

#[test]
fn tail_watch_output_is_line_oriented() {
    let server = NineDoor::new();
    let transport = NineDoorTransport::new(server);
    let mut output = Vec::new();
    let mut shell = Shell::new(transport, &mut output);

    shell.attach(Role::Queen, None).expect("attach queen");
    shell
        .execute("tail /proc/ingest/watch")
        .expect("tail watch");

    let rendered = String::from_utf8(output).expect("output must be utf8");
    assert!(
        rendered
            .lines()
            .any(|line| line.starts_with("[console] OK TAIL path=/proc/ingest/watch")),
        "missing TAIL ack: {rendered}"
    );

    let watch_line = rendered
        .lines()
        .find(|line| line.starts_with("watch "))
        .unwrap_or_else(|| panic!("missing watch output line: {rendered}"));
    let mut parts = watch_line.split_whitespace();
    assert_eq!(parts.next(), Some("watch"));

    let expected_keys = [
        "ts_ms",
        "p50_ms",
        "p95_ms",
        "queued",
        "backpressure",
        "dropped",
        "ui_reads",
        "ui_denies",
    ];
    for key in expected_keys {
        let part = parts
            .next()
            .unwrap_or_else(|| panic!("missing {key} field in watch line"));
        let (field, value) = part
            .split_once('=')
            .unwrap_or_else(|| panic!("missing '=' in field {part}"));
        assert_eq!(field, key);
        assert!(value.parse::<u64>().is_ok(), "field {field} not numeric");
    }
    assert!(parts.next().is_none(), "extra fields in watch line");
}
