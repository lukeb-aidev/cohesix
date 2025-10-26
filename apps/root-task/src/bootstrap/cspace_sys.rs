// Author: Lukas Bower
#![allow(unsafe_code)]

#[cfg(all(test, not(target_os = "none")))]
extern crate alloc;

use core::convert::TryFrom;

use crate::boot;
use crate::sel4 as sys;
use sel4_sys;

#[cfg(all(test, not(target_os = "none")))]
use alloc::boxed::Box;

pub const CANONICAL_CNODE_DEPTH_BITS: u8 = sel4_sys::seL4_WordBits as u8;

#[cfg(target_os = "none")]
#[inline(always)]
fn bootinfo() -> &'static sel4_sys::seL4_BootInfo {
    unsafe { &*sel4_sys::seL4_GetBootInfo() }
}

#[inline(always)]
pub fn bootinfo_init_cnode_cptr() -> sys::seL4_CPtr {
    sel4_sys::seL4_CapInitThreadCNode
}

#[cfg(all(test, not(target_os = "none")))]
static mut TEST_BOOTINFO_PTR: *const sel4_sys::seL4_BootInfo = core::ptr::null();

#[cfg(all(test, not(target_os = "none")))]
#[inline(always)]
fn bootinfo() -> &'static sel4_sys::seL4_BootInfo {
    unsafe {
        TEST_BOOTINFO_PTR
            .as_ref()
            .expect("test bootinfo not installed")
    }
}

#[cfg(all(test, not(target_os = "none")))]
#[inline(always)]
pub(super) unsafe fn install_test_bootinfo_for_tests(
    bootinfo: sel4_sys::seL4_BootInfo,
) -> &'static sel4_sys::seL4_BootInfo {
    let leaked = Box::leak(Box::new(bootinfo));
    TEST_BOOTINFO_PTR = leaked as *const _;
    leaked
}

#[cfg(all(not(target_os = "none"), not(test)))]
#[inline(always)]
fn bootinfo() -> &'static sel4_sys::seL4_BootInfo {
    panic!("bootinfo() unavailable on host targets");
}

#[inline(always)]
pub fn check_slot_in_range(init_cnode_bits: u8, slot: sys::seL4_CPtr) {
    let limit = if init_cnode_bits as usize >= usize::BITS as usize {
        usize::MAX
    } else {
        1usize << init_cnode_bits
    };
    assert!(
        (slot as usize) < limit,
        "slot {} out of range for init_cnode_bits={} (limit={})",
        slot,
        init_cnode_bits,
        limit
    );
}

#[inline(always)]
pub fn encode_cnode_depth(bits: u8) -> sys::seL4_Word {
    bits as sys::seL4_Word
}

#[inline(always)]
pub fn init_cnode_dest(
    slot: sys::seL4_CPtr,
) -> (
    sel4_sys::seL4_CNode,
    sys::seL4_Word,
    sys::seL4_Word,
    sys::seL4_Word,
) {
    let bi = bootinfo();
    let init_bits = bi.initThreadCNodeSizeBits as sys::seL4_Word;
    let capacity = if init_bits as usize >= usize::BITS as usize {
        usize::MAX
    } else {
        1usize << (init_bits as usize)
    };
    debug_assert!(
        (slot as usize) < capacity,
        "slot 0x{slot:04x} exceeds init CNode capacity (limit=0x{capacity:04x})",
    );
    (
        bootinfo_init_cnode_cptr(),
        slot as sys::seL4_Word,
        init_bits,
        0,
    )
}

#[inline(always)]
pub fn retype_dest_init_cnode(
    slot: sys::seL4_CPtr,
) -> (
    sel4_sys::seL4_CNode,
    sys::seL4_Word,
    sys::seL4_Word,
    sys::seL4_Word,
) {
    let bi = bootinfo();
    let init_bits = bi.initThreadCNodeSizeBits as sys::seL4_Word;
    let capacity = if init_bits as usize >= usize::BITS as usize {
        usize::MAX
    } else {
        1usize << (init_bits as usize)
    };
    debug_assert!(
        (slot as usize) < capacity,
        "slot 0x{slot:04x} exceeds init CNode capacity (limit=0x{capacity:04x})",
    );

    (bootinfo_init_cnode_cptr(), 0, 0, slot as sys::seL4_Word)
}

#[cfg(target_os = "none")]
#[inline(always)]
fn log_destination(op: &str, idx: sys::seL4_Word, depth: sys::seL4_Word, offset: sys::seL4_Word) {
    if boot::flags::trace_dest() {
        log::info!(
            "DEST → {op} root=0x{root:04x} idx=0x{idx:04x} depth={depth} off={offset} (ABI order: dest_root,dest_index,dest_depth,dest_offset)",
            op = op,
            root = bootinfo_init_cnode_cptr(),
            idx = idx,
            depth = depth,
            offset = offset,
        );
    }
}

#[cfg(target_os = "none")]
#[inline(always)]
fn log_syscall_result(op: &str, err: sys::seL4_Error) {
    if boot::flags::trace_dest() {
        log::info!(
            "DEST ← {op} result={err} ({name})",
            op = op,
            err = err,
            name = crate::sel4::error_name(err),
        );
    }
    if err != sys::seL4_NoError {
        log::error!(
            "{op} failed: err={err} ({name})",
            op = op,
            err = err,
            name = crate::sel4::error_name(err),
        );
        panic!(
            "{op} failed with seL4 error {err} ({name})",
            op = op,
            err = err,
            name = crate::sel4::error_name(err),
        );
    }
}

#[cfg(not(target_os = "none"))]
#[inline(always)]
fn log_destination(
    _op: &str,
    _idx: sys::seL4_Word,
    _depth: sys::seL4_Word,
    _offset: sys::seL4_Word,
) {
}

#[cfg(not(target_os = "none"))]
#[inline(always)]
fn log_syscall_result(_op: &str, _err: sys::seL4_Error) {}

#[cfg(not(target_os = "none"))]
mod host_trace {
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::sys;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct HostRetypeTrace {
        pub root: sys::seL4_CNode,
        pub node_index: sys::seL4_Word,
        pub node_depth: sys::seL4_Word,
        pub node_offset: sys::seL4_Word,
    }

    static LAST_ROOT: AtomicUsize = AtomicUsize::new(0);
    static LAST_INDEX: AtomicUsize = AtomicUsize::new(0);
    static LAST_DEPTH: AtomicUsize = AtomicUsize::new(0);
    static LAST_OFFSET: AtomicUsize = AtomicUsize::new(0);
    static HAS_TRACE: AtomicBool = AtomicBool::new(false);

    #[inline(always)]
    pub fn record(trace: HostRetypeTrace) {
        LAST_ROOT.store(trace.root as usize, Ordering::SeqCst);
        LAST_INDEX.store(trace.node_index as usize, Ordering::SeqCst);
        LAST_DEPTH.store(trace.node_depth as usize, Ordering::SeqCst);
        LAST_OFFSET.store(trace.node_offset as usize, Ordering::SeqCst);
        HAS_TRACE.store(true, Ordering::SeqCst);
    }

    #[inline(always)]
    pub fn take_last() -> Option<HostRetypeTrace> {
        if !HAS_TRACE.swap(false, Ordering::SeqCst) {
            return None;
        }

        Some(HostRetypeTrace {
            root: LAST_ROOT.load(Ordering::SeqCst) as sys::seL4_CNode,
            node_index: LAST_INDEX.load(Ordering::SeqCst) as sys::seL4_Word,
            node_depth: LAST_DEPTH.load(Ordering::SeqCst) as sys::seL4_Word,
            node_offset: LAST_OFFSET.load(Ordering::SeqCst) as sys::seL4_Word,
        })
    }
}

#[cfg(all(feature = "kernel", not(target_os = "none")))]
pub use host_trace::{take_last as take_last_host_retype_trace, HostRetypeTrace};

#[inline(always)]
pub fn cnode_copy_direct_dest(
    depth_bits: u8,
    dst_slot: sys::seL4_CPtr,
    src_root: sys::seL4_CNode,
    src_index: sys::seL4_CPtr,
    src_depth_bits: u8,
    rights: sys::SeL4CapRights,
) -> sys::seL4_Error {
    #[cfg(target_os = "none")]
    {
        let init_bits = bootinfo().initThreadCNodeSizeBits as u8;
        debug_assert_eq!(depth_bits, init_bits);
        check_slot_in_range(depth_bits, dst_slot);
        let (root, node_index, node_depth, node_offset) = init_cnode_dest(dst_slot);
        debug_assert_eq!(node_offset, 0);
        log_destination("CNode_Copy", node_index, node_depth, node_offset);
        let node_depth_u8 =
            u8::try_from(node_depth).expect("initThreadCNodeSizeBits must fit within u8");
        let err = unsafe {
            sys::seL4_CNode_Copy(
                root,
                node_index,
                node_depth_u8,
                src_root,
                src_index as sys::seL4_Word,
                src_depth_bits,
                rights,
            )
        };
        log_syscall_result("CNode_Copy", err);
        err
    }

    #[cfg(not(target_os = "none"))]
    {
        let (node_index, node_depth, node_offset) =
            init_cnode_direct_destination_words(depth_bits, dst_slot);
        host_trace::record(host_trace::HostRetypeTrace {
            root: bootinfo_init_cnode_cptr(),
            node_index,
            node_depth,
            node_offset,
        });
        let _ = (src_root, src_index, src_depth_bits, rights);
        sys::seL4_NoError
    }
}

#[inline(always)]
pub fn cnode_mint_direct_dest(
    depth_bits: u8,
    dst_slot: sys::seL4_CPtr,
    src_root: sys::seL4_CNode,
    src_index: sys::seL4_CPtr,
    src_depth_bits: u8,
    rights: sys::SeL4CapRights,
    badge: sys::seL4_Word,
) -> sys::seL4_Error {
    #[cfg(target_os = "none")]
    {
        let init_bits = bootinfo().initThreadCNodeSizeBits as u8;
        debug_assert_eq!(depth_bits, init_bits);
        check_slot_in_range(depth_bits, dst_slot);
        let (root, node_index, node_depth, node_offset) = init_cnode_dest(dst_slot);
        debug_assert_eq!(node_offset, 0);
        log_destination("CNode_Mint", node_index, node_depth, node_offset);
        let node_depth_u8 =
            u8::try_from(node_depth).expect("initThreadCNodeSizeBits must fit within u8");
        let err = unsafe {
            sys::seL4_CNode_Mint(
                root,
                node_index,
                node_depth_u8,
                src_root,
                src_index as sys::seL4_Word,
                src_depth_bits,
                rights,
                badge,
            )
        };
        log_syscall_result("CNode_Mint", err);
        err
    }

    #[cfg(not(target_os = "none"))]
    {
        let (node_index, node_depth, node_offset) =
            init_cnode_direct_destination_words(depth_bits, dst_slot);
        host_trace::record(host_trace::HostRetypeTrace {
            root: bootinfo_init_cnode_cptr(),
            node_index,
            node_depth,
            node_offset,
        });
        let _ = (src_root, src_index, src_depth_bits, rights, badge);
        sys::seL4_NoError
    }
}

#[inline(always)]
pub fn cnode_move_direct_dest(
    depth_bits: u8,
    dst_slot: sys::seL4_CPtr,
    src_root: sys::seL4_CNode,
    src_index: sys::seL4_CPtr,
    src_depth_bits: u8,
) -> sys::seL4_Error {
    #[cfg(target_os = "none")]
    {
        let init_bits = bootinfo().initThreadCNodeSizeBits as u8;
        debug_assert_eq!(depth_bits, init_bits);
        check_slot_in_range(depth_bits, dst_slot);
        let (root, node_index, node_depth, node_offset) = init_cnode_dest(dst_slot);
        debug_assert_eq!(node_offset, 0);
        log_destination("CNode_Move", node_index, node_depth, node_offset);
        let node_depth_u8 =
            u8::try_from(node_depth).expect("initThreadCNodeSizeBits must fit within u8");
        let err = unsafe {
            sys::seL4_CNode_Move(
                root,
                node_index,
                node_depth_u8,
                src_root,
                src_index as sys::seL4_Word,
                src_depth_bits,
            )
        };
        log_syscall_result("CNode_Move", err);
        err
    }

    #[cfg(not(target_os = "none"))]
    {
        let (node_index, node_depth, node_offset) =
            init_cnode_direct_destination_words(depth_bits, dst_slot);
        host_trace::record(host_trace::HostRetypeTrace {
            root: bootinfo_init_cnode_cptr(),
            node_index,
            node_depth,
            node_offset,
        });
        let _ = (src_root, src_index, src_depth_bits);
        sys::seL4_NoError
    }
}

#[inline(always)]
pub fn untyped_retype_into_init_cnode(
    depth_bits: u8,
    untyped_slot: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dst_slot: sys::seL4_CPtr,
) -> sys::seL4_Error {
    #[cfg(target_os = "none")]
    {
        let bootinfo = bootinfo();
        let init_bits = bootinfo.initThreadCNodeSizeBits as u8;
        assert_eq!(
            depth_bits, init_bits,
            "retype depth {} does not match initThreadCNodeSizeBits {}",
            depth_bits, init_bits
        );
        let empty_start = bootinfo.empty.start as sys::seL4_CPtr;
        let empty_end = bootinfo.empty.end as sys::seL4_CPtr;
        assert!(
            dst_slot >= empty_start && dst_slot < empty_end,
            "destination slot 0x{dst:04x} outside boot empty window [0x{lo:04x}..0x{hi:04x})",
            dst = dst_slot,
            lo = empty_start,
            hi = empty_end,
        );
        check_slot_in_range(depth_bits, dst_slot);
        let (root, node_index, node_depth, node_offset) = retype_dest_init_cnode(dst_slot);
        log::info!(
            "Retype DEST(root=0x{root:04x}, idx={idx}, depth={depth}, off=0x{off:04x})",
            root = root,
            idx = node_index,
            depth = node_depth,
            off = node_offset,
        );
        log_destination("Untyped_Retype", node_index, node_depth, node_offset);
        let err = unsafe {
            sys::seL4_Untyped_Retype(
                untyped_slot,
                obj_type,
                size_bits,
                root,
                node_index,
                node_depth,
                node_offset,
                1,
            )
        };
        log_syscall_result("Untyped_Retype", err);
        err
    }

    #[cfg(not(target_os = "none"))]
    {
        let node_index = 0;
        let node_depth = 0;
        let node_offset = dst_slot as sys::seL4_Word;
        host_trace::record(host_trace::HostRetypeTrace {
            root: bootinfo_init_cnode_cptr(),
            node_index,
            node_depth,
            node_offset,
        });
        let _ = (depth_bits, untyped_slot, obj_type, size_bits, dst_slot);
        sys::seL4_NoError
    }
}

#[inline(always)]
pub fn untyped_retype_into_cnode(
    dest_root: sys::seL4_CNode,
    depth_bits: u8,
    untyped_slot: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dst_slot: sys::seL4_CPtr,
) -> sys::seL4_Error {
    #[cfg(target_os = "none")]
    {
        if dest_root == sys::seL4_CapInitThreadCNode {
            let expected_bits = bootinfo().initThreadCNodeSizeBits as u8;
            assert_eq!(
                depth_bits, expected_bits,
                "init CNode retypes must honour initThreadCNodeSizeBits (provided={} expected={})",
                depth_bits, expected_bits
            );
            check_slot_in_range(depth_bits, dst_slot);
            let (root, node_index, node_depth, node_offset) = retype_dest_init_cnode(dst_slot);
            log::info!(
                "Retype DEST(root=0x{root:04x}, idx={idx}, depth={depth}, off=0x{off:04x})",
                root = root,
                idx = node_index,
                depth = node_depth,
                off = node_offset,
            );
            log_destination("Untyped_Retype", node_index, node_depth, node_offset);
            let err = unsafe {
                sys::seL4_Untyped_Retype(
                    untyped_slot,
                    obj_type,
                    size_bits,
                    root,
                    node_index,
                    node_depth,
                    node_offset,
                    1,
                )
            };
            log_syscall_result("Untyped_Retype", err);
            err
        } else {
            let node_depth = encode_cnode_depth(depth_bits);
            let err = unsafe {
                sys::seL4_Untyped_Retype(
                    untyped_slot,
                    obj_type,
                    size_bits,
                    dest_root,
                    dst_slot as sys::seL4_Word,
                    node_depth,
                    0,
                    1,
                )
            };
            log_syscall_result("Untyped_Retype", err);
            err
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        let depth = if dest_root == sys::seL4_CapInitThreadCNode {
            0
        } else {
            encode_cnode_depth(depth_bits)
        };
        let node_index = if dest_root == sys::seL4_CapInitThreadCNode {
            0
        } else {
            dst_slot as sys::seL4_Word
        };
        let node_offset = if dest_root == sys::seL4_CapInitThreadCNode {
            dst_slot as sys::seL4_Word
        } else {
            0
        };
        host_trace::record(host_trace::HostRetypeTrace {
            root: dest_root,
            node_index,
            node_depth: depth,
            node_offset,
        });
        let _ = (untyped_slot, obj_type, size_bits);
        sys::seL4_NoError
    }
}

#[cfg(any(test, not(target_os = "none")))]
#[inline(always)]
pub(crate) fn init_cnode_direct_destination_words(
    init_cnode_bits: u8,
    dst_slot: sys::seL4_CPtr,
) -> (sys::seL4_Word, sys::seL4_Word, sys::seL4_Word) {
    let limit = if init_cnode_bits as usize >= usize::BITS as usize {
        usize::MAX
    } else {
        1usize << init_cnode_bits
    };
    debug_assert!(
        (dst_slot as usize) < limit,
        "destination slot {dst_slot:#x} exceeds init CNode capacity (bits={init_cnode_bits})"
    );
    (0, 0, dst_slot as sys::seL4_Word)
}

#[cfg(test)]
#[inline(always)]
pub fn init_cnode_direct_destination_words_for_test(
    depth_bits: u8,
    dst_slot: sys::seL4_CPtr,
) -> (sys::seL4_Word, sys::seL4_Word, sys::seL4_Word) {
    init_cnode_direct_destination_words(depth_bits, dst_slot)
}

#[cfg(test)]
mod tests {
    use super::{
        bootinfo_init_cnode_cptr, init_cnode_dest, init_cnode_direct_destination_words_for_test,
        retype_dest_init_cnode,
    };

    #[test]
    fn init_cnode_dest_radix_depth_is_valid() {
        #[cfg(not(target_os = "none"))]
        unsafe {
            let mut bootinfo: sel4_sys::seL4_BootInfo = core::mem::zeroed();
            bootinfo.initThreadCNodeSizeBits = 13;
            super::install_test_bootinfo_for_tests(bootinfo);
        }
        let slot = 0x00a5u64;
        let (root, idx, depth, off) = init_cnode_dest(slot as _);
        assert_eq!(root, bootinfo_init_cnode_cptr());
        assert_eq!(idx, slot as _);
        assert!(depth > 0 && depth <= sel4_sys::seL4_WordBits as _);
        assert_eq!(off, 0);
    }

    #[test]
    fn retype_dest_selects_root_cnode_slot_directly() {
        #[cfg(not(target_os = "none"))]
        unsafe {
            let mut bootinfo: sel4_sys::seL4_BootInfo = core::mem::zeroed();
            bootinfo.initThreadCNodeSizeBits = 13;
            super::install_test_bootinfo_for_tests(bootinfo);
        }

        let slot = 0x00a6u64;
        let (root, idx, depth, off) = retype_dest_init_cnode(slot as _);
        assert_eq!(root, bootinfo_init_cnode_cptr());
        assert_eq!(idx, 0);
        assert_eq!(depth, 0);
        assert_eq!(off, slot as _);
    }

    #[test]
    fn direct_destination_words_match_depth_bits() {
        let slot = 0x10u64;
        let bits = 13u8;
        let (idx, depth, off) = init_cnode_direct_destination_words_for_test(bits, slot as _);
        assert_eq!(idx, 0);
        assert_eq!(depth, 0);
        assert_eq!(off, slot as _);
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    #[cfg_attr(not(debug_assertions), ignore = "debug assertions disabled")]
    fn init_cnode_dest_rejects_out_of_range_slot() {
        use std::panic;

        unsafe {
            let mut bootinfo: sel4_sys::seL4_BootInfo = core::mem::zeroed();
            bootinfo.initThreadCNodeSizeBits = 5;
            super::install_test_bootinfo_for_tests(bootinfo);
        }

        let limit_slot = 1usize << 5;
        let result = panic::catch_unwind(|| {
            let slot = limit_slot as sel4_sys::seL4_CPtr;
            let _ = init_cnode_dest(slot);
        });

        assert!(
            result.is_err(),
            "init_cnode_dest should panic when slot is out of range"
        );
    }
}
