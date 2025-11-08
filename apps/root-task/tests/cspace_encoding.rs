// Author: Lukas Bower

#![cfg(feature = "kernel")]

use root_task::bootstrap::cspace_sys::{
    cnode_copy, encode_slot, take_last_host_retype_trace, untyped_retype_encoded,
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
fn cnode_copy_uses_selected_style() {
    let rights = seL4_CapRights::new(1, 1, 1, 1);
    let init_bits = 13u8;
    let dst_slot: seL4_CPtr = 0x20;
    let src_slot: seL4_CPtr = 0x10;
    let mut bootinfo: sel4_sys::seL4_BootInfo = unsafe { core::mem::zeroed() };
    bootinfo.initThreadCNodeSizeBits = init_bits as _;
    bootinfo.empty.start = 0x10;
    bootinfo.empty.end = 0x80;

    let root = sel4_sys::seL4_CapInitThreadCNode;
    let err = cnode_copy(&bootinfo, root, dst_slot as _, root, src_slot as _, rights);
    assert_eq!(err, sel4_sys::seL4_NoError);
}

#[test]
fn untyped_retype_encoded_uses_canonical_root_tuple() {
    let init_bits = 13u8;
    let dst_slot: seL4_CPtr = 0x40;
    let ut_cap: seL4_CNode = 0x200;
    let _ = take_last_host_retype_trace();
    let err = untyped_retype_encoded(
        ut_cap,
        sel4_sys::seL4_EndpointObject as u32,
        0,
        sel4_sys::seL4_CapInitThreadCNode,
        dst_slot as u64,
        init_bits,
        1,
    );
    assert_eq!(err, sel4_sys::seL4_NoError);
    let trace = take_last_host_retype_trace().expect("host trace must be captured");
    assert_eq!(trace.root, sel4_sys::seL4_CapInitThreadCNode);
    assert_eq!(trace.node_index, 0);
    assert_eq!(trace.node_depth, sel4_sys::seL4_WordBits as sel4_sys::seL4_Word);
    assert_eq!(trace.node_offset, dst_slot as sel4_sys::seL4_Word);
    assert_eq!(
        trace.object_type,
        sel4_sys::seL4_EndpointObject as sel4_sys::seL4_Word
    );
    assert_eq!(trace.size_bits, 0);
}
