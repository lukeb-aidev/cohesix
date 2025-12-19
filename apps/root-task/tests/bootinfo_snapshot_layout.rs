// Author: Lukas Bower

use root_task::bootinfo_layout::{post_canary_offset, POST_CANARY_BYTES};

fn align_up(value: usize, align: usize) -> usize {
    (value + (align - 1)) & !(align - 1)
}

#[test]
fn post_canary_respects_unpadded_snapshot_length() {
    let payload_len = 0x1800usize;
    let base_addr = 0x3000_0000usize;
    let full_len = payload_len + POST_CANARY_BYTES;

    let post_addr = base_addr + post_canary_offset(payload_len);
    assert_eq!(post_addr, base_addr + full_len - POST_CANARY_BYTES);

    let padded_len = align_up(full_len, 0x1000);
    assert_ne!(
        post_addr,
        base_addr + padded_len - POST_CANARY_BYTES,
        "post-canary must stay outside padding spans",
    );
}
