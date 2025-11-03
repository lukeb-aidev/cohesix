// Author: Lukas Bower

#![cfg(feature = "kernel")]

use root_task::bootstrap::cspace_sys::{
    cnode_copy_legacy, encode_slot, take_last_host_retype_trace, untyped_retype_legacy,
};
use sel4_sys::{self, seL4_CNode, seL4_CPtr, seL4_CapRights};

#[test]
fn encode_slot_applies_word_aligned_shift() {
    let init_bits = 13u8;
    let encoded = encode_slot(0x1, init_bits);
    let expected_shift = (sel4_sys::seL4_WordBits - u32::from(init_bits)) as usize;
    assert_eq!(encoded, 0x1 << expected_shift);
}

#[test]
fn cnode_copy_legacy_encodes_indices_and_succeeds() {
    let rights = seL4_CapRights::new(1, 1, 1, 1);
    let init_bits = 13u8;
    let dst_slot: seL4_CPtr = 0x20;
    let src_slot: seL4_CPtr = 0x10;
    let encoded_dst = encode_slot(dst_slot as sel4_sys::seL4_Word, init_bits);
    let encoded_src = encode_slot(src_slot as sel4_sys::seL4_Word, init_bits);
    assert_ne!(encoded_dst, dst_slot as sel4_sys::seL4_Word);
    assert_ne!(encoded_src, src_slot as sel4_sys::seL4_Word);
    let err = cnode_copy_legacy(init_bits, dst_slot, src_slot, rights);
    assert_eq!(err, sel4_sys::seL4_NoError);
}

#[test]
fn untyped_retype_legacy_uses_canonical_root_tuple() {
    let init_bits = 13u8;
    let dst_slot: seL4_CPtr = 0x40;
    let ut_cap: seL4_CNode = 0x200;
    let _ = take_last_host_retype_trace();
    let err = untyped_retype_legacy(
        ut_cap,
        sel4_sys::seL4_EndpointObject as _,
        0,
        init_bits,
        dst_slot,
    );
    assert_eq!(err, sel4_sys::seL4_NoError);
    let trace = take_last_host_retype_trace().expect("host trace must be captured");
    assert_eq!(trace.root, sel4_sys::seL4_CapInitThreadCNode);
    assert_eq!(trace.node_index, 0);
    assert_eq!(trace.node_depth, 0);
    assert_eq!(trace.node_offset, dst_slot as sel4_sys::seL4_Word);
    assert_eq!(
        trace.object_type,
        sel4_sys::seL4_EndpointObject as sel4_sys::seL4_Word
    );
    assert_eq!(trace.size_bits, 0);
}
