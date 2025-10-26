// Author: Lukas Bower

#![cfg(all(feature = "kernel", not(target_os = "none")))]

use core::mem;

use root_task::bootstrap::cspace_sys::{
    init_cnode_direct_destination_words_for_test, install_test_bootinfo_for_tests,
    take_last_host_retype_trace, untyped_retype_into_cnode, untyped_retype_into_init_root,
};
use sel4_sys::{seL4_CPtr, seL4_CapInitThreadCNode, seL4_NoError, seL4_Word};

fn install_bootinfo(bits: u8) {
    unsafe {
        let mut bootinfo: sel4_sys::seL4_BootInfo = mem::zeroed();
        bootinfo.initThreadCNodeSizeBits = bits as _;
        install_test_bootinfo_for_tests(bootinfo);
    }
}

#[test]
fn init_cnode_retype_uses_direct_destination_encoding() {
    let _ = take_last_host_retype_trace();
    let depth_bits: u8 = 12;
    install_bootinfo(depth_bits);
    let untyped: seL4_CPtr = 0x20;
    let obj_ty: seL4_Word = 4;
    let size_bits: seL4_Word = 0;
    let dst_slot: seL4_CPtr = 0x40;

    let (index, depth, offset) = init_cnode_direct_destination_words_for_test(depth_bits, dst_slot);
    assert_eq!(index, dst_slot as seL4_Word);
    assert_eq!(depth, depth_bits as seL4_Word);
    assert_eq!(offset, 0);

    let err = untyped_retype_into_init_root(untyped, obj_ty, size_bits, dst_slot);
    assert_eq!(err, seL4_NoError);

    let trace =
        take_last_host_retype_trace().expect("host trace must record init CNode retype parameters");

    assert_eq!(trace.root, seL4_CapInitThreadCNode);
    assert_eq!(trace.node_index, dst_slot as seL4_Word);
    assert_eq!(trace.node_depth, depth_bits as seL4_Word);
    assert_eq!(trace.node_offset, 0);
}

#[test]
fn generic_init_cnode_retype_uses_canonical_encoding() {
    let _ = take_last_host_retype_trace();
    let init_bits: u8 = 12;
    install_bootinfo(init_bits);
    let depth_bits: u8 = 6;
    let untyped: seL4_CPtr = 0x30;
    let obj_ty: seL4_Word = 2;
    let size_bits: seL4_Word = 1;
    let dst_slot: seL4_CPtr = 0x44;

    let err = untyped_retype_into_cnode(
        seL4_CapInitThreadCNode,
        depth_bits,
        untyped,
        obj_ty,
        size_bits,
        dst_slot,
    );
    assert_eq!(err, seL4_NoError);

    let trace = take_last_host_retype_trace()
        .expect("host trace must record generic init CNode retype parameters");

    assert_eq!(trace.root, seL4_CapInitThreadCNode);
    assert_eq!(trace.node_index, dst_slot as seL4_Word);
    assert_eq!(trace.node_depth, init_bits as seL4_Word);
    assert_eq!(trace.node_offset, 0);
}
