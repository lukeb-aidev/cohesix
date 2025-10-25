// Author: Lukas Bower
#![allow(unsafe_code)]

use crate::sel4 as sys;

/// Maximum representable depth (in bits) for the init CNode on this architecture.
pub const CANONICAL_CNODE_DEPTH_BITS: u8 = (core::mem::size_of::<sys::seL4_Word>() * 8) as u8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CNodeDepth {
    bits_u8: u8,
    bits_word: sys::seL4_Word,
}

impl CNodeDepth {
    #[inline(always)]
    fn new(bits: u8) -> Self {
        debug_assert!(bits <= CANONICAL_CNODE_DEPTH_BITS);
        Self {
            bits_u8: bits,
            bits_word: bits as sys::seL4_Word,
        }
    }

    #[inline(always)]
    fn as_u8(self) -> u8 {
        self.bits_u8
    }

    #[inline(always)]
    fn as_word(self) -> sys::seL4_Word {
        self.bits_word
    }
}

#[inline(always)]
fn resolve_cnode_depth(invocation_bits: u8) -> CNodeDepth {
    // seL4 interprets the depth as the number of address bits (guard + index)
    // consumed when decoding the capability pointer. The init CSpace provided by
    // bootinfo is single-level, so callers must supply the radix width reported
    // by the kernel (initThreadCNodeSizeBits). Guard against callers attempting
    // to exceed the architectural word width.
    debug_assert!(
        invocation_bits <= CANONICAL_CNODE_DEPTH_BITS,
        "init cnode depth {} exceeds architectural limit {}",
        invocation_bits,
        CANONICAL_CNODE_DEPTH_BITS
    );
    CNodeDepth::new(invocation_bits)
}
#[inline]
/// Ensures that the provided slot falls within the init CNode window defined by `init_cnode_bits`.
pub fn check_slot_in_range(init_cnode_bits: u8, slot: sys::seL4_CPtr) {
    let limit = 1u64 << (init_cnode_bits as u64);
    assert!(
        (slot as u64) < limit,
        "slot {} out of range for init_cnode_bits={} (limit={})",
        slot,
        init_cnode_bits,
        limit
    );
}

#[inline(always)]
pub fn encode_cnode_depth(bits: u8) -> sys::seL4_Word {
    resolve_cnode_depth(bits).as_word()
}

#[cfg(any(test, not(target_os = "none")))]
#[inline(always)]
pub(crate) fn init_cnode_direct_destination_words(
    init_cnode_bits: u8,
    dst_slot: sys::seL4_CPtr,
) -> (sys::seL4_Word, sys::seL4_Word, sys::seL4_Word) {
    (
        0,
        encode_cnode_depth(init_cnode_bits),
        dst_slot as sys::seL4_Word,
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
pub fn untyped_retype_into_init_cnode(
    depth_bits: u8,
    untyped_slot: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dst_slot: sys::seL4_CPtr,
) -> sys::seL4_Error {
    #[cfg(target_os = "none")]
    {
        let bootinfo = unsafe { &*sys::seL4_GetBootInfo() };
        let init_cnode_bits = bootinfo.initThreadCNodeSizeBits as u8;
        assert!(
            depth_bits == init_cnode_bits,
            "retype depth {} does not match initThreadCNodeSizeBits {}",
            depth_bits,
            init_cnode_bits
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

        let depth = bootinfo.initThreadCNodeSizeBits as sys::seL4_Word;
        unsafe {
            sys::seL4_Untyped_Retype(
                untyped_slot,
                obj_type,
                size_bits,
                sys::seL4_CapInitThreadCNode,
                0,
                depth,
                dst_slot,
                1,
            )
        }
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
        // The Rust root-task path enforces `node_depth = initThreadCNodeSizeBits` when targeting the
        // init CNode. Host-side tooling may still call into this helper, so guard the kernel path to
        // ensure we never regress to zero-depth retypes.
        if dest_root == sys::seL4_CapInitThreadCNode {
            let bootinfo = unsafe { &*sys::seL4_GetBootInfo() };
            let init_bits = bootinfo.initThreadCNodeSizeBits as u8;
            assert_eq!(
                depth_bits, init_bits,
                "init CNode retypes must use initThreadCNodeSizeBits depth (provided={} expected={})",
                depth_bits, init_bits
            );
        }
        let depth = resolve_cnode_depth(depth_bits);
        unsafe {
            sys::seL4_Untyped_Retype(
                untyped_slot,
                obj_type,
                size_bits,
                dest_root,
                dst_slot,
                depth.as_word(),
                0,
                1,
            )
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        let depth = resolve_cnode_depth(depth_bits);
        host_trace::record(host_trace::HostRetypeTrace {
            root: dest_root,
            node_index: dst_slot as sys::seL4_Word,
            node_depth: depth.as_word(),
            node_offset: 0,
        });
        let _ = (untyped_slot, obj_type, size_bits);
        sys::seL4_NoError
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::sys;

    #[allow(dead_code)]
    pub fn cnode_mint_direct_dest(
        depth_bits: u8,
        dst_slot: sys::seL4_CPtr,
        src_slot: sys::seL4_CPtr,
        rights: sys::SeL4CapRights,
        badge: sys::seL4_Word,
    ) -> sys::seL4_Error {
        #[cfg(target_os = "none")]
        unsafe {
            sys::seL4_CNode_Mint(
                sys::seL4_CapInitThreadCNode,
                dst_slot,
                depth_bits,
                sys::seL4_CapInitThreadCNode,
                src_slot,
                0u8,
                rights,
                badge,
            )
        }

        #[cfg(not(target_os = "none"))]
        {
            let _ = (depth_bits, dst_slot, src_slot, rights, badge);
            sys::seL4_IllegalOperation
        }
    }

    #[allow(dead_code)]
    pub fn untyped_retype_direct_dest(
        depth_bits: u8,
        untyped: sys::seL4_CPtr,
        obj_type: sys::seL4_Word,
        size_bits: sys::seL4_Word,
        dst_slot: sys::seL4_CPtr,
    ) -> sys::seL4_Error {
        #[cfg(target_os = "none")]
        unsafe {
            let bi = &*sys::seL4_GetBootInfo();
            let depth = bi.initThreadCNodeSizeBits as sys::seL4_Word;
            sys::seL4_Untyped_Retype(
                untyped,
                obj_type,
                size_bits,
                sys::seL4_CapInitThreadCNode,
                0,
                depth,
                dst_slot,
                1,
            )
        }

        #[cfg(not(target_os = "none"))]
        {
            let _ = (depth_bits, untyped, obj_type, size_bits, dst_slot);
            sys::seL4_IllegalOperation
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_cnode_depth, CANONICAL_CNODE_DEPTH_BITS};

    #[test]
    fn resolve_cnode_depth_tracks_invocation_width() {
        for bits in [1u8, 5, 13, CANONICAL_CNODE_DEPTH_BITS] {
            let depth = resolve_cnode_depth(bits);
            assert_eq!(depth.as_u8(), bits);
            assert_eq!(depth.as_word(), bits as super::sys::seL4_Word);
            assert!(bits <= CANONICAL_CNODE_DEPTH_BITS);
        }
    }

    #[test]
    fn resolve_cnode_depth_never_underfills_guard_bits() {
        let depth = resolve_cnode_depth(0);
        assert_eq!(depth.as_u8(), 0);
        assert_eq!(depth.as_word(), 0);
    }
}
