// CLASSIFICATION: COMMUNITY
// Filename: payload_limit.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-25

use cohesix::p9::secure::secure_9p_server::{read_payload, MAX_PAYLOAD};
use std::io::Cursor;

#[test]
fn payload_limit_exceeded() {
    let data = vec![0u8; MAX_PAYLOAD + 1];
    let mut cursor = Cursor::new(data);
    let res = read_payload(&mut cursor, MAX_PAYLOAD);
    assert!(res.is_err());
}
