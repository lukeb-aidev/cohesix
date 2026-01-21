// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate trace encoding/decoding and tamper rejection.
// Author: Lukas Bower

use cohsh_core::trace::{TraceError, TraceFrame, TraceLog, TracePolicy};

#[test]
fn trace_roundtrip() {
    let policy = TracePolicy::new(2048, 512, 128);
    let log = TraceLog {
        frames: vec![TraceFrame {
            request: vec![1, 2, 3, 4],
            response: vec![9, 8, 7],
        }],
        ack_lines: vec!["OK PING".to_owned()],
    };
    let encoded = log.encode(policy).expect("encode trace");
    let decoded = TraceLog::decode(&encoded, policy).expect("decode trace");
    assert_eq!(decoded, log);
}

#[test]
fn trace_tamper_is_rejected() {
    let policy = TracePolicy::new(2048, 512, 128);
    let log = TraceLog {
        frames: vec![TraceFrame {
            request: vec![1, 2, 3, 4],
            response: vec![9, 8, 7],
        }],
        ack_lines: vec!["OK PING".to_owned()],
    };
    let mut encoded = log.encode(policy).expect("encode trace");
    let last = encoded.len().saturating_sub(1);
    encoded[last] ^= 0xff;
    let err = TraceLog::decode(&encoded, policy).expect_err("tampered trace should fail");
    assert_eq!(err, TraceError::HashMismatch);
}
