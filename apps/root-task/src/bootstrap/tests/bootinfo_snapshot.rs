// Author: Lukas Bower

use root_task::bootstrap::bootinfo_snapshot::{post_canary_offset, POST_CANARY_BYTES};

fn align_up(value: usize, align: usize) -> usize {
    (value + (align - 1)) & !(align - 1)
}

#[test]
fn post_canary_sits_after_snapshot_payload() {
    let payload_len = 0x1800usize;
    let base_addr = 0x2000_0000usize;
    let full_len = payload_len + POST_CANARY_BYTES;

    let post_addr = base_addr + post_canary_offset(payload_len);
    assert_eq!(post_addr, base_addr + full_len - POST_CANARY_BYTES);

    let padded_len = align_up(full_len, 0x1000);
    assert_ne!(
        post_addr,
        base_addr + padded_len - POST_CANARY_BYTES,
        "post-canary must not move into padding",
    );
}
