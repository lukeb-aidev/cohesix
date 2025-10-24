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

/// Issues a `seL4_Untyped_Retype` call constrained to the supplied CNode root.
pub fn untyped_retype_invoc(
    dest_root: sys::seL4_CNode,
    depth_bits: u8,
    untyped_slot: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dst_slot: sys::seL4_CPtr,
) -> sys::seL4_Error {
    #[cfg(target_os = "none")]
    {
        let depth = resolve_cnode_depth(depth_bits);
        let (node_index, node_depth, node_offset) = if dest_root == sys::seL4_CapInitThreadCNode {
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
            (0, 0, dst_slot)
        } else {
            (dst_slot, depth.as_word(), 0)
        };

        unsafe {
            sys::seL4_Untyped_Retype(
                untyped_slot,
                obj_type,
                size_bits,
                dest_root,
                node_index,
                node_depth,
                node_offset,
                1,
            )
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        let depth = resolve_cnode_depth(depth_bits);
        let (node_index, node_depth, node_offset) = if dest_root == sys::seL4_CapInitThreadCNode {
            (0, 0, dst_slot)
        } else {
            (dst_slot, depth.as_word(), 0)
        };
        let _ = (
            dest_root,
            depth_bits,
            untyped_slot,
            obj_type,
            size_bits,
            node_index,
            node_depth,
            node_offset,
        );
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
            sys::seL4_Untyped_Retype(
                untyped,
                obj_type,
                size_bits,
                sys::seL4_CapInitThreadCNode,
                0,
                0,
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
