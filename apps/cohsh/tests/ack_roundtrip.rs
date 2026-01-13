// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines tests for cohsh ack_roundtrip.
// Author: Lukas Bower

use cohsh::proto::{parse_ack, AckStatus};

#[test]
fn parser_consumes_root_task_ack_shapes() {
    let canonical_lines = [
        "OK ATTACH role=queen",
        "ERR ATTACH reason=unauthenticated",
        "ERR AUTH reason=expected-token",
        "OK TAIL path=/log/queen.log",
    ];

    for original in canonical_lines {
        let parsed = parse_ack(original).expect("root-task ACK should parse");
        let status_label = match parsed.status {
            AckStatus::Ok => "OK",
            AckStatus::Err => "ERR",
        };

        let mut reconstructed = String::new();
        reconstructed.push_str(status_label);
        reconstructed.push(' ');
        reconstructed.push_str(parsed.verb);
        if let Some(detail) = parsed.detail {
            reconstructed.push(' ');
            reconstructed.push_str(detail);
        }

        assert_eq!(reconstructed, original);
    }
}
