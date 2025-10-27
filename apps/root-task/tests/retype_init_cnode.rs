// Author: Lukas Bower

#![cfg(all(feature = "kernel", not(target_os = "none")))]

use core::mem;

use root_task::bootstrap::cspace_sys::{
    install_test_bootinfo_for_tests, take_last_host_retype_trace, untyped_retype_into_cnode,
    untyped_retype_into_init_root,
};
use sel4_sys::{seL4_CPtr, seL4_CapInitThreadCNode, seL4_NoError, seL4_Word};

fn install_bootinfo(bits: u8, empty_start: seL4_CPtr, empty_end: seL4_CPtr) {
    unsafe {
        let mut bootinfo: sel4_sys::seL4_BootInfo = mem::zeroed();
        bootinfo.initThreadCNodeSizeBits = bits as _;
        bootinfo.empty.start = empty_start;
        bootinfo.empty.end = empty_end;
        install_test_bootinfo_for_tests(bootinfo);
    }
}

#[test]
fn init_cnode_retype_uses_direct_destination_encoding() {
    let _ = take_last_host_retype_trace();
    let depth_bits: u8 = 12;
    let start = 0x20;
    let end = 0x200;
    install_bootinfo(depth_bits, start, end);
    let untyped: seL4_CPtr = 0x20;
    let obj_ty: seL4_Word = 4;
    let size_bits: seL4_Word = 0;
    let dst_slot: seL4_CPtr = 0x40;

    let err = untyped_retype_into_init_root(untyped, obj_ty, size_bits, dst_slot);
    assert!(err.is_ok());

    let trace =
        take_last_host_retype_trace().expect("host trace must record init CNode retype parameters");

    assert_eq!(trace.root, seL4_CapInitThreadCNode);
    assert_eq!(trace.node_index, 0);
    assert_eq!(trace.node_depth, sel4_sys::seL4_WordBits as seL4_Word);
    assert_eq!(trace.node_offset, dst_slot as seL4_Word);
    assert_eq!(trace.object_type, obj_ty);
    assert_eq!(trace.size_bits, size_bits);
}

#[test]
fn generic_init_cnode_retype_uses_canonical_encoding() {
    let _ = take_last_host_retype_trace();
    let init_bits: u8 = 12;
    let start = 0x30;
    let end = 0x220;
    install_bootinfo(init_bits, start, end);
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
    assert_eq!(trace.node_index, 0);
    assert_eq!(trace.node_depth, sel4_sys::seL4_WordBits as seL4_Word);
    assert_eq!(trace.node_offset, dst_slot as seL4_Word);
    assert_eq!(trace.object_type, obj_ty);
    assert_eq!(trace.size_bits, size_bits);
}
