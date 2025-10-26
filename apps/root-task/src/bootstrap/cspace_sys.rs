// Author: Lukas Bower
#![allow(unsafe_code)]

use crate::boot;
use crate::sel4 as sys;
use core::convert::TryFrom;
use sel4_sys;

pub const CANONICAL_CNODE_DEPTH_BITS: u8 = sel4_sys::seL4_WordBits as u8;

#[inline(always)]
fn bootinfo() -> &'static sel4_sys::seL4_BootInfo {
    unsafe { &*sel4_sys::seL4_GetBootInfo() }
}

#[inline(always)]
fn init_cnode_bits_word() -> sys::seL4_Word {
    bootinfo().initThreadCNodeSizeBits as sys::seL4_Word
}

#[inline(always)]
fn init_cnode_capacity() -> usize {
    let init_bits = init_cnode_bits_word();
    if init_bits as usize >= usize::BITS as usize {
        usize::MAX
    } else {
        1usize.checked_shl(init_bits as u32).unwrap_or(usize::MAX)
    }
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
    let capacity = init_cnode_capacity();
    debug_assert!(
        (slot as usize) < capacity,
        "slot 0x{slot:04x} exceeds init CNode capacity (limit=0x{capacity:04x})"
    );
    (
        sel4_sys::seL4_CapInitThreadCNode,
        slot as sys::seL4_Word,
        init_cnode_bits_word(),
        0,
    )
}

#[cfg(target_os = "none")]
#[inline(always)]
fn log_destination(op: &str, idx: sys::seL4_Word, depth: sys::seL4_Word, offset: sys::seL4_Word) {
    if boot::flags::trace_dest() {
        log::info!(
            "DEST → op={op} root=initCNode idx=0x{idx:04x} depth={depth} (initBits) off={offset}",
            op = op,
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
        debug_assert_eq!(depth_bits as sys::seL4_Word, init_cnode_bits_word());
        check_slot_in_range(depth_bits, dst_slot);
        let (root, node_index, node_depth, node_offset) = init_cnode_dest(dst_slot);
        let node_depth_bits = u8::try_from(node_depth)
            .expect("init thread CNode depth exceeds architectural word size");
        log_destination("CNode_Copy", node_index, node_depth, node_offset);
        let err = unsafe {
            sys::seL4_CNode_Copy(
                root,
                node_index,
                node_depth_bits,
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
            root: sys::seL4_CapInitThreadCNode,
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
        debug_assert_eq!(depth_bits as sys::seL4_Word, init_cnode_bits_word());
        check_slot_in_range(depth_bits, dst_slot);
        let (root, node_index, node_depth, node_offset) = init_cnode_dest(dst_slot);
        let node_depth_bits = u8::try_from(node_depth)
            .expect("init thread CNode depth exceeds architectural word size");
        log_destination("CNode_Mint", node_index, node_depth, node_offset);
        let err = unsafe {
            sys::seL4_CNode_Mint(
                root,
                node_index,
                node_depth_bits,
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
            root: sys::seL4_CapInitThreadCNode,
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
        debug_assert_eq!(depth_bits as sys::seL4_Word, init_cnode_bits_word());
        check_slot_in_range(depth_bits, dst_slot);
        let (root, node_index, node_depth, node_offset) = init_cnode_dest(dst_slot);
        let node_depth_bits = u8::try_from(node_depth)
            .expect("init thread CNode depth exceeds architectural word size");
        log_destination("CNode_Move", node_index, node_depth, node_offset);
        let err = unsafe {
            sys::seL4_CNode_Move(
                root,
                node_index,
                node_depth_bits,
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
            root: sys::seL4_CapInitThreadCNode,
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
        let (root, node_index, node_depth, node_offset) = init_cnode_dest(dst_slot);
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
        let (node_index, node_depth, node_offset) =
            init_cnode_direct_destination_words(depth_bits, dst_slot);
        host_trace::record(host_trace::HostRetypeTrace {
            root: sys::seL4_CapInitThreadCNode,
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
            let expected_bits = init_cnode_bits_word() as u8;
            assert_eq!(
                depth_bits, expected_bits,
                "init CNode retypes must honour initThreadCNodeSizeBits (provided={} expected={})",
                depth_bits, expected_bits
            );
            check_slot_in_range(depth_bits, dst_slot);
            let (root, node_index, node_depth, node_offset) = init_cnode_dest(dst_slot);
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
            if err != sys::seL4_NoError {
                log::error!(
                    "Untyped_Retype (non-init root) failed: err={err} ({name})",
                    err = err,
                    name = crate::sel4::error_name(err),
                );
            }
            err
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        let depth = if dest_root == sys::seL4_CapInitThreadCNode {
            init_cnode_bits_word()
        } else {
            encode_cnode_depth(depth_bits)
        };
        host_trace::record(host_trace::HostRetypeTrace {
            root: dest_root,
            node_index: dst_slot as sys::seL4_Word,
            node_depth: depth,
            node_offset: 0,
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
    (
        dst_slot as sys::seL4_Word,
        init_cnode_bits as sys::seL4_Word,
        0,
    )
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
    use super::{init_cnode_dest, init_cnode_direct_destination_words_for_test};

    #[test]
    fn init_cnode_dest_radix_depth_is_valid() {
        let slot = 0x00a5u64;
        let (root, idx, depth, off) = init_cnode_dest(slot as _);
        assert_eq!(root, sel4_sys::seL4_CapInitThreadCNode);
        assert_eq!(idx, slot as _);
        assert!(depth > 0 && depth <= sel4_sys::seL4_WordBits as _);
        assert_eq!(off, 0);
    }

    #[test]
    fn direct_destination_words_match_depth_bits() {
        let slot = 0x10u64;
        let bits = 13u8;
        let (idx, depth, off) = init_cnode_direct_destination_words_for_test(bits, slot as _);
        assert_eq!(idx, slot as _);
        assert_eq!(depth, bits as _);
        assert_eq!(off, 0);
    }
}
