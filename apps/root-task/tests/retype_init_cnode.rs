// Author: Lukas Bower

#![cfg(all(feature = "kernel", not(target_os = "none")))]

use root_task::bootstrap::cspace_sys::{
    init_cnode_direct_destination_words_for_test, take_last_host_retype_trace,
    untyped_retype_into_init_cnode,
};
use sel4_sys::{seL4_CPtr, seL4_CapInitThreadCNode, seL4_NoError, seL4_Word};

#[test]
fn init_cnode_retype_uses_direct_destination_encoding() {
    let _ = take_last_host_retype_trace();
    let depth_bits: u8 = 12;
    let untyped: seL4_CPtr = 0x20;
    let obj_ty: seL4_Word = 4;
    let size_bits: seL4_Word = 0;
    let dst_slot: seL4_CPtr = 0x40;

    let (_index, depth, _offset) =
        init_cnode_direct_destination_words_for_test(depth_bits, dst_slot);
    assert_eq!(depth, depth_bits as seL4_Word);

    let err = untyped_retype_into_init_cnode(depth_bits, untyped, obj_ty, size_bits, dst_slot);
    assert_eq!(err, seL4_NoError);

    let trace =
        take_last_host_retype_trace().expect("host trace must record init CNode retype parameters");

    assert_eq!(trace.root, seL4_CapInitThreadCNode);
    assert_eq!(trace.node_index, 0);
    assert_eq!(trace.node_depth, depth_bits as seL4_Word);
    assert_eq!(trace.node_offset, dst_slot as seL4_Word);
}
